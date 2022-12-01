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
use crate::stdio::STDOUT;

global_asm!(include_str!("arch/riscv64/start.S"));

#[path = "arch/riscv64/mod.rs"]
mod arch;

mod config_generated;
mod types;
mod defines;
mod errors;
mod stdio;

#[no_mangle]
fn lk_main() -> ! {
    println!("Hello, {}! [{}]", "world", 9);
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}
