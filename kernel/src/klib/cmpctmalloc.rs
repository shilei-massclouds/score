/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::{mem, cmp};
use core::ptr::null_mut;
use crate::defines::BYTES_PER_USIZE;
use crate::klib::memory::memset;
use crate::{debug::*, BOOT_CONTEXT, ZX_ASSERT_MSG};
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
    left: *mut header_t,
    /* The size of the memory area in bytes, including this header.
     * The right sentinel will have 0 in this field. */
    size: u32,
    /* The most bit is used to store extra state: see FREE_BIT. */
    flag: u32,
}

impl header_t {
    pub fn size(&self) -> usize {
        self.size as usize
    }
}

#[allow(non_camel_case_types)]
struct free_t {
    header: header_t,
    queue_node: ListNode,   /* linked node */
}

impl Linked<free_t> for free_t {
    fn from_node(ptr: *mut ListNode) -> *mut free_t {
        unsafe {
            crate::container_of!(ptr, free_t, queue_node)
        }
    }

    fn into_node(&mut self) -> *mut ListNode {
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
    cached_os_alloc: *mut header_t,

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
            cached_os_alloc: null_mut(),
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
    unsafe {
        (*BOOT_CONTEXT.data.get()).heap = Some(Heap::new());
    }

    let heap = BOOT_CONTEXT.heap();

    /* Initialize the free lists. */
    for i in 0..NUMBER_OF_BUCKETS {
        heap.free_lists[i].init();
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

    println!("### heap_grow size 0x{:x} ", size);
    /* The new free list entry will have a header on each side (the
     * sentinels) so we need to grow the gross heap size by this much more. */
    size += HEAP_GROW_OVER_HEAD;
    size = ROUNDUP!(size, PAGE_SIZE);

    let mut area = 0;

    let heap = BOOT_CONTEXT.heap();
    let os_alloc = heap.cached_os_alloc;
    if os_alloc != null_mut() {
        unsafe {
            if (*os_alloc).size() >= size {
                dprintf!(INFO, "Using saved 0x{:x}-byte OS (>=0x{:x} bytes)\n",
                         (*os_alloc).size, size);
                area = os_alloc as vaddr_t;
                size = (*os_alloc).size();
                ZX_ASSERT_MSG!(IS_PAGE_ALIGNED!(area), "0x{:x} bytes {:x}", size, area);
                ZX_ASSERT_MSG!(IS_PAGE_ALIGNED!(size), "0x{:x} bytes {:x}", size, area);
            } else {
                /* We need to allocate more from the OS.
                 * Return the cached OS allocation, in case we're holding
                 * an unusually-small block that's unlikely to satisfy
                 * future calls to heap_grow(). */
                dprintf!(INFO, "Returning too-small saved 0x{:x}-byte (<0x{:x} bytes)\n",
                         (*os_alloc).size, size);
                free_to_os(os_alloc as vaddr_t, (*os_alloc).size())?;
            }
        }
        heap.cached_os_alloc = null_mut();
    }

    if area == 0 {
        area = heap_page_alloc(size >> PAGE_SHIFT)?;
        dprintf!(INFO, "Growing heap by 0x{:x} bytes, new area {:x}\n", size, area);
        heap.size += size;
    }

    add_to_heap(area, size)
}

fn heap_page_alloc(pages: usize) -> Result<vaddr_t, ErrNO> {
    ZX_ASSERT!(pages > 0);
    dprintf!(INFO, "heap_page_alloc...\n");
    let alloc = BOOT_CONTEXT.virtual_alloc();
    alloc.alloc_pages(pages)
}

fn heap_page_free(va: vaddr_t, pages: usize) -> Result<(), ErrNO> {
    ZX_ASSERT!(IS_PAGE_ALIGNED!(va));
    ZX_ASSERT!(pages > 0);
    dprintf!(INFO, "address 0x{:x}, pages {}\n", va, pages);

    let alloc = BOOT_CONTEXT.virtual_alloc();
    alloc.free_pages(va, pages)
}

fn create_allocation_header(va: vaddr_t, offset: usize,
        size: usize, left: *mut header_t) -> vaddr_t {

    let ptr = (va + offset) as *mut header_t;
    unsafe {
        (*ptr).left = left;
        (*ptr).size = size as u32;
        (*ptr).flag = 0;
    }
    va + offset + SIZE_OF_HEADER_T
}

fn add_to_heap(area: vaddr_t, size: usize) -> Result<(), ErrNO> {
    /* Set up the left sentinel. */
    let left = area as *mut header_t;
    let free_area = create_allocation_header(area, 0, SIZE_OF_HEADER_T, null_mut());

    /* Set up the usable memory area, which will be marked free. */
    let free_header = free_area as *mut header_t;
    let free_size = size - 2 * SIZE_OF_HEADER_T;
    create_free_area(free_area, left, free_size);

    /* Set up the right sentinel. */
    let right = area + size - SIZE_OF_HEADER_T;
    create_allocation_header(right, 0, 0, free_header);
    Ok(())
}

fn create_free_area(area: vaddr_t, left: *mut header_t, size: usize) {
    let mut ptr = area as *mut free_t;
    unsafe {
        (*ptr).queue_node.init();
        (*ptr).header.left = left;
        (*ptr).header.size = size as u32;
        (*ptr).header.flag = FREE_BIT;
    }

    let bucket = size_to_index_freeing(size - SIZE_OF_HEADER_T);

    let heap = BOOT_CONTEXT.heap();
    heap.set_free_list_bit(bucket);
    heap.free_lists[bucket].add_head(ptr);
    heap.remaining += size;
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

    let (start_bucket, rounded_up) = size_to_index_allocating(size);

    let rounded_up = rounded_up + SIZE_OF_HEADER_T;

    let heap = BOOT_CONTEXT.heap();

    let bucket = match find_nonempty_bucket(start_bucket) {
        Ok(ret) => {
            ret
        },
        Err(_) => {
            /* Grow heap by at least 12% if we can. */
            let mut growby = cmp::min(HEAP_LARGE_ALLOC_BYTES,
                                         cmp::max(heap.size >> 3,
                                         cmp::max(HEAP_USABLE_GROW_SIZE, rounded_up)));
            /* Validate that our growby calculation is correct, and that
             * if we grew the heap by this amount we would actually satisfy
             * our allocation. */
            ZX_ASSERT!(growby >= rounded_up);
            /* Try to add a new OS allocation to the heap, reducing the size
             * until we succeed or get too small. */
            while let Err(_) = heap_grow(growby) {
                if growby <= rounded_up {
                    return null_mut();
                }
                growby = cmp::max(growby >> 1, rounded_up);
            }
            match find_nonempty_bucket(start_bucket) {
                Ok(ret) => {
                    ret
                },
                Err(_) => {
                    panic!("No nonempty bucket!");
                }
            }
        }
    };

    ZX_ASSERT!(!heap.free_lists[bucket].empty());

    let head = heap.free_lists[bucket].head();
    if head == null_mut() {
        panic!("bucket {} is empty!", bucket);
    }

    let left_over = unsafe { (*head).header.size() } - rounded_up;
    // We can't carve off the rest for a new free space if it's smaller than the
    // free-list linked structure.  We also don't carve it off if it's less than
    // 1.6% the size of the allocation.  This is to avoid small long-lived
    // allocations being placed right next to large allocations, hindering
    // coalescing and returning pages to the OS.
    if left_over >= SIZE_OF_FREE_T && left_over > (size >> 6) {
        let right = right_header(head as *mut header_t);
        unlink_free(head, bucket);
        let free = head as usize + rounded_up;
        let left = head as *mut header_t;
        create_free_area(free, left, left_over);
        unsafe {
            (*right).left = free as *mut header_t;
            (*head).header.size -= left_over as u32;
        }
    } else {
        unlink_free(head, bucket);
    }

    let ret;
    unsafe {
        ret = create_allocation_header(head as vaddr_t, 0,
            (*head).header.size(), (*head).header.left);
    }
    memset(ret, 0, size);
    dprintf!(INFO, "cmpct_alloc 0x{:x} 0x{:x}...\n", size, ret);
    ret as *mut u8
}

pub fn cmpct_memalign(align: usize, size: usize) -> *mut u8 {
    if size == 0 {
        return null_mut();
    }

    if align < 8 {
        return cmpct_alloc(size);
    }

    let padded_size = size + align + SIZE_OF_FREE_T;

    let unaligned = cmpct_alloc(padded_size);
    if unaligned == null_mut() {
        return null_mut();
    }
    let unaligned = unaligned as vaddr_t;

    let mask = align - 1;
    let payload = (unaligned + SIZE_OF_FREE_T + mask) & !mask;
    if unaligned != payload {
        let unaligned_header = (unaligned - SIZE_OF_HEADER_T) as *mut header_t;
        let header = payload - SIZE_OF_HEADER_T;
        let left_over = payload - unaligned;
        unsafe {
            create_allocation_header(header, 0,
                                     (*unaligned_header).size() - left_over,
                                     unaligned_header);

            let right = right_header(unaligned_header);
            (*unaligned_header).size = left_over as u32;
            (*right).left = header as *mut header_t;
        }
        cmpct_free(unaligned as *mut u8);
    }

    payload as *mut u8
}

fn unlink_free(free_area: *mut free_t, bucket: usize) {
    let heap = BOOT_CONTEXT.heap();
    unsafe {
        ZX_ASSERT!(heap.remaining >= (*free_area).header.size());
        heap.remaining -= (*free_area).header.size();
        (*free_area).delete_from_list();
        (*free_area).header.flag = 0;
    }
    if heap.free_lists[bucket].empty() {
        heap.clear_free_list_bit(bucket);
    }
}

fn right_header(header: *const header_t) -> *mut header_t {
    unsafe {
        (header as usize + (*header).size()) as *mut header_t
    }
}

fn find_nonempty_bucket(index: usize) -> Result<usize, ErrNO> {
    let heap = BOOT_CONTEXT.heap();
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
            return Ok((i << 5) + mask.leading_zeros() as usize);
        }
    }
    Err(ErrNO::NotFound)
}

pub fn cmpct_free(payload: *mut u8) {
    dprintf!(INFO, "cmpct_free 0x{:x}...\n", payload as usize);
    if payload == null_mut() {
        return;
    }

    let header = (payload as vaddr_t - SIZE_OF_HEADER_T) as *mut header_t;
    if let Err(_) = cmpct_free_internal(payload, header) {
        panic!("cmpct_free error!");
    }
}

fn cmpct_free_internal(_payload: *mut u8, header: *mut header_t)
    -> Result<(), ErrNO> {
    ZX_ASSERT!(!is_tagged_as_free(header));     /* Double free! */
    let size;
    let left;
    unsafe {
        ZX_ASSERT!((*header).size() > SIZE_OF_HEADER_T);
        size = (*header).size();
        left = (*header).left;
    }

    unsafe {
        dprintf!(INFO, "cmpct_free_internal: left {:x} size 0x{:x} flag {:x} self.size 0x{:x}\n",
            left as vaddr_t, (*left).size(), (*left).flag, size);
    }

    if left != null_mut() && is_tagged_as_free(left) {
        /* Coalesce with left free object. */
        unlink_free_unknown_bucket(left as *mut free_t);
        let left_left = unsafe { (*left).left };
        let right = right_header(header);
        if is_tagged_as_free(right) {
            /* Coalesce both sides. */
            unlink_free_unknown_bucket(right as *mut free_t);
            let right_right = right_header(right);
            unsafe {
                (*right_right).left = left;
                free_memory(left as vaddr_t, left_left,
                    (*left).size() + size + (*right).size())?;
            }
        } else {
            /* Coalesce only left. */
            unsafe {
                (*right).left = left;
                free_memory(left as vaddr_t, left_left, (*left).size() + size)?;
            }
        }
    } else {
        let right = right_header(header);
        if is_tagged_as_free(right) {
            /* Coalesce only right. */
            let right_right = right_header(right);
            unlink_free_unknown_bucket(right as *mut free_t);
            unsafe {
                (*right_right).left = header;
                free_memory(header as vaddr_t, left, size + (*right).size())?;
            }
        } else {
            free_memory(header as vaddr_t, left, size)?;
        }
    }

    Ok(())
}

fn is_start_of_os_allocation(header: *mut header_t) -> bool {
    unsafe {
        (*header).left == null_mut()
    }
}

fn is_end_of_os_allocation(header: *const header_t) -> bool {
    unsafe {
        (*header).size == 0
    }
}

// Frees |size| bytes starting at |address|, either to a free bucket or to the
// OS (in which case the left/right sentinels are freed as well). |address|
// should point to what would be the header_t of the memory area to free, and
// |left| and |size| should be set to the values that the header_t would have
// contained. This is broken out because the header_t will not contain the
// proper size when coalescing neighboring areas.
fn free_memory(va: vaddr_t, left: *mut header_t, size: usize)
    -> Result<(), ErrNO> {
    if IS_PAGE_ALIGNED!(left as usize) && is_start_of_os_allocation(left) &&
        is_end_of_os_allocation((va + size) as *mut header_t) {
        /* Assert that it's safe to do a simple 2*sizeof(header_t)) below. */
        unsafe {
            ZX_ASSERT!((*left).size() == SIZE_OF_HEADER_T);
        }
        possibly_free_to_os(left as vaddr_t, size + 2 * SIZE_OF_HEADER_T)
    } else {
        create_free_area(va, left, size);
        Ok(())
    }
}

// May call free_to_os(), or may cache the (non-large) OS allocation in
// cached_os_alloc. |left_sentinel| is the start of the OS allocation, and
// |total_size| is the (page-aligned) number of bytes that were originally
// allocated from the OS.
fn possibly_free_to_os(left_sentinel: vaddr_t, total_size: usize)
    -> Result<(), ErrNO> {
    let heap = BOOT_CONTEXT.heap();
    if heap.cached_os_alloc == null_mut() {
        dprintf!(INFO, "Keeping 0x{:x}-byte OS alloc {:x}\n", total_size, left_sentinel);
        heap.cached_os_alloc = left_sentinel as *mut header_t;
        unsafe {
            (*heap.cached_os_alloc).left = null_mut();
            (*heap.cached_os_alloc).flag = 0;
            (*heap.cached_os_alloc).size = total_size as u32;
        }
        return Ok(());
    }

    dprintf!(INFO, "Returning 0x{:x} bytes to OS\n", total_size);
    free_to_os(left_sentinel, total_size)
}

fn free_to_os(va: vaddr_t, size: usize) -> Result<(), ErrNO> {
    ZX_ASSERT!(IS_PAGE_ALIGNED!(va));
    ZX_ASSERT!(IS_PAGE_ALIGNED!(size));
    heap_page_free(va, size >> PAGE_SHIFT)?;

    let heap = BOOT_CONTEXT.heap();
    heap.size -= size;
    Ok(())
}

fn unlink_free_unknown_bucket(free_area: *mut free_t) {
    unsafe {
        let bucket = size_to_index_freeing((*free_area).header.size() - SIZE_OF_HEADER_T);
        unlink_free(free_area, bucket);
    }
}

// Returns true if this header_t is marked as free.
fn is_tagged_as_free(header: *mut header_t) -> bool {
    if header == null_mut() {
        return false;
    }
    unsafe { (*header).flag & FREE_BIT != 0 }
}
