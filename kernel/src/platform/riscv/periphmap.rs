/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::errors::ErrNO;
use crate::arch::mmu::{PAGE_IOREMAP, boot_map};
use crate::{PAGE_SIZE, IS_PAGE_ALIGNED, IS_ALIGNED};
use crate::{print, dprintf};
use crate::{kernel_base_virt};
use crate::debug::*;
use crate::types::*;
use alloc::vec::Vec;
use spin::Mutex;
use core::ptr::null_mut;
use crate::{periph_tables_start, periph_tables_end, kernel_va_to_pa};
use crate::arch::mmu::PageTable;
use crate::paddr_to_physmap;

pub const MAX_PERIPH_RANGES : usize = 4;

/* peripheral ranges are just allocated below the kernel image. */
static PERIPH_RANGES: Mutex<Vec<PeriphRange>> = Mutex::new(Vec::new());

pub struct PeriphRange {
    pub base_phys:  paddr_t,
    pub base_virt:  vaddr_t,
    pub length:     usize,
}

pub fn add_periph_range(base_phys: usize, length: usize) -> Result<(), ErrNO> {
    let mut ranges = PERIPH_RANGES.lock();

    if ranges.len() >= MAX_PERIPH_RANGES {
        return Err(ErrNO::OutOfRange);
    }

    if !IS_PAGE_ALIGNED!(base_phys) || !IS_PAGE_ALIGNED!(length) {
        return Err(ErrNO::BadAlign);
    }

    /* peripheral ranges are allocated below the kernel image. */
    let mut base_virt = kernel_base_virt();

    /* give ourselves an extra gap of space to try to catch overruns */
    base_virt -= 0x40000000;

    for range in ranges.iter() {
        base_virt -= range.length;
    }

    base_virt -= length;
    dprintf!(INFO, "periphmap: {:x}\n", base_virt);
    dprintf!(INFO, "periph_table: {:x}\n", periph_tables_start());

    let mut alloc = || {
        static mut pos: usize = 0;
        unsafe {
            if pos == 0 {
                pos = periph_tables_start();
            } else if pos >= periph_tables_end() {
                crate::stdio::STDOUT.lock().puts("!!!null!!!\n");
                crate::stdio::STDOUT.lock().put_u64(pos as u64);
                crate::stdio::STDOUT.lock().puts("!!!null!!!\n");
                return null_mut();
            }
            let cur = pos;
            pos += PAGE_SIZE;
            kernel_va_to_pa(cur) as *mut PageTable
        }
    };

    let phys_to_virt = |pa: paddr_t| { paddr_to_physmap(pa) as *mut PageTable };

    boot_map(base_virt, base_phys, length, PAGE_IOREMAP,
             &mut alloc, &phys_to_virt)?;

    ranges.push(PeriphRange {base_phys, base_virt, length});

    Ok(())
}
