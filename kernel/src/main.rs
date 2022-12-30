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

use core::arch::global_asm;
use core::cell::UnsafeCell;
use allocator::VirtualAlloc;
use aspace::{VmAspaceList, VmAspace};
use klib::cmpctmalloc::Heap;

use crate::debug::*;
use crate::allocator::boot_heap_earliest_init;
use crate::errors::ErrNO;
use crate::defines::*;
use crate::klib::cmpctmalloc::{cmpct_alloc, cmpct_free};
use crate::platform::platform_early_init;
use crate::pmm::PMM_NODE;
use crate::aspace::vm_init_preheap;
use crate::klib::list::List;
use crate::platform::RESERVED_PAGE_LIST;
use crate::allocator::heap_init;

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
mod klib;
mod allocator;
mod pmm;
mod page;
mod vm_page_state;
mod aspace;
mod vm;

pub struct BootContext {
    vm_aspace_list: Option<VmAspaceList>,
    kernel_heap_base: usize,
    kernel_heap_size: usize,
    virtual_alloc: Option<VirtualAlloc>,
    heap: Option<Heap>,
}

impl BootContext {
    const fn _new() -> Self {
        Self {
            vm_aspace_list: None,
            kernel_heap_base: 0,
            kernel_heap_size: 0,
            virtual_alloc: None,
            heap: None,
        }
    }

    fn get_aspace_by_id(&mut self, id: usize) -> &mut VmAspace {
        if let Some(aspaces) = &mut self.vm_aspace_list {
            return aspaces.get_aspace_by_id(id);
        }
        panic!("NOT init aspaces yet!");
    }

    fn get_heap(&mut self) -> &mut Heap {
        if let Some(ret) = &mut self.heap {
            return ret;
        }
        panic!("NOT init heap yet!");
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

    fn get_heap(&self) -> &mut Heap {
        unsafe {
            (*self.data.get()).get_heap()
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

    ///////////////////////////

    println!("lk_main ...");

    dprintf!(INFO, "#########\n");
    let ret = cmpct_alloc(16);
    dprintf!(INFO, "alloc 16: {:?}\n", ret);
    let dw0 = ret as *mut usize;
    unsafe {
        dprintf!(INFO, "ret before: {:x}\n", *dw0);
        *dw0 = 0x1234;
        dprintf!(INFO, "ret after: {:x}\n", *dw0);
    }

    cmpct_free(ret);
    dprintf!(INFO, "#########\n");
    /*
    let mut list = List::<vm_page>::new();
    list.init();
    let mut page = vm_page::new();
    page.init(0x1000);
        dprintf!(INFO, "len {:x}\n", list.len());
    list.add_tail((&mut page).into());
        dprintf!(INFO, "len {:x}\n", list.len());
    let mut page = vm_page::new();
    page.init(0x2000);
    list.add_tail((&mut page).into());
    let mut page = vm_page::new();
    page.init(0x3000);
    list.add_head((&mut page).into());
    if let Some(head) = list.head() {
        unsafe {
        dprintf!(INFO, "head {:?} {:x}\n", head, head.as_ref().paddr());
        }
    }

    for page in list.iter() {
    //for page in list.iter_mut() {
        dprintf!(INFO, "page pa {:x}\n", page.paddr());
    }
    */

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
    PMM_NODE.lock().init();
    RESERVED_PAGE_LIST.lock().init();
}

fn arch_early_init() {
}

fn dtb_from_phys() {
    dprintf!(ALWAYS, "kernel image phys [{:x}, {:x}] dtb_phys: {:x} ... \n",
             kernel_base_phys(), kernel_size(), dtb_pa());
}
