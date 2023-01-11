/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::alloc::Layout;
use core::ptr::null_mut;

use crate::BOOT_CONTEXT;
use crate::PFN_TO_PA;
use crate::PTE_TO_PFN;
use crate::PTE_TO_PROT;
use crate::arch::mmu::PAGE_KERNEL;
use crate::arch::mmu::PageTable;
use crate::arch::mmu::_swapper_pgd;
use crate::arch::mmu::protect_pages;
use crate::arch::mmu::vaddr_to_index;
use crate::defines::ARCH_HEAP_ALIGN_BITS;
use crate::defines::HEAP_MAX_SIZE_MB;
use crate::defines::MB;
use crate::defines::PAGE_SIZE;
use crate::defines::PHYSMAP_BASE;
use crate::defines::PHYSMAP_SIZE;
use crate::defines::kernel_size;
use crate::defines::paddr_to_physmap;
use crate::klib::list::Linked;
use crate::klib::list::List;
use crate::klib::list::ListNode;
use crate::locking::mutex::Mutex;
use crate::types::*;
use crate::vm::vm::ARCH_MMU_FLAG_PERM_EXECUTE;
use crate::vm::vm::ARCH_MMU_FLAG_PERM_READ;
use crate::vm::vm::ARCH_MMU_FLAG_PERM_WRITE;
use crate::vm::vm::kernel_regions_base;
use crate::vm::vm::mmu_prot_from_flags;
use crate::vm::vmar::VmAddressRegion;
use crate::debug::*;
use crate::{KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE};
use crate::{ErrNO, types::vaddr_t, ZX_ASSERT};
use crate::pmm::pmm_alloc_page;
use crate::vm_page_state;
use crate::arch::mmu::arch_zero_page;
use crate::arch::mmu::map_pages;

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

/* Map the given array of pages into the virtual address space starting at
 * |vaddr|, in the order they appear in |phys|.
 * If any address in the range [vaddr, vaddr + count * PAGE_SIZE) is already
 * mapped when this is called, and the |existing_action| is |Error| then this
 * returns ZX_ERR_ALREADY_EXISTS, otherwise they are skipped. Skipped pages
 * are stil counted in |mapped|. On failure some pages may still be mapped,
 * the number of which will be reported in |mapped|. */
#[allow(dead_code)]
#[derive(PartialEq)]
pub enum ExistingEntryAction {
    Skip,
    Error,
}

#[allow(dead_code)]
pub struct VmAspace {
    queue_node: ListNode,
    id: usize,
    as_type: VmAspaceType,
    base: vaddr_t,
    size: usize,
    root_vmar: Option<VmAddressRegion>,
}

impl Linked<VmAspace> for VmAspace {
    fn from_node(ptr: *mut ListNode) -> *mut VmAspace {
        unsafe {
            crate::container_of!(ptr, VmAspace, queue_node)
        }
    }

    fn into_node(&mut self) -> *mut ListNode {
        &mut (self.queue_node)
    }
}

impl VmAspace {
    fn init(&mut self, id: usize, as_type: VmAspaceType,
            base: vaddr_t, size: usize) {
        self.queue_node.init();
        self.id = id;
        self.as_type = as_type;
        self.base = base;
        self.size = size;
        self.root_vmar = None;

        /* initialize the architecturally specific part */
        /* zx_status_t status = arch_aspace_.Init()?; */
        /* InitializeAslr(); */
    }

    fn get_root_vmar(&mut self) -> &mut VmAddressRegion {
        if let Some(vmar) = &mut self.root_vmar {
            return vmar;
        }
        panic!("no root vmar!");
    }

    const fn is_valid_vaddr(&self, vaddr: vaddr_t) -> bool {
        vaddr >= self.base && vaddr <= self.base + self.size - 1
    }

    pub fn map(&mut self, vaddr: vaddr_t, phys: &[paddr_t],
               count: usize, mmu_flags: usize,
               action: ExistingEntryAction) -> Result<usize, ErrNO> {

        if !self.is_valid_vaddr(vaddr) {
            return Err(ErrNO::OutOfRange);
        }
        for i in 0..count {
            ZX_ASSERT!(IS_PAGE_ALIGNED!(phys[i]));
            if !IS_PAGE_ALIGNED!(phys[i]) {
              return Err(ErrNO::InvalidArgs);
            }
        }

        if (mmu_flags & ARCH_MMU_FLAG_PERM_READ) == 0 {
            return Err(ErrNO::InvalidArgs);
        }

        /* vaddr must be aligned. */
        ZX_ASSERT!(IS_PAGE_ALIGNED!(vaddr));
        if !IS_PAGE_ALIGNED!(vaddr) {
            return Err(ErrNO::InvalidArgs);
        }

        if count == 0 {
            return Ok(0);
        }

        let mut v = vaddr;
        let prot = PAGE_KERNEL;
        for idx in 0..count {
            let paddr = phys[idx];
            ZX_ASSERT!(IS_PAGE_ALIGNED!(paddr));
            if let Err(e) = map_pages(v, paddr, PAGE_SIZE, prot) {
                if e != ErrNO::AlreadyExists ||
                    action == ExistingEntryAction::Error {
                        return Err(e);
                }
            };
            //MarkAspaceModified();

            v += PAGE_SIZE;
        }

        /* Tlb flush!!! We need tlb flush here?! */
        /*
        unsafe {
            local_flush_tlb_all();
        }
        */

        Ok(count)
    }

    pub fn unmap(&self, _va: vaddr_t, _count: usize, _enlarge: bool)
        -> Result<usize, ErrNO> {
        todo!("unmap!");
    }

    pub fn protect(&self, vaddr: vaddr_t, count: usize, mmu_flags: usize)
        -> Result<(), ErrNO> {
        if !self.is_valid_vaddr(vaddr) {
            return Err(ErrNO::InvalidArgs);
        }

        if !IS_PAGE_ALIGNED!(vaddr) {
            return Err(ErrNO::InvalidArgs);
        }

        if (mmu_flags & ARCH_MMU_FLAG_PERM_READ) == 0 {
            return Err(ErrNO::InvalidArgs);
        }

        if (mmu_flags & ARCH_MMU_FLAG_PERM_EXECUTE) != 0 {
            todo!("ARCH_MMU_FLAG_PERM_EXECUTE");
        }

        let prot = mmu_prot_from_flags(mmu_flags);
        let status = protect_pages(vaddr, count * PAGE_SIZE, prot);
        // MarkAspaceModified();
        status
    }

    pub fn query(&self, va: vaddr_t) -> Result<(paddr_t, usize), ErrNO> {
        self.query_locked(va)
    }

    fn query_locked(&self, va: vaddr_t) -> Result<(paddr_t, usize), ErrNO> {
        if !self.is_valid_vaddr(va) {
            return Err(ErrNO::OutOfRange);
        }

        let mut level = 0;
        let mut page_table = unsafe { &mut _swapper_pgd };
        loop {
            let index = vaddr_to_index(va, level);
            if !page_table.item_present(index) {
                return Err(ErrNO::NotFound);
            }

            let pte = page_table.item(index);
            let pa = PFN_TO_PA!(PTE_TO_PFN!(pte));
            if page_table.item_leaf(index) {
                let prot = PTE_TO_PROT!(pte);
                return Ok((pa, prot));
            }

            unsafe {
                page_table = &mut *(paddr_to_physmap(pa) as *mut PageTable);
            }
            level += 1;
        }
    }
}

pub fn vm_init_preheap() -> Result<(), ErrNO> {
    ASPACE_LIST.lock().init();
    println!("vm_init_preheap");

    /* allow the vmm a shot at initializing some of its data structures */
    kernel_aspace_init_preheap()?;

    vm_init_preheap_vmars();

    // grab a page and mark it as the zero page
    let zero_page = pmm_alloc_page(0);
    if zero_page == null_mut() {
        panic!("alloc zero page error!");
    }
    /* consider the zero page a wired page part of the kernel. */
    unsafe {
        (*zero_page).set_state(vm_page_state::WIRED);
        let va = paddr_to_physmap((*zero_page).paddr());
        ZX_ASSERT!(va != 0);
        arch_zero_page(va);
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

    let aspace_list = ASPACE_LIST.lock();
    println!("vm_init_preheap_vmars");
    let kernel_aspace = aspace_list.head();
    let root_vmar = unsafe { (*kernel_aspace).get_root_vmar() };

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

    unsafe {
        let ctx = &mut (*BOOT_CONTEXT.data.get());
        ctx.kernel_heap_base = kernel_heap_base;
        ctx.kernel_heap_size = heap_bytes;
    }
}

fn kernel_aspace_init_preheap() -> Result<(), ErrNO> {
    let flags = VMAR_FLAG_CAN_MAP_SPECIFIC | VMAR_CAN_RWX_FLAGS;
    let mut root_vmar = VmAddressRegion::new();
    root_vmar.init(KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE, flags);

    let layout = Layout::new::<VmAspace>();
    use alloc::alloc::alloc;
    let kernel_aspace = unsafe { alloc(layout) as *mut VmAspace };
    unsafe {
        (*kernel_aspace).init(0, VmAspaceType::Kernel,
                              KERNEL_ASPACE_BASE, KERNEL_ASPACE_SIZE);
        (*kernel_aspace).root_vmar = Some(root_vmar);
    }

    let mut aspace_list = ASPACE_LIST.lock();
    println!("kernel_aspace_init_preheap");
    aspace_list.add_head(kernel_aspace);
    dprintf!(INFO, "kernel_aspace_init_preheap ok!\n");
    Ok(())
}

/* Request the heap dimensions. */
pub fn vm_get_kernel_heap_base() -> usize {
    unsafe {
        (*BOOT_CONTEXT.data.get()).kernel_heap_base
    }
}

pub fn vm_get_kernel_heap_size() -> usize {
    unsafe {
        (*BOOT_CONTEXT.data.get()).kernel_heap_size
    }
}

pub static ASPACE_LIST: Mutex<List<VmAspace>> = Mutex::new(List::<VmAspace>::new());