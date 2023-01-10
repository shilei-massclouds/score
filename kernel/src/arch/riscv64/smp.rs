/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::thread::ThreadInfo;

pub fn raw_smp_processor_id() -> usize {
    ThreadInfo::current().cpu
}

pub fn arch_curr_cpu_num() -> usize {
    raw_smp_processor_id()
}