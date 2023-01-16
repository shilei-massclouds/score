/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::page::vm_page_t;
use super::vm_cow_pages::VmCowPages;

pub struct PageQueues {
}

impl PageQueues {
    pub const fn new() -> Self {
        Self {
        }
    }

    pub fn set_anonymous(&self, page: *mut vm_page_t, object: &VmCowPages, page_offset: usize) {
        //ZX_ASSERT!(!object.is_null());
        /*
        SetQueueBacklinkLocked(page, object, page_offset,
            kAnonymousIsReclaimable ? mru_gen_to_queue() : PageQueueAnonymous);
        */
        todo!("set_anonymous");
    }
}