/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::ptr::null_mut;

use crate::ZX_ASSERT;
use crate::klib::range::is_in_range;
use crate::locking::mutex::Mutex;
use crate::types::vaddr_t;
use crate::vm_page_state;
use crate::arch::mmu::zero_page;
use crate::defines::{PAGE_SIZE, paddr_to_physmap};
use crate::errors::ErrNO;
use crate::klib::list::List;
use crate::page::{vm_page_t, vm_page, vm_page_object};
use super::page_source::PageSource;
use super::vm_page_list::{VmPageList, VmPageOrMarker};
use crate::pmm::pmm_page_queues;
use crate::debug::*;

#[allow(dead_code)]
type VmCowPagesPtr = *mut VmCowPages;

/* Controls the type of content that can be overwritten by
 * the Add[New]Page[s]Locked functions. */
pub enum CanOverwriteContent {
    // Do not overwrite any kind of content, i.e. only add a page at the slot if there is true
    // absence of content.
    #[allow(dead_code)]
    None,
    // Only overwrite slots that represent zeros. In the case of anonymous VMOs, both gaps and zero
    // page markers represent zeros, as the entire VMO is implicitly zero on creation. For pager
    // backed VMOs, zero page markers and gaps after supply_zero_offset_ represent zeros.
    Zero,
    // Overwrite any slots, regardless of the type of content.
    #[allow(dead_code)]
    NonZero,
}

pub struct VmCowPages {
    #[allow(dead_code)]
    base: vaddr_t,
    size: usize,
    options: u32,
    #[allow(dead_code)]
    pmm_alloc_flags: u32,
    page_list: Mutex<VmPageList>,
    page_source: *mut PageSource,
    /* Counts the total number of pages pinned by ::CommitRange.
     * If one page is pinned n times, it contributes n to this count. */
    pinned_page_count: usize,
}

impl VmCowPages {
    // Externally-usable flags:
    pub const K_NONE: u32 = 0;

    // With this clear, zeroing a page tries to decommit the page.  With this set, zeroing never
    // decommits the page.  Currently this is only set for contiguous VMOs.
    //
    // TODO(dustingreen): Once we're happy with the reliability of page borrowing, we should be able
    // to relax this restriction.  We may still need to flush zeroes to RAM during reclaim to mitigate
    // a hypothetical client incorrectly assuming that cache-clean status will remain intact while
    // pages aren't pinned, but that mitigation should be sufficient (even assuming such a client) to
    // allow implicit decommit when zeroing or when zero scanning, as long as no clients are doing DMA
    // to/from contiguous while not pinned.
    #[allow(dead_code)]
    pub const K_CANNOT_DECOMMIT_ZERO_PAGES: u32 = 1 << 0;

    // Internal-only flags:
    pub const K_HIDDEN:         u32 = 1 << 1;
    pub const K_SLICE:          u32 = 1 << 2;
    #[allow(dead_code)]
    pub const K_UNPIN_ON_DELETE:u32 = 1 << 3;

    pub const K_INTERNAL_ONLY_MASK: u32 = Self::K_HIDDEN | Self::K_SLICE;

    fn new(options: u32, pmm_alloc_flags: u32, size: usize) -> Self {
        Self {
            base: 0,
            size,
            options,
            pmm_alloc_flags,
            page_list: Mutex::new(VmPageList::new()),
            page_source: null_mut(),
            pinned_page_count: 0,
        }
    }

    pub fn create(options: u32, pmm_alloc_flags: u32, size: usize)
        -> Result<VmCowPages, ErrNO>
    {
        ZX_ASSERT!((options & Self::K_INTERNAL_ONLY_MASK) == 0);
        let cow = Self::new(options, pmm_alloc_flags, size);
        Ok(cow)
    }

    pub fn add_new_pages(&mut self, start_offset: usize,
                         pages: &mut List<vm_page_t>,
                         overwrite: CanOverwriteContent,
                         zero: bool, do_range_update: bool)
        -> Result<(), ErrNO>
    {
        ZX_ASSERT!(!matches!(overwrite, CanOverwriteContent::NonZero));
        ZX_ASSERT!(IS_PAGE_ALIGNED!(start_offset));

        let mut offset = start_offset;
        loop {
            let p = pages.pop_head();
            if p == null_mut() {
                break;
            }
            /* Defer the range change update by passing false
             * as we will do it in bulk at the end if needed. */
            self.add_new_page(offset, p, &overwrite, None, zero, false)?;

            offset += PAGE_SIZE;
        }

        if do_range_update {
            /* other mappings may have covered this offset into the vmo,
             * so unmap those ranges */
            // RangeChangeUpdateLocked(start_offset, offset - start_offset, RangeChangeOp::Unmap);
            todo!("do_range_update!");
        }

        Ok(())
    }

    fn zero_page(page_ptr: *mut vm_page_t) {
        let pa = unsafe { (*page_ptr).paddr() };
        let va = paddr_to_physmap(pa);
        ZX_ASSERT!(va != 0);
        zero_page(va);
    }

    fn init_vm_page(p: *mut vm_page_t) {
        unsafe {
            ZX_ASSERT!((*p).state() == vm_page_state::ALLOC);
            (*p).set_state(vm_page_state::OBJECT);
            (*p).object.set_pin_count(0);
            (*p).object.set_cow_left_split(false);
            (*p).object.set_cow_right_split(false);
            (*p).object.set_always_need(false);
            (*p).object.set_dirty_state(vm_page_object::DIRTY_STATE_UNTRACKED);
        }
    }

    #[allow(dead_code)]
    fn is_user_pager_backed(&self) -> bool {
        if self.page_source.is_null() {
            return false;
        }
        todo!("self.page_source.properties().is_user_pager");
    }

    fn is_source_preserving_page_content(&self) -> bool {
        if self.page_source.is_null() {
            return false;
        }
        todo!("is_source_preserving_page_content");
    }

    fn add_new_page(&mut self, offset: usize, page: *mut vm_page_t,
                    overwrite: &CanOverwriteContent,
                    released_page: Option<&mut VmPageOrMarker>,
                    zero: bool, do_range_update: bool)
        -> Result<(), ErrNO>
    {
        ZX_ASSERT!(IS_PAGE_ALIGNED!(offset));

        Self::init_vm_page(page);
        if zero {
            Self::zero_page(page);
        }

        /* Pages being added to pager backed VMOs should have
         * a valid dirty_state before being added to the page list,
         * so that they can be inserted in the correct page queue.
         * New pages start off clean. */
        if self.is_source_preserving_page_content() {
            todo!("is_source_preserving_page_content");
            /* Only zero pages can be added as new pages to
             * pager backed VMOs. */
            /*
            ZX_ASSERT!(zero || IsZeroPage(page));
            UpdateDirtyStateLocked(page, offset, DirtyState::Clean, /*is_pending_add=*/true);
            */
        }

        let p = VmPageOrMarker::as_page(page);
        self.add_page(&p, offset, overwrite, released_page, do_range_update)
    }

    fn add_page(&mut self, p: &VmPageOrMarker, offset: usize,
                overwrite: &CanOverwriteContent,
                released_page: Option<&mut VmPageOrMarker>,
                do_range_update: bool)
        -> Result<(), ErrNO>
    {
        if p.is_page() {
            println!("vmo offset {}, page (0x{:x}))\n",
                     offset, unsafe { (*p.page()).paddr() });
        } else if p.is_reference() {
            todo!("is_reference");
        } else {
            ZX_ASSERT!(p.is_marker());
        }

        if let Some(r) = released_page {
            r.set_empty();
        }

        if offset >= self.size {
            return Err(ErrNO::OutOfRange);
        }

        let mut pl = self.page_list.lock();
        let page = pl.lookup_or_allocate(offset)?;

        /* We cannot overwrite any kind of content. */
        if matches!(overwrite, CanOverwriteContent::None) {
            todo!("CanOverwriteContent::None!");
        }

        // We're only permitted to overwrite zero content. This has different meanings based on the
        // whether the VMO is anonymous or is backed by a pager.
        //
        //  * For anonymous VMOs, the initial content for the entire VMO is implicitly all zeroes at the
        //  time of creation. So both zero page markers and empty slots represent zero content. Therefore
        //  the only content type that cannot be overwritten in this case is an actual page.
        //
        //  * For pager backed VMOs, content is either explicitly supplied by the user pager before
        //  supply_zero_offset_, or implicitly supplied as zeros beyond supply_zero_offset_. So zero
        //  content is represented by either zero page markers before supply_zero_offset_ (supplied by the
        //  user pager), or by gaps after supply_zero_offset_ (supplied by the kernel). Therefore the only
        //  content type that cannot be overwritten in this case as well is an actual page.
        if matches!(overwrite, CanOverwriteContent::Zero) && page.is_page_or_ref() {
            todo!("CanOverwriteContent::Zero! and page or ref!");
        }

        /* If the old entry is actual content, release it. */
        if page.is_page_or_ref() {
            todo!("is page or ref!");
        }

        // If the new page is an actual page and we have a page source,
        // the page source should be able to validate the page.
        // Note that having a page source implies that any content must be an actual page and so
        // although we return an error for any kind of content, the debug check only gets run for page
        // sources where it will be a real page.
        ZX_ASSERT!(!p.is_page_or_ref() || self.page_source.is_null());

        // If this is actually a real page, we need to place it into the appropriate queue.
        if p.is_page() {
            let low_level_page = p.page();
            unsafe {
                ZX_ASSERT!((*low_level_page).state() == vm_page_state::OBJECT);
                ZX_ASSERT!((*low_level_page).object.pin_count() == 0);
                self.set_not_wired_locked(low_level_page, offset);
            }
        }

        page.set(p);

        if do_range_update {
            /* other mappings may have covered this offset into the vmo,
             * so unmap those ranges */
            // RangeChangeUpdateLocked(offset, PAGE_SIZE, RangeChangeOp::Unmap);
            todo!("do_range_update!");
        }

        Ok(())
    }

    fn set_not_wired_locked(&self, page: *mut vm_page_t, offset: usize) {
        if self.is_source_preserving_page_content() {
            todo!("is_source_preserving_page_content!");
            /*
            DEBUG_ASSERT(is_page_dirty_tracked(page));
            // We can only move Clean pages to the pager backed queues as they track age information for
            // eviction; only Clean pages can be evicted. Pages in AwaitingClean and Dirty are protected
            // from eviction in the Dirty queue.
            if (is_page_clean(page)) {
                pmm_page_queues()->SetPagerBacked(page, this, offset);
            } else {
                DEBUG_ASSERT(!page->is_loaned());
                pmm_page_queues()->SetPagerBackedDirty(page, this, offset);
            }
            */
        } else {
            let object = &(*self) as *const VmCowPages as usize;
            pmm_page_queues().set_anonymous(page, object, offset);
        }
    }

    pub fn pin_range(&mut self, offset: usize, len: usize) -> Result<(), ErrNO> {
        dprintf!(INFO, "pin_range: offset 0x{:x}, len 0x{:x}\n", offset, len);

        ZX_ASSERT!(IS_PAGE_ALIGNED!(offset));
        ZX_ASSERT!(IS_PAGE_ALIGNED!(len));
        ZX_ASSERT!(is_in_range(offset, len, 0, self.size));

        if self.is_slice_locked() {
            todo!("is_slice_locked!");
        }

        /* Tracks our expected page offset when iterating to
         * ensure all pages are present. */
        let mut next_offset = offset;

        let mut per_page_func = |p: &VmPageOrMarker, page_offset| {
            if page_offset != next_offset || !p.is_page() {
                return Err(ErrNO::BadState);
            }
            let page = unsafe { &mut (*p.page()) };
            ZX_ASSERT!(page.state() == vm_page_state::OBJECT);
            ZX_ASSERT!(!page.is_loaned());

            if page.object.pin_count == vm_page::VM_PAGE_OBJECT_MAX_PIN_COUNT as u8 {
                return Err(ErrNO::BadState);
            }

            page.object.pin_count += 1;
            if page.object.pin_count == 1 {
                Self::move_to_wired_locked(page, page_offset);
            }

            next_offset += PAGE_SIZE;
            return Ok(());
        };

        let pl = self.page_list.lock();
        pl.for_every_page_in_range(&mut per_page_func, offset, offset + len)?;

        let actual = (next_offset - offset) / PAGE_SIZE;
        /* Count whatever pages we pinned, in the failure scenario
         * this will get decremented on the unpin. */
        self.pinned_page_count += actual;

        /* If the missing pages were at the end of the range
         * (or the range was empty) then our iteration will have just
         * returned ZX_OK. Perform one final check that we actually
         * pinned the number of pages we expected to. */
        let expected = len / PAGE_SIZE;
        if actual != expected {
            return Err(ErrNO::BadState);
        }

        dprintf!(INFO, "pin_range!");
        Ok(())
    }

    fn move_to_wired_locked(page: *mut vm_page_t, _offset: usize) {
        pmm_page_queues().move_to_wired(page);
    }

    fn is_slice_locked(&self) -> bool {
        (self.options & Self::K_SLICE) != 0
    }

}
