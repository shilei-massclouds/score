/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::ptr::null_mut;

use crate::ZX_ASSERT;
use crate::config_generated::_CONFIG_NR_CPUS;
use crate::locking::mutex::Mutex;
use crate::thread::{Thread, thread_construct_first};
use crate::sched::Scheduler;

pub const BOOT_CPU_ID: usize = 0;

pub struct PerCPU {
    idle_thread: Thread,
    scheduler: Scheduler,
}

impl PerCPU {
    pub fn init(&mut self) {
        self.scheduler = Scheduler::new();
        self.idle_thread = Thread::new();
    }

    pub fn idle_thread_ptr(&mut self) -> *mut Thread {
        &mut self.idle_thread as *mut Thread
    }

    pub fn init_boot() {
        let mut percpu_array = unsafe { PERCPU_ARRAY.lock() };
        let boot_percpu = percpu_array.get(BOOT_CPU_ID);
        boot_percpu.scheduler.this_cpu = BOOT_CPU_ID;
        let t = boot_percpu.idle_thread_ptr();

        /* create a thread to cover the current running state */
        thread_construct_first(t, "bootstrap");
    }

    pub fn scheduler(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }
}

type PerCPUPtr = *mut PerCPU;

pub struct PerCPUArray {
    data: [PerCPUPtr; _CONFIG_NR_CPUS],
}

impl PerCPUArray {
    const fn new() -> Self {
        Self {
            data: [null_mut(); _CONFIG_NR_CPUS],
        }
    }

    pub fn get(&mut self, index: usize) -> &mut PerCPU {
        let ptr = self.data[index];
        ZX_ASSERT!(!ptr.is_null());
        unsafe { &mut (*ptr) }
    }

    pub fn set(&mut self, index: usize, percpu_ptr: PerCPUPtr) {
        self.data[index] = percpu_ptr;
    }
}

pub static mut PERCPU_ARRAY: Mutex<PerCPUArray> =
    Mutex::new(PerCPUArray::new());