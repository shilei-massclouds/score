/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::types::*;

#[allow(dead_code)]
pub fn memset(va: vaddr_t, value: u8, size: usize) {
    let ptr = va as *mut u8;
    for _i in 0..size {
        unsafe { *ptr = value; }
    }
}