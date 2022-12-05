/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::panic::PanicInfo;
use crate::arch::sbi::machine_power_off;
use crate::println;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);

    /* Power off on panic */
    machine_power_off();
    loop {}
}

#[macro_export]
macro_rules! ZX_DEBUG_ASSERT {
    ($expr: expr) => (assert!($expr));
}
