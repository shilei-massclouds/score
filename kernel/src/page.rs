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

#[allow(non_camel_case_types)]
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
    _loaned_state: AtomicU8,
}

impl Linked<vm_page> for vm_page {
    fn into_node(&mut self) -> &mut ListNode {
        &mut (self.queue_node)
    }
}

impl vm_page {
    pub const fn _new() -> Self {
        Self {
            queue_node: ListNode::new(),
            paddr: 0,
            state: AtomicU8::new(vm_page_state::FREE),
            _loaned_state: AtomicU8::new(0),
        }
    }

    pub fn init(&mut self, paddr: paddr_t) {
        self.paddr = paddr;
    }

    pub fn set_state(&mut self, new_state: vm_page_state_t) {
        let _old_state = self.state.swap(new_state, Ordering::Relaxed);
        /*
            auto& p = percpu::GetCurrent();
            p.vm_page_counts.by_state[VmPageStateIndex(old_state)] -= 1;
            p.vm_page_counts.by_state[VmPageStateIndex(new_state)] += 1;
        */
    }
}

#[allow(non_camel_case_types)]
pub type vm_page_t = vm_page;
