/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::sync::atomic::AtomicU32;

pub const ARCH_SPIN_LOCK_UNLOCKED: u32 = 0;

pub struct RawSpinLock {
    lock: AtomicU32,
}

impl RawSpinLock {
    pub const fn new() -> Self {
        Self {
            lock: AtomicU32::new(ARCH_SPIN_LOCK_UNLOCKED),
        }
    }
}