/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::errors::ErrNO;
use crate::page::vm_page_t;
use crate::ZX_ASSERT;
use crate::types::*;
use crate::BIT_MASK;
use crate::debug::*;
use crate::defines::{PAGE_SIZE, PAGE_SHIFT};
use rbtree::RBTree;

// RAII helper for representing content in a page list node. This supports being in one of three
// states
//  * Empty       - Contains nothing
//  * Page p      - Contains a vm_page 'p'. This 'p' is considered owned by this wrapper and
//                  `ReleasePage` must be called to give up ownership.
//  * Reference r - Contains a reference 'r' to some content. This 'r' is considered owned by this
//                  wrapper and `ReleaseReference` must be called to give up ownership.
//  * Marker      - Indicates that whilst not a page, it is also not empty. Markers can be used to
//                  separate the distinction between "there's no page because we've deduped to the
//                  zero page" and "there's no page because our parent contains the content".
#[derive(Clone, Copy)]
pub struct VmPageOrMarker {
    raw: usize,
}

impl VmPageOrMarker {
    // The low 2 bits of raw_ are reserved to select the type, any other data has to fit into the
    // remaining high bits. Note that there is no explicit Empty type, rather a PageType with a zero
    // pointer is used to represent Empty.
    const K_TYPE_BITS: usize = 2;
    const K_PAGE_TYPE:          usize = 0b00;
    const K_ZERO_MARKER_TYPE:   usize = 0b01;
    const K_REFERENCE_TYPE:     usize = 0b10;

    pub const fn new(raw: usize) -> Self {
        Self {
            raw,
        }
    }

    /* reference to the underlying vm_page*.
     * Is only valid to call if `IsPage` is true. */
    pub fn page(&self) -> *mut vm_page_t {
        ZX_ASSERT!(self.is_page());
        /* Do not need to mask any bits out of raw_,
         * since Page has 0's for the type anyway. */
        self.raw as *mut vm_page_t
    }
    /*
    ReferenceValue Reference() const {
        DEBUG_ASSERT(IsReference());
        return ReferenceValue(raw_ & ~BIT_MASK(kReferenceBits));
    }
    */

    pub fn as_page(p: *mut vm_page_t) -> Self {
        /* A null page is incorrect for two reasons
         * 1. It's a violation of the API of this method
         * 2. A null page cannot be represented internally
         *    as this is used to represent Empty */
        ZX_ASSERT!(!p.is_null());
        let raw = p as vaddr_t;
        /* A pointer should be aligned by definition, and hence
         * the low bits should always be zero, but assert this anyway just
         * in case kTypeBits is increased or someone passed
         * an invalid pointer. */
        ZX_ASSERT!((raw & BIT_MASK!(Self::K_TYPE_BITS)) == 0);
        Self::new(raw | Self::K_PAGE_TYPE)
    }

    #[allow(dead_code)]
    pub const fn empty() -> Self {
        Self::new(Self::K_PAGE_TYPE)
    }
    #[allow(dead_code)]
    pub const fn marker() -> Self {
        Self::new(Self::K_ZERO_MARKER_TYPE)
    }

    pub fn set_empty(&mut self) {
        self.raw = Self::K_PAGE_TYPE;
    }

    fn get_type(&self) -> usize {
        self.raw & BIT_MASK!(Self::K_TYPE_BITS)
    }

    pub fn is_page(&self) -> bool {
        !self.is_empty() && (self.get_type() == Self::K_PAGE_TYPE)
    }
    pub fn is_marker(&self) -> bool {
        self.get_type() == Self::K_ZERO_MARKER_TYPE
    }
    pub fn is_empty(&self) -> bool {
        self.raw == Self::K_PAGE_TYPE
    }
    pub fn is_reference(&self) -> bool {
        self.get_type() == Self::K_REFERENCE_TYPE
    }
    #[allow(dead_code)]
    pub fn is_page_or_ref(&self) -> bool {
        self.is_page() || self.is_reference()
    }

}

pub struct VmPageListNode {
    obj_offset: usize,
    pages: [VmPageOrMarker; Self::K_PAGE_FAN_OUT],
}

impl VmPageListNode {
    const K_PAGE_FAN_OUT: usize = 16;

    pub const fn new(obj_offset: usize) -> Self {
        Self {
            obj_offset,
            pages: [VmPageOrMarker::empty(); Self::K_PAGE_FAN_OUT],
        }
    }

    #[allow(dead_code)]
    pub fn lookup(&self, index: usize) -> &VmPageOrMarker {
        ZX_ASSERT!(index < Self::K_PAGE_FAN_OUT);
        &self.pages[index]
    }

    pub fn lookup_mut(&mut self, index: usize) -> &mut VmPageOrMarker {
        ZX_ASSERT!(index < Self::K_PAGE_FAN_OUT);
        &mut self.pages[index]
    }
}

pub struct VmPageList {
    list: RBTree<usize, VmPageListNode>,

    /* A skew added to offsets provided as arguments to VmPageList functions
     * before interfacing with list_. This allows all VmPageLists within
     * a clone tree to place individual vm_page_t entries at the same offsets
     * within their nodes, so that the nodes can be moved between
     * different lists without having to worry about needing to
     * split up a node. */
    list_skew: usize,
}

impl VmPageList {
    /* Allow the implementation to use a one-past-the-end for
     * VmPageListNode offsets, plus to account for skew_. */
    const MAX_SIZE: usize =
        ROUNDDOWN!(usize::MAX, 2 * VmPageListNode::K_PAGE_FAN_OUT * PAGE_SIZE);

    pub const fn new() -> Self {
        Self {
            list: RBTree::new(),
            list_skew: 0,
        }
    }

    #[inline]
    fn offset_to_node_offset(offset: usize, skew: usize) -> usize {
        ROUNDDOWN!(offset + skew, PAGE_SIZE * VmPageListNode::K_PAGE_FAN_OUT)
    }

    #[inline]
    fn offset_to_node_index(offset: usize, skew: usize) -> usize {
        ((offset + skew) >> PAGE_SHIFT) % VmPageListNode::K_PAGE_FAN_OUT
    }

    pub fn lookup_or_allocate(&mut self, offset: usize)
        -> Result<&mut VmPageOrMarker, ErrNO>
    {
        let node_offset = Self::offset_to_node_offset(offset, self.list_skew);
        let index = Self::offset_to_node_index(offset, self.list_skew);

        if node_offset >= VmPageList::MAX_SIZE {
            return Err(ErrNO::OutOfRange);
        }

        dprintf!(INFO, "offset {} node_offset {} index {}\n",
                 offset, node_offset, index);

        if !self.list.contains_key(&node_offset) {
            let pl = VmPageListNode::new(node_offset);
            self.list.insert(node_offset, pl);
        }

        /* lookup the tree node that holds this page */
        if let Some(pln) = self.list.get_mut(&node_offset) {
            return Ok(pln.lookup_mut(index));
        }

        panic!("Bad VmPageListNode!");
    }
}