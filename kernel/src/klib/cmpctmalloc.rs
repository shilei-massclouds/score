/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::mem;
use core::ptr::NonNull;
use crate::defines::BYTES_PER_USIZE;
use crate::{debug::*, BOOT_CONTEXT};
use crate::types::vaddr_t;
use crate::{errors::ErrNO, ZX_ASSERT, defines::{PAGE_SIZE, PAGE_SHIFT}};
use super::list::{ListNode, Linked, List};

/*
 * HEAP_GROW_SIZE is minimum size by which the heap is grown.
 *
 * A larger value can provide some performance improvement
 * at the cost of wasted memory.
 *
 * See also |HEAP_LARGE_ALLOC_BYTES|.
 */
const HEAP_GROW_SIZE: usize = 256 * 1024;

/*
 * HEAP_ALLOC_VIRTUAL_BITS defines the largest allocation bucket.
 *
 * The requirements on virtual bits is that the largest allocation
 * (including header), must roundup to not more 2**HEAP_ALLOC_VIRTUAL_BITS
 * than this alignment, and similarly the heap cannot grow by amounts
 * that would not round down to 2**HEAP_ALLOC_VIRTUAL_BITS or less.
 * As such the heap can grow by more than this many bits at once,
 * but not so many as it must fall into the next bucket.
 */
const HEAP_ALLOC_VIRTUAL_BITS: usize = 21;

// HEAP_LARGE_ALLOC_BYTES limits size of any single allocation.
//
// A larger value will, on average, "waste" more memory. Why is that? When
// freeing memory the heap may hold on to a block before returning it to the
// underlying allocator (see |theheap.cached_os_alloc|). The size of the cached
// block is limited by HEAP_LARGE_ALLOC_BYTES so reducing this value limits the
// size of the cached block.
//
// Note that HEAP_LARGE_ALLOC_BYTES is the largest internal allocation that the
// heap can do, and includes any headers. The largest allocation cmpct_alloc
// could theoretically (it may be artificially limited) provide is therefore
// slightly less than this.
//
// See also |HEAP_GROW_SIZE|.
const HEAP_LARGE_ALLOC_BYTES: usize = (1 << HEAP_ALLOC_VIRTUAL_BITS) - HEAP_GROW_OVER_HEAD;

/*
 * Buckets for allocations.
 * The smallest 15 buckets are 8, 16, 24, etc. up to 120 bytes.
 * After that we round up to the nearest size that can be written /^0*1...0*$/,
 * giving 8 buckets per order of binary magnitude.
 * The freelist entries in a given bucket have at least the given size,
 * plus the header size.
 * On 64 bit, the 8 byte bucket is useless, since the freelist header is
 * 16 bytes larger than the header, but we have it for simplicity.
 */
const NUMBER_OF_BUCKETS: usize = 1 + 15 + (HEAP_ALLOC_VIRTUAL_BITS - 7) * 8;

const BUCKET_WORDS: usize = ((NUMBER_OF_BUCKETS) + 31) >> 5;

#[allow(non_camel_case_types)]
struct header_t {
    /* linked node */
    queue_node: ListNode,
    size: usize,
}

impl Linked<header_t> for header_t {
    fn into_node(&mut self) -> &mut ListNode {
        &mut (self.queue_node)
    }
}

struct Heap {
    /* Total bytes allocated from the OS for the heap. */
    size: usize,

    /* Bytes of usable free space in the heap. */
    remaining: usize,

    /* A non-large OS allocation that could have been freed to the OS but
     * wasn't. We will attempt to use this before allocating more memory from
     * the OS, to reduce churn. May be null. If non-null, cached_os_alloc->size
     * holds the total size allocated from the OS for this block. */
    //cached_os_alloc: NonNull<header_t>,

    /* Free lists, bucketed by size. See size_to_index_helper(). */
    free_lists: [List<header_t>; NUMBER_OF_BUCKETS],

    /* Bitmask that tracks whether a given free_lists entry has any elements.
     * See set_free_list_bit(), clear_free_list_bit(). */
    free_list_bits: [u32; BUCKET_WORDS],
}

const EMPTY_LIST: List<header_t> = List::new();

impl Heap {
    const fn new() -> Self {
        Self {
            size: 0,
            remaining: 0,
            free_lists: [EMPTY_LIST; NUMBER_OF_BUCKETS],
            free_list_bits: [0; BUCKET_WORDS],
        }
    }

    #[inline]
    fn set_free_list_bit(&mut self, index: usize) {
        self.free_list_bits[index >> 5] |= 1 << (31 - (index & 0x1f));
    }

    #[inline]
    fn _clear_free_list_bit(&mut self, index: usize) {
        self.free_list_bits[index >> 5] &= !(1 << (31 - (index & 0x1f)));
    }
}

unsafe impl Send for Heap {}
unsafe impl Sync for Heap {}

pub fn cmpct_init() -> Result<(), ErrNO> {
    let mut heap: Heap = Heap::new();

    /* Initialize the free lists. */
    for i in 0..NUMBER_OF_BUCKETS {
        heap.free_lists[i].init();
    }

    dprintf!(INFO, "cmpct_init ...\n");
    heap_grow(&mut heap, HEAP_USABLE_GROW_SIZE)
}

/* When the heap is grown the requested internal usable size will be increased
 * by this amount before allocating from the OS. This can be factored into
 * any heap_grow requested to precisely control the OS allocation amount. */
const HEAP_GROW_OVER_HEAD: usize = mem::size_of::<header_t>() * 2;

/* Precalculated version of HEAP_GROW_SIZE
 * that takes into account the grow overhead. */
const HEAP_USABLE_GROW_SIZE: usize = HEAP_GROW_SIZE - HEAP_GROW_OVER_HEAD;

/* Create a new free-list entry of at least size bytes (including the
 * allocation header).  Called with the lock, apart from during init. */
fn heap_grow(theheap: &mut Heap, mut size: usize) -> Result<(), ErrNO> {
    /* This function accesses field members of header_t which are poisoned
     * so it has to be NO_ASAN.
     *
     * We expect to never have been asked to grow by more than
     * the maximum allocation */
    ZX_ASSERT!(size <= HEAP_LARGE_ALLOC_BYTES);

    /* The new free list entry will have a header on each side (the
     * sentinels) so we need to grow the gross heap size by this much more. */
    size += HEAP_GROW_OVER_HEAD;
    size = ROUNDUP!(size, PAGE_SIZE);
    let area = heap_page_alloc(size >> PAGE_SHIFT)?;
    dprintf!(INFO, "Growing heap by 0x{:x} bytes, new area {:x}\n", size, area);
    theheap.size += size;

    add_to_heap(theheap, area, size)
}

fn heap_page_alloc(pages: usize) -> Result<vaddr_t, ErrNO> {
    ZX_ASSERT!(pages > 0);
    dprintf!(INFO, "heap_page_alloc...\n");
    unsafe {
        match &mut (*BOOT_CONTEXT.data.get()).virtual_alloc {
            Some(alloc) => {
                alloc.alloc_pages(pages)
            },
            None => {
                panic!("VirtualAlloc uninitialized!");
            }
        }
    }
}

fn create_allocation_header(va: vaddr_t, offset: usize, size: usize) -> vaddr_t {
    let ptr = (va + offset) as *mut header_t;
    unsafe {
        (*ptr).queue_node.init();
        (*ptr).size = size;
    }
    va + offset + mem::size_of::<header_t>()
}

fn add_to_heap(heap: &mut Heap, area: vaddr_t, size: usize) -> Result<(), ErrNO> {
    /* Set up the left sentinel. Its |left| field will not have FREE_BIT set,
     * stopping attempts to coalesce left. */
    let free_area = create_allocation_header(area, 0, 0);

    /* Set up the usable memory area, which will be marked free. */
    let free_size = size - 2 * mem::size_of::<header_t>();
    create_free_area(heap, free_area, free_size);

    /* Set up the right sentinel. Its |left| field will not have FREE_BIT bit set,
     * stopping attempts to coalesce right. */
    let top = area + size - mem::size_of::<header_t>();
    create_allocation_header(top, 0, 0);
    Ok(())
}

fn create_free_area(heap: &mut Heap, area: vaddr_t, size: usize) {
    let mut ptr = NonNull::new(area as *mut header_t).unwrap();
    unsafe {
        ptr.as_mut().size = size;
    }

    let index = size_to_index_freeing(size - mem::size_of::<header_t>());
    heap.set_free_list_bit(index);
    heap.free_lists[index].add_head(ptr);
    heap.remaining += size;
    dprintf!(INFO, "create_free_area index 0x{:x}: limit 0x{:x}\n",
             index, NUMBER_OF_BUCKETS);
}

/* Round down size to next bucket when freeing. */
fn size_to_index_freeing(size: usize) -> usize {
    let (bucket, _) = size_to_index_helper(size, 0, 0);
    return bucket
}

/* Operates in sizes that don't include the allocation header;
 * i.e., the usable portion of a memory area. */
fn size_to_index_helper(size: usize, adjust: usize, increment: usize) -> (usize, usize) {
    /* First buckets are simply 8-spaced up to 128. */
    if size <= 128 {
        /* No allocation is smaller than 8 bytes, so the first bucket is for 8
        * byte spaces (not including the header).  For 64 bit, the free list
        * struct is 16 bytes larger than the header, so no allocation can be
        * smaller than that (otherwise how to free it), but we have empty 8
        * and 16 byte buckets for simplicity. */
        return ((size >> 3) - 1, size);
    }

    /* We are going to go up to the next size to round up,
     * but if we hit a bucket size exactly we don't want to go up.
     * By subtracting 8 here, we will do the right thing (the carry
     * propagates up for the round numbers we are interested in). */
    let size = size + adjust;
    /* After 128 the buckets are logarithmically spaced, every 16 up to 256,
     * every 32 up to 512 etc.  This can be thought of as rows of 8 buckets.
     * GCC intrinsic count-leading-zeros.
     * Eg. 128-255 has 24 leading zeros and we want row to be 4. */
    let row = BYTES_PER_USIZE * 8 - 4 - size.leading_zeros() as usize;

    /* For row 4 we want to shift down 4 bits. */
    let column = (size >> row) & 7;
    let mut row_column = (row << 3) | column;
    row_column += increment;
    let size = (8 + (row_column & 7)) << (row_column >> 3);
    /* We start with 15 buckets, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96,
     * 104, 112, 120.  Then we have row 4, sizes 128 and up,
     * with the row-column 8 and up. */
    let answer = row_column + 15 - 32;
    ZX_ASSERT!(answer < NUMBER_OF_BUCKETS);

    (answer, size)
}