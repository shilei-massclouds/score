/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::arch::asm;

pub unsafe fn local_flush_tlb_all() {
    asm!("sfence.vma x0, x0");
}