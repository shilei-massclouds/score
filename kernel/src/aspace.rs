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
use crate::vm::kernel_regions_base;
use crate::{KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE};
use crate::{ErrNO, types::vaddr_t, ZX_DEBUG_ASSERT};

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
    fn alloc_spot_locked(size: usize, align_pow2: usize, arch_mmu_flags: usize, upper_limit: vaddr_t)
        -> vaddr_t {
        ZX_DEBUG_ASSERT!(size > 0 && IS_PAGE_ALIGNED!(size));
        dprintf!(INFO, "aspace size 0x{:x} align {} upper_limit 0x{:x}\n",
                 size, align_pow2, upper_limit);

        let align_pow2 = max(align_pow2, PAGE_SHIFT);
        let align: vaddr_t = 1 << align_pow2;

        /*
  zx_status_t status = subregions_.GetAllocSpot(&alloc_spot, align_pow2, entropy, size, base_,
                                                size_, prng, upper_limit);
  if (status != ZX_OK) {
    return status;
  }

  // Sanity check that the allocation fits.
  vaddr_t alloc_last_byte;
  bool overflowed = add_overflow(alloc_spot, size - 1, &alloc_last_byte);
  ASSERT(!overflowed);
  auto after_iter = subregions_.UpperBound(alloc_last_byte);
  auto before_iter = after_iter;

  if (after_iter == subregions_.begin() || subregions_.IsEmpty()) {
    before_iter = subregions_.end();
  } else {
    --before_iter;
  }

  ASSERT(before_iter == subregions_.end() || before_iter.IsValid());
  VmAddressRegionOrMapping* before = nullptr;
  if (before_iter.IsValid()) {
    before = &(*before_iter);
  }
  VmAddressRegionOrMapping* after = nullptr;
  if (after_iter.IsValid()) {
    after = &(*after_iter);
  }
  if (auto va = CheckGapLocked(before, after, alloc_spot, align, size, 0, arch_mmu_flags)) {
    *spot = *va;
    return ZX_OK;
  }
  panic("Unexpected allocation failure\n");
  */

        return 0;
    }

}

struct BootContext {
    vm_aspace_list: Option<VmAspaceList>,
}

impl BootContext {
    const fn new() -> Self {
        Self {
            vm_aspace_list: None,
        }
    }

    fn get_aspace_by_id(&mut self, id: usize) -> &mut VmAspace {
        if let Some(aspaces) = &mut self.vm_aspace_list {
            return aspaces.get_aspace_by_id(id);
        }
        panic!("NOT init aspaces yet!");
    }
}

static boot_context: Mutex<BootContext> = Mutex::new(BootContext::new());

pub fn vm_init_preheap() -> Result<(), ErrNO> {
    /* allow the vmm a shot at initializing some of its data structures */
    kernel_aspace_init_preheap()?;

    vm_init_preheap_vmars();

    /*
  // mark the physical pages used by the boot time allocator
  if (boot_alloc_end != boot_alloc_start) {
    dprintf(INFO, "VM: marking boot alloc used range [%#" PRIxPTR ", %#" PRIxPTR ")\n",
            boot_alloc_start, boot_alloc_end);

    MarkPagesInUsePhys(boot_alloc_start, boot_alloc_end - boot_alloc_start);
  }

  zx_status_t status;

  // grab a page and mark it as the zero page
  status = pmm_alloc_page(0, &zero_page, &zero_page_paddr);
  DEBUG_ASSERT(status == ZX_OK);

  // consider the zero page a wired page part of the kernel.
  zero_page->set_state(vm_page_state::WIRED);

  void* ptr = paddr_to_physmap(zero_page_paddr);
  DEBUG_ASSERT(ptr);

  arch_zero_page(ptr);

  AnonymousPageRequester::Init();
  */
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

    let mut ctx = boot_context.lock();
    let mut kernel_aspace = ctx.get_aspace_by_id(0);
    let mut root_vmar = kernel_aspace.get_root_vmar();

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

    /*
    vaddr_t kernel_heap_base = 0;
    {
      Guard<CriticalMutex> guard(root_vmar->lock());
      zx_status_t status = root_vmar->AllocSpotLocked(
          heap_bytes, ARCH_HEAP_ALIGN_BITS, ARCH_MMU_FLAG_PERM_READ | ARCH_MMU_FLAG_PERM_WRITE,
          &kernel_heap_base);
      ASSERT_MSG(status == ZX_OK, "Failed to allocate VMAR for heap");
    }
    */

    /*
     * The heap has nothing to initialize later and we can create this
     * from the beginning with only read and write and no execute.
     */
    /*
    vmar = fbl::AdoptRef<VmAddressRegion>(&kernel_heap_vmar.Initialize(
        *root_vmar, kernel_heap_base, heap_bytes,
        VMAR_FLAG_CAN_MAP_SPECIFIC | VMAR_FLAG_CAN_MAP_READ | VMAR_FLAG_CAN_MAP_WRITE,
        "kernel heap"));
    {
      Guard<CriticalMutex> guard(kernel_heap_vmar->lock());
      kernel_heap_vmar->Activate();
    }
    dprintf!(INFO, "VM: kernel heap placed in range [{:x}, {:x})\n",
             kernel_heap_vmar.base, kernel_heap_vmar.base + kernel_heap_vmar.size);
    */

    ZX_DEBUG_ASSERT!(root_vmar.children.len() == 2);
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
    boot_context.lock().vm_aspace_list = Some(aspaces);
    dprintf!(INFO, "kernel_aspace_init_preheap ok!\n");

    Ok(())
}
