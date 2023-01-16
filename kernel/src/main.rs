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
#![feature(const_mut_refs)]
#![feature(const_nonnull_new)]
#![feature(negative_impls)]

use core::arch::global_asm;
use core::cell::UnsafeCell;
use alloc::vec::Vec;
use allocator::VirtualAlloc;
use klib::cmpctmalloc::Heap;
use page::vm_page_t;
use platform::boot_reserve::BootReserveRange;
use platform::periphmap::PeriphRange;
use pmm::PMM_NODE;
use stdio::StdOut;
use thread::ThreadArg;

use crate::arch::topology::topology_init;
use crate::debug::*;
use crate::allocator::boot_heap_earliest_init;
use crate::errors::ErrNO;
use crate::defines::*;
use crate::mp::mp_init;
use crate::platform::platform_early_init;
use crate::aspace::vm_init_preheap;
use crate::klib::list::List;
use crate::allocator::heap_init;
use crate::thread::{thread_init_early, Thread};
use crate::vm::vm::vm_init;

global_asm!(include_str!("arch/riscv64/start.S"));

extern crate alloc;

#[path = "arch/riscv64/mod.rs"]
mod arch;

#[path = "platform/riscv/mod.rs"]
mod platform;

#[macro_use]
mod align;

#[macro_use]
mod debug;

#[macro_use]
mod stdio;

#[cfg(feature = "unittest")]
mod tests;

mod panic;
mod config_generated;
mod types;
mod defines;
mod errors;
mod klib;
mod allocator;
mod pmm;
mod page;
mod vm_page_state;
mod aspace;
mod vm;
mod mp;
mod thread;
mod init;
mod locking;
mod percpu;
mod sched;
mod cpu;

pub struct BootContext {
    reserve_ranges: Vec::<BootReserveRange>,
    periph_ranges: Vec::<PeriphRange>,
    reserved_page_list: List<vm_page_t>,
    kernel_heap_base: usize,
    kernel_heap_size: usize,
    virtual_alloc: Option<VirtualAlloc>,
    heap: Option<Heap>,
    stdout: Option<StdOut>,
}

impl BootContext {
    const fn _new() -> Self {
        Self {
            reserve_ranges: Vec::<BootReserveRange>::new(),
            periph_ranges: Vec::<PeriphRange>::new(),
            reserved_page_list: List::<vm_page_t>::new(),
            kernel_heap_base: 0,
            kernel_heap_size: 0,
            virtual_alloc: None,
            heap: None,
            stdout: Some(StdOut),
        }
    }

    fn heap(&mut self) -> &mut Heap {
        if let Some(ret) = &mut self.heap {
            return ret;
        }
        panic!("NOT init heap yet!");
    }

    fn virtual_alloc(&mut self) -> &mut VirtualAlloc {
        if let Some(ret) = &mut self.virtual_alloc {
            return ret;
        }
        panic!("NOT init virtual_alloc yet!");
    }

    fn periph_ranges(&mut self) -> &mut Vec<PeriphRange> {
        &mut self.periph_ranges
    }

    fn reserve_ranges(&mut self) -> &mut Vec<BootReserveRange> {
        &mut self.reserve_ranges
    }

    fn reserved_page_list(&mut self) -> &mut List<vm_page_t> {
        if self.reserved_page_list.is_initialized() {
            return &mut self.reserved_page_list;
        }
        panic!("NOT init reserved page list yet!");
    }

    fn stdout(&mut self) -> &mut StdOut {
        if let Some(ret) = &mut self.stdout {
            return ret;
        }
        panic!("NOT init stdout yet!");
    }

}

pub struct WrapBootContext {
    data: UnsafeCell<BootContext>,
}

unsafe impl Sync for WrapBootContext {}
unsafe impl Send for WrapBootContext {}

impl WrapBootContext {
    pub const fn new() -> Self {
        Self {
            data: UnsafeCell::new(BootContext::_new()),
        }
    }

    fn heap(&self) -> &mut Heap {
        unsafe {
            (*self.data.get()).heap()
        }
    }

    fn virtual_alloc(&self) -> &mut VirtualAlloc {
        unsafe {
            (*self.data.get()).virtual_alloc()
        }
    }

    fn periph_ranges(&self) -> &mut Vec<PeriphRange> {
        unsafe {
            (*self.data.get()).periph_ranges()
        }
    }

    fn reserve_ranges(&self) -> &mut Vec<BootReserveRange> {
        unsafe {
            (*self.data.get()).reserve_ranges()
        }
    }

    fn reserved_page_list(&self) -> &mut List<vm_page_t> {
        unsafe {
            (*self.data.get()).reserved_page_list()
        }
    }

    fn stdout(&self) -> &mut StdOut {
        unsafe {
            (*self.data.get()).stdout()
        }
    }
}

pub static BOOT_CONTEXT: WrapBootContext = WrapBootContext::new();

#[no_mangle]
fn lk_main() -> ! {
    if let Err(e) = _lk_main() {
        panic!("Fatal: {:?}", e);
    };

    panic!("Never Reach Here!");
}

#[no_mangle]
fn _lk_main() -> Result<(), ErrNO> {
    /* prepare heap for rust types (as string, vec, etc.) */
    boot_heap_earliest_init();

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
    ZX_ASSERT!(dtb_pa() != 0);

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
    vm_init_preheap()?;
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_VM_PREHEAP,
    //                           LK_INIT_LEVEL_HEAP - 1);

    /* bring up the kernel heap */
    dprintf!(SPEW, "initializing heap\n");
    heap_init()?;
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_HEAP, LK_INIT_LEVEL_VM - 1);

    // enable virtual memory
    dprintf!(SPEW, "initializing vm\n");
    vm_init()?;
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_VM, LK_INIT_LEVEL_TOPOLOGY - 1);

    // initialize the system topology
    dprintf!(SPEW, "initializing system topology\n");
    topology_init()?;
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_TOPOLOGY, LK_INIT_LEVEL_KERNEL - 1);

    // initialize other parts of the kernel
    dprintf!(SPEW, "initializing kernel\n");
    kernel_init()?;
    // lk_primary_cpu_init_level(LK_INIT_LEVEL_KERNEL, LK_INIT_LEVEL_THREADING - 1);

    // create a thread to complete system initialization
    dprintf!(SPEW, "creating bootstrap completion thread\n");
    let thread = Thread::create("bootstrap2", bootstrap2, None,
                                Thread::DEFAULT_PRIORITY)?;
    thread.detach();
    thread.resume();

    println!("lk_main ok!");

    ///////////////////////////

    /* Do unit tests */
    #[cfg(feature = "unittest")]
    crate::tests::do_tests();

    Ok(())
}

fn bootstrap2(_arg: Option<ThreadArg>) -> Result<(), ErrNO> {
    todo!("bootstrap2!");
}

fn kernel_init() -> Result<(), ErrNO> {
    dprintf!(SPEW, "initializing mp\n");
    mp_init()
}

fn jtrace_init() {
}

/* bring the debuglog up early so we can safely printf */
fn dlog_init_early() {
}

/* deal with any static constructors */
fn call_constructors() {
    unsafe {
        (*BOOT_CONTEXT.data.get()).reserved_page_list.init();
    }
    PMM_NODE.init();
}

fn arch_early_init() {
}

fn dtb_from_phys() {
    dprintf!(ALWAYS, "kernel image phys [{:x}, {:x}] dtb_phys: {:x} ... \n",
             kernel_base_phys(), kernel_size(), dtb_pa());
}
