/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::mem;
use core::ptr::{NonNull, null_mut};
use crate::defines::BYTES_PER_USIZE;
use crate::klib::memory::memset;
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

/* If a header's |flag| field has this bit set,
 * it is free and lives in a free bucket. */
const FREE_BIT: u32 = 1 << 0;

#[allow(non_camel_case_types)]
struct header_t {
    /* Pointer to the previous area in memory order. */
    left: Option<NonNull<header_t>>,
    /* The size of the memory area in bytes, including this header.
     * The right sentinel will have 0 in this field. */
    size: u32,
    /* The most bit is used to store extra state: see FREE_BIT. */
    flag: u32,
}

#[allow(non_camel_case_types)]
struct free_t {
    header: header_t,
    queue_node: ListNode,   /* linked node */
}

impl Linked<free_t> for free_t {
    fn from_node(ptr: NonNull<ListNode>) -> Option<NonNull<free_t>> {
        unsafe {
            NonNull::<free_t>::new(
                crate::container_of!(ptr.as_ptr(), free_t, queue_node)
            )
        }
    }

    fn into_node(&mut self) -> &mut ListNode {
        &mut (self.queue_node)
    }
}

pub struct Heap {
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
    free_lists: [List<free_t>; NUMBER_OF_BUCKETS],

    /* Bitmask that tracks whether a given free_lists entry has any elements.
     * See set_free_list_bit(), clear_free_list_bit(). */
    free_list_bits: [u32; BUCKET_WORDS],
}

const EMPTY_LIST: List<free_t> = List::new();

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
    fn clear_free_list_bit(&mut self, index: usize) {
        self.free_list_bits[index >> 5] &= !(1 << (31 - (index & 0x1f)));
    }
}

unsafe impl Send for Heap {}
unsafe impl Sync for Heap {}

pub fn cmpct_init() -> Result<(), ErrNO> {
    dprintf!(INFO, "cmpct_init ...\n");
    let mut heap: Heap = Heap::new();

    /* Initialize the free lists. */
    for i in 0..NUMBER_OF_BUCKETS {
        heap.free_lists[i].init();
    }

    unsafe {
        (*BOOT_CONTEXT.data.get()).heap = Some(heap);
    }
    heap_grow(HEAP_USABLE_GROW_SIZE)
}

const SIZE_OF_HEADER_T: usize = mem::size_of::<header_t>();
const SIZE_OF_FREE_T: usize = mem::size_of::<free_t>();

// Factors in the header for an allocation. Value chosen here is hard coded and could be less than
// the actual largest allocation that cmpct_alloc could provide. This is done so that larger buckets
// can exist in order to allow the heap to grow by amounts larger than what we would like to allow
// clients to allocate.
const HEAP_MAX_ALLOC_SIZE: usize = (1 << 20) - SIZE_OF_HEADER_T;

/* When the heap is grown the requested internal usable size will be increased
 * by this amount before allocating from the OS. This can be factored into
 * any heap_grow requested to precisely control the OS allocation amount. */
const HEAP_GROW_OVER_HEAD: usize = SIZE_OF_HEADER_T * 2;

/* Precalculated version of HEAP_GROW_SIZE
 * that takes into account the grow overhead. */
const HEAP_USABLE_GROW_SIZE: usize = HEAP_GROW_SIZE - HEAP_GROW_OVER_HEAD;

/* Create a new free-list entry of at least size bytes (including the
 * allocation header).  Called with the lock, apart from during init. */
fn heap_grow(mut size: usize) -> Result<(), ErrNO> {
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
    BOOT_CONTEXT.get_heap().size += size;

    add_to_heap(area, size)
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

fn create_allocation_header(va: vaddr_t, offset: usize,
        size: usize, left: Option<NonNull<header_t>>) -> vaddr_t {

    let ptr = (va + offset) as *mut header_t;
    unsafe {
        (*ptr).left = left;
        (*ptr).size = size as u32;
    }
    va + offset + SIZE_OF_HEADER_T
}

fn add_to_heap(area: vaddr_t, size: usize) -> Result<(), ErrNO> {
    /* Set up the left sentinel. Its |left| field will not have FREE_BIT set,
     * stopping attempts to coalesce left. */
    let left = NonNull::new(area as *mut header_t);
    let free_area = create_allocation_header(area, 0, SIZE_OF_HEADER_T, None);

    /* Set up the usable memory area, which will be marked free. */
    let free_header = NonNull::new(free_area as *mut header_t);
    let free_size = size - 2 * SIZE_OF_HEADER_T;
    create_free_area(free_area, left, free_size);

    /* Set up the right sentinel. Its |left| field will not have FREE_BIT bit set,
     * stopping attempts to coalesce right. */
    let right = area + size - SIZE_OF_HEADER_T;
    create_allocation_header(right, 0, 0, free_header);
    Ok(())
}

fn create_free_area(area: vaddr_t, left: Option<NonNull<header_t>>, size: usize) {
    let mut ptr = NonNull::new(area as *mut free_t).unwrap();
    unsafe {
        ptr.as_mut().queue_node.init();
        ptr.as_mut().header.left = left;
        ptr.as_mut().header.size = size as u32;
        ptr.as_mut().header.flag = FREE_BIT;
    }

    let index = size_to_index_freeing(size - SIZE_OF_HEADER_T);

    let heap = BOOT_CONTEXT.get_heap();
    heap.set_free_list_bit(index);
    heap.free_lists[index].add_head(ptr);
    heap.remaining += size;
    dprintf!(INFO, "create_free_area index 0x{:x}: limit 0x{:x}\n",
             index, NUMBER_OF_BUCKETS);
}

// Round up size to next bucket when allocating.
fn size_to_index_allocating(size: usize) -> (usize, usize) {
    let rounded = ROUNDUP!(size, 8);
    size_to_index_helper(rounded, -8, 1)
}

/* Round down size to next bucket when freeing. */
fn size_to_index_freeing(size: usize) -> usize {
    let (bucket, _) = size_to_index_helper(size, 0, 0);
    return bucket
}

/* Operates in sizes that don't include the allocation header;
 * i.e., the usable portion of a memory area. */
fn size_to_index_helper(size: usize, adjust: isize, increment: usize) -> (usize, usize) {
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
    let size = (size as isize + adjust) as usize;
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

pub fn cmpct_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return null_mut();
    }

    /* Large allocations are no longer allowed. */
    if size > HEAP_MAX_ALLOC_SIZE {
        return null_mut();
    }

    let alloc_size = size;

    let (start_bucket, rounded_up) = size_to_index_allocating(size);

    let rounded_up = rounded_up + SIZE_OF_HEADER_T;

    let bucket;
    match find_nonempty_bucket(start_bucket) {
        Ok(ret) => {
            bucket = ret;
        },
        Err(_) => {
            todo!("when find_nonempty_bucket error!");
        }
    }

    let heap = BOOT_CONTEXT.get_heap();

    let mut head = heap.free_lists[bucket].head().unwrap();
    let ptr_header = unsafe { &head.as_ref().header } as *const header_t;
    let left_over = unsafe { head.as_ref().header.size as usize } - rounded_up;
    // We can't carve off the rest for a new free space if it's smaller than the
    // free-list linked structure.  We also don't carve it off if it's less than
    // 1.6% the size of the allocation.  This is to avoid small long-lived
    // allocations being placed right next to large allocations, hindering
    // coalescing and returning pages to the OS.
    if left_over >= SIZE_OF_FREE_T && left_over > (size >> 6) {
        let right = right_header(ptr_header);
        unlink_free(head, bucket);
        let free = head.as_ptr() as usize + rounded_up;
        let left = NonNull::new(ptr_header as *mut header_t);
        create_free_area(free, left, left_over);
        unsafe {
            (*right).left = NonNull::new(free as *mut header_t);
            head.as_mut().header.size -= left_over as u32;
        }
    } else {
        unlink_free(head, bucket);
    }

    let ret;
    unsafe {
        ret = create_allocation_header(ptr_header as vaddr_t, 0,
            (*ptr_header).size as usize, (*ptr_header).left);
    }
    memset(ret, 0, alloc_size);
    ret as *mut u8
}

fn unlink_free(mut free_area: NonNull<free_t>, bucket: usize) {
    let heap = BOOT_CONTEXT.get_heap();
    unsafe {
        ZX_ASSERT!(heap.remaining >= free_area.as_ref().header.size as usize);
        heap.remaining -= free_area.as_ref().header.size as usize;
        free_area.as_mut().delete_from_list();
        free_area.as_mut().header.flag = 0;
    }
    if heap.free_lists[bucket].empty() {
        heap.clear_free_list_bit(bucket);
    }
}

fn right_header(header: *const header_t) -> *mut header_t {
    unsafe {
        (header as usize + (*header).size as usize) as *mut header_t
    }
}

fn find_nonempty_bucket(index: usize) -> Result<usize, ErrNO> {
    let heap = BOOT_CONTEXT.get_heap();

    let mut mask = (1u32 << (31 - (index & 0x1f))) - 1;
    mask = mask * 2 + 1;
    mask &= heap.free_list_bits[index >> 5];
    if mask != 0 {
        return Ok((index & !0x1f) + mask.leading_zeros() as usize);
    }
    let start = ROUNDUP!(index +1 , 32) >> 5;
    let end = NUMBER_OF_BUCKETS >> 5;
    for i in start..end {
        mask = heap.free_list_bits[i];
        if mask != 0 {
            return Ok(i + mask.leading_zeros() as usize);
        }
    }
    Err(ErrNO::NotFound)
}