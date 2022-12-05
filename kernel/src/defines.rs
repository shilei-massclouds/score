/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::config_generated::*;

pub const PAGE_SHIFT    : usize = _CONFIG_PAGE_SHIFT;
pub const PAGE_SIZE     : usize = 1 << PAGE_SHIFT;
pub const KERNEL_BASE   : usize = _CONFIG_KERNEL_BASE;

pub const ARCH_PHYSMAP_SIZE: usize = _CONFIG_ARCH_PHYSMAP_SIZE;
pub const MMU_MAX_LEVEL: usize = _CONFIG_MMU_MAX_LEVEL;

/* Virtual address where the kernel address space begins.
 * Below this is the user address space. */
pub const KERNEL_ASPACE_BASE: usize = _CONFIG_KERNEL_ASPACE_BASE;
pub const KERNEL_ASPACE_SIZE: usize =
    0_usize.wrapping_sub(_CONFIG_KERNEL_ASPACE_BASE);

pub const KERNEL_ASPACE_MASK: usize = KERNEL_ASPACE_SIZE - 1;

/* clang-format off */
macro_rules! IFTE {
    ($c: expr, $t: expr, $e: expr) => {
        if $c != 0usize { $t } else { $e }
    }
}

macro_rules! NBITS01 {
    ($n: expr) => {
        IFTE!($n, 1, 0)
    }
}
macro_rules! NBITS02 {
    ($n: expr) => {
        IFTE!(($n) >>  1,  1 + NBITS01!(($n) >>  1), NBITS01!($n))
    }
}
macro_rules! NBITS04 {
    ($n: expr) => {
        IFTE!(($n) >>  2,  2 + NBITS02!(($n) >>  2), NBITS02!($n))
    }
}
macro_rules! NBITS08 {
    ($n: expr) => {
        IFTE!(($n) >>  4,  4 + NBITS04!(($n) >>  4), NBITS04!($n))
    }
}
macro_rules! NBITS16 {
    ($n: expr) => {
        IFTE!(($n) >>  8,  8 + NBITS08!(($n) >>  8), NBITS08!($n))
    }
}
macro_rules! NBITS32 {
    ($n: expr) => {
        IFTE!(($n) >> 16, 16 + NBITS16!(($n) >> 16), NBITS16!($n))
    }
}
macro_rules! NBITS {
    ($n: expr) => {
        IFTE!(($n) >> 32, 32 + NBITS32!(($n) >> 32), NBITS32!($n))
    }
}

pub const KERNEL_ASPACE_BITS: usize = NBITS!(KERNEL_ASPACE_MASK);

/* These symbols come from kernel.ld */
extern "C" {
    pub fn _start();
    pub fn _end();
    pub fn _boot_heap();
    pub fn _boot_heap_end();
    pub static _kernel_base_phys: usize;
    pub static _dtb_pa: usize;
}

pub fn kernel_base_phys() -> usize {
    unsafe { _kernel_base_phys }
}

pub fn kernel_size() -> usize {
    (_end as usize) - (_start as usize)
}

pub fn dtb_pa() -> usize {
    unsafe { _dtb_pa }
}
