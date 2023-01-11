/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use crate::thread::{ThreadPtr, thread_get_current};

use super::spinlock::RawSpinLock;

pub struct Mutex<T: ?Sized> {
    owner: AtomicUsize,
    _wait_lock: RawSpinLock,
    _wait_list: Vec<ThreadPtr>,
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
            _wait_lock: RawSpinLock::new(),
            _wait_list: Vec::new(),
            data: UnsafeCell::new(t),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        if !self.try_lock_fast() {
            todo!("__mutex_lock_slowpath(lock);");
        }
        println!("lock!");
        MutexGuard::new(self)
    }

    /* Optimistic trylock that only works in the uncontended case.
     * Make sure to follow with a trylock before failing */
    fn try_lock_fast(&self) -> bool {
        let ret =
            self.owner.compare_exchange(0, thread_get_current(),
                                        Ordering::AcqRel,
                                        Ordering::Relaxed);
        match ret {
            Ok(_) => true,
            Err(val) => {
                if val == thread_get_current() {
                    panic!("Find nested locking for 0x{:x}", val);
                }
                false
            }
        }
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

    fn unlock(&self) {
        println!("unlock!");
        if self.unlock_fast() {
            return;
        }
        todo!("__mutex_unlock_slowpath(lock, _RET_IP_)");
    }

    fn unlock_fast(&self) -> bool {
        let ret =
            self.lock.owner.compare_exchange(thread_get_current(), 0,
                                     Ordering::Release,
                                     Ordering::Relaxed);
        match ret {
            Ok(_) => true,
            Err(val) => {
                if val == 0 {
                    panic!("Mutex already unlocked! current 0x{:x}",
                           thread_get_current());
                }
                false
            }
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

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.unlock();
    }
}

impl<T: ?Sized> !Send for MutexGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}