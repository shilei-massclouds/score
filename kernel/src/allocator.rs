/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use spin::{Mutex, MutexGuard};
use crate::vm_page_state::{self, *};
use crate::defines::{_boot_heap, _boot_heap_end};
use crate::ARCH_HEAP_ALIGN_BITS;
use crate::aspace::{vm_get_kernel_heap_base, vm_get_kernel_heap_size};
use crate::{ErrNO, PAGE_SHIFT, PAGE_SIZE, CHAR_BITS};
use crate::types::*;

extern crate alloc;

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

// VirtualAlloc is a page granule allocator that manages a given virtual region and provides
// virtually contiguous allocations inside that region. This allocator explicitly has no dependency
// on the heap and retrieves all its backing memory directly from the pmm. To achieve this it maps
// pages directly into the hardware page tables via the arch aspace, and consequently assumes that
// these operations will only depend on the pmm and not the heap for allocating any intermediate
// page tables.
//
// This class is thread-unsafe.
struct VirtualAlloc {
    allocated_page_state: vm_page_state_t,
    alloc_base: vaddr_t,
    align_log2: usize,
}

impl VirtualAlloc {
    const fn new(allocated_page_state: vm_page_state_t) -> Self {
        Self {
            allocated_page_state,
            alloc_base: 0,
            align_log2: 0,
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
        let bits_per_page = PAGE_SIZE * CHAR_BITS;
        let bitmap_pages = ROUNDUP!(total_pages, bits_per_page) / bits_per_page;

        /* Validate that there will be anything left after allocating
         * the bitmap for an actual allocation.
         * A single allocation needs padding on both sides of it.
         * This ignores alignment problems caused by the bitmap,
         * and so it's still possible for non page size alignments that
         * if this check passes that no allocations are possible,
         * but this is not meant to be an exhaustive guard. */
        if (bitmap_pages + alloc_guard * 2 >= total_pages) {
            return Err(ErrNO::InvalidArgs);
        }
        todo!("NOW!\n");
        /*
  // Allocate and map the bitmap pages into the start of the range we were given.
  zx_status_t status = AllocMapPages(base, bitmap_pages);
  if (status != ZX_OK) {
    return status;
  }
  bitmap_.StorageUnsafe()->Init(reinterpret_cast<void *>(base), bitmap_pages * PAGE_SIZE);

  // Initialize the bitmap, reserving its own pages.
  alloc_base_ = base;
  bitmap_.Reset(total_pages);
  bitmap_.Set(0, bitmap_pages);

  // Set our first search to happen after the bitmap.
  next_search_start_ = bitmap_pages;

  alloc_guard_ = alloc_guard;
  return ZX_OK;
*/
    }
}

static VIRTUAL_ALLOC: Mutex<VirtualAlloc> =
    Mutex::new(VirtualAlloc::new(vm_page_state::HEAP));

pub fn heap_init() -> Result<(), ErrNO> {
    let mut virtual_alloc = VIRTUAL_ALLOC.lock();
    virtual_alloc.init(vm_get_kernel_heap_base(), vm_get_kernel_heap_size(),
                       1, ARCH_HEAP_ALIGN_BITS)?;

    /*
    dprintf!(INFO, "Kernel heap [{:x}, {:x}) "
             "using {} pages ({} KiB) for tracking bitmap\n",
             vm_get_kernel_heap_base(),
             vm_get_kernel_heap_base() + vm_get_kernel_heap_size(),
             virtual_alloc.DebugBitmapPages(),
             virtual_alloc.DebugBitmapPages() * PAGE_SIZE / 1024);
             */
    Ok(())

    /*
  cmpct_init();
  */
}
