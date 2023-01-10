/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::{config_generated::_CONFIG_NR_CPUS, locking::mutex::Mutex, thread::{Thread, thread_construct_first, thread_set_current}, sched::Scheduler};

const BOOT_CPU_ID: usize = 0;

pub struct PerCPU {
    scheduler: Scheduler,
    idle_thread: Thread,
}

impl PerCPU {
    const INIT_VAL: PerCPU = PerCPU::new();

    const fn new() -> Self {
        Self {
            scheduler: Scheduler::new(),
            idle_thread: Thread::new(),
        }
    }

    pub fn idle_thread_ptr(&mut self) -> *mut Thread {
        &mut self.idle_thread as *mut Thread
    }

    pub fn init_boot() {
        let mut percpu_array = unsafe { PERCPU_ARRAY.lock() };
        let boot_percpu = percpu_array.get(BOOT_CPU_ID);
        boot_percpu.scheduler.this_cpu = BOOT_CPU_ID;

        let t = boot_percpu.idle_thread_ptr();
        unsafe {
            (*t).thread_info.cpu = BOOT_CPU_ID;
        }
        thread_set_current(t as usize);

        /* create a thread to cover the current running state */
        thread_construct_first(t, "bootstrap");
    }

    pub fn scheduler(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }
}

pub struct PerCPUArray {
    data: [PerCPU; _CONFIG_NR_CPUS],
}

impl PerCPUArray {
    const fn new() -> Self {
        Self {
            data: [PerCPU::INIT_VAL; _CONFIG_NR_CPUS],
        }
    }

    pub fn get(&mut self, index: usize) -> &mut PerCPU {
        &mut self.data[index]
    }
}

pub static mut PERCPU_ARRAY: Mutex<PerCPUArray> =
    Mutex::new(PerCPUArray::new());