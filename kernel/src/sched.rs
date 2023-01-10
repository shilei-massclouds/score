/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::ptr::null_mut;
use crate::debug::*;

use crate::thread::Thread;
use crate::arch::smp::arch_curr_cpu_num;
use crate::cpu::{cpu_num_t, cpu_mask_t, INVALID_CPU, CPU_MASK_ALL, cpu_num_to_mask};
use crate::percpu::PERCPU_ARRAY;

type SchedWeight = usize;
type SchedDuration = usize;
type SchedPerformanceScale = usize;

macro_rules! ZX_MSEC {
    ($n: expr) => { (1000000usize * $n) }
}

const fn sched_ms(milliseconds: usize) -> SchedDuration {
    ZX_MSEC!(milliseconds)
}

/* Default minimum granularity of time slices. */
const K_DEFAULT_MINIMUM_GRANULARITY: SchedDuration = sched_ms(1);

// Table of fixed-point constants converting from kernel priority to fair
// scheduler weight.
const K_PRIORITY_TO_WEIGHT_TABLE: [SchedWeight; 32] = [
    121,   149,   182,   223,   273,   335,   410,   503,   616,   754,  924,
    1132,  1386,  1698,  2080,  2549,  3122,  3825,  4685,  5739,  7030, 8612,
    10550, 12924, 15832, 19394, 23757, 29103, 35651, 43672, 53499, 65536
];

// Converts from kernel priority value in the interval [0, 31] to weight in the
// interval (0.0, 1.0]. See the definition of SchedWeight for an explanation of
// the weight distribution.
const fn priority_to_weight(priority: usize) -> SchedWeight {
    K_PRIORITY_TO_WEIGHT_TABLE[priority]
}

struct SchedFairParams {
    weight: SchedWeight,
}

impl SchedFairParams {
    const fn new(weight: SchedWeight) -> Self {
        Self {
            weight,
        }
    }
}

struct SchedDeadlineParams {

}

// Specifies the type of scheduling algorithm applied to a thread.
enum SchedDiscipline {
    None,
    Fair(SchedFairParams),
    _Deadline(SchedDeadlineParams),
}

enum ThreadState {
    ThreadInitial,
    _ThreadReady,
    ThreadRunning,
    _ThreadBlocked,
    _ThreadBlockedReadLock,
    _ThreadSleeping,
    _ThreadSuspended,
    _ThreadDeath,
}

pub struct SchedulerState {
    base_priority: usize,
    effective_priority: usize,
    inherited_priority: i32,
    expected_runtime_ns: SchedDuration,
    discipline: SchedDiscipline,
    pub active: bool,    /* whether thread is associated with a run queue. */
    state: ThreadState,  /* The scheduling state of the thread. */
    curr_cpu: cpu_num_t, /* The current CPU the thread is READY or RUNNING on */
    last_cpu: cpu_num_t, /* The last CPU the thread ran on. */
    next_cpu: cpu_num_t, /* The next CPU the thread should run on
                          * after the thread's migrate function is called */
    hard_affinity: cpu_mask_t, /* The set of CPUs the thread is permitted to
                                * run on. The thread is never assigned to
                                * CPUs outside of this set. */
}

impl SchedulerState {
    pub const fn new() -> Self {
        Self {
            base_priority: 0,
            effective_priority: 0,
            inherited_priority: 0,
            expected_runtime_ns: 0,
            discipline: SchedDiscipline::None,
            active: false,
            state: ThreadState::ThreadInitial,
            curr_cpu: INVALID_CPU,
            last_cpu: INVALID_CPU,
            next_cpu: INVALID_CPU,
            hard_affinity: CPU_MASK_ALL,
        }
    }

    fn set_discipline(&mut self, discipline: SchedDiscipline) {
        self.discipline = discipline;
    }
}

pub struct Scheduler {
    pub this_cpu: usize,
    /* thread actively running on this CPU. */
    pub active_thread: *mut Thread,
    /* Total weights of threads running on this CPU, including threads
     * in the run queue and the currently running thread.
     * Does not include the idle thread. */
    pub weight_total: SchedWeight,
    /* Count of the fair threads running on this CPU, including threads
     * in the run queue and the currently running thread.
     * Does not include the idle thread. */
    pub runnable_fair_task_count: i32,
    /* The sum of the expected runtimes of all active threads on this CPU.
     * This value is an estimate of the average queuimg time for this CPU,
     * given the current set of active threads. */
    pub total_expected_runtime_ns: SchedDuration,
    pub exported_total_expected_runtime_ns: SchedDuration,

    /* Performance scale of this CPU relative to the highest performance CPU.
     * This value is initially determined from the system topology,
     * when available, and by userspace performance/thermal management
     * at runtime. */
    _performance_scale: SchedPerformanceScale,
    performance_scale_reciprocal: SchedPerformanceScale,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            this_cpu: 0,
            active_thread: null_mut(),
            weight_total: 0,
            runnable_fair_task_count: 0,
            total_expected_runtime_ns: 0,
            exported_total_expected_runtime_ns: 0,
            _performance_scale: 1,
            performance_scale_reciprocal: 1,
        }
    }

    pub fn init_first_thread(thread: *mut Thread) {
        let current_cpu = arch_curr_cpu_num();

        /* Construct our scheduler state and assign a "priority" */
        Self::init_thread(thread, Thread::HIGHEST_PRIORITY);

        /* Fill out other details about the thread, making sure to assign it to
         * the current CPU with hard affinity. */
        let ss = unsafe { (*thread).sched_state() };
        ss.state = ThreadState::ThreadRunning;
        ss.curr_cpu = current_cpu;
        ss.last_cpu = current_cpu;
        ss.next_cpu = INVALID_CPU;
        ss.hard_affinity = cpu_num_to_mask(current_cpu);

        /* Finally, make sure that the thread is the active thread
         * for the scheduler, and that the weight_total bookkeeping
         * is accurate. */
        {
            let mut percpu_array = unsafe { PERCPU_ARRAY.lock() };
            let percpu = percpu_array.get(current_cpu);
            let sched = percpu.scheduler();
            ss.active = true;
            sched.active_thread = thread;
            if let SchedDiscipline::Fair(params) = &ss.discipline {
                sched.weight_total = params.weight;
            } else {
                panic!("Bad discipline! Only support fair!");
            }
            sched.runnable_fair_task_count += 1;
            sched.update_total_expected_runtime(ss.expected_runtime_ns);
        }
    }

    pub fn init_thread(thread: *mut Thread, priority: usize) {
        let weight = priority_to_weight(priority);
        let sched_state = unsafe { &mut (*thread).sched_state };
        let discipline = SchedDiscipline::Fair(SchedFairParams::new(weight));
        sched_state.set_discipline(discipline);
        sched_state.base_priority = priority;
        sched_state.effective_priority = priority;
        sched_state.inherited_priority = -1;
        sched_state.expected_runtime_ns = K_DEFAULT_MINIMUM_GRANULARITY;
    }

    /* Updates the total expected runtime estimator with the given delta.
     * The exported value is scaled by the relative performance factor of
     * the CPU to account for performance differences in the estimate. */
    fn update_total_expected_runtime(&mut self, delta_ns: SchedDuration) {
        self.total_expected_runtime_ns += delta_ns;
        //ZX_ASSERT!(self.total_expected_runtime_ns >= 0);
        let scaled_ns: SchedDuration = self.scale_up(self.total_expected_runtime_ns);
        self.exported_total_expected_runtime_ns = scaled_ns;
        dprintf!(INFO, "Est Load {} cpu: {}\n", scaled_ns, self.this_cpu);
    }

    /* Scales the given value up by the reciprocal of
     * the CPU performance scale. */
    fn scale_up(&self, value: SchedDuration) -> SchedDuration {
        value * self.performance_scale_reciprocal()
    }

    /* the reciprocal performance scale of the CPU this scheduler instance
     * is associated with. */
    fn performance_scale_reciprocal(&self) -> SchedPerformanceScale {
        self.performance_scale_reciprocal
    }
}