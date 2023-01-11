/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use crate::errors::ErrNO;
use crate::{types::*, ZX_ASSERT};
use crate::defines::ARCH_DEFAULT_STACK_SIZE;

use super::vmar::VmAddressRegion;

/* stack size */
pub const DEFAULT_STACK_SIZE: usize = ARCH_DEFAULT_STACK_SIZE;

struct StackType {
    name: &'static str,
    size: usize,
}

const K_SAFE: StackType = StackType {
    name: "kernel-safe-stack",
    size: DEFAULT_STACK_SIZE,
};

/* Holds the relevant metadata and pointers for an individual mapping */
struct KernelStackMapping {
    base: vaddr_t,
    size: usize,
    vmar: VmAddressRegion,
}

impl KernelStackMapping {
    const fn new() -> Self {
        Self {
            base: 0,
            size: 0,
            vmar: VmAddressRegion::new(),
        }
    }

    fn top(&self) -> vaddr_t {
        self.base + self.size
    }
}

pub struct KernelStack {
    main_map: KernelStackMapping,
}

impl KernelStack {
    pub const fn new() -> Self {
        Self {
            main_map: KernelStackMapping::new(),
        }
    }

    pub fn init(&mut self) -> Result<(), ErrNO> {
        allocate_map(K_SAFE, &self.main_map)
    }
}

/* Allocates and maps a kernel stack with one page of padding
 * before and after the mapping. */
fn allocate_map(_stype: StackType, map: &KernelStackMapping)
    -> Result<(), ErrNO>
{
    /* assert that this mapping hasn't already be created */
    ZX_ASSERT!(map.base == 0);
    ZX_ASSERT!(map.size == 0);

    /* get a handle to the root vmar */
  /*
  auto vmar = VmAspace::kernel_aspace()->RootVmar()->as_vm_address_region();
  DEBUG_ASSERT(!!vmar);
  */

    todo!("allocate_map!");
}