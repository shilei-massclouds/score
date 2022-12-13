/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

use core::mem;
use core::ptr::NonNull;
use core::marker::PhantomData;

pub trait Linked<T> {
    fn from_node(ptr: NonNull<ListNode>) -> Option<NonNull<T>> {
        NonNull::<T>::new(ptr.as_ptr() as *mut T)
    }

    fn into_node(&mut self) -> &mut ListNode;

    fn delete_from_list(&mut self) {
        self.into_node().delete_from_list();
    }
}

pub struct ListNode {
    next: Option<NonNull<ListNode>>,
    prev: Option<NonNull<ListNode>>,
}

impl ListNode {
    pub const fn new() -> Self {
        ListNode {next: None, prev: None}
    }

    pub fn delete_from_list(&mut self) {
        if self.prev.is_none() || self.next.is_none() {
            return;
        }

        if let Some(next) = self.next {
            unsafe {(*next.as_ptr()).prev = self.prev.take();}
        }

        if let Some(prev) = self.prev {
            unsafe {(*prev.as_ptr()).next = self.next.take();}
        }
    }
}

pub struct List<T: Linked<T>> {
    node: ListNode,
    ref_node: Option<NonNull<ListNode>>,    /* ref to node */
    len: usize,
    marker: PhantomData<NonNull<T>>,
}

impl<T: Linked<T>> List<T> {
    /* Creates an empty `LinkedList`. */
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        let mut list = List {
            node: ListNode::new(),
            ref_node: None,
            len: 0,
            marker: PhantomData
        };

        list.ref_node = NonNull::new(&mut list.node);
        list.node.next = list.ref_node;
        list.node.prev = list.ref_node;

        list
    }

    pub fn empty(&self) -> bool {
        self.len == 0
    }

    pub fn add_head(&mut self, mut elt: NonNull<T>) {
        unsafe { self.add_head_node(elt.as_mut().into_node()); }
    }

    /* Adds the given node to the head of the list. */
    #[inline]
    fn add_head_node(&mut self, node: &mut ListNode) {
        node.next = self.node.next;
        node.prev = self.ref_node;
        let node = Some(node.into());

        if let Some(next) = self.node.next {
            unsafe {(*next.as_ptr()).prev = node;}
        }
        self.node.next = node;

        self.len += 1;
    }

    /* Adds the given node to the tail of the list. */
    #[inline]
    fn add_tail_node(&mut self, node: &mut ListNode) {
        node.prev = self.node.prev;
        node.next = self.ref_node;
        let node = Some(node.into());

        if let Some(prev) = self.node.prev {
            unsafe {(*prev.as_ptr()).next = node;}
        }
        self.node.prev = node;

        self.len += 1;
    }

    pub fn add_tail(&mut self, mut elt: NonNull<T>) {
        unsafe { self.add_tail_node(elt.as_mut().into_node()); }
    }

    pub fn append(&mut self, other: &mut Self) {
        if other.node.prev == other.ref_node {
            return;
        }

        if let Some(next) = other.node.next {
            unsafe {(*next.as_ptr()).prev = self.node.prev;}
        }
        if let Some(prev) = other.node.prev {
            unsafe {(*prev.as_ptr()).next = self.ref_node;}
        }

        if let Some(prev) = self.node.prev {
            unsafe {(*prev.as_ptr()).next = other.node.next.take();}
        }
        self.node.prev = other.node.prev.take();

        self.len += mem::replace(&mut other.len, 0);
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

unsafe impl Send for ListNode {}
unsafe impl Sync for ListNode {}

unsafe impl<T: Send + Linked<T>> Send for List<T> {}
unsafe impl<T: Sync + Linked<T>> Sync for List<T> {}
