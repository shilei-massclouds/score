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

#[repr(C)]
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
            unsafe {(*next.as_ptr()).prev = self.prev;}
        }

        if let Some(prev) = self.prev.take() {
            unsafe {(*prev.as_ptr()).next = self.next.take();}
        }
    }
}

pub struct Iter<'a, T: Linked<T> + 'a> {
    ref_node: Option<NonNull<ListNode>>,    /* ref to node */
    len: usize,
    marker: PhantomData<&'a NonNull<T>>,
}

impl<'a, T: Linked<T>> Iterator for Iter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        if self.len == 0 {
            None
        } else {
            if let Some(node) = self.ref_node {
                unsafe {
                    self.len -= 1;
                    self.ref_node = (*node.as_ptr()).next;
                    T::from_node(node).map(|ptr| {
                        &(*ptr.as_ptr())
                    })
                }
            } else {
                None
            }
        }
    }
}

pub struct IterMut<'a, T: Linked<T> + 'a> {
    ref_node: Option<NonNull<ListNode>>,    /* ref to node */
    len: usize,
    marker: PhantomData<&'a NonNull<T>>,
}

impl<'a, T: Linked<T>> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    #[inline]
    fn next(&mut self) -> Option<&'a mut T> {
        if self.len == 0 {
            None
        } else {
            if let Some(node) = self.ref_node {
                unsafe {
                    self.len -= 1;
                    self.ref_node = (*node.as_ptr()).next;
                    T::from_node(node).map(|ptr| {
                        &mut (*ptr.as_ptr())
                    })
                }
            } else {
                None
            }
        }
    }
}

#[repr(C)]
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
    pub const fn new() -> Self {
        Self {
            node: ListNode::new(),
            ref_node: None,
            len: 0,
            marker: PhantomData
        }
    }

    #[inline]
    #[must_use]
    pub fn init(&mut self) {
        self.ref_node = NonNull::new(&mut self.node);
        self.node.next = self.ref_node;
        self.node.prev = self.ref_node;
    }

    pub fn iter(&self) -> Iter<T> {
        Iter { ref_node: self.node.next, len: self.len, marker: PhantomData }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut { ref_node: self.node.next, len: self.len, marker: PhantomData }
    }

    pub fn empty(&self) -> bool {
        self.len == 0
    }

    pub fn add_head(&mut self, mut elt: NonNull<T>) {
        unsafe { self.add_head_node(elt.as_mut().into_node()); }
    }

    pub fn head(&self) -> Option<NonNull<T>> {
        if let Some(next) = self.node.next {
            T::from_node(next)
        } else {
            None
        }
    }

    pub fn pop_head(&mut self) -> Option<NonNull<T>> {
        if self.node.next == self.ref_node {
            return None;
        }

        let mut head = self.head();
        if let Some(mut node) = head {
            unsafe {
                node.as_mut().delete_from_list();
            }
        }

        head
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

    pub fn splice(&mut self, other: &mut Self) {
        if other.node.prev == other.ref_node {
            return;
        }

        if let Some(next) = other.node.next {
            unsafe {(*next.as_ptr()).prev = self.node.prev;}
        }
        if let Some(prev) = other.node.prev {
            unsafe {(*prev.as_ptr()).next = self.ref_node;}
        }

        if self.node.next == self.ref_node {
            self.node.next = other.node.next;
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
