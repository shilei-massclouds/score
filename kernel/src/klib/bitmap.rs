/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::ptr::{slice_from_raw_parts, NonNull, slice_from_raw_parts_mut, self};

use crate::{types::vaddr_t, errors::ErrNO, defines::BYTE_BITS};

pub struct Bitmap {
    size: usize,
    storage_size: usize,    /* units of usize */
    storage_data: NonNull<usize>,
}

impl Bitmap {
    pub const fn new() -> Self {
        Self {
            size: 0,
            storage_size: 0,
            storage_data: NonNull::<usize>::dangling(),
        }
    }

    pub fn storage_init(&mut self, base: vaddr_t, size: usize) {
        self.storage_size = size / 8;
        self.storage_data = NonNull::new(base as *mut usize).unwrap();
    }

    pub fn init(&mut self, size: usize) {
        self.size = size;
    }

    pub fn set(&mut self, bitoff: usize, bitmax: usize) -> Result<(), ErrNO> {
        if bitoff > bitmax || bitmax > self.size {
            return Err(ErrNO::InvalidArgs);
        }
        if bitoff == bitmax {
            return Ok(());
        }
        let first_idx = first_idx(bitoff);
        let last_idx = last_idx(bitmax);

        for i in first_idx..=last_idx {
            unsafe {
                *self.storage_data.as_ptr().offset(i as isize) |=
                    get_mask(i == first_idx, i == last_idx, bitoff, bitmax);
            }
        }
        Ok(())
    }
}

unsafe impl Sync for Bitmap {}
unsafe impl Send for Bitmap {}

const BITMAP_UNIT_BITS: usize = usize::BITS as usize * BYTE_BITS;

/* Translates a bit offset into a starting index in the bitmap array. */
const fn first_idx(bitoff: usize) -> usize {
    bitoff / BITMAP_UNIT_BITS
}

/* Translates a max bit into a final index in the bitmap array. */
const fn last_idx(bitmax: usize) -> usize {
    (bitmax - 1) / BITMAP_UNIT_BITS
}

/*
 * get_mask returns a 64-bit bitmask. If the block of the bitmap we're looking
 * at isn't the first or last, all bits are set.  Otherwise, the bits outside
 * of [off,max) are cleared.
 * Bits are counted with the LSB as 0 and the MSB as 63.
 *
 * Examples:
 * get_mask(false, false, 16, 48) => 0xffffffffffffffff
 * get_mask(true,  false, 16, 48) => 0xffffffffffff0000
 * get_mask(false,  true, 16, 48) => 0x0000ffffffffffff
 * get_mask(true,   true, 16, 48) => 0x0000ffffffff0000
 */
fn get_mask(first: bool, last: bool, off: usize, max: usize) -> usize {
    let mut mask = usize::MAX;
    if first {
        mask &= usize::MAX << (off % BITMAP_UNIT_BITS);
    }
    if last {
        mask &= usize::MAX >>
            ((BITMAP_UNIT_BITS - (max % BITMAP_UNIT_BITS)) % BITMAP_UNIT_BITS);
    }

    mask
}