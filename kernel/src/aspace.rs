/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use spin::Mutex;
use crate::debug::*;
use crate::{KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE};
use crate::{ErrNO, types::vaddr_t};

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
}

static boot_context: Mutex<BootContext> = Mutex::new(BootContext::new());

pub fn vm_init_preheap() -> Result<(), ErrNO> {
    /* allow the vmm a shot at initializing some of its data structures */
    kernel_aspace_init_preheap()?;

    /*
  vm_init_preheap_vmars();

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
