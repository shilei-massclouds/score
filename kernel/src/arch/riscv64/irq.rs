/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::arch::asm;

use super::csr::SR_IE;

/* read interrupt enabled status */
#[inline]
pub fn arch_local_save_flags() -> usize {
    let flags: usize;
    unsafe {
        asm!(
            "csrr {0}, sstatus",
            out(reg) flags,
        );
    }
    flags
}

/* test flags */
#[inline]
pub fn arch_irqs_disabled_flags(flags: usize) -> bool {
    (flags & SR_IE) == 0
}

/* test hardware interrupt enable bit */
#[inline]
pub fn arch_irqs_disabled() -> bool {
    arch_irqs_disabled_flags(arch_local_save_flags())
}