/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::{cmp, ptr::null_mut};

use crate::{types::vaddr_t, errors::ErrNO, defines::{BYTE_BITS, BYTES_PER_USIZE}};

pub struct Bitmap {
    size: usize,
    storage_num: usize,    /* units of storage */
    storage_data: *mut usize,
}

impl Bitmap {
    pub const fn new() -> Self {
        Self {
            size: 0,
            storage_num: 0,
            storage_data: null_mut(),
        }
    }

    pub fn storage_init(&mut self, base: vaddr_t, size: usize) {
        self.storage_num = size / BYTES_PER_USIZE;
        self.storage_data = base as *mut usize;
    }

    pub fn storage_num(&self) -> usize {
        self.storage_num
    }

    pub fn init(&mut self, size: usize) {
        self.size = size;
    }

    pub fn size(&self) -> usize {
        self.size
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
                *self.storage_data.offset(i as isize) |=
                    get_mask(i == first_idx, i == last_idx, bitoff, bitmax);
            }
        }

        Ok(())
    }

    pub fn clear(&mut self, bitoff: usize, bitmax: usize) -> Result<(), ErrNO> {
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
                *self.storage_data.offset(i as isize) &=
                    !get_mask(i == first_idx, i == last_idx, bitoff, bitmax);
            }
        }
        Ok(())
    }

    fn _storage_unit_ptr(&self, idx: usize) -> *mut usize {
        unsafe {
            self.storage_data.offset(idx as isize)
        }
    }

    fn storage_unit_ref(&self, idx: usize) -> usize {
        unsafe {
            *self.storage_data.offset(idx as isize)
        }
    }

    pub fn find(&self, is_set: bool, mut bitoff: usize, bitmax: usize,
        run_len: usize) -> Result<usize, ErrNO> {
        if bitmax <= bitoff {
            return Err(ErrNO::InvalidArgs);
        }

        let mut start = bitoff;
        loop {
            if self.scan(bitoff, bitmax, !is_set, &mut start) {
                return Err(ErrNO::NoResources);
            }
            if bitmax - start < run_len {
                return Err(ErrNO::NoResources);
            }
            if self.scan(start, start + run_len, is_set, &mut bitoff) {
                return Ok(start);
            }
        }
    }

    pub fn scan(&self, bitoff: usize, mut bitmax: usize, is_set: bool,
        out: &mut usize) -> bool {
        bitmax = cmp::min(bitmax, self.size);
        if bitoff >= bitmax {
            return true;
        }
        for i in first_idx(bitoff)..last_idx(bitmax) {
            let data = self.storage_unit_ref(i);
            let masked = mask_bits(data, i, bitoff, bitmax, is_set);
            if masked != 0 {
                *out = i * BITMAP_UNIT_BITS + masked.trailing_zeros() as usize;
                return false;
            }
        }
        return true;
    }

    pub fn reverse_scan(&self, bitoff: usize, mut bitmax: usize, is_set: bool,
                        out: &mut usize) -> bool {
        bitmax = cmp::min(bitmax, self.size);
        if bitoff >= bitmax {
            return true;
        }
        let mut i = last_idx(bitmax);
        loop {
            let data = self.storage_unit_ref(i);
            let masked = mask_bits(data, i, bitoff, bitmax, is_set);
            if masked != 0 {
                *out = (i + 1) * BITMAP_UNIT_BITS - (masked.leading_zeros() as usize + 1);
                return false;
            }
            if i == first_idx(bitoff) {
                return true;
            }
            i -= 1;
        }
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

/* Applies a bitmask to the given data value. The result value has bits set
 * which fall within the mask but do not match is_set. */
fn mask_bits(data: usize, idx: usize, bitoff: usize, bitmax: usize, is_set: bool) -> usize {
    let mask = get_mask(idx == first_idx(bitoff), idx == last_idx(bitmax),
        bitoff, bitmax);
    if is_set {
        /* If is_set=true, invert the mask, OR it with the value,
         * and invert it again to hopefully get all zeros. */
        !(!mask | data)
    } else {
        /* If is_set=false, just AND the mask with the value to
         * hopefully get all zeros. */
        mask & data
  }
}

#[macro_export]
macro_rules! BIT_MASK {
    ($bits: expr) => {
        if $bits >= usize::BITS as usize {
            usize::MAX
        } else {
            (1 << $bits) - 1
        }
    }
}