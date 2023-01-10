/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::cmp::max;
use alloc::vec::Vec;
use crate::ZX_ASSERT;
use crate::debug::*;
use crate::defines::PAGE_SHIFT;
use crate::types::vaddr_t;

pub struct VmAddressRegion {
    pub base: vaddr_t,
    pub size: usize,
    pub flags: usize,
    children: Vec<VmAddressRegion>,
}

impl VmAddressRegion {
    pub const fn new() -> Self {
        Self {
            base: 0,
            size: 0,
            flags: 0,
            children: Vec::new(),
        }
    }

    pub fn init(&mut self, base: vaddr_t, size: usize, flags: usize) {
        self.base = base;
        self.size = size;
        self.flags = flags;
    }

    fn cover_range(&self, base: vaddr_t, size: usize) -> bool {
        /*
         * NOTE: DON'T compare end of the range directly, as:
         * (base + size) <= (self.base + self.size)
         * Typically, the value end may overbound and become ZERO!
         */
        let offset = base - self.base;
        base >= self.base && offset < self.size && self.size - offset >= size
    }

    pub fn insert_child(&mut self, child: Self) {
        /* Validate we are a correct child of our parent. */
        ZX_ASSERT!(self.cover_range(child.base, child.size));

        let start = child.base;
        let end = start + child.size;
        match self.children.iter().position(|r| r.base >= end) {
            Some(index) => self.children.insert(index, child),
            None => self.children.push(child),
        }
    }

    /*
     * Perform allocations for VMARs. This allocator works by choosing uniformly
     * at random from a set of positions that could satisfy the allocation.
     * The set of positions are the 'left' most positions of the address space
     * and are capped by the address entropy limit. The entropy limit is retrieved
     * from the address space, and can vary based on whether the user has
     * requested compact allocations or not.
     */
    pub fn alloc_spot_locked(&mut self, size: usize, align_pow2: usize,
                             _arch_mmu_flags: usize, upper_limit: vaddr_t)
        -> vaddr_t
    {
        ZX_ASSERT!(size > 0 && IS_PAGE_ALIGNED!(size));
        dprintf!(INFO, "aspace size 0x{:x} align {} upper_limit 0x{:x}\n",
                 size, align_pow2, upper_limit);

        let align_pow2 = max(align_pow2, PAGE_SHIFT);
        let alloc_spot = self.get_alloc_spot(align_pow2, size,
            self.base, self.size, upper_limit);
        /* Sanity check that the allocation fits. */
        let (_, overflowed) = alloc_spot.overflowing_add(size - 1);
        ZX_ASSERT!(!overflowed);
        return alloc_spot;
    }

    /* Get the allocation spot that is free and large enough for the aligned size. */
    fn get_alloc_spot(&mut self, align_pow2: usize, size: usize,
        parent_base: vaddr_t, parent_size: usize, upper_limit: vaddr_t) -> vaddr_t {
        let (alloc_spot, found) =
            self.find_alloc_spot_in_gaps(size, align_pow2, parent_base, parent_size, upper_limit);
        ZX_ASSERT!(found);

        let align: vaddr_t = 1 << align_pow2;
        ZX_ASSERT!(IS_ALIGNED!(alloc_spot, align));
        return alloc_spot;
    }

    /* Try to find the spot among all the gaps. */
    fn find_alloc_spot_in_gaps(&mut self, size: usize, align_pow2: usize,
        parent_base: vaddr_t, parent_size: vaddr_t, upper_limit: vaddr_t) -> (vaddr_t, bool) {
        let align = 1 << align_pow2;
        /* Found indicates whether we have found the spot with index |selected_indexes|. */
        let mut found = false;
        /* alloc_spot is the virtual start address of the spot to allocate if we find one. */
        let mut alloc_spot: vaddr_t = 0;
        let func = |gap_base: vaddr_t, gap_len: usize| {
            ZX_ASSERT!(IS_ALIGNED!(gap_base, align));
            if gap_len < size || gap_base + size > upper_limit {
                /* Ignore gap that is too small or out of range. */
                return true;
            }
            found = true;
            alloc_spot = gap_base;
            return false;
        };

        self.for_each_gap(func, align_pow2, parent_base, parent_size);

        (alloc_spot, found)
    }

    /* Utility for allocators for iterating over gaps between allocations.
     * F should have a signature of bool func(vaddr_t gap_base, size_t gap_size).
     * If func returns false, the iteration stops.
     * And gap_base will be aligned in accordance with align_pow2. */
    fn for_each_gap<F>(&mut self, mut func: F, align_pow2: usize, parent_base: vaddr_t, parent_size: usize)
    where F: FnMut(usize, usize) -> bool {
        let align = 1 << align_pow2;

        /* Scan the regions list to find the gap to the left of each region.
         * We round up the end of the previous region to the requested alignment,
         * so all gaps reported will be for aligned ranges. */
        let mut prev_region_end = ROUNDUP!(parent_base, align);
        for child in &self.children {
            if child.base > prev_region_end {
                let gap = child.base - prev_region_end;
                if !func(prev_region_end, gap) {
                    return;
                }
            }
            let (end, ret) = child.base.overflowing_add(child.size);
            if ret {
                /* This region is already the last region. */
                return;
            }
            prev_region_end = ROUNDUP!(end, align);
        }

        /* Grab the gap to the right of the last region. Note that if there are
         * no regions, this handles reporting the VMAR's whole span as a gap. */
         if parent_size > prev_region_end - parent_base {
            /* This is equal to parent_base + parent_size - prev_region_end,
             * but guarantee no overflow. */
            let gap = parent_size - (prev_region_end - parent_base);
            func(prev_region_end, gap);
        }
    }

}
