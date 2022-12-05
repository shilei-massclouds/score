/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

//use crate::{print, dprintf};
//use crate::debug::*;
//use crate::types::*;
use crate::defines::*;
use crate::errors::ErrNO;
use crate::platform::boot_reserve::boot_reserve_init;

mod boot_reserve;

pub fn platform_early_init() -> Result<(), ErrNO> {
    /* initialize the boot memory reservation system */
    boot_reserve_init(kernel_base_phys(), kernel_size())
}
