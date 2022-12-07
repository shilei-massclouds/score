/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]
#![allow(non_camel_case_types)]

/*
 * Defines the state of a VM page (|vm_page_t|).
 * Be sure to keep this enum in sync with the definition of |vm_page_t|.
 */
pub const FREE:     u8 = 0;
pub const ALLOC:    u8 = 1;
pub const OBJECT:   u8 = 2;
pub const WIRED:    u8 = 3;
pub const HEAP:     u8 = 4;
pub const MMU:      u8 = 5; /* serve arch-specific mmu purposes */
pub const IOMMU:    u8 = 6; /* platform-specific iommu structures */
pub const IPC:      u8 = 7;
pub const CACHE:    u8 = 8;
pub const SLAB:     u8 = 9;

pub const COUNT_:   u8 = 10;

pub type vm_page_state_t = u8;
