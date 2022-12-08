/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::ErrNO;

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
    /*
    let kernel_aspace =
        VmAspace::new("kernel", KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE,
                      VmAspaceType::Kernel);

    kernel_aspace.borrow_mut().root_vmar =
        Some(VmAddressRegion::new(kernel_aspace.clone()));

    kernel_aspace.borrow_mut().init()?;

    //aspaces.push_front(kernel_aspace);
    dprint!(INFO, "kernel_aspace_init_pre_heap ok!\n");
    */

    Ok(())
}
