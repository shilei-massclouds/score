/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::defines::SMP_MAX_CPUS;

#[allow(non_camel_case_types)]
pub type cpu_num_t = usize;

#[allow(non_camel_case_types)]
pub type cpu_mask_t = usize;

pub const INVALID_CPU: usize = usize::MAX;
pub const CPU_MASK_ALL: usize = usize::MAX;

pub const fn is_valid_cpu_num(num: cpu_num_t) -> bool {
    num < SMP_MAX_CPUS
}

pub const fn cpu_num_to_mask(num: cpu_num_t) -> cpu_mask_t {
    if !is_valid_cpu_num(num) {
        return 0;
    }

    1 << num
}
