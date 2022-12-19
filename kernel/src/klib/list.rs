/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

use core::ptr::NonNull;
use core::marker::PhantomData;
use crate::ZX_ASSERT;

pub trait Linked<T> {
    fn from_node(ptr: NonNull<ListNode>) -> Option<NonNull<T>> {
        NonNull::<T>::new(ptr.as_ptr() as *mut T)
    }

    fn into_node(&mut self) -> &mut ListNode;

    fn delete_from_list(&mut self) {
        self.into_node().delete_from_list();
    }

    fn next(&mut self) -> Option<NonNull<T>> {
        match self.into_node().next() {
            Some(ptr) => Self::from_node(ptr),
            None => None,
        }
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

    pub fn next(&self) -> Option<NonNull<Self>> {
        self.next
    }
}

pub struct Iter<'a, T: Linked<T> + 'a> {
    ref_node: Option<NonNull<ListNode>>,    /* ref to node */
    head: Option<NonNull<ListNode>>,   /* head of list */
    marker: PhantomData<&'a NonNull<T>>,
}

impl<'a, T: Linked<T>> Iterator for Iter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        if self.ref_node == self.head {
            None
        } else {
            if let Some(node) = self.ref_node {
                unsafe {
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
    head: Option<NonNull<ListNode>>,   /* head of list */
    marker: PhantomData<&'a NonNull<T>>,
}

impl<'a, T: Linked<T>> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    #[inline]
    fn next(&mut self) -> Option<&'a mut T> {
        if self.ref_node == self.head {
            None
        } else {
            if let Some(node) = self.ref_node {
                unsafe {
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
            marker: PhantomData
        }
    }

    #[inline]
    pub fn init(&mut self) {
        self.ref_node = NonNull::new(&mut self.node);
        self.node.next = self.ref_node;
        self.node.prev = self.ref_node;
    }

    #[inline]
    pub fn is_initialized(&self) -> bool {
        !self.ref_node.is_none()
    }

    #[inline]
    pub fn node(&self) -> Option<NonNull<T>> {
        match self.ref_node {
            Some(node) => T::from_node(node),
            None => None
        }
    }

    pub fn iter(&self) -> Iter<T> {
        Iter { ref_node: self.node.next, head: self.ref_node, marker: PhantomData }
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut { ref_node: self.node.next, head: self.ref_node, marker: PhantomData }
    }

    pub fn empty(&self) -> bool {
        self.node.next == self.ref_node
    }

    pub fn add_head(&mut self, mut elt: NonNull<T>) {
        ZX_ASSERT!(self.is_initialized());
        unsafe { self.add_head_node(elt.as_mut().into_node()); }
    }

    pub fn head(&self) -> Option<NonNull<T>> {
        ZX_ASSERT!(self.is_initialized());
        if let Some(next) = self.node.next {
            T::from_node(next)
        } else {
            None
        }
    }

    pub fn tail(&self) -> Option<NonNull<T>> {
        ZX_ASSERT!(self.is_initialized());
        if let Some(prev) = self.node.prev {
            T::from_node(prev)
        } else {
            None
        }
    }

    pub fn pop_head(&mut self) -> Option<NonNull<T>> {
        ZX_ASSERT!(self.is_initialized());
        if self.node.next == self.ref_node {
            return None;
        }

        let head = self.head();
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
    }

    pub fn add_tail(&mut self, mut elt: NonNull<T>) {
        ZX_ASSERT!(self.is_initialized());
        unsafe { self.add_tail_node(elt.as_mut().into_node()); }
    }

    pub fn splice(&mut self, other: &mut Self) {
        ZX_ASSERT!(self.is_initialized());
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
    }

    pub fn len(&self) -> usize {
        let mut ret = 0;
        let mut next = self.node.next;
        while next != self.ref_node {
            ret += 1;
            if let Some(n) = next {
                unsafe {
                    next = (*n.as_ptr()).next;
                }
            } else {
                break;
            }
        }

        ret
    }
}

unsafe impl Send for ListNode {}
unsafe impl Sync for ListNode {}

unsafe impl<T: Send + Linked<T>> Send for List<T> {}
unsafe impl<T: Sync + Linked<T>> Sync for List<T> {}
