/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

extern crate alloc;

use crate::arch::sbi;
use core::ptr::null_mut;
use alloc::alloc::{GlobalAlloc, Layout};

pub struct DummyAllocator;

unsafe impl GlobalAlloc for DummyAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        sbi::console_putchar('3');
        sbi::console_putchar('\n');
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        sbi::console_putchar('4');
        sbi::console_putchar('\n');
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout);
}

#[global_allocator]
static ALLOCATOR: DummyAllocator = DummyAllocator;
