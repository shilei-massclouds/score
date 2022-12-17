/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use spin::lazy::Lazy;
use crate::types::*;
use crate::defines::*;
use crate::debug::*;
use crate::pmm::pmm_alloc_range;
use crate::klib::list::List;
use crate::vm_page_state;

#[allow(dead_code)]
pub const ARCH_MMU_FLAG_PERM_USER:      usize = 1 << 2;
pub const ARCH_MMU_FLAG_PERM_READ:      usize = 1 << 3;
pub const ARCH_MMU_FLAG_PERM_WRITE:     usize = 1 << 4;
pub const ARCH_MMU_FLAG_PERM_EXECUTE:   usize = 1 << 5;

/* List of the kernel program's various segments. */
#[allow(dead_code)]
struct KernelRegion {
    name: &'static str,
    base: vaddr_t,
    size: usize,
    arch_mmu_flags: usize,
}

/*
 * construct an array of kernel program segment descriptors for use here
 * and elsewhere
 */
static KERNEL_REGIONS: Lazy<[KernelRegion; 4]> = Lazy::new(|| [
    KernelRegion {
        name: "kernel_code",
        base: _text_start as vaddr_t,
        size: ROUNDUP!(_text_end as usize - _text_start as usize, PAGE_SIZE),
        arch_mmu_flags: ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_EXECUTE,
    },
    KernelRegion {
        name: "kernel_rodata",
        base: _rodata_start as vaddr_t,
        size: ROUNDUP!(_rodata_end as usize - _rodata_start as usize, PAGE_SIZE),
        arch_mmu_flags: ARCH_MMU_FLAG_PERM_READ,
    },
    KernelRegion {
        name: "kernel_data",
        base: _data_start as vaddr_t,
        size: ROUNDUP!(_data_end as usize - _data_start as usize, PAGE_SIZE),
        arch_mmu_flags: ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_WRITE,
    },
    KernelRegion {
        name: "kernel_bss",
        base: _bss_start as vaddr_t,
        size: ROUNDUP!(_bss_end as usize - _bss_start as usize, PAGE_SIZE),
        arch_mmu_flags: ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_WRITE,
    },
]);

pub fn kernel_regions_base() -> usize {
    KERNEL_REGIONS[0].base
}

// mark a range of physical pages as WIRED
#[allow(dead_code)]
pub fn mark_pages_in_use(pa: paddr_t, len: usize) {
    dprintf!(INFO, "pa {:x}, len {:x}\n", pa, len);

    /* make sure we are inclusive of all of the pages in the address range */
    let len = PAGE_ALIGN!(len + (pa & (PAGE_SIZE - 1)));
    let pa = ROUNDDOWN!(pa, PAGE_SIZE);

    dprintf!(INFO, "aligned pa {:x}, len {:x}\n", pa, len);

    let mut list = List::new();
    list.init();
    pmm_alloc_range(pa, len / PAGE_SIZE, &mut list).unwrap();

    /* mark all of the pages we allocated as WIRED */
    for page in list.iter_mut() {
        page.set_state(vm_page_state::WIRED);
    }
}
