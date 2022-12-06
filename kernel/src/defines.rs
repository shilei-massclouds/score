/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

use crate::types::*;
use crate::ZX_DEBUG_ASSERT;
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
    pub fn _periph_tables_start();
    pub fn _periph_tables_end();
    pub static _kernel_base_phys: usize;
    pub static _boot_cpu_hartid: usize;
    pub static _dtb_pa: usize;
}

pub fn kernel_base_phys() -> usize {
    unsafe { _kernel_base_phys }
}

pub fn kernel_base_virt() -> usize {
    _start as usize
}

pub fn kernel_size() -> usize {
    (_end as usize) - (_start as usize)
}

pub fn boot_cpu_id() -> usize {
    unsafe { _boot_cpu_hartid }
}

pub fn dtb_pa() -> usize {
    unsafe { _dtb_pa }
}

pub fn periph_tables_start() -> usize {
    _periph_tables_start as usize
}

pub fn periph_tables_end() -> usize {
    _periph_tables_end as usize
}

const PHYSMAP_BASE: usize = KERNEL_ASPACE_BASE;
const PHYSMAP_SIZE: usize = ARCH_PHYSMAP_SIZE;
const PHYSMAP_BASE_PHYS: usize = 0;

// check to see if an address is in the physmap virtually and physically
pub fn is_physmap_addr(va: vaddr_t) -> bool {
    va >= PHYSMAP_BASE && (va - PHYSMAP_BASE < PHYSMAP_SIZE)
}

pub fn is_physmap_phys_addr(pa: paddr_t) -> bool {
    pa - PHYSMAP_BASE_PHYS < PHYSMAP_SIZE
}

/* physical to virtual in the big kernel map */
pub fn paddr_to_physmap(pa: paddr_t) -> vaddr_t {
    pa - PHYSMAP_BASE_PHYS + PHYSMAP_BASE
}

/* given a pointer into the physmap, reverse back to a physical address */
pub fn physmap_to_paddr(va: vaddr_t) -> paddr_t {
    ZX_DEBUG_ASSERT!(is_physmap_addr(va));
    va - PHYSMAP_BASE + PHYSMAP_BASE_PHYS
}

/* given a pointer into the physmap, reverse back to a physical address */
pub fn kernel_va_to_pa(va: vaddr_t) -> paddr_t {
    ZX_DEBUG_ASSERT!(!is_physmap_addr(va));
    va - kernel_base_virt() + kernel_base_phys()
}
