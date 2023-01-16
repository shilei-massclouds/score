/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

use core::{marker::PhantomData, ptr::null_mut};
use crate::{ZX_ASSERT_MSG, ZX_ASSERT};

#[macro_export(local_inner_macros)]
macro_rules! offset_of {
    ($type:ty, $field:tt) => ({
        let dummy = core::mem::MaybeUninit::<$type>::uninit();

        let dummy_ptr = dummy.as_ptr();
        let member_ptr = core::ptr::addr_of!((*dummy_ptr).$field);

        member_ptr as usize - dummy_ptr as usize
    })
}

#[macro_export]
macro_rules! container_of {
	($ptr:expr, $type:path, $field:ident) => {
		$ptr.cast::<u8>()
			.sub($crate::offset_of!($type, $field))
			.cast::<$type>()
	};
}

pub trait Linked<T> {
    fn from_node(ptr: *mut ListNode) -> *mut T;

    fn into_node(&mut self) -> *mut ListNode;

    fn is_in_list(&mut self) -> bool {
        unsafe {
            (*self.into_node()).is_in_list()
        }
    }

    fn delete_from_list(&mut self) {
        unsafe {
            (*self.into_node()).delete_from_list();
        }
    }

    fn next(&mut self) -> *mut T {
        unsafe {
            Self::from_node((*self.into_node()).next)
        }
    }
}

#[repr(C)]
pub struct ListNode {
    next: *mut ListNode,
    prev: *mut ListNode,
}

impl ListNode {
    pub const fn new() -> Self {
        ListNode {next: null_mut(), prev: null_mut()}
    }

    pub fn init(&mut self) {
        self.next = null_mut();
        self.prev = null_mut();
    }

    pub fn is_in_list(&self) -> bool {
        !self.next.is_null()
    }

    pub fn delete_from_list(&mut self) {
        if self.prev == null_mut() || self.next == null_mut() {
            return;
        }

        unsafe {
            (*self.next).prev = self.prev;
            (*self.prev).next = self.next;
        }
        self.next = null_mut();
        self.prev = null_mut();
    }

    pub fn next(&self) -> *mut Self {
        self.next
    }
}

pub struct Iter<'a, T: Linked<T> + 'a> {
    cursor: *mut ListNode,
    head: *mut ListNode,
    marker: PhantomData<&'a *mut T>,
}

impl<'a, T: Linked<T>> Iterator for Iter<'a, T> {
    type Item = *mut T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor == self.head {
            None
        } else {
            unsafe {
                self.cursor = (*self.cursor).next;
                Some(T::from_node(self.cursor))
            }
        }
    }
}

pub struct IterMut<'a, T: Linked<T> + 'a> {
    cursor: *mut ListNode,
    head: *mut ListNode,
    marker: PhantomData<&'a *mut T>,
}

impl<'a, T: Linked<T>> Iterator for IterMut<'a, T> {
    type Item = *mut T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor == self.head {
            None
        } else {
            unsafe {
                self.cursor = (*self.cursor).next;
                Some(T::from_node(self.cursor))
            }
        }
    }
}

#[repr(C)]
pub struct List<T: Linked<T>> {
    node: ListNode,
    ref_node: *mut ListNode,    /* ref to node */
    marker: PhantomData<*mut T>,
}

impl<T: Linked<T>> List<T> {
    /* Creates an empty `LinkedList`. */
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            node: ListNode::new(),
            ref_node: null_mut(),
            marker: PhantomData
        }
    }

    #[inline]
    pub fn init(&mut self) {
        self.ref_node = &mut self.node;
        self.node.next = self.ref_node;
        self.node.prev = self.ref_node;
    }

    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.ref_node != null_mut()
    }

    #[inline]
    pub fn node(&self) -> *mut T {
        T::from_node(self.ref_node)
    }

    pub fn iter(&self) -> Iter<T> {
        Iter { cursor: self.node.next, head: self.ref_node, marker: PhantomData }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut { cursor: self.node.next, head: self.ref_node, marker: PhantomData }
    }

    pub fn empty(&self) -> bool {
        self.node.next == self.ref_node
    }

    pub fn add_head(&mut self, elt: *mut T) {
        ZX_ASSERT_MSG!(self.is_initialized(), "List hasn't been initialized!");
        unsafe { self.add_head_node((*elt).into_node()); }
    }

    pub fn head(&self) -> *mut T {
        ZX_ASSERT_MSG!(self.is_initialized(), "List hasn't been initialized!");
        T::from_node(self.node.next)
    }

    pub fn tail(&self) -> *mut T {
        ZX_ASSERT_MSG!(self.is_initialized(), "List hasn't been initialized!");
        T::from_node(self.node.prev)
    }

    pub fn pop_head(&mut self) -> *mut T {
        ZX_ASSERT_MSG!(self.is_initialized(), "List hasn't been initialized!");
        if self.node.next == self.ref_node {
            return null_mut();
        }

        let head = self.head();
        unsafe { (*head).delete_from_list(); }
        head
    }

    /* Adds the given node to the head of the list. */
    #[inline]
    fn add_head_node(&mut self, node: *mut ListNode) {
        unsafe {
            (*node).next = self.node.next;
            (*node).prev = self.ref_node;
            (*self.node.next).prev = node;
        }
        self.node.next = node;
    }

    /* Adds the given node to the tail of the list. */
    #[inline]
    fn add_tail_node(&mut self, node: *mut ListNode) {
        unsafe {
            (*node).prev = self.node.prev;
            (*node).next = self.ref_node;
            (*self.node.prev).next = node;
        }
        self.node.prev = node;
    }

    pub fn add_tail(&mut self, elt: *mut T) {
        ZX_ASSERT_MSG!(self.is_initialized(), "List hasn't been initialized!");
        unsafe { self.add_tail_node((*elt).into_node()); }
    }

    pub fn splice(&mut self, other: &mut Self) {
        ZX_ASSERT_MSG!(self.is_initialized(), "List hasn't been initialized!");
        if other.node.prev == other.ref_node {
            return;
        }

        unsafe {
            (*other.node.next).prev = self.node.prev;
            (*other.node.prev).next = self.ref_node;
            (*self.node.prev).next = other.node.next;
        }
        self.node.prev = other.node.prev;

        other.init();
    }

    pub fn _len(&self) -> usize {
        let mut ret = 0;
        let mut next = self.node.next;
        ZX_ASSERT!(next != null_mut());
        while next != self.ref_node {
            ret += 1;
            unsafe {
                next = (*next).next;
            }
        }

        ret
    }
}

unsafe impl Send for ListNode {}
unsafe impl Sync for ListNode {}

unsafe impl<T: Send + Linked<T>> Send for List<T> {}
unsafe impl<T: Sync + Linked<T>> Sync for List<T> {}