/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::alloc::Layout;
use core::arch::asm;
use core::mem;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU32, Ordering};
use alloc::alloc::{alloc, alloc_zeroed};
use alloc::string::String;

use crate::arch::smp::arch_curr_cpu_num;
use crate::errors::ErrNO;
use crate::klib::list::{Linked, List, ListNode};
use crate::locking::mutex::Mutex;
use crate::ZX_ASSERT;
use crate::percpu::{PerCPU, BOOT_CPU_ID, PERCPU_ARRAY};
use crate::arch::irq::arch_irqs_disabled;
use crate::sched::{SchedulerState, Scheduler};
use crate::vm::kstack::KernelStack;

pub const THREAD_FLAG_DETACHED:     u32 = 1 << 0;
pub const THREAD_FLAG_FREE_STRUCT:  u32 = 1 << 1;
/*
pub const THREAD_FLAG_IDLE                     (1 << 2)
pub const THREAD_FLAG_VCPU                     (1 << 3)

pub const THREAD_SIGNAL_KILL                   (1 << 0)
pub const THREAD_SIGNAL_SUSPEND                (1 << 1)
pub const THREAD_SIGNAL_POLICY_EXCEPTION       (1 << 2)
*/

#[allow(dead_code)]
pub struct ThreadArg {
}

impl ThreadArg {
    const fn _new() -> Self {
        Self {
        }
    }
}

type ThreadStartEntry = fn(Option<ThreadArg>) -> Result<(), ErrNO>;
type _ThreadTrampolineEntry = dyn Fn();

fn dummy_thread_start_entry(_arg: Option<ThreadArg>) -> Result<(), ErrNO> {
    panic!("Please implement it!");
}

/*
 * ThreadInfo is included in Thread at an offset of 0.
 * This means that tp points to both ThreadInfo and Thread.
 */
pub struct ThreadInfo {
    flags: u32,             /* low level flags */
    _preempt_count: i32,    /* 0=>preemptible, <0=>BUG */
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

// TaskState is responsible for running the task defined by
// |entry(arg)|, and reporting its value to any joining threads.
pub struct TaskState {
    /* The Thread's entry point, and its argument. */
    entry: ThreadStartEntry,
    arg: Option<ThreadArg>,
}

impl TaskState {
    const fn new() -> Self {
        Self {
            entry: dummy_thread_start_entry,
            arg: None,
        }
    }

    fn init(&mut self, entry: ThreadStartEntry, arg: Option<ThreadArg>) {
        self.entry = entry;
        self.arg = arg;
    }
}

pub struct Thread {
    pub thread_info: ThreadInfo,
    queue_node: ListNode,
    name: String,
    percpu: *mut PerCPU,
    pub sched_state: SchedulerState,
    pub task_state: TaskState,
    pub preemption_state: PreemptionState,
    pub stack: KernelStack,
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

impl Linked<Thread> for Thread {
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
    /* thread priority */
    const NUM_PRIORITIES: usize = 32;

    const _LOWEST_PRIORITY:  usize = 0;
    pub const HIGHEST_PRIORITY: usize = Self::NUM_PRIORITIES - 1;
    const _DPC_PRIORITY:     usize = Self::NUM_PRIORITIES - 2;
    const _IDLE_PRIORITY:    usize = Self::_LOWEST_PRIORITY;
    const _LOW_PRIORITY:     usize = Self::NUM_PRIORITIES / 4;
    pub const DEFAULT_PRIORITY: usize = Self::NUM_PRIORITIES / 2;
    const _HIGH_PRIORITY:    usize = (Self::NUM_PRIORITIES / 4) * 3;

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
            percpu: null_mut(),
            sched_state: SchedulerState::new(),
            task_state: TaskState::new(),
            preemption_state: PreemptionState::new(),
            stack: KernelStack::new(),
        }
    }

    pub fn percpu(&self) -> &mut PerCPU {
        ZX_ASSERT!(!self.percpu.is_null());
        unsafe { &mut (*self.percpu) }
    }

    #[allow(dead_code)]
    pub fn percpu_ptr(&self) -> *mut PerCPU {
        ZX_ASSERT!(!self.percpu.is_null());
        self.percpu
    }

    #[allow(dead_code)]
    pub fn set_percpu_ptr(&mut self, ptr: *mut PerCPU) {
        ZX_ASSERT!(self.percpu.is_null());
        self.percpu = ptr;
    }

    #[allow(dead_code)]
    pub fn create(name: &str, entry: ThreadStartEntry, arg: Option<ThreadArg>,
                  priority: usize) -> Result<Self, ErrNO> {
        Thread::create_etc(null_mut(), name, entry, arg, priority, None)
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
    fn create_etc(mut thread: *mut Thread, name: &str,
                  entry: ThreadStartEntry, arg: Option<ThreadArg>,
                  priority: usize,
                  _alt_trampoline: Option<&_ThreadTrampolineEntry>)
        -> Result<Self, ErrNO>
    {
        let mut _flags: u32 = 0;

        if thread == null_mut() {
            let layout = Layout::new::<Thread>();
            thread = unsafe { alloc(layout) as *mut Thread };
            if thread.is_null() {
                panic!("Out of memory!");
            }
            _flags |= THREAD_FLAG_FREE_STRUCT;
        }

        /* thread is at least as aligned as the thread is supposed to be */
        ZX_ASSERT!(IS_ALIGNED!(thread as usize, mem::align_of::<Thread>()));

        construct_thread(thread, name);

        unsafe {
            (*thread).task_state.init(entry, arg);
        }
        Scheduler::init_thread(thread, priority);

        unsafe {
            (*thread).stack.init()?;
        }

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
    construct_boot_percpu();

    ZX_ASSERT!(arch_curr_cpu_num() == 0);

    /* Initialize the thread list. */
    THREAD_LIST.lock().init();

    /* Init the boot percpu data. */
    PerCPU::init_boot();
}

fn construct_boot_percpu() {
    let layout = Layout::new::<PerCPU>();
    unsafe {
        let boot_percpu = alloc_zeroed(layout) as *mut PerCPU;
        (*boot_percpu).init();

        let t = (*boot_percpu).idle_thread_ptr();
        (*t).thread_info.cpu = BOOT_CPU_ID;
        (*t).percpu = boot_percpu;
        thread_set_current(t as usize);

        let mut percpu_array = PERCPU_ARRAY.lock();
        percpu_array.set(BOOT_CPU_ID, boot_percpu);
    }
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