/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::{print, dprintf};
use crate::debug::*;
use crate::types::*;
use crate::defines::*;
use crate::errors::ErrNO;

pub fn platform_early_init() -> Result<(), ErrNO> {
    /* initialize the boot memory reservation system */
    boot_reserve_init(kernel_base_phys(), kernel_size())
}

fn boot_reserve_init(pa: paddr_t, len: usize) -> Result<(), ErrNO> {
    /* add the kernel to the boot reserve list */
    boot_reserve_add_range(pa, len)
}

fn boot_reserve_add_range(pa: usize, len: usize) -> Result<(), ErrNO> {
    dprintf!(INFO, "PMM: boot reserve add [0x{:x}, 0x{:x}]\n",
             pa, pa + len - 1);
    Ok(())
}
