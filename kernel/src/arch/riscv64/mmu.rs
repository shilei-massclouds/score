/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::cmp::min;
use core::ptr::null_mut;
use core::arch::asm;
use crate::types::*;
use crate::defines::*;
use crate::errors::ErrNO;
use crate::stdio::STDOUT;
use crate::debug::*;
use crate::vm_page_state;
use core::ptr::NonNull;
use crate::page::vm_page_t;
use crate::pmm::{pmm_alloc_page, PMM_ALLOC_FLAG_ANY};
use crate::{dprintf, print};

const PAGE_TABLE_ENTRIES: usize = 1 << (PAGE_SHIFT - 3);

/*
 * PTE format:
 * | XLEN-1  10 | 9             8 | 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0
 *       PFN      reserved for SW   D   A   G   U   X   W   R   V
 */

const _PAGE_PFN_SHIFT: usize = 10;

const _PAGE_PRESENT : usize = 1 << 0;     /* Valid */
const _PAGE_READ    : usize = 1 << 1;     /* Readable */
const _PAGE_WRITE   : usize = 1 << 2;     /* Writable */
const _PAGE_EXEC    : usize = 1 << 3;     /* Executable */
const _PAGE_USER    : usize = 1 << 4;     /* User */
const _PAGE_GLOBAL  : usize = 1 << 5;     /* Global */
const _PAGE_ACCESSED: usize = 1 << 6;     /* Accessed (set by hardware) */
const _PAGE_DIRTY   : usize = 1 << 7;     /* Dirty (set by hardware)*/

/*
 * when all of R/W/X are zero, the PTE is a pointer to the next level
 * of the page table; otherwise, it is a leaf PTE.
 */
const _PAGE_LEAF: usize = _PAGE_READ | _PAGE_WRITE | _PAGE_EXEC;

const PAGE_TABLE: usize = _PAGE_PRESENT;

pub const PAGE_KERNEL: usize =
    _PAGE_PRESENT | _PAGE_READ | _PAGE_WRITE |
    _PAGE_GLOBAL | _PAGE_ACCESSED | _PAGE_DIRTY;

pub const PAGE_KERNEL_EXEC : usize = PAGE_KERNEL | _PAGE_EXEC;

/*
 * The RISC-V ISA doesn't yet specify how to query or modify PMAs,
 * so we can't change the properties of memory regions.
 */
pub const PAGE_IOREMAP: usize = PAGE_KERNEL;

pub const SATP_MODE_39: usize = 0x8000000000000000;
pub const SATP_MODE_48: usize = 0x9000000000000000;
pub const SATP_MODE_57: usize = 0xa000000000000000;

const MMU_PTE_DESCRIPTOR_LEAF_MAX_SHIFT: usize = 30;

#[repr(C, align(4096))]
pub struct PageTable([usize; PAGE_TABLE_ENTRIES]);

impl PageTable {
    fn mk_item(&mut self, index: usize, pfn: usize, prot: usize) {
        self.0[index] = (pfn << _PAGE_PFN_SHIFT) | prot;
    }

    fn item_present(&self, index: usize) -> bool {
        (self.0[index] & _PAGE_PRESENT) == _PAGE_PRESENT
    }

    fn item_leaf(&self, index: usize) -> bool {
        self.item_present(index) && ((self.0[index] & _PAGE_LEAF) != 0)
    }

    fn item_descend(&self, index: usize) -> usize {
        (self.0[index] >> _PAGE_PFN_SHIFT) << PAGE_SHIFT
    }

    fn item(&self, index: usize) -> usize {
        self.0[index]
    }
}

extern "C" {
    pub fn _start();
    pub static mut _swapper_pgd: PageTable;
    pub static mut _swapper_tables: [PageTable; MMU_MAX_LEVEL-1];
    pub static mut _satp_mode: usize;
}

#[no_mangle]
pub extern "C" fn setup_vm() {
    let mut used: usize = 0;
    let mut alloc = || {
        unsafe {
            if used >= (MMU_MAX_LEVEL - 1) {
                STDOUT.lock().puts("Out of boot tables!\n");
                return null_mut();
            }
            let base = &mut _swapper_tables[used] as *mut PageTable;
            STDOUT.lock().puts("In alloc[");
            STDOUT.lock().put_u64(base as u64);
            STDOUT.lock().puts("]In alloc!\n");
            used += 1;
            return base;
        }
    };

    let phys_to_virt = |pa: paddr_t| { pa as *mut PageTable };

    /*
     * map a large run of physical memory at the base of
     * the kernel's address space.
     */
    let ret = boot_map(KERNEL_ASPACE_BASE, 0, ARCH_PHYSMAP_SIZE,
                       PAGE_KERNEL, &mut alloc, &phys_to_virt);
    if let Err(_) = ret {
        STDOUT.lock().puts("map physmap error!\n");
        panic!("map physmap error!");
    }

    /* map the kernel to a fixed address */
    let ret = boot_map(KERNEL_BASE,
                       _start as usize, (_end as usize) - (_start as usize),
                       PAGE_KERNEL_EXEC, &mut alloc, &phys_to_virt);
    if let Err(_) = ret {
        STDOUT.lock().puts("map kernel image error!\n");
        panic!("map kernel image error!");
    }

    unsafe {
        _satp_mode = match MMU_LEVELS {
            5 => SATP_MODE_57,
            4 => SATP_MODE_48,
            3 => SATP_MODE_39,
            _ => panic!("bad satp mode!"),
        };
    }
}

pub fn boot_map<F1, F2>(vaddr: vaddr_t, paddr: paddr_t, len: usize,
                        prot: prot_t, alloc: &mut F1, phys_to_virt: &F2)
    -> Result<(), ErrNO>
    where F1: FnMut() -> *mut PageTable, F2: Fn(paddr_t) -> *mut PageTable {

    /* Loop through the virtual range and map each physical page,
     * using the largest page size supported.
     * Allocates necessar page tables along the way. */
    unsafe {
        _boot_map(&mut _swapper_pgd, 0, vaddr, paddr, len, prot,
                  alloc, phys_to_virt)
    }
}

/* Todo: Check KERNEL_ASPACE_BITS < 57 because SV57 is
 * the highest mode that is supported. */
const MMU_LEVELS: usize =
    (KERNEL_ASPACE_BITS - PAGE_SHIFT) / (PAGE_SHIFT - 3) + 1;

macro_rules! LEVEL_SHIFT {
    ($level: expr) => {
        ((MMU_LEVELS - ($level)) * (PAGE_SHIFT - 3) + 3)
    }
}

macro_rules! LEVEL_SIZE {
    ($level: expr) => {
        1 << LEVEL_SHIFT!($level)
    }
}

macro_rules! LEVEL_MASK {
    ($level: expr) => {
        !(LEVEL_SIZE!($level) - 1)
    }
}

macro_rules! LEVEL_PA_TO_PFN {
    ($pa: expr, $level: expr) => {
        (($pa) >> LEVEL_SHIFT!($level))
    }
}

macro_rules! PA_TO_PFN {
    ($pa: expr) => {
        (($pa) >> PAGE_SHIFT)
    }
}

#[allow(dead_code)]
pub const MMU_KERNEL_SIZE_SHIFT: usize = KERNEL_ASPACE_BITS;

pub const MMU_KERNEL_TOP_SHIFT: usize = LEVEL_SHIFT!(1);

fn vaddr_to_index(addr: usize, level: usize) -> usize {
    (addr >> LEVEL_SHIFT!(level)) & (PAGE_TABLE_ENTRIES - 1)
}

fn aligned_in_level(addr: usize, level: usize) -> bool {
    (addr & !(LEVEL_MASK!(level))) == 0
}

fn _boot_map<F1, F2>(table: &mut PageTable, level: usize,
                     vaddr: vaddr_t, paddr: paddr_t, len: usize, prot: prot_t,
                     alloc: &mut F1, phys_to_virt: &F2) -> Result<(), ErrNO>
    where F1: FnMut() -> *mut PageTable, F2: Fn(paddr_t) -> *mut PageTable {

    let mut off = 0;
    while off < len {
        let index = vaddr_to_index(vaddr + off, level);
        if level == (MMU_LEVELS-1) {
            /* generate a standard leaf mapping */
            table.mk_item(index, PA_TO_PFN!(paddr + off), prot);

            off += PAGE_SIZE;
            continue;
        }
        if !table.item_present(index) {
            if (level != 0) &&
                aligned_in_level(vaddr+off, level) &&
                aligned_in_level(paddr+off, level) &&
                ((len - off) >= LEVEL_SIZE!(level)) {
                /* set up a large leaf at this level */
                table.mk_item(index,
                              LEVEL_PA_TO_PFN!(paddr + off, level),
                              prot);

                off += LEVEL_SIZE!(level);
                continue;
            }

            let pa: usize = alloc() as usize;
            table.mk_item(index, PA_TO_PFN!(pa), PAGE_TABLE);
        }
        if table.item_leaf(index) {
            /* not legal as a leaf at this level */
            return Err(ErrNO::BadState);
        }

        let lower_table_ptr = phys_to_virt(table.item_descend(index));
        let lower_len = min(LEVEL_SIZE!(level), len-off);
        unsafe {
            _boot_map(&mut (*lower_table_ptr), level+1,
                      vaddr+off, paddr+off, lower_len,
                      prot, alloc, phys_to_virt)?;
        }

        off += LEVEL_SIZE!(level);
    }

    Ok(())
}

pub unsafe fn arch_zero_page(va: vaddr_t) {
    asm!(
        "ble {1}, {0}, 2f
        1:
         sd zero, ({0})
         add {0}, {0}, 8
         blt {0}, {1}, 1b
        2:",

        in(reg) va,
        in(reg) (va + PAGE_SIZE),
    );
}

pub fn map_pages(vaddr: vaddr_t, paddr: paddr_t, size: usize,
                 prot: prot_t, vaddr_base: vaddr_t,
                 top_size_shift: usize,
                 top_index_shift: usize)
    -> Result<usize, ErrNO> {
    let vaddr_rel = vaddr - vaddr_base;
    let vaddr_rel_max = 1 << top_size_shift;

    dprintf!(INFO, "vaddr {:x}, paddr {:x}, size {:x}, prot {:x}\n",
             vaddr, paddr, size, prot);

    if vaddr_rel > vaddr_rel_max - size || size > vaddr_rel_max {
        return Err(ErrNO::InvalidArgs);
    }

    unsafe {
        map_page_table(vaddr, vaddr_rel, paddr, size, prot, top_index_shift,
                       &mut _swapper_pgd)
    }
}

pub fn map_page_table(mut vaddr: vaddr_t, mut vaddr_rel: vaddr_t,
                      mut paddr: paddr_t, mut size: usize, prot: prot_t,
                      index_shift: usize, page_table: &mut PageTable)
    -> Result<usize, ErrNO> {

    let block_size = 1 << index_shift;
    let block_mask = block_size - 1;
    dprintf!(INFO, "vaddr {:x}, vaddr_rel {:x}, paddr {:x}, size {:x}, \
             prot {:x}, index shift {}\n",
             vaddr, vaddr_rel, paddr, size, prot, index_shift);

    if ((vaddr_rel | paddr | size) & ((1 << PAGE_SHIFT) - 1)) != 0 {
        return Err(ErrNO::InvalidArgs);
    }

    let mut mapped_size = 0;
    while size > 0 {
        let vaddr_rem = vaddr_rel & block_mask;
        let chunk_size = min(size, block_size - vaddr_rem);
        let index = vaddr_rel >> index_shift;
        let pte = page_table.item(index);

        /* if we're at an unaligned address, not trying to map a block,
         * and not at the terminal level, recurse one more level of
         * the page table tree */
        if ((vaddr_rel | paddr) & block_mask) != 0 ||
            (chunk_size != block_size) ||
            (index_shift > MMU_PTE_DESCRIPTOR_LEAF_MAX_SHIFT) {

            let next_pt: *mut PageTable;
            if page_table.item_present(index) {
                if page_table.item_leaf(index) {
                    dprintf!(WARN, "page table entry already in use, {:x}\n",
                             pte);
                    return Err(ErrNO::AlreadyExists);
                } else {
                    next_pt = paddr_to_physmap(page_table.item_descend(index))
                        as *mut PageTable;
                }
            } else {
                let page_table_paddr = alloc_page_table()?;
                let pt_vaddr = paddr_to_physmap(page_table_paddr);
                dprintf!(INFO, "allocated page table, va {:x}, pa {:x}\n",
                         pt_vaddr, page_table_paddr);

                unsafe {
                    arch_zero_page(pt_vaddr);
                    /* Fence */
                }

                page_table.mk_item(index, PA_TO_PFN!(page_table_paddr),
                                   PAGE_TABLE);
                next_pt = pt_vaddr as *mut PageTable;
            }

            unsafe {
                map_page_table(vaddr, vaddr_rem, paddr, chunk_size, prot,
                               index_shift - (PAGE_SHIFT - 3),
                               &mut (*next_pt))?;
            }
        } else {
            if page_table.item_present(index) {
                dprintf!(WARN, "page table entry already in use, {:x}\n", pte);
                return Err(ErrNO::AlreadyExists);
            }

            page_table.mk_item(index, PA_TO_PFN!(paddr), prot);
            dprintf!(INFO, "pte [{}] = {:x} (pa {:x})\n", index, pte, paddr);
        }

        vaddr += chunk_size;
        vaddr_rel += chunk_size;
        paddr += chunk_size;
        size -= chunk_size;
        mapped_size += chunk_size;
    }

    Ok(mapped_size)
}

fn alloc_page_table() -> Result<paddr_t, ErrNO> {
    let mut page = cache_alloc_page()?;

    unsafe {
        page.as_mut().set_state(vm_page_state::MMU);
        //kcounter_add(vm_mmu_page_table_alloc, 1);
        return Ok(page.as_ref().paddr());
    }
}

fn cache_alloc_page() -> Result<NonNull<vm_page_t>, ErrNO> {
    /* Todo: Implement PageCache on the next step. */
    pmm_alloc_page(PMM_ALLOC_FLAG_ANY).ok_or_else(||ErrNO::NoMem)
}
