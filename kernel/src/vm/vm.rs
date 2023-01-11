/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::vec::Vec;
use spin::lazy::Lazy;
use crate::BOOT_CONTEXT;
use crate::ZX_ASSERT;
use crate::arch::mmu::PAGE_READ;
use crate::arch::mmu::PAGE_WRITE;
use crate::aspace::ASPACE_LIST;
use crate::errors::ErrNO;
use crate::pmm::PmmArena;
use crate::types::*;
use crate::defines::*;
use crate::debug::*;
use crate::pmm::pmm_alloc_range;
use crate::klib::list::List;
use crate::vm_page_state;

pub const ARCH_MMU_FLAG_CACHED:     usize = 0 << 0;
pub const _ARCH_MMU_FLAG_UNCACHED:   usize = 1 << 0;
pub const ARCH_MMU_FLAG_UNCACHED_DEVICE: usize = 2 << 0;
pub const _ARCH_MMU_FLAG_WRITE_COMBINING: usize = 3 << 0;
pub const _ARCH_MMU_FLAG_CACHE_MASK: usize = 3 << 0;

pub const _ARCH_MMU_FLAG_PERM_USER:      usize = 1 << 2;
pub const ARCH_MMU_FLAG_PERM_READ:      usize = 1 << 3;
pub const ARCH_MMU_FLAG_PERM_WRITE:     usize = 1 << 4;
pub const ARCH_MMU_FLAG_PERM_EXECUTE:   usize = 1 << 5;

// Permissions & flags for regions of the physmap that are not backed by memory; they
// may represent MMIOs or non-allocatable (ACPI NVS) memory. The kernel may access
// some peripherals in these addresses (such as MMIO-based UARTs) in early boot.
pub const GAP_MMU_FLAGS: usize = ARCH_MMU_FLAG_PERM_READ |
    ARCH_MMU_FLAG_PERM_WRITE | ARCH_MMU_FLAG_UNCACHED_DEVICE;

pub fn mmu_prot_from_flags(mmu_flags: usize) -> prot_t {
    let mask = ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_WRITE |
        ARCH_MMU_FLAG_UNCACHED_DEVICE;
    if (mmu_flags & !mask) != 0 {
        panic!("bad flags: 0x{:x}", mmu_flags);
    }

    let mut prot = 0;
    if (mmu_flags & ARCH_MMU_FLAG_PERM_READ) != 0 {
        prot |= PAGE_READ;
    }
    if (mmu_flags & ARCH_MMU_FLAG_PERM_WRITE) != 0 {
        prot |= PAGE_WRITE;
    }

    prot
}

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
        unsafe {
            (*page).set_state(vm_page_state::WIRED);
        }
    }
}

pub fn vm_init() -> Result<(), ErrNO> {
    // Protect the regions of the physmap that are not backed by normal memory.
    //
    // See the comments for |phsymap_protect_non_arena_regions| for why we're doing this.
    //
    physmap_protect_non_arena_regions();

    // Mark the physmap no-execute.
    physmap_protect_arena_regions_noexecute();

    /* Todo: vm_init! */
    Ok(())
}

// Protect the region [ |base|, |base| + |size| ) from the physmap.
fn physmap_protect_region(base: vaddr_t, size: usize, mmu_flags: usize) {
    ZX_ASSERT!(base % PAGE_SIZE == 0);
    ZX_ASSERT!(size % PAGE_SIZE == 0);
    let page_count = size / PAGE_SIZE;
    dprintf!(INFO, "base=0x{:x}; page_count=0x{:x}\n", base, page_count);

    {
        let aspace_list = ASPACE_LIST.lock();
        let kernel_aspace = aspace_list.head();
        unsafe {
            let status = (*kernel_aspace).protect(base, page_count, mmu_flags);
            ZX_ASSERT!(status.is_ok());
        }
    }
}

fn physmap_protect_non_arena_regions() {
    // Create a buffer to hold the pmm_arena_info_t objects.
    let pmm_node = BOOT_CONTEXT.pmm_node();
    let physmap_protect_gap = |base: vaddr_t, size: usize| {
        // Ideally, we'd drop the range completely, but early boot code currently relies
        // on peripherals being mapped in.
        //
        // TODO(fxbug.dev/47856): Remove these regions completely.
        physmap_protect_region(base, size, GAP_MMU_FLAGS);
    };
    physmap_for_each_gap(&physmap_protect_gap, pmm_node.get_arenas());
}

fn physmap_for_each_gap<F>(func: &F, arenas: &Vec<PmmArena>)
    where F: Fn(vaddr_t, usize) {
    // Iterate over the arenas and invoke |func| for the gaps between them.
    //
    // |gap_base| is the base address of the last identified gap.
    let mut gap_base = PHYSMAP_BASE;
    for arena in arenas {
        let arena_base = paddr_to_physmap(arena.base());
        ZX_ASSERT!(arena_base >= gap_base && arena_base % PAGE_SIZE == 0);

        let arena_size = arena.size();
        ZX_ASSERT!(arena_size > 0 && arena_size % PAGE_SIZE == 0);

        dprintf!(SPEW, "gap_base=0x{:x}; arena_base=0x{:x}; arena_size=0x{:x}\n",
                 gap_base, arena_base, arena_size);

        let gap_size = arena_base - gap_base;
        if gap_size > 0 {
            func(gap_base, gap_size);
        }
        gap_base = arena_base + arena_size;
    }

    // Don't forget the last gap.
    let physmap_end = PHYSMAP_BASE + PHYSMAP_SIZE;
    let gap_size = physmap_end - gap_base;
    if gap_size > 0 {
        func(gap_base, gap_size);
    }
}

fn physmap_protect_arena_regions_noexecute() {
}
/*
  const size_t num_arenas = pmm_num_arenas();
  fbl::AllocChecker ac;
  auto arenas = ktl::unique_ptr<pmm_arena_info_t[]>(new (&ac) pmm_arena_info_t[num_arenas]);
  ASSERT(ac.check());
  const size_t size = num_arenas * sizeof(pmm_arena_info_t);

  zx_status_t status = pmm_get_arena_info(num_arenas, 0, arenas.get(), size);
  ASSERT(status == ZX_OK);

  for (uint i = 0; i < num_arenas; i++) {
    physmap_protect_region(reinterpret_cast<vaddr_t>(paddr_to_physmap(arenas[i].base)),
                           /*size=*/arenas[i].size, /*mmu_flags=*/kPhysmapMmuFlags);
  }
}
*/