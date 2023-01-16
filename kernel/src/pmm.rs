/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::mem;
use core::ptr::null_mut;
use core::sync::atomic::AtomicUsize;
use alloc::string::String;
use crate::debug::*;
use crate::ErrNO;
use crate::klib::list::Linked;
use crate::locking::mutex::Mutex;
use crate::locking::mutex::MutexGuard;
use crate::vm::page_queues::PageQueues;
use crate::{print, dprintf, ZX_ASSERT};
use crate::{PAGE_SIZE, PAGE_SHIFT, paddr_to_physmap};
use alloc::vec::Vec;
use crate::types::*;
use crate::klib::list::List;
use crate::page::vm_page_t;
use crate::vm_page_state::{self, vm_page_state_t};
use core::sync::atomic::Ordering;
use crate::platform::boot_reserve::{
    BootReserveRange, boot_reserve_range_search
};

/* flags for allocation routines below */

/* no restrictions on which arena to allocate from */
pub const PMM_ALLOC_FLAG_ANY: u32 = 0 << 0;
/* allocate only from arenas marked LO_MEM */
#[allow(dead_code)]
pub const PMM_ALLOC_FLAG_LO_MEM: u32 = 1 << 0;
// The caller is able to wait and retry this allocation and so pmm allocation functions are allowed
// to return ZX_ERR_SHOULD_WAIT, as opposed to ZX_ERR_NO_MEMORY, to indicate that the caller should
// wait and try again. This is intended for the PMM to tell callers who are able to wait that memory
// is low. The caller should not infer anything about memory state if it is told to wait, as the PMM
// may tell it to wait for any reason.
pub const PMM_ALLOC_FLAG_CAN_WAIT: u32 = 1 << 1;
// The default (flag not set) is to not allocate a loaned page, so that we don't end up with loaned
// pages allocated for arbitrary purposes that prevent us from getting the loaned page back quickly.
#[allow(dead_code)]
pub const PMM_ALLOC_FLAG_CAN_BORROW: u32 = 1 << 2;
// Require a loaned page, and fail to allocate if a loaned page isn't available.
#[allow(dead_code)]
pub const PMM_ALLOC_FLAG_MUST_BORROW: u32 = 1 << 3;

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
        let page = self.get_page(index);
        if page == null_mut() {
            return Err(ErrNO::NoMem);
        }
        unsafe { (*page).init(paddr); }
        Ok(())
    }

    fn get_page(&self, index: usize) -> *mut vm_page_t {
        let ptr = index * self.obj_size + self.start;
        if ptr >= (self.start + self.len) {
            return null_mut();
        }

        ptr as *mut vm_page_t
    }

    fn set_page_state(&self, index: usize, state: vm_page_state_t)
        -> Result<(), ErrNO> {
        let page = self.get_page(index);
        if page == null_mut() {
            return Err(ErrNO::NoMem);
        }
        unsafe { (*page).set_state(state); }
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

    pub fn init(&mut self, pmm_node: &PmmNode) -> Result<(), ErrNO> {
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
        list.init();

        let mut i = 0;
        while i < page_count {
            let paddr = self.info.base + i * PAGE_SIZE;
            self.page_array.init_page(i, paddr)?;

            if i >= array_start_index && i < array_end_index {
                self.page_array.set_page_state(i, vm_page_state::WIRED)?;
            } else {
                let page = self.page_array.get_page(i);
                if page == null_mut() {
                    return Err(ErrNO::NoMem);
                }

                list.add_tail(page);
            }
            i += 1;
        }

        pmm_node.add_free_pages(&mut list, page_count);
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

    fn address_in_arena(&self, pa: paddr_t) -> bool {
        pa >= self.base() && pa <= self.base() + self.size() - 1
    }

    fn find_specific(&self, pa: paddr_t) -> *mut vm_page_t {
        if !self.address_in_arena(pa) {
            return null_mut();
        }

        let index = (pa - self.base()) / PAGE_SIZE;
        ZX_ASSERT!(index < self.size() / PAGE_SIZE);
        self.page_array.get_page(index)
    }
}

struct FreePageList {
    count: usize,
    list: List<vm_page_t>,
}

impl FreePageList {
    pub const fn new() -> Self {
        Self {
            count: 0,
            list : List::<vm_page_t>::new(),
        }
    }

    fn init(&mut self) {
        self.list.init();
    }
}

/* per numa node collection of pmm arenas and worker threads */
pub struct PmmNode {
    arenas: Mutex<Vec<PmmArena>>,
    arena_cumulative_size: AtomicUsize,

    free_list  : Mutex<FreePageList>,
    page_queues: PageQueues,
}

impl PmmNode {
    pub const fn new() -> Self {
        Self {
            arenas: Mutex::new(Vec::<PmmArena>::new()),
            arena_cumulative_size: AtomicUsize::new(0),

            free_list   : Mutex::new(FreePageList::new()),
            page_queues : PageQueues::new(),
        }
    }

    pub fn init(&self) {
        self.free_list.lock().init();
        self.page_queues.init();
    }

    pub fn page_queues(&self) -> &PageQueues {
        &self.page_queues
    }

    /* during early boot before threading exists. */
    pub fn add_arena(&self, info: ArenaInfo) -> Result<(), ErrNO> {
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

        self.arena_cumulative_size.fetch_add(arena.size(), Ordering::Relaxed);

        /* insert arena in ascending order of its base address */
        let mut pos = 0;
        let mut arenas = self.arenas.lock();
        for a in arenas.iter() {
            if arena.base() < a.base() {
                arenas.insert(pos, arena);
                return Ok(())
            }
            pos += 1;
        }

        arenas.push(arena);
        Ok(())
    }

    pub fn add_free_pages(&self, list: &mut List<vm_page_t>, count: usize) {
        let mut free_list = self.free_list.lock();
        free_list.count += count;
        free_list.list.splice(list);
        // free_pages_evt_.Signal();

        dprintf!(INFO, "free count now {}\n", free_list.count);
    }

    fn alloc_range(&self, address: paddr_t, count: usize,
                   list: &mut List<vm_page_t>) -> Result<(), ErrNO> {
        dprintf!(INFO, "address {:x}, count {:x}\n", address, count);

        /* ZX_ASSERT!(
         * Thread::Current::memory_allocation_state().IsEnabled()); */

        ZX_ASSERT!(list.empty());
        if count == 0 {
            return Ok(());
        }

        let mut address = ROUNDDOWN!(address, PAGE_SIZE);

        let mut allocated: usize = 0;
        /* walk through the arenas, looking to see
         * if the physical page belongs to it */
        let mut free_list = self.free_list.lock();
        let arenas = self.arenas.lock();
        for area in arenas.iter() {
            while allocated < count && area.address_in_arena(address) {
                let page = area.find_specific(address);

                /* As we hold lock_, we can assume that any page
                 * in the FREE state is owned by us, and protected by lock_,
                 * and so should is_free() be true we will be allowed
                 * to assume it is in the free list, remove it from said list,
                 * and allocate it. */
                unsafe {
                    if !(*page).is_free() {
                        break;
                    }
                    /* never allocate loaned pages for caller of AllocRange() */
                    if (*page).is_loaned() {
                        break;
                    }

                    (*page).delete_from_list();
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

        free_list.count -= allocated;

        if allocated != count {
            /* we were not able to allocate the entire run, free these pages */
            self.free_list_locked(list);
            return Err(ErrNO::NotFound);
        }

        Ok(())
    }

    fn alloc_page(&self, _flags: u32) -> *mut vm_page_t {
        let mut free_list = self.free_list.lock();
        let page = free_list.list.pop_head();
        unsafe {
            dprintf!(INFO, "alloc page: pa {:x}\n", (*page).paddr());
            ZX_ASSERT!(!(*page).is_loaned());
            self.alloc_page_helper_locked(page);
        }
        free_list.count -= 1;
        page
    }

    fn alloc_pages(&self, mut count: usize, alloc_flags: u32,
                   list: &mut List<vm_page_t>)
        -> Result<(), ErrNO> {

        //ZX_ASSERT!(Thread::Current::memory_allocation_state().IsEnabled());

        /* list must be initialized prior to calling this */
        ZX_ASSERT!(list.is_initialized());

        if count == 0 {
            return Ok(());
        } else if count == 1 {
            let page = self.alloc_page(alloc_flags);
            if page == null_mut() {
                return Err(ErrNO::NoMem);
            }
            list.add_tail(page);
            return Ok(());
        }

        while count > 0 {
            let mut free_list = self.free_list.lock();
            let page = free_list.list.pop_head();
            if page == null_mut() {
                return Err(ErrNO::NoMem);
            }
            unsafe {
                self.alloc_page_helper_locked(page);
            }
            list.add_tail(page);
            free_list.count -= 1;
            count -= 1;
        }

        Ok(())
    }

    fn free_list_locked(&self, _list: &mut List<vm_page_t>) {
        todo!("Implement [free_list_locked]");
    }

    unsafe fn alloc_page_helper_locked(&self, page: *mut vm_page_t) {
        dprintf!(SPEW, "allocating page pa {:x}, prev state {:x}\n",
                 (*page).paddr(), (*page).state());

        ZX_ASSERT!((*page).is_free());

        if (*page).is_loaned() {
            /* We want the set_stack_owner() to be visible before set_state(),
             * but we don't need to make set_state() a release just for
             * the benefit of loaned pages, so we use this fence. */
            //ktl::atomic_thread_fence(ktl::memory_order_release);
            todo!("Fence!");
        }

        /*
         * Here we transition the page from FREE->ALLOC,
         * completing the transfer of ownership from the PmmNode to the stack.
         * This must be done under lock_, and more specifically
         * the same lock_ acquisition that removes the page from the free list,
         * as both being the free list, or being in the ALLOC state,
         * indicate ownership by the PmmNode.
         */
        (*page).set_state(vm_page_state::ALLOC);
    }

    /* We don't need to hold the arena lock while executing this,
       since it is only accesses values that are set once
       during system initialization. */
    fn paddr_to_page(&self, pa: paddr_t) -> *mut vm_page_t {
        let arenas = self.arenas.lock();
        for arena in arenas.iter() {
            if !arena.address_in_arena(pa) {
                continue;
            }
            let index = (pa - arena.base()) / PAGE_SIZE;
            return arena.page_array.get_page(index);
        }
        null_mut()
    }

    pub fn _num_arenas(&self) -> usize {
        self.arenas.lock().len()
    }

    pub fn get_arenas(&self) -> MutexGuard<Vec<PmmArena>> {
        self.arenas.lock()
    }
}

pub fn pmm_alloc_range(pa: paddr_t, count: usize, list: &mut List<vm_page_t>)
    -> Result<(), ErrNO>{
    PMM_NODE.alloc_range(pa, count, list)
}

pub fn pmm_alloc_page(flags: u32) -> *mut vm_page_t {
    PMM_NODE.alloc_page(flags)
}

pub fn pmm_alloc_pages(count: usize, alloc_flags: u32,
                       list: &mut List<vm_page_t>)
    -> Result<(), ErrNO> {
    PMM_NODE.alloc_pages(count, alloc_flags, list)
}

pub fn pmm_add_arena(info: ArenaInfo) -> Result<(), ErrNO> {
    dprintf!(INFO, "Arena.{}: flags[{:x}] {:x} {:x}\n",
             info.name, info.flags, info.base, info.size);
    PMM_NODE.add_arena(info)
}

pub fn pmm_alloc_contiguous(count: usize, alloc_flags: u32,
                            alignment_log2: usize, _pa: &mut paddr_t,
                            list: &mut List<vm_page_t>)
    -> Result<(), ErrNO> {
    /* if we're called with a single page, just fall through to
     * the regular allocation routine */
    if count == 1 && alignment_log2 <= PAGE_SHIFT {
        let page = PMM_NODE.alloc_page(alloc_flags);
        if page == null_mut() {
            return Err(ErrNO::NoMem);
        }
        list.add_tail(page);
        return Ok(());
    }

    todo!("pmm_alloc_contiguous");
    //pmm_node.alloc_contiguous(count, alloc_flags, alignment_log2, pa, list)
}

pub fn paddr_to_vm_page(pa: paddr_t) -> *mut vm_page_t {
    PMM_NODE.paddr_to_page(pa)
}

pub fn pmm_free(_list: &List::<vm_page_t>) {
    todo!("pmm_free!");
    //pmm_node.FreeList(list)
}

pub fn pmm_page_queues() -> &'static PageQueues {
    PMM_NODE.page_queues()
}

pub static PMM_NODE: PmmNode = PmmNode::new();