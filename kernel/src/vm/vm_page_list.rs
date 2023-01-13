/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::page::vm_page_t;
use crate::ZX_ASSERT;
use crate::types::*;
use crate::BIT_MASK;

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

    pub fn Page(p: *mut vm_page_t) -> VmPageOrMarker {
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
}

pub struct VmPageList {
}

impl VmPageList {
    pub const fn new() -> Self {
        Self {
        }
    }
}