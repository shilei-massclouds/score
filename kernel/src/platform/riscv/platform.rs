/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::slice;
use crate::{print, dprintf, ZX_ASSERT, IS_PAGE_ALIGNED, IS_ALIGNED};
use crate::debug::*;
use crate::types::*;
use alloc::vec::Vec;
use crate::defines::*;
use crate::errors::ErrNO;
use crate::platform::boot_reserve::boot_reserve_init;
use crate::pmm::{MAX_ARENAS, ArenaInfo};
use device_tree::{DeviceTree, Node};
use crate::platform::periphmap::add_periph_range;
use crate::platform::boot_reserve::{boot_reserve_add_range, RESERVE_RANGES};
use crate::pmm::pmm_add_arena;
use crate::page::vm_page_t;
use crate::{ROUNDUP_PAGE_SIZE, ROUNDUP};
use crate::List;
use crate::pmm::pmm_alloc_range;
use spin::Mutex;
use crate::vm_page_state;

pub mod boot_reserve;
mod periphmap;

pub const MAX_ZBI_MEM_RANGES: usize = 32;

pub enum ZBIMemRangeType {
    RAM,
    PERIPHERAL,
    RESERVED,
}

pub struct ZBIMemRange {
    pub mtype:      ZBIMemRangeType,
    pub paddr:      paddr_t,
    pub length:     usize,
    pub reserved:   u32,
}

impl ZBIMemRange {
    pub fn new(mtype: ZBIMemRangeType, paddr: paddr_t, length: usize)
        -> ZBIMemRange {
        ZBIMemRange { mtype, paddr, length, reserved: 0, }
    }
}

type ZBIMemRangeVec = Vec<ZBIMemRange>;

const OF_ROOT_NODE_SIZE_CELLS_DEFAULT: u32 = 1;
const OF_ROOT_NODE_ADDR_CELLS_DEFAULT: u32 = 1;

pub fn platform_early_init() -> Result<(), ErrNO> {
    /* initialize the boot memory reservation system */
    boot_reserve_init(kernel_base_phys(), kernel_size())?;

    let mut mem_arenas = process_dtb_early()?;

    /* is the cmdline option to bypass dlog set ? */
    dlog_bypass_init();

    /* Serial port should be active now */

    /* Check if serial should be enabled (i.e., not using the null driver). */
    /*
    ktl::visit([](const auto& uart) { uart_disabled = uart.extra() == 0; },
               gBootOptions->serial);
    */

    /* Initialize the PmmChecker now that the cmdline has been parsed. */
    pmm_checker_init_from_cmdline();

    /*
     * check if a memory limit was passed in via kernel.memory-limit-mb and
     * find memory ranges to use if one is found.
     */
    let have_limit = memory_limit_init().is_ok();
    /* find memory ranges to use if one is found. */
    while let Some(arena) = mem_arenas.pop() {
        if have_limit {
            /*
             * Figure out and add arenas based on the memory limit and
             * our range of DRAM
             */
            match memory_limit_add_range(arena.base, arena.size) {
                Ok(_) => continue,
                Err(err) => {
                    if let ErrNO::NotSupported = err {
                    } else {
                        dprintf!(WARN, "memory limit lib returned an error {:?},
                                 falling back to defaults\n", err);
                    }
                }
            }
        }

        /*
         * If no memory limit was found, or adding arenas from the range failed,
         * then add the existing global arena.
         */

        /* Init returns not supported if no limit exists */
        pmm_add_arena(arena)?;
    }

    /* add any pending memory arenas the memory limit library has pending */
    if have_limit {
        ZX_ASSERT!(memory_limit_add_arenas().is_ok());
    }

    /* tell the boot allocator to mark ranges we've reserved as off limits */
    boot_reserve_wire()
}

fn memory_limit_init() -> Result<(), ErrNO> {
    Err(ErrNO::NotSupported)
}

fn memory_limit_add_range(_base: paddr_t, _size: usize) -> Result<(), ErrNO> {
    todo!();
}

fn memory_limit_add_arenas() -> Result<(), ErrNO> {
    todo!();
}

fn dlog_bypass_init() {
}

fn pmm_checker_init_from_cmdline() {
}

pub static RESERVED_PAGE_LIST: Mutex<List<vm_page_t>> =
    Mutex::new(List::<vm_page_t>::new());

fn boot_reserve_wire() -> Result<(), ErrNO> {
    let mut total_list = List::new();
    total_list.init();
    {
        let res = RESERVE_RANGES.lock();
        for r in res.iter() {
            dprintf!(INFO, "PMM: boot reserve marking WIRED [{:x}, {:x}]\n",
                     r.pa, r.pa + r.len -1);
            let mut alloc_page_list = List::new();
            alloc_page_list.init();
            let pages = ROUNDUP_PAGE_SIZE!(r.len) / PAGE_SIZE;
            pmm_alloc_range(r.pa, pages, &mut alloc_page_list)?;
            total_list.splice(&mut alloc_page_list);
        }
    }

    /* mark all of the pages we allocated as WIRED */
    for page in &mut total_list.iter_mut() {
        page.set_state(vm_page_state::WIRED);
    }

    RESERVED_PAGE_LIST.lock().splice(&mut total_list);
    Ok(())
}

const FDT_MAGIC: u32 = 0xd00dfeed;
const FDT_MAGIC_OFFSET: usize = 0;
const FDT_TOTALSIZE_OFFSET: usize = 4;

fn process_dtb_early() -> Result<Vec<ArenaInfo>, ErrNO> {
    /* discover memory ranges */
    let dtb_va = paddr_to_physmap(dtb_pa());
    dprintf!(CRITICAL, "HartID {:x}; DTB 0x{:x} -> 0x{:x}\n",
             boot_cpu_id(), dtb_pa(), dtb_va);

    let dt = early_init_dt_load(dtb_va)?;
    let mut mem_config = early_init_dt_scan(&dt)?;
    init_mem_config_arch(&mut mem_config);
    process_mem_ranges(mem_config)
}

fn init_mem_config_arch(config: &mut Vec<ZBIMemRange>) {
    config.push(
        ZBIMemRange::new(ZBIMemRangeType::PERIPHERAL, 0, 0x40000000)
    );
}

fn process_mem_ranges(mem_config: Vec<ZBIMemRange>)
    -> Result<Vec<ArenaInfo>, ErrNO> {

    let mut mem_arenas = Vec::<ArenaInfo>::with_capacity(MAX_ARENAS);

    for range in mem_config {
        match &(range.mtype) {
            ZBIMemRangeType::RAM => {
                dprintf!(INFO, "ZBI: mem arena {:x} - {:x}\n",
                         range.paddr, range.length);

                if mem_arenas.len() >= MAX_ARENAS {
                    dprintf!(CRITICAL, "ZBI: too many memory arenas,
                             dropping additional\n");
                    break;
                }
                mem_arenas.push(
                    ArenaInfo::new("ram", 0, range.paddr, range.length)
                );
            },
            ZBIMemRangeType::PERIPHERAL => {
                dprintf!(INFO, "ZBI: peripheral range {:x} - {:x}\n",
                         range.paddr, range.length);
                add_periph_range(range.paddr, range.length)?;
            },
            ZBIMemRangeType::RESERVED => {
                dprintf!(INFO, "FIND RESERVED Memory Range {:x} {:x}!\n",
                         range.paddr, range.length);
                boot_reserve_add_range(range.paddr, range.length)?;
            }
        }
    }

    Ok(mem_arenas)
}

fn early_init_dt_load(dtb_va: usize) -> Result<DeviceTree, ErrNO> {
    early_init_dt_verify(dtb_va)?;

    let totalsize = fdt_get_u32(dtb_va, FDT_TOTALSIZE_OFFSET);
    unsafe {
        let buf = slice::from_raw_parts_mut(dtb_va as *mut u8,
                                            totalsize as usize);
        DeviceTree::load(buf).or_else(|e| {
            dprintf!(CRITICAL, "Can't load dtb: {:?}\n", e);
            Err(ErrNO::BadDTB)
        })
    }
}

fn early_init_dt_verify(dtb_va: usize) -> Result<(), ErrNO> {
    if dtb_va == 0 {
        dprintf!(CRITICAL, "No DTB passed to the kernel\n");
        return Err(ErrNO::NoDTB);
    }

    /* check device tree validity */
    if fdt_get_u32(dtb_va, FDT_MAGIC_OFFSET) != FDT_MAGIC {
        dprintf!(CRITICAL, "Bad DTB passed to the kernel\n");
        return Err(ErrNO::BadDTB);
    }

    Ok(())
}

fn fdt_get_u32(dtb_va: usize, offset: usize) -> u32 {
    let ptr = (dtb_va + offset) as *const u32;
    unsafe {
        u32::from_be(*ptr)
    }
}

fn early_init_dt_scan(dt: &DeviceTree) -> Result<ZBIMemRangeVec, ErrNO> {
    /* Initialize {size,address}-cells info */
    let (addr_cells, size_cells) = early_init_dt_scan_root(dt);

    /* Retrieve various information from the /chosen node */
    let cmdline = early_init_dt_scan_chosen(dt);
    dprintf!(INFO, "command line = {}\n", cmdline);

    /* Setup memory, calling early_init_dt_add_memory_arch */
    early_init_dt_scan_memory(dt, addr_cells, size_cells)
}

/*
 * early_init_dt_scan_root - fetch the top level address and size cells
 */
fn early_init_dt_scan_root(dt: &DeviceTree) -> (u32, u32) {
    let root = match dt.find("/") {
        Some(node) => { node },
        None => {
            dprintf!(CRITICAL, "Can't find root of this dtb!\n");
            return (OF_ROOT_NODE_ADDR_CELLS_DEFAULT,
                    OF_ROOT_NODE_SIZE_CELLS_DEFAULT);
        }
    };

    let addr_cells = root.prop_u32("#address-cells")
        .unwrap_or_else(|_| OF_ROOT_NODE_ADDR_CELLS_DEFAULT);
    dprintf!(INFO, "dt_root_addr_cells = 0x{:x}\n", addr_cells);

    let size_cells = root.prop_u32("#size-cells")
        .unwrap_or_else(|_| OF_ROOT_NODE_SIZE_CELLS_DEFAULT);
    dprintf!(INFO, "dt_root_size_cells = 0x{:x}\n", size_cells);

    (addr_cells, size_cells)
}

fn early_init_dt_scan_chosen(dt: &DeviceTree) -> &str {
    let chosen = match dt.find("/chosen") {
        Some(node) => { node },
        None => {
            if let Some(node) = dt.find("/chosen@0") {
                node
            } else {
                dprintf!(WARN, "No chosen node found!\n");
                return "";
            }
        }
    };

    /* Add the data ZBI ramdisk to the boot reserve memory list. */
    /* For RiscV, parse initrd in dtb, as below:
        chosen {
            linux,initrd-start = <0x82000000>;
            linux,initrd-end = <0x82800000>;
        };
    */
    if chosen.has_prop("linux,initrd-start") &&
       chosen.has_prop("linux,initrd-end") {
        let start =
            chosen.prop_u32_at("linux,initrd-start", 0).unwrap() as paddr_t;
        let end =
            chosen.prop_u32_at("linux,initrd-end", 0).unwrap() as paddr_t;

        ZX_ASSERT!(IS_PAGE_ALIGNED!(end));
        dprintf!(INFO, "reserving ramdisk phys range [{:x}, {:x}]\n",
                 start, end - 1);

        boot_reserve_add_range(start, end - start).unwrap();
    }

    /* Retrieve command line */
    if let Ok(s) = chosen.prop_str("bootargs") {
        return s;
    }

    ""
}

/*
 * early_init_dt_scan_memory - Look for and parse memory nodes
 */
fn early_init_dt_scan_memory(dt: &DeviceTree, addr_cells: u32, size_cells: u32)
    -> Result<ZBIMemRangeVec, ErrNO> {

    let root = dt.find("/").ok_or_else(|| ErrNO::BadDTB)?;

    let mut mem_config = Vec::<ZBIMemRange>::with_capacity(MAX_ZBI_MEM_RANGES);

    let mut cb = |base, size| {
        add_memory_arch(&mut mem_config, base, size);
    };

    for child in &root.children {
        /* We are scanning "memory" nodes only */
        if let Ok(t) = child.prop_str("device_type") {
            if t != "memory" {
                continue;
            }
        } else {
            continue;
        }

        parse_reg(child, addr_cells, size_cells, &mut cb);
    }

    early_scan_reserved_mem(dt, &mut mem_config, addr_cells, size_cells)?;
    Ok(mem_config)
}

fn parse_reg<F>(node: &Node, addr_cells: u32, size_cells: u32, mut cb: F)
where
    F: FnMut(usize, usize)
{
    let mut pos = 0;
    let reg_len = node.prop_len("reg");
    while pos < reg_len {
        let base = if addr_cells == 2 {
            node.prop_u64_at("reg", pos).unwrap() as usize
        } else {
            node.prop_u32_at("reg", pos).unwrap() as usize
        };
        pos += (addr_cells << 2) as usize;

        let size = if size_cells == 2 {
            node.prop_u64_at("reg", pos).unwrap() as usize
        } else {
            node.prop_u32_at("reg", pos).unwrap() as usize
        };
        pos += (size_cells << 2) as usize;

        if size == 0 {
            continue;
        }
        dprintf!(INFO, " - 0x{:x}, 0x{:x}\n", base, size);

        cb(base, size);
    }
}

fn early_scan_reserved_mem(dt: &DeviceTree, config: &mut ZBIMemRangeVec,
                           addr_cells: u32, size_cells: u32)
    -> Result<(), ErrNO> {

    let mut cb = |base, size| {
        add_reserved_memory_arch(config, base, size);
    };

    let regions = dt.find("/reserved-memory").ok_or_else(|| ErrNO::BadDTB)?;
    for region in &regions.children {
        parse_reg(region, addr_cells, size_cells, &mut cb);
    }

    Ok(())
}

fn add_memory_arch(config: &mut ZBIMemRangeVec, base: usize, size: usize) {
    config.push(ZBIMemRange::new(ZBIMemRangeType::RAM, base, size));
}

fn add_reserved_memory_arch(config: &mut ZBIMemRangeVec,
                            base: usize, size: usize) {
    config.push(ZBIMemRange::new(ZBIMemRangeType::RESERVED, base, size));
}
