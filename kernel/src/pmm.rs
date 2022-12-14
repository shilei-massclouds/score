/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::mem;
use core::ptr::NonNull;
use alloc::string::String;
use crate::debug::*;
use crate::ErrNO;
use crate::klib::list::Linked;
use crate::{print, dprintf, ZX_DEBUG_ASSERT};
use crate::{PAGE_SIZE, paddr_to_physmap};
use spin::Mutex;
use alloc::vec::Vec;
use core::sync::atomic::AtomicU64;
use crate::types::*;
use crate::klib::list::List;
use crate::page::vm_page_t;
use crate::vm_page_state::{self, vm_page_state_t};
use core::sync::atomic::Ordering;
use crate::platform::boot_reserve::{
    BootReserveRange, boot_reserve_range_search
};

/* all of the configured memory arenas */
pub const MAX_ARENAS: usize = 16;

pub struct ArenaInfo {
    pub name: String,
    pub flags: u32,
    pub base: usize,
    pub size: usize,
}

impl ArenaInfo {
    pub fn new(name: &str, flags: u32, base: usize, size: usize) -> ArenaInfo {
        ArenaInfo {
            name: String::from(name),
            flags, base, size
        }
    }
}

struct PageArray {
    start:      paddr_t,
    len:        usize,
    obj_size:   usize,
}

impl PageArray {
    fn new() -> Self {
        Self {
            start:  0,
            len:    0,
            obj_size: mem::size_of::<vm_page_t>(),
        }
    }

    fn init(&mut self, start: paddr_t, len: usize) {
        self.start = start;
        self.len = len;
    }

    fn init_page(&self, index: usize, paddr: paddr_t) -> Result<(), ErrNO> {
        let mut page = self.get_page(index).ok_or_else(|| ErrNO::NoMem)?;

        unsafe { page.as_mut().init(paddr); }
        Ok(())
    }

    fn get_page(&self, index: usize) -> Option<NonNull<vm_page_t>> {
        let ptr = index * self.obj_size + self.start;
        if ptr >= (self.start + self.len) {
            return None;
        }

        NonNull::<vm_page_t>::new(ptr as *mut vm_page_t)
    }

    fn set_page_state(&self, index: usize, state: vm_page_state_t)
        -> Result<(), ErrNO> {
        let mut page = self.get_page(index).ok_or_else(|| ErrNO::NoMem)?;

        unsafe { page.as_mut().set_state(state); }
        Ok(())
    }
}

pub struct PmmArena {
    info: ArenaInfo,
    page_array: PageArray,
}

impl PmmArena {
    pub fn new(info: ArenaInfo) -> PmmArena {
        PmmArena {
            info,
            page_array: PageArray::new(),
        }
    }

    pub fn init(&mut self, pmm_node: &mut PmmNode) -> Result<(), ErrNO> {
        /* allocate an array of pages to back this one */
        let page_count = self.info.size / PAGE_SIZE;
        let vm_page_sz = mem::size_of::<vm_page_t>();
        let page_array_size = ROUNDUP_PAGE_SIZE!(page_count*vm_page_sz);

        /* if the arena is too small to be useful, bail */
        if page_array_size >= self.info.size {
            dprintf!(CRITICAL,
                     "PMM: arena too small to hold page array ({:x})\n",
                     self.info.size);
            return Err(ErrNO::LackBuf);
        }

        /* allocate a chunk to back the page array out of
         * the arena itself, near the top of memory */
        let mut range = BootReserveRange::default();
        boot_reserve_range_search(self.info.base, self.info.size,
                                  page_array_size,
                                  &mut range)?;

        if range.pa < self.info.base || range.len > page_array_size {
            return Err(ErrNO::OutOfRange);
        }

        dprintf!(INFO, "page array chunk {:x} ~ {:x}\n", range.pa, range.len);

        let page_array_va = paddr_to_physmap(range.pa);
        self.page_array.init(page_array_va, page_array_size);

        /* |page_count| pages in the state FREE */
        //vm_page::add_to_initial_count(vm_page_state::FREE, page_count);

        /* compute the range of the array that backs the array itself */
        let array_start_index =
            (PAGE_ALIGN!(range.pa) - self.info.base) / PAGE_SIZE;
        let array_end_index = array_start_index + page_array_size / PAGE_SIZE;

        dprintf!(INFO, "array_start_index {}, array_end_index {}\n",
                 array_start_index, array_end_index);

        if array_start_index >= page_count || array_end_index > page_count {
            return Err(ErrNO::BadRange);
        }

        dprintf!(INFO, "init page_array ...\n");

        /* add all pages that aren't part of the page array
         * to the free list pages */
        let mut list = List::new();

        let mut i = 0;
        while i < page_count {
            let paddr = self.info.base + i * PAGE_SIZE;
            self.page_array.init_page(i, paddr)?;

            if i >= array_start_index && i < array_end_index {
                self.page_array.set_page_state(i, vm_page_state::WIRED)?;
            } else {
                let page = self.page_array.get_page(i)
                    .ok_or_else(|| ErrNO::NoMem)?;

                list.add_tail(page);
            }
            i += 1;
        }

        pmm_node.add_free_pages(&mut list);
        dprintf!(INFO, "init page_array ok!\n");
        Ok(())
    }

    pub fn name(&self) -> &str {
        self.info.name.as_str()
    }

    pub fn base(&self) -> paddr_t {
        self.info.base
    }

    pub fn size(&self) -> usize {
        self.info.size
    }

    fn address_in_arena(&self, address: paddr_t) -> bool {
        address >= self.base() && address <= self.base() + self.size() - 1
    }

    fn find_specific(&self, pa: paddr_t) -> Option<NonNull<vm_page_t>> {
        if !self.address_in_arena(pa) {
            return None;
        }

        let index = (pa - self.base()) / PAGE_SIZE;
        ZX_DEBUG_ASSERT!(index < self.size() / PAGE_SIZE);

        self.page_array.get_page(index)
    }
}

/* per numa node collection of pmm arenas and worker threads */
pub struct PmmNode {
    arenas: Vec<PmmArena>,

    arena_cumulative_size: usize,

    /* Free pages where !loaned. */
    free_count  : AtomicU64,
    free_list   : Option<List<vm_page_t>>,
}

impl PmmNode {
    pub const fn new() -> Self {
        Self {
            arenas: Vec::<PmmArena>::new(),

            arena_cumulative_size: 0,

            free_count  : AtomicU64::new(0),
            free_list   : None,
        }
    }

    pub fn init(&mut self) {
        self.free_list = Some(List::<vm_page_t>::new());
    }

    /* during early boot before threading exists. */
    pub fn add_arena(&mut self, info: ArenaInfo) -> Result<(), ErrNO> {
        dprintf!(INFO, "PMM: adding arena '{}' base {:x} size {:x}\n",
                 info.name, info.base, info.size);

        if !IS_PAGE_ALIGNED!(info.base) ||
           !IS_PAGE_ALIGNED!(info.size) ||
           (info.size == 0) {
            return Err(ErrNO::BadAlign);
        }

        let mut arena = PmmArena::new(info);
        if let Err(e) = arena.init(self) {
            dprintf!(CRITICAL, "PMM: pmm_add_arena failed {:?}\n", e);
            /* but ignore this failure */
            return Ok(());
        }

        dprintf!(INFO, "Adding arena '{}' ...\n", arena.name());

        self.arena_cumulative_size += arena.size();

        /* insert arena in ascending order of its base address */
        let mut pos = 0;
        for a in &(self.arenas) {
            if arena.base() < a.base() {
                return Ok(self.arenas.insert(pos, arena));
            }
            pos += 1;
        }

        Ok(self.arenas.push(arena))
    }

    pub fn add_free_pages(&mut self, list: &mut List<vm_page_t>) {
        self.free_count.fetch_add(list.len() as u64, Ordering::Relaxed);
        match &mut self.free_list {
            Some(free_list) => {
                free_list.append(list);
            },
            None => {
                panic!("free_list is None");
            }
        }
        // free_pages_evt_.Signal();

        dprintf!(INFO, "free count now {}\n",
                 self.free_count.load(Ordering::Relaxed));
    }

    fn alloc_range(&mut self, address: paddr_t, count: usize, list: &mut List<vm_page_t>)
        -> Result<(), ErrNO> {
        dprintf!(INFO, "address {:x}, count {:x}\n", address, count);

        /* ZX_DEBUG_ASSERT!(Thread::Current::memory_allocation_state().IsEnabled()); */
        ZX_DEBUG_ASSERT!(list.empty());
        if count == 0 {
            return Ok(());
        }

        let mut address = ROUNDDOWN!(address, PAGE_SIZE);

        let mut allocated: usize = 0;
        /* walk through the arenas, looking to see if the physical page belongs to it */
        for area in &self.arenas {
            while allocated < count && area.address_in_arena(address) {
                let mut page: NonNull<vm_page_t>;
                if let Some(p) = area.find_specific(address) {
                    page = p;
                } else {
                    break;
                }

                /* As we hold lock_, we can assume that any page
                 * in the FREE state is owned by us, and protected by lock_,
                 * and so should is_free() be true we will be allowed
                 * to assume it is in the free list, remove it from said list,
                 * and allocate it. */
                unsafe {
                    if !page.as_ref().is_free() {
                        break;
                    }
                    /* We never allocate loaned pages for caller of AllocRange() */
                    if page.as_ref().is_loaned() {
                        break;
                    }

                    page.as_mut().delete_from_list();
                    self.alloc_page_helper_locked(page);
                    list.add_tail(page);
                    allocated += 1;
                }

                address += PAGE_SIZE;
            }

            if allocated == count {
                break;
            }
        }

        self.decrement_free_count_locked(allocated as u64);

        if allocated != count {
            /* we were not able to allocate the entire run, free these pages */
            self.free_list_locked(list);
            return Err(ErrNO::NotFound);
        }

        Ok(())
    }

    fn free_list_locked(&self, list: &mut List<vm_page_t>) {
        todo!("Implement [free_list_locked]");
    }

    unsafe fn alloc_page_helper_locked(&self, mut page: NonNull<vm_page_t>) {
        dprintf!(INFO, "allocating page pa {:x}, prev state {:x}\n",
                 page.as_ref().paddr(), page.as_ref().state());

        ZX_DEBUG_ASSERT!(page.as_ref().is_free());

        if page.as_ref().is_loaned() {
            /* We want the set_stack_owner() to be visible before set_state(), but we don't need to make
             * set_state() a release just for the benefit of loaned pages, so we use this fence. */
            //ktl::atomic_thread_fence(ktl::memory_order_release);
            todo!("Fence!");
        }

        /*
         * Here we transition the page from FREE->ALLOC, completing the transfer of ownership
         * from the PmmNode to the stack. This must be done under lock_, and more specifically
         * the same lock_ acquisition that removes the page from the free list, as both being
         * the free list, or being in the ALLOC state, indicate ownership by the PmmNode.
         */
        page.as_mut().set_state(vm_page_state::ALLOC);
    }

    fn decrement_free_count_locked(&mut self, amount: u64) {
        ZX_DEBUG_ASSERT!(self.free_count.load(Ordering::Relaxed) >= amount);
        self.free_count.fetch_sub(amount, Ordering::Relaxed);
    }

}

pub fn pmm_alloc_range(address: paddr_t, count: usize, list: &mut List<vm_page_t>) {
    PMM_NODE.lock().alloc_range(address, count, list);
}

pub fn pmm_add_arena(info: ArenaInfo) -> Result<(), ErrNO> {
    let mut pmm_node = PMM_NODE.lock();
    dprintf!(INFO, "Arena.{}: flags[{:x}] {:x} {:x}\n",
             info.name, info.flags, info.base, info.size);
    pmm_node.add_arena(info)
}

pub static PMM_NODE: Mutex<PmmNode> = Mutex::new(PmmNode::new());
