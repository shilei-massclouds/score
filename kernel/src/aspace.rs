/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::vec::Vec;
use spin::Mutex;
use core::cmp::max;
use crate::defines::*;
use crate::debug::*;
use crate::vm::*;
use crate::{KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE};
use crate::{ErrNO, types::vaddr_t, ZX_DEBUG_ASSERT};
use crate::allocator::boot_heap_mark_pages_in_use;
use crate::pmm::pmm_alloc_page;
use crate::vm_page_state;
use crate::arch::mmu::arch_zero_page;

/* Allow VmMappings to be created inside the new region with the SPECIFIC
 * or OFFSET_IS_UPPER_LIMIT flag. */
const VMAR_FLAG_CAN_MAP_SPECIFIC: usize = 1 << 3;
/* When on a VmAddressRegion, allow VmMappings to be created inside the region
 * with read permissions.  When on a VmMapping, controls whether or not the
 * mapping can gain this permission. */
const VMAR_FLAG_CAN_MAP_READ: usize = 1 << 4;
/* When on a VmAddressRegion, allow VmMappings to be created inside the region
 * with write permissions.  When on a VmMapping, controls whether or not the
 * mapping can gain this permission. */
const VMAR_FLAG_CAN_MAP_WRITE: usize = 1 << 5;
/* When on a VmAddressRegion, allow VmMappings to be created inside the region
 * with execute permissions.  When on a VmMapping, controls whether or not the
 * mapping can gain this permission. */
const VMAR_FLAG_CAN_MAP_EXECUTE: usize = 1 << 6;

const VMAR_CAN_RWX_FLAGS: usize = VMAR_FLAG_CAN_MAP_READ |
    VMAR_FLAG_CAN_MAP_WRITE | VMAR_FLAG_CAN_MAP_EXECUTE;

#[allow(dead_code)]
pub enum VmAspaceType {
    User,
    Kernel,
    /* You probably do not want to use LOW_KERNEL. It is primarily used
     * for SMP bootstrap or mexec to allow mappings of very low memory
     * using the standard VMM subsystem. */
    LowKernel,
    /* an address space representing hypervisor guest memory */
    GuestPhysical,
}

struct VmAspaceList {
    inner: Vec<VmAspace>,
}

impl VmAspaceList {
    const fn new() -> Self {
        Self {
            inner: Vec::new(),
        }
    }

    fn push(&mut self, aspace: VmAspace) {
        self.inner.push(aspace);
    }

    fn get_aspace_by_id(&mut self, id: usize) -> &mut VmAspace {
        &mut self.inner[id]
    }
}

#[allow(dead_code)]
struct VmAspace {
    id: usize,
    as_type: VmAspaceType,
    base: vaddr_t,
    size: usize,
    root_vmar: Option<VmAddressRegion>,
}

impl VmAspace {
    fn new(id: usize, as_type: VmAspaceType,
        base: vaddr_t, size: usize) -> Self {
        Self {
            id,
            as_type,
            base,
            size,
            root_vmar: None,
        }
    }

    fn init(&self) -> Result<(), ErrNO> {
        /* initialize the architecturally specific part */
        /* zx_status_t status = arch_aspace_.Init()?; */
        /* InitializeAslr(); */

        if let None = self.root_vmar {
            todo!("CreateRootLocked for userspace!");
        }
        Ok(())
    }

    fn get_root_vmar(&mut self) -> &mut VmAddressRegion {
        if let Some(vmar) = &mut self.root_vmar {
            return vmar;
        }
        panic!("no root vmar!");
    }
}

struct VmAddressRegion {
    base: vaddr_t,
    size: usize,
    flags: usize,
    children: Vec<VmAddressRegion>,
}

impl VmAddressRegion {
    const fn new() -> Self {
        Self {
            base: 0,
            size: 0,
            flags: 0,
            children: Vec::new(),
        }
    }

    fn init(&mut self, base: vaddr_t, size: usize, flags: usize) {
        self.base = base;
        self.size = size;
        self.flags = flags;
    }

    fn cover_range(&self, base: vaddr_t, size: usize) -> bool {
        /*
         * NOTE: DON'T compare end of the range directly, as:
         * (base + size) <= (self.base + self.size)
         * Typically, the value end may overbound and become ZERO!
         */
        let offset = base - self.base;
        base >= self.base && offset < self.size && self.size - offset >= size
    }

    fn insert_child(&mut self, child: Self) {
        /* Validate we are a correct child of our parent. */
        ZX_DEBUG_ASSERT!(self.cover_range(child.base, child.size));

        let start = child.base;
        let end = start + child.size;
        match self.children.iter().position(|r| r.base >= end) {
            Some(index) => self.children.insert(index, child),
            None => self.children.push(child),
        }
    }

    /*
     * Perform allocations for VMARs. This allocator works by choosing uniformly
     * at random from a set of positions that could satisfy the allocation.
     * The set of positions are the 'left' most positions of the address space
     * and are capped by the address entropy limit. The entropy limit is retrieved
     * from the address space, and can vary based on whether the user has
     * requested compact allocations or not.
     */
    fn alloc_spot_locked(&mut self, size: usize, align_pow2: usize,
                         _arch_mmu_flags: usize, upper_limit: vaddr_t)
        -> vaddr_t {
        ZX_DEBUG_ASSERT!(size > 0 && IS_PAGE_ALIGNED!(size));
        dprintf!(INFO, "aspace size 0x{:x} align {} upper_limit 0x{:x}\n",
                 size, align_pow2, upper_limit);

        let align_pow2 = max(align_pow2, PAGE_SHIFT);
        let alloc_spot = self.get_alloc_spot(align_pow2, size,
            self.base, self.size, upper_limit);
        /* Sanity check that the allocation fits. */
        let (_, overflowed) = alloc_spot.overflowing_add(size - 1);
        ZX_DEBUG_ASSERT!(!overflowed);
        return alloc_spot;
    }

    /* Get the allocation spot that is free and large enough for the aligned size. */
    fn get_alloc_spot(&mut self, align_pow2: usize, size: usize,
        parent_base: vaddr_t, parent_size: usize, upper_limit: vaddr_t) -> vaddr_t {
        let (alloc_spot, found) =
            self.find_alloc_spot_in_gaps(size, align_pow2, parent_base, parent_size, upper_limit);
        ZX_DEBUG_ASSERT!(found);

        let align: vaddr_t = 1 << align_pow2;
        ZX_DEBUG_ASSERT!(IS_ALIGNED!(alloc_spot, align));
        return alloc_spot;
    }

    /* Try to find the spot among all the gaps. */
    fn find_alloc_spot_in_gaps(&mut self, size: usize, align_pow2: usize,
        parent_base: vaddr_t, parent_size: vaddr_t, upper_limit: vaddr_t) -> (vaddr_t, bool) {
        let align = 1 << align_pow2;
        /* Found indicates whether we have found the spot with index |selected_indexes|. */
        let mut found = false;
        /* alloc_spot is the virtual start address of the spot to allocate if we find one. */
        let mut alloc_spot: vaddr_t = 0;
        let func = |gap_base: vaddr_t, gap_len: usize| {
            ZX_DEBUG_ASSERT!(IS_ALIGNED!(gap_base, align));
            if gap_len < size || gap_base + size > upper_limit {
                /* Ignore gap that is too small or out of range. */
                return true;
            }
            found = true;
            alloc_spot = gap_base;
            return false;
        };

        self.for_each_gap(func, align_pow2, parent_base, parent_size);

        (alloc_spot, found)
    }

    /* Utility for allocators for iterating over gaps between allocations.
     * F should have a signature of bool func(vaddr_t gap_base, size_t gap_size).
     * If func returns false, the iteration stops.
     * And gap_base will be aligned in accordance with align_pow2. */
    fn for_each_gap<F>(&mut self, mut func: F, align_pow2: usize, parent_base: vaddr_t, parent_size: usize)
    where F: FnMut(usize, usize) -> bool {
        let align = 1 << align_pow2;

        /* Scan the regions list to find the gap to the left of each region.
         * We round up the end of the previous region to the requested alignment,
         * so all gaps reported will be for aligned ranges. */
        let mut prev_region_end = ROUNDUP!(parent_base, align);
        for child in &self.children {
            if child.base > prev_region_end {
                let gap = child.base - prev_region_end;
                if !func(prev_region_end, gap) {
                    return;
                }
            }
            let (end, ret) = child.base.overflowing_add(child.size);
            if ret {
                /* This region is already the last region. */
                return;
            }
            prev_region_end = ROUNDUP!(end, align);
        }

        /* Grab the gap to the right of the last region. Note that if there are
         * no regions, this handles reporting the VMAR's whole span as a gap. */
         if parent_size > prev_region_end - parent_base {
            /* This is equal to parent_base + parent_size - prev_region_end,
             * but guarantee no overflow. */
            let gap = parent_size - (prev_region_end - parent_base);
            func(prev_region_end, gap);
        }
    }

}

struct BootContext {
    vm_aspace_list: Option<VmAspaceList>,
    kernel_heap_base: usize,
    kernel_heap_size: usize,
}

impl BootContext {
    const fn new() -> Self {
        Self {
            vm_aspace_list: None,
            kernel_heap_base: 0,
            kernel_heap_size: 0,
        }
    }

    fn get_aspace_by_id(&mut self, id: usize) -> &mut VmAspace {
        if let Some(aspaces) = &mut self.vm_aspace_list {
            return aspaces.get_aspace_by_id(id);
        }
        panic!("NOT init aspaces yet!");
    }
}

static BOOT_CONTEXT: Mutex<BootContext> = Mutex::new(BootContext::new());

pub fn vm_init_preheap() -> Result<(), ErrNO> {
    /* allow the vmm a shot at initializing some of its data structures */
    kernel_aspace_init_preheap()?;

    vm_init_preheap_vmars();

    /* mark the physical pages used by the boot time allocator */
    boot_heap_mark_pages_in_use();

    // grab a page and mark it as the zero page
    let zero_page = pmm_alloc_page(0);
    if let Some(mut page) = zero_page {
        /* consider the zero page a wired page part of the kernel. */
        unsafe {
            page.as_mut().set_state(vm_page_state::WIRED);
            let va = paddr_to_physmap(page.as_ref().paddr());
            ZX_DEBUG_ASSERT!(va != 0);
            arch_zero_page(va);
        }
    } else {
        panic!("alloc zero page error!");
    }

    /* AnonymousPageRequester::Init(); */
    dprintf!(INFO, "prevm\n");
    Ok(())
}

fn vm_init_preheap_vmars() {
    /*
     * For VMARs that we are just reserving we request full RWX permissions.
     * This will get refined later in the proper vm_init.
     */
    let flags = VMAR_FLAG_CAN_MAP_SPECIFIC | VMAR_CAN_RWX_FLAGS;

    let mut kernel_physmap_vmar= VmAddressRegion::new();
    kernel_physmap_vmar.init(PHYSMAP_BASE, PHYSMAP_SIZE, flags);

    let mut ctx = BOOT_CONTEXT.lock();
    let kernel_aspace = ctx.get_aspace_by_id(0);
    let root_vmar = kernel_aspace.get_root_vmar();

    root_vmar.insert_child(kernel_physmap_vmar);

    /*
     * |kernel_image_size| is the size in bytes of the region of memory occupied by
     * the kernel program's various segments (code, rodata, data, bss, etc.),
     * inclusive of any gaps between them.
     */
    let kernel_image_size = kernel_size();

    /*
     * Create a VMAR that covers the address space occupied by
     * the kernel program segments (code, * rodata, data, bss ,etc.).
     * By creating this VMAR, we are effectively marking these addresses
     * as off limits to the VM. That way, the VM won't inadvertently use them
     * for something else. This is consistent with the initial mapping in start.S
     * where the whole kernel region mapping was written into the page table.
     *
     * Note: Even though there might be usable gaps in between the segments, we're covering the whole
     * regions. The thinking is that it's both simpler and safer to not use the address space that
     * exists between kernel program segments.
     */
    let mut kernel_image_vmar= VmAddressRegion::new();
    kernel_image_vmar.init(kernel_regions_base(), kernel_image_size, flags);
    root_vmar.insert_child(kernel_image_vmar);

    /* Reserve the range for the heap. */
    let heap_bytes = ROUNDUP!(HEAP_MAX_SIZE_MB * MB, 1 << ARCH_HEAP_ALIGN_BITS);
    let kernel_heap_base =
        root_vmar.alloc_spot_locked(heap_bytes, ARCH_HEAP_ALIGN_BITS,
            ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_WRITE,
            usize::MAX);

    /*
     * The heap has nothing to initialize later and we can create this
     * from the beginning with only read and write and no execute.
     */
    let mut kernel_heap_vmar= VmAddressRegion::new();
    kernel_heap_vmar.init(kernel_heap_base, heap_bytes,
        VMAR_FLAG_CAN_MAP_SPECIFIC | VMAR_FLAG_CAN_MAP_READ | VMAR_FLAG_CAN_MAP_WRITE);

    dprintf!(INFO, "VM: kernel heap placed in range [{:x}, {:x})\n",
             kernel_heap_vmar.base, kernel_heap_vmar.base + kernel_heap_vmar.size);
    root_vmar.insert_child(kernel_heap_vmar);

    ctx.kernel_heap_base = kernel_heap_base;
    ctx.kernel_heap_size = heap_bytes;
}

fn kernel_aspace_init_preheap() -> Result<(), ErrNO> {
    let mut kernel_aspace =
        VmAspace::new(0, VmAspaceType::Kernel,
            KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE);

    let flags = VMAR_FLAG_CAN_MAP_SPECIFIC | VMAR_CAN_RWX_FLAGS;
    let mut root_vmar = VmAddressRegion::new();
    root_vmar.init(KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE, flags);

    kernel_aspace.root_vmar = Some(root_vmar);
    kernel_aspace.init()?;

    let mut aspaces = VmAspaceList::new();
    aspaces.push(kernel_aspace);
    BOOT_CONTEXT.lock().vm_aspace_list = Some(aspaces);
    dprintf!(INFO, "kernel_aspace_init_preheap ok!\n");

    Ok(())
}

/* Request the heap dimensions. */
pub fn vm_get_kernel_heap_base() -> usize {
    BOOT_CONTEXT.lock().kernel_heap_base
}

pub fn vm_get_kernel_heap_size() -> usize {
    BOOT_CONTEXT.lock().kernel_heap_size
}
