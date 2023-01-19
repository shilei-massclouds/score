/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::ZX_ASSERT;
use crate::types::*;
use crate::klib::list::{Linked, ListNode};
use crate::vm_page_state;
use crate::vm_page_state::vm_page_state_t;
use core::sync::atomic::{fence, AtomicU8, Ordering, AtomicUsize};

  // logically private, use loaned getters and setters below.
#[allow(non_upper_case_globals)]
const kLoanedStateIsLoaned: u8 = 1;
#[allow(non_upper_case_globals)]
const _kLoanedStateIsLoanCancelled: u8 = 2;


#[allow(non_camel_case_types)]
pub struct vm_page_object {
    object_or_stack_owner: AtomicUsize,

    // When object_or_event_priv is pointing to a VmCowPages, this is the offset in the VmCowPages
    // that contains this page.
    //
    // Else this field is 0.
    //
    // Field should be modified by the setters and getters to allow for future encoding changes.
    page_offset_priv: usize,

    // Identifies which queue this page is in.
    pub page_queue: AtomicU8,

    pub pin_count: u8,

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

    const K_OBJECT_OR_STACK_OWNER_IS_STACK_OWNER_FLAG:  usize = 0x1;
    #[allow(dead_code)]
    const K_OBJECT_OR_STACK_OWNER_HAS_WAITER:           usize = 0x2;
    #[allow(dead_code)]
    const K_OBJECT_OR_STACK_OWNER_FLAGS:                usize = 0x3;

    #[allow(dead_code)]
    const fn new() -> Self {
        Self {
            object_or_stack_owner: AtomicUsize::new(0),
            page_offset_priv: 0,
            page_queue: AtomicU8::new(0),
            pin_count: 0,
            dirty_state: Self::DIRTY_STATE_UNTRACKED,
        }
    }

    fn is_stack_owned(&self) -> bool {
        /* This can return true for a page that was loaned fairly recently
         * but is no longer loaned. */
        let value = self.object_or_stack_owner.load(Ordering::Relaxed);
        (value & Self::K_OBJECT_OR_STACK_OWNER_IS_STACK_OWNER_FLAG) != 0
    }

    pub fn get_object(&self) -> usize {
        let value = self.object_or_stack_owner.load(Ordering::Relaxed);
        if (value & Self::K_OBJECT_OR_STACK_OWNER_IS_STACK_OWNER_FLAG) != 0 {
            return 0;
        }
        value
    }

    /* This also logically does clear_stack_owner() atomically. */
    pub fn set_object(&mut self, obj: usize) {
        /* If the caller wants to clear the object, use clear_object() instead. */
        ZX_ASSERT!(obj != 0);
        fence(Ordering::Release);
        if self.is_stack_owned() {
            self.clear_stack_owner_internal(obj);
            return;
        }
        self.object_or_stack_owner.store(obj, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    fn clear_stack_owner(&self) {
        self.clear_stack_owner_internal(0);
    }

    fn clear_stack_owner_internal(&self, obj: usize) {
        // If this fires, it likely means there's an extra clear somewhere, possibly by the current
        // thread, or possibly by a different thread.  This call could be the "extra" clear if the
        // caller didn't check whether there's a stack owner before calling.
        ZX_ASSERT!(self.is_stack_owned());
        loop {
            let old_value = self.object_or_stack_owner.load(Ordering::Relaxed);
            // If this fires, it likely means that some other thread did a clear (so either this
            // thread or the other thread shouldn't have cleared).  If this thread had already done a
            // previous clear, the assert near the top would have fired instead.
            ZX_ASSERT!((old_value & Self::K_OBJECT_OR_STACK_OWNER_IS_STACK_OWNER_FLAG) != 0);
            // We don't want to be acquiring thread_lock here every time we free a loaned page, so we
            // only acquire the thread_lock if the page's StackOwnedLoanedPagesInterval has a waiter,
            // which is much more rare.  In that case we must acquire the thread_lock to avoid letting
            // this thread continue and signal and delete the StackOwnedLoanedPagesInterval until
            // after the waiter has finished blocking on the OwnedWaitQueue, so that the waiter can be
            // woken and removed from the OwnedWaitQueue before the OwnedWaitQueue is deleted.
            /*
            ktl::optional<Guard<MonitoredSpinLock, IrqSave>> maybe_thread_lock_guard;
            if (old_value & kObjectOrStackOwnerHasWaiter) {
                // Acquire thread_lock.
                maybe_thread_lock_guard.emplace(ThreadLock::Get(), SOURCE_TAG);
            }
            */
            if self.object_or_stack_owner.compare_exchange_weak(old_value, obj,
                Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                break;
            }
            // ~maybe_thread_lock_guard will release thread_lock if it was acquired
        }
    }

    pub fn get_page_offset(&self) -> usize {
        self.page_offset_priv
    }

    pub fn set_page_offset(&mut self, page_offset: usize) {
        self.page_offset_priv = page_offset;
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
    pub const VM_PAGE_OBJECT_PIN_COUNT_BITS: usize = 5;
    pub const VM_PAGE_OBJECT_MAX_PIN_COUNT: usize =
        (1 << Self::VM_PAGE_OBJECT_PIN_COUNT_BITS) - 1;

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
