/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::arch::asm;
use alloc::alloc::{alloc, Layout};

use crate::{defines::ARCH_DEFAULT_STACK_SIZE, errors::ErrNO, klib::list::Linked};

// thread priority
const NUM_PRIORITIES: i32 = 32;

const _LOWEST_PRIORITY:  i32 = 0;
const _HIGHEST_PRIORITY: i32 = NUM_PRIORITIES - 1;
const _DPC_PRIORITY:     i32 = NUM_PRIORITIES - 2;
const _IDLE_PRIORITY:    i32 = _LOWEST_PRIORITY;
const _LOW_PRIORITY:     i32 = NUM_PRIORITIES / 4;
pub const DEFAULT_PRIORITY: i32 = NUM_PRIORITIES / 2;
const _HIGH_PRIORITY:    i32 = (NUM_PRIORITIES / 4) * 3;

// stack size
pub const _DEFAULT_STACK_SIZE: usize = ARCH_DEFAULT_STACK_SIZE;

pub struct ThreadArg {

}

type ThreadStartEntry = dyn Fn(Option<ThreadArg>) -> Result<(), ErrNO>;
type ThreadTrampolineEntry = dyn Fn();

pub struct Thread {
}

impl Thread {
    pub fn create(name: &str, entry: &ThreadStartEntry, arg: Option<ThreadArg>,
                  priority: i32) -> Result<Self, ErrNO> {
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
    fn create_etc(_t: Option<&Thread>, _name: &str, _entry: &ThreadStartEntry,
                  _arg: Option<ThreadArg>, _priority: i32,
                  _alt_trampoline: Option<&ThreadTrampolineEntry>)
        -> Result<Self, ErrNO> {
        todo!("create_etc!");
    }

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
}

/* get us into some sort of thread context so Thread::Current works. */
pub fn thread_init_early() {
    unsafe {
        let layout: Layout = Layout::new::<Thread>();
        let init_thread = alloc(layout);
        println!("init thread: {:?} 0x{:x}",
                 init_thread, thread_get_current());
        thread_set_current(init_thread as usize);
        println!("init thread: then 0x{:x}",
                 thread_get_current());
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