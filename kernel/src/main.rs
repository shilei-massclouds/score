/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::arch::global_asm;
use crate::arch::sbi::console_putchar;

global_asm!(include_str!("arch/riscv64/start.S"));

#[path = "arch/riscv64/mod.rs"]
mod arch;

#[no_mangle]
fn lk_main() -> ! {
    console_putchar('H');
    console_putchar('i');
    console_putchar('!');
    console_putchar('\n');
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
