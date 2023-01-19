/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

use core::sync::atomic::{Ordering, AtomicUsize};

use crate::ZX_ASSERT;
use crate::klib::list::List;
use crate::vm_page_state;
use crate::page::vm_page_t;
use crate::klib::list::Linked;
use crate::locking::mutex::Mutex;

pub struct PageQueues {
    // The page queues are placed into an array, indexed by page queue, for consistency and uniformity
    // of access. This does mean that the list for PageQueueNone does not actually have any pages in
    // it, and should always be empty.
    // The reclaimable queues are the more complicated as, unlike the other categories, pages can be
    // in one of the queues, and can move around. The reclaimable queues themselves store pages that
    // are roughly grouped by their last access time. The relationship is not precise as pages are not
    // moved between queues unless it becomes strictly necessary. This is in contrast to the queue
    // counts that are always up to date.
    //
    // What this means is that the vm_page::page_queue index is always up to do date, and the
    // page_queue_counts_ represent an accurate count of pages with that vm_page::page_queue index,
    // but counting the pages actually in the linked list may not yield the correct number.
    //
    // New reclaimable pages are always placed into the queue associated with the MRU generation. If
    // they get accessed the vm_page_t::page_queue gets updated along with the counts. At some point
    // the LRU queue will get processed (see |ProcessDontNeedAndLruQueues|) and this will cause pages
    // to get relocated to their correct list.
    //
    // Consider the following example:
    //
    //  LRU  MRU            LRU  MRU            LRU   MRU            LRU   MRU        MRU  LRU
    //    |  |                |  |                |     |              |     |            |  |
    //    |  |    Insert A    |  |    Age         |     |  Touch A     |     |  Age       |  |
    //    V  v    Queue=2     v  v    Queue=2     v     v  Queue=3     v     v  Queue=3   v  v
    // [][ ][ ][] -------> [][ ][a][] -------> [][ ][a][ ] -------> [][ ][a][ ] -------> [ ][ ][a][]
    //
    // At this point page A, in its vm_page_t, has its queue marked as 3, and the page_queue_counts
    // are {0,0,1,0}, but the page itself remains in the linked list for queue 2. If the LRU queue is
    // then processed to increment it we would do.
    //
    //  MRU  LRU             MRU  LRU            MRU    LRU
    //    |  |                 |    |              |      |
    //    |  |       Move LRU  |    |    Move LRU  |      |
    //    V  v       Queue=3   v    v    Queue=3   v      v
    //   [ ][ ][a][] -------> [ ][][a][] -------> [][ ][][a]
    //
    // In the second processing of the LRU queue it gets noticed that the page, based on
    // vm_page_t::page_queue, is in the wrong queue and gets moved into the correct one.
    //
    // For specifics on how LRU and MRU generations map to LRU and MRU queues, see comments on
    // |lru_gen_| and |mru_gen_|.
    page_queues: [Mutex<List<vm_page_t>>; Self::PAGE_QUEUE_NUM_QUEUES],

    // Tracks the counts of pages in each queue in O(1) time complexity. As pages are moved between
    // queues, the corresponding source and destination counts are decremented and incremented,
    // respectively.
    //
    // The first entry of the array is left special: it logically represents pages not in any queue.
    // For simplicity, it is initialized to zero rather than the total number of pages in the system.
    // Consequently, the value of this entry is a negative number with absolute value equal to the
    // total number of pages in all queues. This approach avoids unnecessary branches when updating
    // counts.
    page_queue_counts: [AtomicUsize; Self::PAGE_QUEUE_NUM_QUEUES],
}

impl PageQueues {
    // The number of reclamation queues is slightly arbitrary, but to be useful you want at least 3
    // representing
    //  * Very new pages that you probably don't want to evict as doing so probably implies you are in
    //    swap death
    //  * Slightly old pages that could be evicted if needed
    //  * Very old pages that you'd be happy to evict
    // With two active queues 8 page queues are used so that there is some fidelity of information in
    // the inactive queues. Additional queues have reduced value as sufficiently old pages quickly
    // become equivalently unlikely to be used in the future.
    const K_NUM_RECLAIM: usize = 8;

    /* Specifies the indices for both the page_queues and the page_queue_counts */
    pub const PAGE_QUEUE_NONE       : usize = 0;
    pub const PAGE_QUEUE_ANONYMOUS  : usize = 1;
    pub const PAGE_QUEUE_WIRED      : usize = 2;
    #[allow(dead_code)]
    pub const PAGE_QUEUE_ANONYMOUS_ZERO_FORK: usize = 3;
    #[allow(dead_code)]
    pub const PAGE_QUEUE_PAGER_BACKED_DIRTY : usize = 4;
    #[allow(dead_code)]
    pub const PAGE_QUEUE_RECLAIM_DONT_NEED  : usize = 5;

    pub const PAGE_QUEUE_RECLAIM_BASE : usize = 6;
    pub const PAGE_QUEUE_RECLAIM_LAST : usize =
        Self::PAGE_QUEUE_RECLAIM_BASE + Self::K_NUM_RECLAIM - 1;

    pub const PAGE_QUEUE_NUM_QUEUES: usize = Self::PAGE_QUEUE_RECLAIM_LAST + 1;

    const _PAGE_QUEUE_INIT: Mutex<List::<vm_page_t>> =
        Mutex::new(List::<vm_page_t>::new());

    const _PAGE_QUEUE_COUNT_INIT: AtomicUsize = AtomicUsize::new(0);

    pub const fn new() -> Self {
        Self {
            page_queues: [Self::_PAGE_QUEUE_INIT; Self::PAGE_QUEUE_NUM_QUEUES],
            page_queue_counts: [Self::_PAGE_QUEUE_COUNT_INIT; Self::PAGE_QUEUE_NUM_QUEUES],
        }
    }

    pub fn init(&self) {
        for pl in &self.page_queues {
            pl.lock().init();
        }
    }

    pub fn set_anonymous(&self, page: *mut vm_page_t,
                         object: usize, page_offset: usize)
    {
        let page_ref = unsafe { &mut (*page) };
        self.set_queue_backlink_locked(page_ref, object, page_offset,
                                       Self::PAGE_QUEUE_ANONYMOUS);
    }

    fn move_to_queue_locked(&self, _page: *mut vm_page_t, _queue: usize) {
        todo!("move_to_queue_locked!");
    }

    pub fn move_to_wired(&self, page: *mut vm_page_t) {
        self.move_to_queue_locked(page, Self::PAGE_QUEUE_WIRED);
    }

    fn set_queue_backlink_locked(&self, page: &mut vm_page_t, object: usize,
                                 page_offset: usize, queue: usize)
    {
        ZX_ASSERT!(page.state() == vm_page_state::OBJECT);
        ZX_ASSERT!(!page.is_free());
        ZX_ASSERT!(!page.is_in_list());
        ZX_ASSERT!(page.object.get_object() == 0);
        ZX_ASSERT!(page.object.get_page_offset() == 0);

        page.object.set_object(object);
        page.object.set_page_offset(page_offset);

        ZX_ASSERT!(page.object.page_queue.load(Ordering::Relaxed) == Self::PAGE_QUEUE_NONE as u8);
        page.object.page_queue.store(queue as u8, Ordering::Relaxed);

        let ptr = &mut (*page) as *mut vm_page_t;
        self.page_queues[queue].lock().add_head(ptr);
        self.page_queue_counts[queue].fetch_add(1, Ordering::Relaxed);
        // UpdateActiveInactiveLocked(PageQueueNone, queue);
    }
}