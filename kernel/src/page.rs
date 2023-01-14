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
pub struct vm_page_object {
    pin_count: u8,

    /* Tracks state used to determine whether the page is dirty and
     * its contents need to written back to the page source at some point,
     * and when it has been cleaned. Used for pages backed by a user pager.
     * The three states supported are Clean, Dirty, and AwaitingClean
     * (more details in VmCowPages::DirtyState). */
    dirty_state: u8,
}

impl vm_page_object {
    const VM_PAGE_OBJECT_MAX_PIN_COUNT: u8 = 31;

    // Bits used by VmObjectPaged implementation of COW clones.
    //
    // Pages of VmObjectPaged have two "split" bits. These bits are used to track which
    // pages in children of hidden VMOs have diverged from their parent. There are two
    // bits, left and right, one for each child. In a hidden parent, a 1 split bit means
    // that page in the child has diverged from the parent and the parent's page is
    // no longer accessible to that child.
    //
    // It should never be the case that both split bits are set, as the page should
    // be moved into the child instead of setting the second bit.
    const VM_PAGE_OBJECT_COW_LEFT_SPLIT:    u8 = 1 << 5;
    const VM_PAGE_OBJECT_COW_RIGHT_SPLIT:   u8 = 1 << 6;

    /* Hint for whether the page is always needed and should not be considered
     * for reclamation under memory pressure (unless the kernel decides to
     * override hints for some reason). */
    const VM_PAGE_OBJECT_ALWAYS_NEED:       u8 = 1 << 7;

    // Used to track dirty_state in the vm_page_t.
    //
    // The transitions between the three states can roughly be summarized as follows:
    // 1. A page starts off as Clean when supplied.
    // 2. A write transitions the page from Clean to Dirty.
    // 3. A writeback_begin moves the Dirty page to AwaitingClean.
    // 4. A writeback_end moves the AwaitingClean page to Clean.
    // 5. A write that comes in while the writeback is in progress (i.e. the page is AwaitingClean)
    // moves the AwaitingClean page back to Dirty.
    pub const DIRTY_STATE_UNTRACKED:    u8 = 0;
    #[allow(dead_code)]
    pub const DIRTY_STATE_CLEAN:        u8 = 1;
    #[allow(dead_code)]
    pub const DIRTY_STATE_DIRTY:        u8 = 2;
    #[allow(dead_code)]
    pub const DIRTY_STATE_AWAITINGCLEAN:u8 = 3;
    pub const DIRTY_STATE_NUM_STATES:   u8 = 4;

    #[allow(dead_code)]
    const fn new() -> Self {
        Self {
            pin_count: 0,
            dirty_state: Self::DIRTY_STATE_UNTRACKED,
        }
    }

    #[allow(dead_code)]
    pub fn pin_count(&self) -> u8 {
        self.pin_count
    }

    pub fn set_pin_count(&mut self, pin_count: u8) {
        if pin_count > Self::VM_PAGE_OBJECT_MAX_PIN_COUNT {
            panic!("pin count {} is too large!", pin_count);
        }
        self.pin_count = pin_count;
    }

    #[allow(dead_code)]
    pub fn cow_left_split(&self) -> bool {
        (self.pin_count & Self::VM_PAGE_OBJECT_COW_LEFT_SPLIT) != 0
    }

    pub fn set_cow_left_split(&mut self, bval: bool) {
        if bval {
            self.pin_count |= Self::VM_PAGE_OBJECT_COW_LEFT_SPLIT;
        } else {
            self.pin_count &= !Self::VM_PAGE_OBJECT_COW_LEFT_SPLIT;
        }
    }

    #[allow(dead_code)]
    pub fn cow_right_split(&self) -> bool {
        (self.pin_count & Self::VM_PAGE_OBJECT_COW_RIGHT_SPLIT) != 0
    }

    pub fn set_cow_right_split(&mut self, bval: bool) {
        if bval {
            self.pin_count |= Self::VM_PAGE_OBJECT_COW_RIGHT_SPLIT;
        } else {
            self.pin_count &= !Self::VM_PAGE_OBJECT_COW_RIGHT_SPLIT;
        }
    }

    /* Hint for whether the page is always needed and should not be considered
     * for reclamation under memory pressure (unless the kernel decides to
     * override hints for some reason). */
    #[allow(dead_code)]
    pub fn always_need(&self) -> bool {
        (self.pin_count & Self::VM_PAGE_OBJECT_ALWAYS_NEED) != 0
    }

    pub fn set_always_need(&mut self, bval: bool) {
        if bval {
            self.pin_count |= Self::VM_PAGE_OBJECT_ALWAYS_NEED;
        } else {
            self.pin_count &= !Self::VM_PAGE_OBJECT_ALWAYS_NEED;
        }
    }

    #[allow(dead_code)]
    pub fn dirty_state(&self) -> u8 {
        self.dirty_state
    }

    pub fn set_dirty_state(&mut self, dirty_state: u8) {
        if dirty_state >= Self::DIRTY_STATE_NUM_STATES {
            panic!("dirty state {} is out of limit!", dirty_state);
        }
        self.dirty_state = dirty_state;
    }
}

#[allow(non_camel_case_types)]
type vm_page_object_t = vm_page_object;

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct vm_page {
    /* linked node */
    queue_node: ListNode,

    /* read-only after being set up */
    paddr: paddr_t,  /* use paddr() accessor */

    /* offset 0x18 */

    pub object: vm_page_object_t,   /* attached to a vm object */

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
