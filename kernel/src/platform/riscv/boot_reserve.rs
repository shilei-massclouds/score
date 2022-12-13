/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::types::*;
use crate::errors::ErrNO;
use alloc::vec::Vec;
use crate::debug::*;
use crate::{dprintf, print, ZX_DEBUG_ASSERT};
use spin::Mutex;
use crate::klib::range::intersects;

pub const MAX_RESERVES: usize = 64;

#[derive(Default)]
pub struct BootReserveRange {
    pub pa: paddr_t,
    pub len: usize,
}

static RESERVE_RANGES: Mutex<Vec<BootReserveRange>> = Mutex::new(Vec::new());

pub fn boot_reserve_init(pa: paddr_t, len: usize) -> Result<(), ErrNO> {
    /* add the kernel to the boot reserve list */
    boot_reserve_add_range(pa, len)
}

pub fn boot_reserve_add_range(pa: paddr_t, len: usize) -> Result<(), ErrNO> {
    dprintf!(INFO, "PMM: boot reserve add [0x{:x}, 0x{:x}]\n",
             pa, pa + len - 1);

    let mut res = RESERVE_RANGES.lock();
    if res.len() == (MAX_RESERVES - 1) {
        panic!("too many boot reservations");
    }

    /* insert into the list, sorted */
    let end: paddr_t = pa + len - 1;
    ZX_DEBUG_ASSERT!(end > pa);

    let mut i = 0;
    while i < res.len() {
        if intersects(res[i].pa, res[i].len, pa, len) {
            /* we have a problem that we are not equipped to handle right now */
            panic!("pa {:x} len {:x} intersects existing range", pa, len);
        }

        if res[i].pa > end {
            break;
        }

        i += 1;
    }

    let range = BootReserveRange{pa: pa, len: len};
    res.insert(i, range);

    dprintf!(INFO, "Boot reserve #range {}\n", res.len());
    Ok(())
}

pub fn boot_reserve_range_search(range_pa: paddr_t, range_len: usize,
                                 alloc_len: usize,
                                 alloc_range: &mut BootReserveRange)
    -> Result<(), ErrNO> {

    dprintf!(INFO, "range pa {:x} len {:x} alloc_len {:x}\n",
             range_pa, range_len, alloc_len);

    let mut alloc_pa = upper_align(range_pa, range_len, alloc_len);

    /* see if it intersects any reserved range */
    dprintf!(INFO, "trying alloc range {:x} len {:x}\n",
             alloc_pa, alloc_len);

    let res = RESERVE_RANGES.lock();
    'retry: loop {
        for r in res.iter() {
            if intersects(r.pa, r.len, alloc_pa, alloc_len) {
                alloc_pa = r.pa - alloc_len;
                /* make sure this still works with input constraints */
                if alloc_pa < range_pa {
                    return Err(ErrNO::NoMem);
                }

                continue 'retry;
            }
        }

        break;
    }

    alloc_range.pa = alloc_pa;
    alloc_range.len = alloc_len;
    Ok(())
}

fn upper_align(r_pa: paddr_t, r_len: usize, alloc_len: usize) -> paddr_t {
    r_pa + r_len - alloc_len
}
