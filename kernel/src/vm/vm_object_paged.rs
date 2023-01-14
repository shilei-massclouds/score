/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::string::String;

use crate::ZX_ASSERT;
use crate::defines::PAGE_SIZE;
use crate::errors::ErrNO;
use crate::klib::list::List;
use crate::page::vm_page_t;
use crate::pmm::{PMM_ALLOC_FLAG_CAN_WAIT, pmm_alloc_pages};
use crate::vm::vm_cow_pages::{VmCowPages, CanOverwriteContent};

pub struct VmObject {
    name: String,
}

impl VmObject {
    #[allow(dead_code)]
    const fn new() -> Self {
        Self {
            name: String::new(),
        }
    }

    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
}

type VmObjectPagedPtr = *mut VmObjectPaged;

pub struct VmObjectPaged {
    vmo: VmObject,
}

impl VmObjectPaged {
    /* |options_| is a bitmask of: */
    pub const K_RESIZABLE:      u32 = 1 << 0;
    pub const K_CONTIGUOUS:     u32 = 1 << 1;
    #[allow(dead_code)]
    pub const K_SLICE:          u32 = 1 << 3;
    #[allow(dead_code)]
    pub const K_DISCARDABLE:    u32 = 1 << 4;
    pub const K_ALWAYS_PINNED:  u32 = 1 << 5;
    pub const K_CAN_BLOCK_ON_PAGE_REQUESTS: u32 = 1 << 31;

    #[allow(dead_code)]
    pub const fn new() -> Self {
        Self {
            vmo: VmObject::new(),
        }
    }

    pub fn set_name(&mut self, name: &str) {
        self.vmo.set_name(name);
    }

    fn check_bits(options: u32, refval: u32) -> bool {
        (options & refval) != 0
    }

    pub fn create(pmm_alloc_flags: u32, options: u32, size: usize)
        -> Result<VmObjectPagedPtr, ErrNO>
    {
        let refval = Self::K_CONTIGUOUS | Self::K_CAN_BLOCK_ON_PAGE_REQUESTS;
        if Self::check_bits(options, refval) {
            /* Force callers to use CreateContiguous() instead. */
            return Err(ErrNO::InvalidArgs);
        }
        Self::create_common(pmm_alloc_flags, options, size)
    }

    fn create_common(pmm_alloc_flags: u32, mut options: u32, size: usize)
        -> Result<VmObjectPagedPtr, ErrNO>
    {
        let refval = Self::K_CONTIGUOUS | Self::K_CAN_BLOCK_ON_PAGE_REQUESTS;
        ZX_ASSERT!(!Self::check_bits(options, refval));

        /* Cannot be resizable and pinned,
         * otherwise we will lose track of the pinned range. */
        if Self::check_bits(options, Self::K_RESIZABLE) &&
           Self::check_bits(options, Self::K_ALWAYS_PINNED) {
            return Err(ErrNO::InvalidArgs);
        }

        if Self::check_bits(pmm_alloc_flags, PMM_ALLOC_FLAG_CAN_WAIT) {
            options |= Self::K_CAN_BLOCK_ON_PAGE_REQUESTS;
        }

        /* make sure size is page aligned */
        let size = ROUNDUP_PAGE_SIZE!(size);

        let mut cow_pages =
            VmCowPages::create(VmCowPages::K_NONE, pmm_alloc_flags, size)?;

        /* If this VMO will always be pinned, allocate and pin the pages
         * in the VmCowPages prior to creating the VmObjectPaged.
         * This ensures the VmObjectPaged destructor can assume
         * that the pages are committed and pinned. */
        if Self::check_bits(options, Self::K_ALWAYS_PINNED) {
            let mut prealloc_pages = List::<vm_page_t>::new();
            prealloc_pages.init();
            pmm_alloc_pages(size / PAGE_SIZE,
                            pmm_alloc_flags,
                            &mut prealloc_pages)?;

            /* Add all the preallocated pages to the object, this takes
             * ownership of all pages regardless of the outcome.
             * This is a new VMO, but this call could fail due to OOM. */
            cow_pages.add_new_pages(0, &mut prealloc_pages,
                                    CanOverwriteContent::Zero,
                                    true, false)?;

            /* With all the pages in place, pin them. */
            cow_pages.pin_range(0, size)?;

            todo!("K_ALWAYS_PINNED!");
        }

        todo!("create_common!");
    }

}