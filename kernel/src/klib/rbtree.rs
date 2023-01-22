/*
 * Copyright (c) 2022 Shi Lei
 *
 * Use of this source code is governed by a MIT-style license
 * that can be found in the LICENSE file or
 * at https://opensource.org/licenses/MIT
 */

#![allow(dead_code)]

use alloc::boxed::Box;
use core::cmp::Ord;
use core::fmt::Debug;
use core::cmp::Ordering;
use core::{ptr, marker};

extern crate alloc;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

/* A red black tree implemented with Rust
 * It is required that the keys implement the [`Ord`] traits. */
pub struct RBTree<K: Ord, V> {
    root: NodePtr<K, V>,
    len: usize,
}

impl<K: Ord, V> RBTree<K, V> {
    pub const fn new() -> Self {
        Self {
            root: NodePtr::null(),
            len: 0,
        }
    }

    // upper_bound(key) : Finds the element (E) in the tree such that E.key > key
    pub fn upper_bound(&self, k: &K) -> IterMut<K, V> {
        self.bound(k, true)
    }

    // lower_bound(key) : Finds the element (E) in the tree such that E.key >= key
    pub fn lower_bound(&self, k: &K) -> IterMut<K, V> {
        self.bound(k, false)
    }

    fn bound(&self, k: &K, strict: bool) -> IterMut<K, V> {
        if self.root.is_null() {
            return IterMut::null();
        }

        let go_right = |order| {
            if strict {
                matches!(order, Ordering::Greater | Ordering::Equal)
            } else {
                matches!(order, Ordering::Greater)
            }
        };

        let mut found = NodePtr::null();
        let mut temp = &self.root;
        unsafe {
            loop {
                if go_right(k.cmp(&(*temp.0).key)) {
                    temp = &mut (*temp.0).right;
                } else {
                    /* If this node's key must be greater than or
                     * equal to the user's key.
                     * This node is now our candidate for our found node. */
                    found = *temp;

                    temp = &mut (*temp.0).left;
                };
                if temp.is_null() {
                    break;
                }
            }
        }
        if found.is_null() {
            IterMut::null()
        } else {
            IterMut::new(found)
        }
    }

    #[inline]
    fn find_node(&self, k: &K) -> NodePtr<K, V> {
        if self.root.is_null() {
            return NodePtr::null();
        }
        let mut temp = &self.root;
        unsafe {
            loop {
                let next = match k.cmp(&(*temp.0).key) {
                    Ordering::Less => &mut (*temp.0).left,
                    Ordering::Greater => &mut (*temp.0).right,
                    Ordering::Equal => return *temp,
                };
                if next.is_null() {
                    break;
                }
                temp = next;
            }
        }
        NodePtr::null()
    }

    #[inline]
    pub fn get(&self, k: &K) -> Option<&V> {
        let node = self.find_node(k);
        if node.is_null() {
            return None;
        }

        unsafe { Some(&(*node.0).value) }
    }

    #[inline]
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let node = self.find_node(k);
        if node.is_null() {
            return None;
        }

        unsafe { Some(&mut (*node.0).value) }
    }

    #[inline]
    pub fn contains_key(&self, k: &K) -> bool {
        let node = self.find_node(k);
        if node.is_null() {
            return false;
        }
        true
    }

    #[inline]
    pub fn insert(&mut self, k: K, v: V) {
        self.len += 1;
        let mut node = NodePtr::new(k, v);
        let mut y = NodePtr::null();
        let mut x = self.root;

        while !x.is_null() {
            y = x;
            match node.cmp(&&mut x) {
                Ordering::Less => {
                    x = x.left();
                }
                _ => {
                    x = x.right();
                }
            };
        }
        node.set_parent(y);

        if y.is_null() {
            self.root = node;
        } else {
            match node.cmp(&&mut y) {
                Ordering::Less => {
                    y.set_left(node);
                }
                _ => {
                    y.set_right(node);
                }
            };
        }

        node.set_red_color();
        unsafe {
            self.insert_fixup(node);
        }
    }

    #[inline]
    unsafe fn insert_fixup(&mut self, mut node: NodePtr<K, V>) {
        let mut parent;
        let mut gparent;

        while node.parent().is_red_color() {
            parent = node.parent();
            gparent = parent.parent();
            if parent == gparent.left() {
                let mut uncle = gparent.right();
                if !uncle.is_null() && uncle.is_red_color() {
                    uncle.set_black_color();
                    parent.set_black_color();
                    gparent.set_red_color();
                    node = gparent;
                    continue;
                }

                if parent.right() == node {
                    self.left_rotate(parent);
                    let temp = parent;
                    parent = node;
                    node = temp;
                }

                parent.set_black_color();
                gparent.set_red_color();
                self.right_rotate(gparent);
            } else {
                let mut uncle = gparent.left();
                if !uncle.is_null() && uncle.is_red_color() {
                    uncle.set_black_color();
                    parent.set_black_color();
                    gparent.set_red_color();
                    node = gparent;
                    continue;
                }

                if parent.left() == node {
                    self.right_rotate(parent);
                    let temp = parent;
                    parent = node;
                    node = temp;
                }

                parent.set_black_color();
                gparent.set_red_color();
                self.left_rotate(gparent);
            }
        }
        self.root.set_black_color();
    }

    #[inline]
    unsafe fn left_rotate(&mut self, mut node: NodePtr<K, V>) {
        let mut temp = node.right();
        node.set_right(temp.left());

        if !temp.left().is_null() {
            temp.left().set_parent(node.clone());
        }

        temp.set_parent(node.parent());
        if node == self.root {
            self.root = temp.clone();
        } else if node == node.parent().left() {
            node.parent().set_left(temp.clone());
        } else {
            node.parent().set_right(temp.clone());
        }

        temp.set_left(node.clone());
        node.set_parent(temp.clone());
    }

    #[inline]
    unsafe fn right_rotate(&mut self, mut node: NodePtr<K, V>) {
        let mut temp = node.left();
        node.set_left(temp.right());

        if !temp.right().is_null() {
            temp.right().set_parent(node.clone());
        }

        temp.set_parent(node.parent());
        if node == self.root {
            self.root = temp.clone();
        } else if node == node.parent().right() {
            node.parent().set_right(temp.clone());
        } else {
            node.parent().set_left(temp.clone());
        }

        temp.set_right(node.clone());
        node.set_parent(temp.clone());
    }

    #[inline]
    fn first_child(&self) -> NodePtr<K, V> {
        if self.root.is_null() {
            NodePtr::null()
        } else {
            let mut temp = self.root;
            while !temp.left().is_null() {
                temp = temp.left();
            }
            temp
        }
    }

    #[inline]
    fn last_child(&self) -> NodePtr<K, V> {
        if self.root.is_null() {
            NodePtr::null()
        } else {
            let mut temp = self.root;
            while !temp.right().is_null() {
                temp = temp.right();
            }
            temp
        }
    }

    /// Return the key and value iter
    #[inline]
    pub fn iter(&self) -> Iter<K, V> {
        Iter {
            cursor: self.first_child(),
            _marker: marker::PhantomData,
        }
    }

    /// Return the key and mut value iter
    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<K, V> {
        IterMut::new(self.first_child())
    }
}

/*****************RBTreeNode***************************/
struct RBTreeNode<K: Ord, V> {
    color: Color,
    left: NodePtr<K, V>,
    right: NodePtr<K, V>,
    parent: NodePtr<K, V>,
    key: K,
    value: V,
}

/*****************NodePtr***************************/
#[derive(Debug)]
struct NodePtr<K: Ord, V>(*mut RBTreeNode<K, V>);

unsafe impl<K: Ord, V> Send for NodePtr<K, V> {}

impl<K: Ord, V> Clone for NodePtr<K, V> {
    fn clone(&self) -> NodePtr<K, V> {
        NodePtr(self.0)
    }
}

impl<K: Ord, V> Copy for NodePtr<K, V> {}

impl<K: Ord, V> PartialEq for NodePtr<K, V> {
    fn eq(&self, other: &NodePtr<K, V>) -> bool {
        self.0 == other.0
    }
}

impl<K: Ord, V> PartialOrd for NodePtr<K, V> {
    fn partial_cmp(&self, other: &NodePtr<K, V>) -> Option<Ordering> {
        unsafe { Some((*self.0).key.cmp(&(*other.0).key)) }
    }
}

impl<K: Ord, V> Eq for NodePtr<K, V> {}

impl<K: Ord, V> Ord for NodePtr<K, V> {
    fn cmp(&self, other: &NodePtr<K, V>) -> Ordering {
        unsafe { (*self.0).key.cmp(&(*other.0).key) }
    }
}

impl<K: Ord, V> NodePtr<K, V> {
    fn new(k: K, v: V) -> NodePtr<K, V> {
        let node = RBTreeNode {
            color: Color::Black,
            left: NodePtr::null(),
            right: NodePtr::null(),
            parent: NodePtr::null(),
            key: k,
            value: v,
        };
        NodePtr(Box::into_raw(Box::new(node)))
    }

    #[inline]
    fn set_parent(&mut self, parent: NodePtr<K, V>) {
        if self.is_null() {
            return;
        }
        unsafe { (*self.0).parent = parent }
    }

    #[inline]
    fn set_left(&mut self, left: NodePtr<K, V>) {
        if self.is_null() {
            return;
        }
        unsafe { (*self.0).left = left }
    }

    #[inline]
    fn set_right(&mut self, right: NodePtr<K, V>) {
        if self.is_null() {
            return;
        }
        unsafe { (*self.0).right = right }
    }

    #[inline]
    fn parent(&self) -> NodePtr<K, V> {
        if self.is_null() {
            return NodePtr::null();
        }
        unsafe { (*self.0).parent.clone() }
    }

    #[inline]
    fn left(&self) -> NodePtr<K, V> {
        if self.is_null() {
            return NodePtr::null();
        }
        unsafe { (*self.0).left.clone() }
    }

    #[inline]
    fn right(&self) -> NodePtr<K, V> {
        if self.is_null() {
            return NodePtr::null();
        }
        unsafe { (*self.0).right.clone() }
    }

    #[inline]
    const fn null() -> NodePtr<K, V> {
        NodePtr(ptr::null_mut())
    }

    #[inline]
    fn is_null(&self) -> bool {
        self.0.is_null()
    }

    #[inline]
    fn set_color(&mut self, color: Color) {
        if self.is_null() {
            return;
        }
        unsafe {
            (*self.0).color = color;
        }
    }

    #[inline]
    fn set_red_color(&mut self) {
        self.set_color(Color::Red);
    }

    #[inline]
    fn set_black_color(&mut self) {
        self.set_color(Color::Black);
    }

    #[inline]
    fn get_color(&self) -> Color {
        if self.is_null() {
            return Color::Black;
        }
        unsafe { (*self.0).color }
    }

    #[inline]
    fn is_red_color(&self) -> bool {
        if self.is_null() {
            return false;
        }
        unsafe { (*self.0).color == Color::Red }
    }

    #[inline]
    fn is_black_color(&self) -> bool {
        if self.is_null() {
            return true;
        }
        unsafe { (*self.0).color == Color::Black }
    }

    #[inline]
    fn min_node(self) -> NodePtr<K, V> {
        let mut temp = self.clone();
        while !temp.left().is_null() {
            temp = temp.left();
        }
        return temp;
    }

    #[inline]
    fn is_left_child(&self) -> bool {
        self.parent().left() == *self
    }

    #[inline]
    fn next(self) -> NodePtr<K, V> {
        if !self.right().is_null() {
            self.right().min_node()
        } else {
            let mut temp = self;
            loop {
                if temp.parent().is_null() {
                    return NodePtr::null();
                }
                if temp.is_left_child() {
                    return temp.parent();
                }
                temp = temp.parent();
            }
        }
    }
}

/// provide iter ref for RBTree
/// # Examples
/// ```
/// use rbtree::RBTree;
/// let mut m = RBTree::new();
/// for i in 0..32 {
///     m.insert(i, i * 2);
/// }
/// assert_eq!(m.len(), 32);
/// let mut observed: u32 = 0;
/// for (k, v) in m.iter() {
///     assert_eq!(*v, *k * 2);
///     observed |= 1 << *k;
/// }
/// assert_eq!(observed, 0xFFFF_FFFF);
/// ```
pub struct Iter<K: Ord, V> {
    cursor: NodePtr<K, V>,
    _marker: marker::PhantomData<V>,
}

/// provide iter mut ref for RBTree
/// # Examples
/// ```
/// use rbtree::RBTree;
/// let mut m = RBTree::new();
/// for i in 0..32 {
///     m.insert(i, i);
/// }
/// assert_eq!(m.len(), 32);
/// for (_, v) in m.iter_mut() {
///     *v *= 2;
/// }
/// for i in 0..32 {
///     assert_eq!(m.get(&i).unwrap(), &(i * 2));
/// }
/// ```
pub struct IterMut<'a, K: Ord, V> {
    cursor: NodePtr<K, V>,
    _marker: marker::PhantomData<&'a ()>,
}

impl<'a, K: Ord, V> IterMut<'a, K, V> {
    fn new(ptr: NodePtr<K, V>) -> Self {
        IterMut {
            cursor: ptr,
            _marker: marker::PhantomData,
        }
    }

    pub fn null() -> Self {
        IterMut {
            cursor: NodePtr::null(),
            _marker: marker::PhantomData,
        }
    }

    pub fn is_null(&self) -> bool {
        self.cursor.is_null()
    }
}

impl<'a, K: Ord + 'a, V: 'a> Iterator for IterMut<'a, K, V> {
    type Item = (&'a K, &'a mut V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor.is_null() {
            return None;
        }

        let (k, v) = unsafe {
            (&(*self.cursor.0).key, &mut (*self.cursor.0).value)
        };

        self.cursor = self.cursor.next();
        Some((k, v))
    }
}