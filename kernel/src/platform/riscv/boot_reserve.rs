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

pub const MAX_RESERVES: usize = 64;

struct BootReserveRange {
    pub pa: paddr_t,
    pub len: usize,
}

static RESERVE_RANGES: Mutex<Vec<BootReserveRange>> = Mutex::new(Vec::new());

pub fn boot_reserve_init(pa: paddr_t, len: usize) -> Result<(), ErrNO> {
    /* add the kernel to the boot reserve list */
    boot_reserve_add_range(pa, len)
}

fn boot_reserve_add_range(pa: usize, len: usize) -> Result<(), ErrNO> {
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

/* given two offset/length pairs, determine if they overlap at all */
#[inline]
fn intersects(offset1: usize, len1: usize,
              offset2: usize, len2: usize) -> bool {
    /* Can't overlap a zero-length region. */
    if len1 == 0 || len2 == 0 {
        return false;
    }

    if offset1 <= offset2 {
        /* doesn't intersect, 1 is completely below 2 */
        if offset1 + len1 <= offset2 {
            return false;
        }
    } else if offset1 >= offset2 + len2 {
        /* 1 is completely above 2 */
        return false;
    }

    true
}
