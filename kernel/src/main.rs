/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

use core::panic::PanicInfo;
use core::arch::global_asm;
use alloc::string::String;
use crate::stdio::STDOUT;
use crate::arch::sbi::machine_power_off;

global_asm!(include_str!("arch/riscv64/start.S"));

extern crate alloc;

#[path = "arch/riscv64/mod.rs"]
mod arch;

mod config_generated;
mod types;
mod defines;
mod errors;
mod stdio;
mod allocator;

#[no_mangle]
fn lk_main() -> ! {
    println!("lk_main ...");
    let s = String::from("Test");
    println!("string: {}", s);
    println!("Hello, {}! [{}]", "world", 9);
    panic!("Reach End!");
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);

    /* Power off on panic */
    machine_power_off();
    loop {}
}
