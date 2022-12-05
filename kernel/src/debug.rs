/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

/* debug print levels */
pub const CRITICAL  : u32 = 0;
pub const ALWAYS    : u32 = 0;
pub const INFO      : u32 = 1;
pub const SPEW      : u32 = 2;

pub const DEBUG_PRINT_LEVEL: u32 = SPEW;

#[macro_export]
macro_rules! dprintf {
    ($level: expr, $($arg:tt)*) => (
        if $level <= DEBUG_PRINT_LEVEL {
            print!($($arg)*);
        }
    );
}
