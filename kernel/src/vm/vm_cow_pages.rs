/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::ptr::null_mut;

use crate::types::vaddr_t;
use crate::{ZX_ASSERT, vm_page_state};
use crate::arch::mmu::zero_page;
use crate::defines::{PAGE_SIZE, paddr_to_physmap};
use crate::errors::ErrNO;
use crate::klib::list::List;
use crate::page::{vm_page_t, vm_page_object};
use super::page_source::PageSource;
use super::vm_page_list::{VmPageList, VmPageOrMarker};

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
    #[allow(dead_code)]
    options: u32,
    #[allow(dead_code)]
    pmm_alloc_flags: u32,
    page_list: VmPageList,
    page_source: *mut PageSource,
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
            page_list: VmPageList::new(),
            page_source: null_mut(),
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
                         overwrite: CanOverwriteContent, zero: bool,
                         _do_range_update: bool)
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

        todo!("add_new_pages!");
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

        let mut p = VmPageOrMarker::as_page(page);
        self.add_page(&mut p, offset, overwrite, released_page, do_range_update)
    }

    fn add_page(&mut self, p: &mut VmPageOrMarker, offset: usize,
                _overwrite: &CanOverwriteContent,
                released_page: Option<&mut VmPageOrMarker>,
                _do_range_update: bool)
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

        let _page = self.page_list.lookup_or_allocate(offset)?;

        todo!("add_page!");
    }

    pub fn pin_range(&self, _offset: usize, _len: usize) -> Result<(), ErrNO> {
        todo!("add_new_pages!");
    }

}
