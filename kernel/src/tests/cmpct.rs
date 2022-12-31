/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::ptr::null_mut;
use crate::klib::cmpctmalloc::{cmpct_alloc, cmpct_free};

const PADDING_SEED: usize = 0xCDEF_0123_4567_89AB;

pub fn test_cmpct() {
    for i in 0..=32 {
        test_alloc_and_free(i as usize);
    }
    for i in 512..=528 {
        test_alloc_and_free(i as usize);
    }

    test_bundle_alloc();
}

fn test_alloc_and_free(size: usize) {
    println!(" Test: alloc and free for 0x{:x} ...", size);
    let ptr = cmpct_alloc(size);
    fill_in(ptr, size);
    check_on(ptr, size);
    cmpct_free(ptr);
    println!(" Test: alloc and free for 0x{:x} ok!\n", size);
}

fn test_bundle_alloc() {
    println!(" Test: bundle alloc ...");
    let mut ptr: [*mut u8; 16] = [null_mut(); 16];
    for i in 0..16 {
        ptr[i] = cmpct_alloc(i + 16);
        fill_in(ptr[i], i + 16);
    }
    for i in 0..16 {
        check_on(ptr[i], i + 16);
        cmpct_free(ptr[i]);
    }
    println!(" Test: bundle alloc ok!\n");
}

fn fill_in(mut ptr: *mut u8, mut size: usize) {
    let padding = (PADDING_SEED ^ size) as u64;
    while size >= 8 {
        let ptr64 = ptr as *mut u64;
        unsafe {
            (*ptr64) = padding;
            ptr = ptr.add(8);
        }
        size -= 8;
    }
    while size >= 4 {
        let ptr32 = ptr as *mut u32;
        unsafe {
            (*ptr32) = (padding & 0xFFFF_FFFF) as u32;
            ptr = ptr.add(4);
        }
        size -= 4;
    }
    while size >= 2 {
        let ptr16 = ptr as *mut u16;
        unsafe {
            (*ptr16) = (padding & 0xFFFF) as u16;
            ptr = ptr.add(2);
        }
        size -= 2;
    }
    while size >= 1 {
        let ptr8 = ptr as *mut u8;
        unsafe {
            (*ptr8) = (padding & 0xFF) as u8;
            ptr = ptr.add(1);
        }
        size -= 1;
    }
}

fn check_on(mut ptr: *mut u8, mut size: usize) {
    let padding = (PADDING_SEED ^ size) as u64;
    while size >= 8 {
        let ptr64 = ptr as *mut u64;
        unsafe {
            assert!((*ptr64) == padding);
            ptr = ptr.add(8);
        }
        size -= 8;
    }
    while size >= 4 {
        let ptr32 = ptr as *mut u32;
        unsafe {
            assert!((*ptr32) == (padding & 0xFFFF_FFFF) as u32);
            ptr = ptr.add(4);
        }
        size -= 4;
    }
    while size >= 2 {
        let ptr16 = ptr as *mut u16;
        unsafe {
            assert!((*ptr16) == (padding & 0xFFFF) as u16);
            ptr = ptr.add(2);
        }
        size -= 2;
    }
    while size >= 1 {
        let ptr8 = ptr as *mut u8;
        unsafe {
            assert!((*ptr8) == (padding & 0xFF) as u8);
            ptr = ptr.add(1);
        }
        size -= 1;
    }
}