/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::{debug::*, BOOT_CONTEXT};
use crate::klib::cmpctmalloc::cmpct_init;
use alloc::alloc::{GlobalAlloc, Layout};
use core::cmp::min;
use core::ptr::null_mut;
use spin::{Mutex, MutexGuard};
use crate::klib::bitmap::Bitmap;
use crate::vm_page_state::{self, *};
use crate::defines::{_boot_heap, _boot_heap_end, BYTES_PER_USIZE};
use crate::ARCH_HEAP_ALIGN_BITS;
use crate::aspace::{
    vm_get_kernel_heap_base, vm_get_kernel_heap_size, ExistingEntryAction
};
use crate::{ErrNO, PAGE_SHIFT, PAGE_SIZE, BYTE_BITS, ZX_ASSERT};
use crate::types::*;
use crate::klib::list::{List, Linked};
use crate::page::vm_page_t;
use crate::pmm::{pmm_alloc_pages, pmm_alloc_contiguous, paddr_to_vm_page, pmm_free};
use crate::vm::{
    ARCH_MMU_FLAG_CACHED, ARCH_MMU_FLAG_PERM_READ, ARCH_MMU_FLAG_PERM_WRITE
};

extern crate alloc;

const BATCH_PAGES: usize = 128;

/// A wrapper around spin::Mutex to permit trait implementations.
pub struct Locked<A> {
    inner: Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: Mutex::new(inner),
        }
    }
    pub fn lock(&self) -> MutexGuard<A> {
        self.inner.lock()
    }
}

pub struct BumpAllocator {
    start:  usize,
    end:    usize,
    next:   usize,
    allocations: usize,
}

impl BumpAllocator {
    /// Creates a new empty bump allocator.
    pub const fn new() -> Self {
        BumpAllocator {
            start: 0,
            end: 0,
            next: 0,
            allocations: 0,
        }
    }

    /// Initializes the bump allocator with the given heap bounds.
    ///
    /// This method is unsafe because the caller must ensure that the given
    /// memory range is unused. Also, this method must be called only once.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        self.start = start;
        self.end = start + size;
        self.next = start;
    }
}

unsafe impl GlobalAlloc for Locked<BumpAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut bump = self.lock(); // get a mutable reference

        let start = ALIGN!(bump.next, layout.align());
        let end = start + layout.size();
        if end > bump.end {
            null_mut()  // out of memory
        } else {
            bump.next = end;
            bump.allocations += 1;
            start as *mut u8
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        let mut bump = self.lock(); // get a mutable reference

        bump.allocations -= 1;
        if bump.allocations == 0 {
            bump.next = bump.start;
        }
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout);
}

#[global_allocator]
static ALLOCATOR: Locked<BumpAllocator> = Locked::new(BumpAllocator::new());

pub fn boot_heap_earliest_init() {
    let start = _boot_heap as usize;
    let size = _boot_heap_end as usize - start;
    unsafe {
        ALLOCATOR.lock().init(start, size);
    }
}

pub fn boot_heap_mark_pages_in_use() {
    /* boot heap area is included in kernel */
    /*
    let allocator = ALLOCATOR.lock();

    let start = kernel_va_to_pa(allocator.start);
    let end = kernel_va_to_pa(allocator.next);
    mark_pages_in_use(start, end - start);
    */
}

/* VirtualAlloc is a page granule allocator that manages a given virtual region
 * and provides virtually contiguous allocations inside that region.
 * This allocator explicitly has no dependency on the heap and retrieves
 * all its backing memory directly from the pmm. To achieve this it maps pages
 * directly into the hardware page tables via the arch aspace, and consequently
 * assumes that these operations will only depend on the pmm and not the heap
 * for allocating any intermediate page tables. */

/* This class is thread-unsafe. */
pub struct VirtualAlloc {
    allocated_page_state: vm_page_state_t,

    /* Record of that padding to be applied to every allocation. */
    alloc_guard: usize,

    alloc_base: vaddr_t,
    /* Heuristic used to attempt to begin searching
     * for a free run in bitmap_ at an optimal point. */
    next_search_start: usize,
    align_log2: usize,
    bitmap: Bitmap,
}

impl VirtualAlloc {
    const fn new(allocated_page_state: vm_page_state_t) -> Self {
        Self {
            allocated_page_state,
            alloc_guard: 0,
            alloc_base: 0,
            next_search_start: 0,
            align_log2: 0,
            bitmap: Bitmap::new(),
        }
    }

    fn init(&mut self, base: vaddr_t, size: usize, alloc_guard: usize,
            align_log2: usize) -> Result<(), ErrNO> {

        if self.alloc_base != 0 {
            /* This has already been initialized. */
            return Err(ErrNO::BadState);
        }

        if align_log2 < PAGE_SHIFT {
            return Err(ErrNO::InvalidArgs);
        }
        self.align_log2 = align_log2;

        let vaddr_align = 1 << self.align_log2;

        if size == 0 || !IS_ALIGNED!(size, vaddr_align) ||
            !IS_ALIGNED!(base, vaddr_align) || base + size < base {
            return Err(ErrNO::InvalidArgs);
        }

        /* Work how how many pages we need for the bitmap. */
        let total_pages = size / PAGE_SIZE;
        let bits_per_page = PAGE_SIZE * BYTE_BITS;
        let bitmap_pages = ROUNDUP!(total_pages, bits_per_page) / bits_per_page;

        /* Validate that there will be anything left after allocating
         * the bitmap for an actual allocation.
         * A single allocation needs padding on both sides of it.
         * This ignores alignment problems caused by the bitmap,
         * and so it's still possible for non page size alignments that
         * if this check passes that no allocations are possible,
         * but this is not meant to be an exhaustive guard. */
        if bitmap_pages + alloc_guard * 2 >= total_pages {
            return Err(ErrNO::InvalidArgs);
        }
        /* Allocate and map the bitmap pages into the start of the range
         * we were given. */
        self.alloc_map_pages(base, bitmap_pages)?;

        self.bitmap.storage_init(base, bitmap_pages * PAGE_SIZE);

        /* Initialize the bitmap, reserving its own pages. */
        self.alloc_base = base;
        self.bitmap.init(total_pages);
        self.bitmap.set(0, bitmap_pages)?;

        /* Set our first search to happen after the bitmap. */
        self.next_search_start = bitmap_pages;

        self.alloc_guard = alloc_guard;

        Ok(())
    }

    fn alloc_map_pages(&self, va: vaddr_t, num_pages: usize)
        -> Result<(), ErrNO> {

        let mmu_flags = ARCH_MMU_FLAG_CACHED |
            ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_WRITE;

        ZX_ASSERT!(num_pages > 0);

        let align_pages = 1 << (self.align_log2 - PAGE_SHIFT);
        ZX_ASSERT!(align_pages > 1);

        let mut alloc_pages = List::<vm_page_t>::new();
        alloc_pages.init();

        let mut mapped_count = 0;
        while mapped_count + align_pages <= num_pages {
            let mut contiguous_pages = List::<vm_page_t>::new();
            contiguous_pages.init();
            /* Being in this path we know that align_pages is >1,
             * which can only happen if our align_log_2 is greater than
             * the system PAGE_SIZE_SHIFT. As such we need to allocate
             * multiple contiguous pages at a greater than system page size
             * alignment, and so we must use the general
             * pmm_alloc_contiguous. */
            let mut pa: paddr_t = 0;
            pmm_alloc_contiguous(align_pages, 0, self.align_log2,
                                 &mut pa, &mut contiguous_pages)?;
            panic!("mapped_count + align_pages <= num_pages");
        }

        if mapped_count == num_pages {
            return Ok(());
        }

        /* Allocate any remaining pages. */
        let mut remaining_pages = List::<vm_page_t>::new();
        remaining_pages.init();
        pmm_alloc_pages(num_pages - mapped_count, 0, &mut remaining_pages)?;

        /* Place them specifically at the end of any already allocated pages.
         * This ensures that if we should iterate too far we will hit
         * a null page and not one of our contiguous pages to ensure we can
         * never attempt to map something twice.
         * Due to how list_node's work this does not affect the current_page
         * pointer we already retrieved. */
        alloc_pages.splice(&mut remaining_pages);

        let mut page = alloc_pages.head();

        while mapped_count < num_pages {
            let mut paddrs: [usize; BATCH_PAGES] = [0; BATCH_PAGES];
            let map_pages = min(BATCH_PAGES, num_pages - mapped_count);
            ZX_ASSERT!(map_pages > 0);
            for i in 0..BATCH_PAGES {
                ZX_ASSERT!(page != null_mut());
                unsafe {
                    (*page).set_state(self.allocated_page_state);
                    paddrs[i] = (*page).paddr();
                    page = (*page).next();
                    if page == alloc_pages.node() {
                        break;
                    }
                }
            }

            unsafe {
                let mapped = (*BOOT_CONTEXT.data.get()).get_aspace_by_id(0).map(
                    va + mapped_count * PAGE_SIZE, &paddrs[..], map_pages,
                    mmu_flags, ExistingEntryAction::Error)?;
                ZX_ASSERT!(mapped == map_pages);
            }

            mapped_count += map_pages;
        }

        Ok(())
    }

    pub fn alloc_pages(&mut self, pages: usize) -> Result<vaddr_t, ErrNO> {
        if self.alloc_base == 0 {
            return Err(ErrNO::BadState);
        }

        if pages == 0 {
            return Err(ErrNO::InvalidArgs);
        }

        /* Allocate space from the bitmap, it will set the bits and
         * ensure padding is left around the allocation. */
        let start = self.bitmap_alloc(pages)?;

        /* Turn the bitmap index into a virtual address and
         * allocate the pages there. */
        let vstart = self.alloc_base + start * PAGE_SIZE;
        self.alloc_map_pages(vstart, pages)?;

        Ok(vstart)
    }

    pub fn free_pages(&mut self, vaddr: vaddr_t, pages: usize)
        -> Result<(), ErrNO> {
        ZX_ASSERT!(self.alloc_base != 0);
        ZX_ASSERT!(pages > 0);
        ZX_ASSERT!(IS_PAGE_ALIGNED!(vaddr));

        dprintf!(INFO, "Free {} pages at {:x}\n", pages, vaddr);
        /* Release the bitmap range prior to unmapping to ensure any attempts
         * to free an invalid range are caught before attempting to
         * unmap 'random' memory. */
        self.bitmap_free((vaddr - self.alloc_base) / PAGE_SIZE, pages)?;
        self.unmap_free_pages(vaddr, pages)
    }

    fn bitmap_free(&mut self, start: usize, num_pages: usize)
        -> Result<(), ErrNO> {
        let mut _dummy: usize = 0;
        ZX_ASSERT!(start >= self.bitmap_pages());
        ZX_ASSERT!(self.bitmap.scan(start, start + num_pages, true, &mut _dummy));

        self.bitmap.clear(start, start + num_pages)?;

        if start < self.next_search_start {
            self.next_search_start = start;
            /* To attempt to keep allocations compact check alloc_guard_ bits
             * backwards, and move our search start if unset. This ensures that
             * if we alloc+free that our search_start_ gets reset to
             * the original location, otherwise it will constantly creep by alloc_guard_. */
            if self.next_search_start >= self.alloc_guard {
                let mut candidate: usize = 0;
                if self.bitmap.reverse_scan(self.next_search_start - self.alloc_guard,
                                            self.next_search_start, false,
                                            & mut candidate) {
                    dprintf!(INFO, "Reverse scan moved search from {} all the way to {}\n",
                             self.next_search_start, self.next_search_start - self.alloc_guard);
                    self.next_search_start -= self.alloc_guard;
                } else {
                    dprintf!(INFO, "Reverse scan moved search from {} part way to {}\n",
                             self.next_search_start, candidate + 1);
                    self.next_search_start = candidate + 1;
                }
            }
        }
        Ok(())
    }

    fn unmap_free_pages(&self, va: vaddr_t, pages: usize) -> Result<(), ErrNO> {
        let mut free_list = List::<vm_page_t>::new();
        free_list.init();
        dprintf!(INFO, "Unmapping {} pages at 0x{:x}\n", pages, va);
        for i in 0..pages {
            let (pa, _) = BOOT_CONTEXT.kernel_aspace().query(va + i * PAGE_SIZE)?;
            let page = paddr_to_vm_page(pa);
            free_list.add_tail(page);
        }
        let unmapped = BOOT_CONTEXT.kernel_aspace().unmap(va, pages, false)?;
        ZX_ASSERT!(unmapped == pages);
        pmm_free(&free_list);

        todo!("unmap_free_pages!");
    }

    fn bitmap_alloc(&mut self, num_pages: usize) -> Result<vaddr_t, ErrNO> {
        /* First search from our saved recommended starting location. */
        let ret = self.bitmap_alloc_range(num_pages,
            self.next_search_start, self.bitmap.size());
        ret.or_else(
            /* Try again from the beginning (skipping the bitmap itself).
             * Still search to the end just in case the original search start was
             * in the middle of a free run. */
            |_| self.bitmap_alloc_range(num_pages,
                self.bitmap_pages(), self.bitmap.size())
        )
    }

    fn bitmap_alloc_range(&mut self, num_pages: usize, start: usize, end: usize)
        -> Result<vaddr_t, ErrNO> {
        ZX_ASSERT!(end >= start);
        ZX_ASSERT!(num_pages > 0);
        let align_pages: usize = 1 << (self.align_log2 - PAGE_SHIFT);
        /* Want to find a run of num_pages + padding on either end.
         * By over-searching we can ensure there is always
         * alloc_guard_ unused pages / unset-bits between each allocation. */
        let find_pages: usize = num_pages + self.alloc_guard * 2;

        /* If requested less pages than the alignment then do not bother
         * finding an aligned range, just find anything.
         * The assumption here is that the block of pages we map in later
         * will not be large enough to benefit from any alignment,
         * so might as well avoid fragmentation and do a more efficient search. */
        if num_pages >= align_pages && align_pages > 1 {
            todo!("num_pages >= align_pages && align_pages > 1");
        }

        /* See if there's an unaligned range that will satisfy. */
        let mut alloc_start = self.bitmap.find(false, start, end, find_pages)?;

        /* Increase our start to skip the padding we want to leave. */
        alloc_start += self.alloc_guard;
        /* Record the end of this allocation as our next search start.
         * We set the end to not include the padding so that the padding
         * at the end of this allocation becomes the padding at the start
         * of the next one. */
        self.next_search_start = alloc_start + num_pages;
        /* Set the bits for the 'inner' allocation,
         * leaving the padding we found unset. */
        self.bitmap.set(alloc_start, alloc_start + num_pages)?;

        Ok(alloc_start)
    }

    pub fn bitmap_pages(&self) -> usize {
        self.bitmap.storage_num() * BYTES_PER_USIZE / PAGE_SIZE
    }

}

pub fn heap_init() -> Result<(), ErrNO> {
    unsafe {
        (*BOOT_CONTEXT.data.get()).virtual_alloc =
            Some(VirtualAlloc::new(vm_page_state::HEAP));
    }

    let virtual_alloc = BOOT_CONTEXT.virtual_alloc();
    virtual_alloc.init(vm_get_kernel_heap_base(), vm_get_kernel_heap_size(),
                       1, ARCH_HEAP_ALIGN_BITS)?;

    dprintf!(INFO, "Kernel heap [{:x}, {:x}) using {} pages ({} KiB) \
             for tracking bitmap\n",
             vm_get_kernel_heap_base(),
             vm_get_kernel_heap_base() + vm_get_kernel_heap_size(),
             virtual_alloc.bitmap_pages(),
             virtual_alloc.bitmap_pages() * PAGE_SIZE / 1024);

    cmpct_init()
}
