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
use crate::defines::{_boot_heap, _boot_heap_end};

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
