/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::arch::asm;
use core::sync::atomic::{AtomicU32, Ordering};
use alloc::string::String;

use crate::defines::ARCH_DEFAULT_STACK_SIZE;
use crate::errors::ErrNO;
use crate::klib::list::{Linked, List, ListNode};
use crate::locking::mutex::Mutex;
use crate::ZX_ASSERT;
use crate::percpu::PerCPU;
use crate::arch::irq::arch_irqs_disabled;
use crate::sched::{SchedulerState, Scheduler};

// thread priority
const NUM_PRIORITIES: usize = 32;

const _LOWEST_PRIORITY:  usize = 0;
pub const HIGHEST_PRIORITY: usize = NUM_PRIORITIES - 1;
const _DPC_PRIORITY:     usize = NUM_PRIORITIES - 2;
const _IDLE_PRIORITY:    usize = _LOWEST_PRIORITY;
const _LOW_PRIORITY:     usize = NUM_PRIORITIES / 4;
pub const _DEFAULT_PRIORITY: usize = NUM_PRIORITIES / 2;
const _HIGH_PRIORITY:    usize = (NUM_PRIORITIES / 4) * 3;

// stack size
pub const _DEFAULT_STACK_SIZE: usize = ARCH_DEFAULT_STACK_SIZE;

pub const THREAD_FLAG_DETACHED: usize = 1 << 0;
/*
#define THREAD_FLAG_FREE_STRUCT              (1 << 1)
#define THREAD_FLAG_IDLE                     (1 << 2)
#define THREAD_FLAG_VCPU                     (1 << 3)

#define THREAD_SIGNAL_KILL                   (1 << 0)
#define THREAD_SIGNAL_SUSPEND                (1 << 1)
#define THREAD_SIGNAL_POLICY_EXCEPTION       (1 << 2)
*/

#[allow(dead_code)]
pub struct ThreadArg {
}

type _ThreadStartEntry = dyn Fn(Option<ThreadArg>) -> Result<(), ErrNO>;
type _ThreadTrampolineEntry = dyn Fn();

/*
 * ThreadInfo is included in Thread at an offset of 0.
 * This means that tp points to both ThreadInfo and Thread.
 */
pub struct ThreadInfo {
    flags: usize,           /* low level flags */
    _preempt_count: i32,     /* 0=>preemptible, <0=>BUG */
    //kernel_sp: usize,     /* Kernel stack pointer */
    //user_sp: usize,       /* User stack pointer */
    pub cpu: usize,
}

impl ThreadInfo {
    pub fn current() -> &'static mut ThreadInfo {
        unsafe {
            &mut *(thread_get_current() as *mut ThreadInfo)
        }
    }

    const fn new() -> Self {
        Self {
            flags: 0,
            _preempt_count: 0,
            cpu: 0,
        }
    }
}

pub struct PreemptionState {
    // state_ contains three fields:
    //
    //  * a 15-bit preempt disable counter (bits 0-14)
    //  * a 15-bit eager resched disable counter (bits 15-29)
    //  * a 2-bit for TimesliceExtensionFlags (bits 30-31)
    //
    // This is a single field so that both counters and the flags can be compared
    // against zero with a single memory access and comparison.
    //
    // state_'s counts are modified by interrupt handlers, but the counts are
    // always restored to their original value before the interrupt handler
    // returns, so modifications are not visible to the interrupted thread.
    state: AtomicU32,
}

impl PreemptionState {
    // Counters contained in state_ are limited to 15 bits.
    const K_MAX_COUNT_VALUE: u32 = 0x7fff;
    // The preempt disable count is in the lowest 15 bits.
    const K_PREEMPT_DISABLE_MASK: u32 = Self::K_MAX_COUNT_VALUE;

    const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
        }
    }

    // PreemptDisable() increments the preempt disable counter for the current
    // thread. While preempt disable is non-zero, preemption of the thread is
    // disabled, including preemption from interrupt handlers. During this time,
    // any call to Reschedule() will only record that a reschedule is pending, and
    // won't do a context switch.
    //
    // Note that this does not disallow blocking operations (e.g.
    // mutex.Acquire()). Disabling preemption does not prevent switching away from
    // the current thread if it blocks.
    //
    // A call to PreemptDisable() must be matched by a later call to
    // PreemptReenable() to decrement the preempt disable counter.
    fn preempt_disable(&self) {
        let old_state = self.state.fetch_add(1, Ordering::Relaxed);
        ZX_ASSERT!(Self::preempt_disable_count(old_state) < Self::K_MAX_COUNT_VALUE);
    }

    fn preempt_disable_count(state: u32) -> u32 {
        state & Self::K_PREEMPT_DISABLE_MASK
    }
}

pub struct Thread {
    pub thread_info: ThreadInfo,
    queue_node: ListNode,
    name: String,
    pub sched_state: SchedulerState,
    pub preemption_state: PreemptionState,
}

impl Linked<Thread> for Thread{
    fn from_node(ptr: *mut ListNode) -> *mut Thread {
        unsafe {
            crate::container_of!(ptr, Thread, queue_node)
        }
    }

    fn into_node(&mut self) -> *mut ListNode {
        &mut (self.queue_node)
    }
}

impl Thread {
    #[allow(dead_code)]
    pub fn current() -> &'static mut Thread {
        unsafe {
            &mut *(thread_get_current() as *mut Thread)
        }
    }

    pub const fn new() -> Self {
        Self {
            thread_info: ThreadInfo::new(),
            queue_node: ListNode::new(),
            name: String::new(),
            sched_state: SchedulerState::new(),
            preemption_state: PreemptionState::new(),
        }
    }

    #[allow(dead_code)]
    pub fn create(name: &str, entry: &_ThreadStartEntry, arg: Option<ThreadArg>,
                  priority: usize) -> Result<Self, ErrNO> {
        Thread::create_etc(None, name, entry, arg, priority, None)
    }

    /*
     * @brief  Create a new thread
     *
     * This function creates a new thread. The thread is initially suspended,
     * so you need to call resume() to execute it.
     *
     * @param  t               If not nullptr, use the supplied Thread
     * @param  name            Name of thread
     * @param  entry           Entry point of thread
     * @param  arg             Arbitrary argument passed to entry(). It can be null.
     *                         in which case |user_thread| will be used.
     * @param  priority        Execution priority for the thread.
     * @param  alt_trampoline  If not nullptr, an alternate trampoline for the thread
     *                         to start on.
     *
     * Thread priority is an integer from 0 (lowest) to 31 (highest).
     *
     *  HIGHEST_PRIORITY
     *  DPC_PRIORITY
     *  HIGH_PRIORITY
     *  DEFAULT_PRIORITY
     *  LOW_PRIORITY
     *  IDLE_PRIORITY
     *  LOWEST_PRIORITY
     *
     * Stack size is set to DEFAULT_STACK_SIZE
     *
     * @return  Pointer to thread object, or nullptr on failure.
     */
    fn create_etc(_t: Option<&Thread>, _name: &str, _entry: &_ThreadStartEntry,
                  _arg: Option<ThreadArg>, _priority: usize,
                  _alt_trampoline: Option<&_ThreadTrampolineEntry>)
        -> Result<Self, ErrNO> {
        todo!("create_etc!");
    }

    #[allow(dead_code)]
    pub fn detach(&self) {
        todo!("detach!");
        /*
  Guard<MonitoredSpinLock, IrqSave> guard{ThreadLock::Get(), SOURCE_TAG};

  // if another thread is blocked inside Join() on this thread,
  // wake them up with a specific return code
  task_state_.WakeJoiners(ZX_ERR_BAD_STATE);

  // if it's already dead, then just do what join would have and exit
  if (state() == THREAD_DEATH) {
    flags_ &= ~THREAD_FLAG_DETACHED;  // makes sure Join continues
    guard.Release();
    return Join(nullptr, 0);
  } else {
    flags_ |= THREAD_FLAG_DETACHED;
    return ZX_OK;
  }
  */
    }

    /**
     * @brief  Make a suspended thread executable.
     *
     * This function is called to start a thread which has just been
     * created with thread_create() or which has been suspended with
     * thread_suspend(). It can not fail.
     */
    #[allow(dead_code)]
    pub fn resume(&self) {
        todo!("resume!");
        /*
  Guard<MonitoredSpinLock, IrqSave> guard{ThreadLock::Get(), SOURCE_TAG};

  if (state() == THREAD_DEATH) {
    // The thread is dead, resuming it is a no-op.
    return;
  }

  // Clear the suspend signal in case there is a pending suspend
  signals_.fetch_and(~THREAD_SIGNAL_SUSPEND, ktl::memory_order_relaxed);
  if (state() == THREAD_INITIAL || state() == THREAD_SUSPENDED) {
    // Wake up the new thread, putting it in a run queue on a cpu.
    Scheduler::Unblock(this);
  }

  kcounter_add(thread_resume_count, 1);
  */
    }

    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }

    #[allow(dead_code)]
    fn detatched(&self) -> bool {
        (self.thread_info.flags & THREAD_FLAG_DETACHED) != 0
    }

    fn set_detached(&mut self, detatched: bool) {
        if detatched {
            self.thread_info.flags |= THREAD_FLAG_DETACHED;
        } else {
            self.thread_info.flags &= !THREAD_FLAG_DETACHED;
        }
    }

    pub fn sched_state(&mut self) -> &mut SchedulerState {
        &mut self.sched_state
    }
}

/* get us into some sort of thread context so Thread::Current works. */
pub fn thread_init_early() {
    ZX_ASSERT!(thread_get_current() == 0);

    /* Initialize the thread list. */
    THREAD_LIST.lock().init();

    /* Init the boot percpu data. */
    PerCPU::init_boot();
}

/**
 * @brief Construct a thread t around the current running state
 *
 * This should be called once per CPU initialization.  It will create
 * a thread that is pinned to the current CPU and running at the
 * highest priority.
 */
pub fn thread_construct_first(thread: *mut Thread, name: &str) {
    ZX_ASSERT!(arch_irqs_disabled());

    construct_thread(thread, name);
    unsafe {
        (*thread).set_detached(true);
    }

    /* Setup the scheduler state. */
    Scheduler::init_first_thread(thread);

    /* Start out with preemption disabled to avoid attempts to reschedule
     * until threading is fulling enabled. This simplifies code paths shared
     * between initialization and runtime (e.g. logging). Preemption is enabled
     * when the idle thread for the current CPU is ready. */
    unsafe {
        (*thread).preemption_state.preempt_disable();
    }

    arch_thread_construct_first(thread);

    {
        let mut thread_list = THREAD_LIST.lock();
        thread_list.add_tail(thread);
    }
}

fn arch_thread_construct_first(_t: *mut Thread) {
}

fn construct_thread(thread: *mut Thread, name: &str) {
    unsafe {
        (*thread).set_name(name);
    }
}

#[inline(always)]
pub fn thread_set_current(current: usize) {
    unsafe {
        asm!(
            "mv tp, a0",
            in("a0") current
        );
    }
}

#[inline(always)]
pub fn thread_get_current() -> usize {
    let current: usize;
    unsafe {
        asm!(
            "mv a0, tp",
            out("a0") current
        );
    }
    current
}

pub type ThreadPtr = usize;

pub static THREAD_LIST: Mutex<List<Thread>> = Mutex::new(List::<Thread>::new());