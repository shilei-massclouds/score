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

use core::arch::global_asm;
use alloc::string::String;
use crate::debug::*;
use crate::allocator::init_heap;
use crate::errors::ErrNO;
use crate::defines::*;
use crate::platform::platform_early_init;

global_asm!(include_str!("arch/riscv64/start.S"));

extern crate alloc;

#[path = "arch/riscv64/mod.rs"]
mod arch;

#[path = "platform/riscv/platform.rs"]
mod platform;

#[macro_use]
mod align;

#[macro_use]
mod debug;

#[macro_use]
mod stdio;

mod panic;
mod config_generated;
mod types;
mod defines;
mod errors;
mod allocator;

#[no_mangle]
fn lk_main() -> ! {
    if let Err(e) = _lk_main() {
        panic!("Fatal: {:?}", e);
    };

    panic!("Never Reach Here!");
}

#[no_mangle]
fn _lk_main() -> Result<(), ErrNO> {
    /* get us into some sort of thread context so Thread::Current works. */
    thread_init_early();

    jtrace_init();

    /* bring the debuglog up early so we can safely printf */
    dlog_init_early();

    /* deal with any static constructors */
    call_constructors();

    /* we can safely printf now since we have the debuglog,
     * the current thread set which holds (a per-line buffer),
     * and global ctors finished (some of the printf machinery
     * depends on ctors right now). */
    dprintf!(ALWAYS, "printing enabled\n");

    /*
    lk_primary_cpu_init_level(LK_INIT_LEVEL_EARLIEST, LK_INIT_LEVEL_ARCH_EARLY);
    */

    /*
     * Carry out any early architecture-specific and platform-specific init
     * required to get the boot CPU and platform into a known state.
     */
    arch_early_init();

    /*
    lk_primary_cpu_init_level(LK_INIT_LEVEL_ARCH_EARLY,
                              LK_INIT_LEVEL_PLATFORM_EARLY);
                              */

    /* At this point the physmap is available. */
    dtb_from_phys();
    ZX_DEBUG_ASSERT!(dtb_pa() != 0);

    platform_early_init()?;

    // DriverHandoffEarly(*gPhysHandoff);
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_PLATFORM_EARLY,
    //                           LK_INIT_LEVEL_ARCH_PREVM - 1);

    /* At this point, the kernel command line and serial are set up. */

    dprintf!(INFO, "\nwelcome to sCore\n\n");
    dprintf!(SPEW, "KASLR: .text section at 0x{:x}\n", kernel_base_phys());

    /* Perform any additional arch and platform-specific set up
     * that needs to be done before virtual memory or the heap are set up. */
    dprintf!(SPEW, "initializing arch pre-vm\n");
    // arch_prevm_init();
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_ARCH_PREVM,
    //                           LK_INIT_LEVEL_PLATFORM_PREVM - 1);
    dprintf!(SPEW, "initializing platform pre-vm\n");
    // platform_prevm_init();
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_PLATFORM_PREVM,
    //                           LK_INIT_LEVEL_VM_PREHEAP - 1);

    /* perform basic virtual memory setup */
    dprintf!(SPEW, "initializing vm pre-heap\n");
    vm_init_preheap();
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_VM_PREHEAP,
    //                           LK_INIT_LEVEL_HEAP - 1);

    ///////////////////////////

    println!("lk_main ...");
    init_heap();
    let s = String::from("Test");
    println!("string: {}", s);
    println!("Hello, {}! [{}]", "world", 9);

    Ok(())
}

/* get us into some sort of thread context so Thread::Current works. */
fn thread_init_early() {
}

fn jtrace_init() {
}

/* bring the debuglog up early so we can safely printf */
fn dlog_init_early() {
}

/* deal with any static constructors */
fn call_constructors() {
}

fn arch_early_init() {
}

fn dtb_from_phys() {
    dprintf!(ALWAYS, "kernel image phys [{:x}, {:x}] dtb_phys: {:x} ... \n",
             kernel_base_phys(), kernel_size(), dtb_pa());
}

fn vm_init_preheap() {
}
