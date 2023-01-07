/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::vec::Vec;
use core::{sync::atomic::{AtomicUsize, Ordering}, cell::UnsafeCell, ops::{Deref, DerefMut}};
use crate::thread::{ThreadPtr, thread_get_current};

use super::spinlock::RawSpinLock;

pub struct Mutex<T: ?Sized> {
    owner: AtomicUsize,
    wait_lock: RawSpinLock,
    wait_list: Vec<ThreadPtr>,
    data: UnsafeCell<T>,
}

// these are the only places where `T: Send` matters;
// all other functionality works fine on a single thread.
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    #[inline]
    pub const fn new(t: T) -> Mutex<T> {
        Mutex {
            owner: AtomicUsize::new(0),
            wait_lock: RawSpinLock::new(),
            wait_list: Vec::new(),
            data: UnsafeCell::new(t),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        if !self.try_lock_fast() {
            todo!("__mutex_lock_slowpath(lock);");
        }
        MutexGuard::new(self)
    }

    /* Optimistic trylock that only works in the uncontended case.
     * Make sure to follow with a trylock before failing */
    fn try_lock_fast(&self) -> bool {
        self.owner.compare_exchange(0, thread_get_current(),
                                    Ordering::AcqRel,
                                    Ordering::Relaxed).is_ok()
    }
}

pub struct MutexGuard<'a, T: ?Sized + 'a> {
    lock: &'a Mutex<T>,
}

impl<'mutex, T: ?Sized> MutexGuard<'mutex, T> {
    fn new(lock: &'mutex Mutex<T>) -> MutexGuard<'mutex, T> {
        MutexGuard {
            lock
        }
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

/*
impl<T: ?Sized> !Send for MutexGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}
*/