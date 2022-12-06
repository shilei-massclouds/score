/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::cmp::min;
use core::ptr::null_mut;
use crate::types::*;
use crate::defines::*;
use crate::errors::ErrNO;
use crate::stdio::STDOUT;

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
}

extern "C" {
    pub fn _start();
    pub static mut _swapper_pgd: PageTable;
    pub static mut _swapper_tables: [PageTable; MMU_MAX_LEVEL-1];
    pub static mut _satp_mode: usize;
}

#[no_mangle]
pub extern "C" fn setup_vm() {
    STDOUT.lock().puts("step1\n");

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
    STDOUT.lock().puts("boot_map[");
    STDOUT.lock().put_u64(KERNEL_ASPACE_BITS as u64);
    STDOUT.lock().puts("]boot_map\n");
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
        STDOUT.lock().puts("step2\n");
        if !table.item_present(index) {
            STDOUT.lock().puts("step2.0\n");
            if (level != 0) &&
                aligned_in_level(vaddr+off, level) &&
                aligned_in_level(paddr+off, level) &&
                ((len - off) >= LEVEL_SIZE!(level)) {
                STDOUT.lock().puts("step2.1\n");
                /* set up a large leaf at this level */
                table.mk_item(index,
                              LEVEL_PA_TO_PFN!(paddr + off, level),
                              prot);

                off += LEVEL_SIZE!(level);
                continue;
            }
            STDOUT.lock().puts("step2.2\n");

            let pa: usize = alloc() as usize;
            table.mk_item(index, PA_TO_PFN!(pa), PAGE_TABLE);
        }
        STDOUT.lock().puts("step3.1\n");
        if table.item_leaf(index) {
            /* not legal as a leaf at this level */
            return Err(ErrNO::BadState);
        }
        STDOUT.lock().puts("step4\n");

        let lower_table_ptr = phys_to_virt(table.item_descend(index));
        let lower_len = min(LEVEL_SIZE!(level), len-off);
        unsafe {
            _boot_map(&mut (*lower_table_ptr), level+1,
                      vaddr+off, paddr+off, lower_len,
                      prot, alloc, phys_to_virt)?;
        }

        off += LEVEL_SIZE!(level);
        STDOUT.lock().puts("step5\n");
    }

    Ok(())
}
