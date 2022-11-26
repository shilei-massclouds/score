/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

use core::arch::asm;

/* Legacy Extensions (EIDs 0x00 - 0x0F) */
const SBI_SET_TIMER         : usize = 0x0;
const SBI_CONSOLE_PUTCHAR   : usize = 0x1;
const SBI_CONSOLE_GETCHAR   : usize = 0x2;
const SBI_CLEAR_IPI         : usize = 0x3;
const SBI_SEND_IPI          : usize = 0x4;
const SBI_REMOTE_FENCE_I    : usize = 0x5;
const SBI_REMOTE_SFENCE_VMA : usize = 0x6;
const SBI_REMOTE_SFENCE_VMA_ASID: usize = 0x7;
const SBI_SHUTDOWN          : usize = 0x8;

const SBI_HSM : usize = 0x48534D;

#[inline(always)]
fn sbi_call(eid: usize, fid: usize,
            arg0: usize, arg1: usize, arg2: usize)
    -> (usize, usize) {
    let ret0;
    let ret1;
    unsafe {
        asm!(
            "ecall",
            in("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            in("a6") fid,
            in("a7") eid,
            lateout("a0") ret0,
            lateout("a1") ret1,
        );
    }
    (ret0, ret1)
}

pub fn console_putchar(ch: char) {
    sbi_call(SBI_CONSOLE_PUTCHAR, 0, ch as usize, 0, 0);
}
