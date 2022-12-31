/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::sync::atomic::{AtomicU8, Ordering};
use crate::types::*;
use crate::klib::list::{Linked, ListNode};
use crate::vm_page_state;
use crate::vm_page_state::vm_page_state_t;

  // logically private, use loaned getters and setters below.
#[allow(non_upper_case_globals)]
const kLoanedStateIsLoaned: u8 = 1;
#[allow(non_upper_case_globals)]
const _kLoanedStateIsLoanCancelled: u8 = 2;

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct vm_page {
    /* linked node */
    pub queue_node: ListNode,

    /* read-only after being set up */
    paddr: paddr_t,  /* use paddr() accessor */

    /* offset 0x2b */

    /* logically private; use |state()| and |set_state()| */
    state: AtomicU8,

    /* offset 0x2c */

    /* logically private, use loaned getters and setters below. */
    loaned_state: AtomicU8,
}

impl Linked<vm_page> for vm_page {
    fn from_node(ptr: *mut ListNode) -> *mut vm_page_t {
        unsafe {
            crate::container_of!(ptr, vm_page_t, queue_node)
        }
    }

    fn into_node(&mut self) -> *mut ListNode {
        &mut (self.queue_node)
    }
}

impl vm_page {
    pub fn init(&mut self, paddr: paddr_t) {
        self.queue_node = ListNode::new();
        self.paddr = paddr;
        self.state = AtomicU8::new(vm_page_state::FREE);
        self.loaned_state = AtomicU8::new(0);
    }

    pub fn paddr(&self) -> paddr_t {
        self.paddr
    }

    pub fn state(&self) -> u8 {
        self.state.load(Ordering::Relaxed)
    }

    pub fn set_state(&mut self, new_state: vm_page_state_t) {
        let _old_state = self.state.swap(new_state, Ordering::Relaxed);
        /*
            auto& p = percpu::GetCurrent();
            p.vm_page_counts.by_state[VmPageStateIndex(old_state)] -= 1;
            p.vm_page_counts.by_state[VmPageStateIndex(new_state)] += 1;
        */
    }

    pub fn is_free(&self) -> bool {
        self.state() == vm_page_state::FREE
    }

    /* If true, this page is "loaned" in the sense of being loaned from
     * a contiguous VMO (via decommit) to Zircon. If the original contiguous VMO
     * is deleted, this page will no longer be loaned. A loaned page cannot be pinned.
     * Instead a different physical page (non-loaned) is used for the pin.
     * A loaned page can be (re-)committed back into its original contiguous VMO,
     * which causes the data in the loaned page to be moved into
     * a different physical page (which itself can be non-loaned or loaned).
     * A loaned page cannot be used to allocate a new contiguous VMO. */
    pub fn is_loaned(&self) -> bool {
        let loaned_state = self.loaned_state.load(Ordering::Relaxed);
        loaned_state & kLoanedStateIsLoaned == kLoanedStateIsLoaned
    }
}

#[allow(non_camel_case_types)]
pub type vm_page_t = vm_page;
