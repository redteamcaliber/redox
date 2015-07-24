// Copyright 2012-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! A pointer type for heap allocation.
//!
//! `Box<T>`, casually referred to as a 'box', provides the simplest form of heap allocation in
//! Rust. Boxes provide ownership for this allocation, and drop their contents when they go out of
//! scope.
//!
//! # Examples
//!
//! Creating a box:
//!
//! ```
//! let x = Box::new(5);
//! ```
//!
//! Creating a recursive data structure:
//!
//! ```
//! #[derive(Debug)]
//! enum List<T> {
//!     Cons(T, Box<List<T>>),
//!     Nil,
//! }
//!
//! fn main() {
//!     let list: List<i32> = List::Cons(1, Box::new(List::Cons(2, Box::new(List::Nil))));
//!     println!("{:?}", list);
//! }
//! ```
//!
//! This will print `Cons(1, Cons(2, Nil))`.
//!
//! Recursive structures must be boxed, because if the definition of `Cons` looked like this:
//!
//! ```rust,ignore
//! Cons(T, List<T>),
//! ```
//!
//! It wouldn't work. This is because the size of a `List` depends on how many elements are in the
//! list, and so we don't know how much memory to allocate for a `Cons`. By introducing a `Box`,
//! which has a defined size, we know how big `Cons` needs to be.

use core::prelude::*;

use core::any::Any;
use core::cmp::Ordering;
use core::fmt;
use core::hash::{self, Hash};
use core::mem;
use core::ops::{Deref, DerefMut};
use core::ptr::{Unique};
use core::raw::{TraitObject};

#[cfg(not(stage0))]
use core::marker::Unsize;
#[cfg(not(stage0))]
use core::ops::CoerceUnsized;

use common::memory;

#[lang="exchange_malloc"]
#[allow(unused_variables)]
pub unsafe fn exchange_malloc(size: usize, align: usize) -> *mut u8{
    memory::alloc(size) as *mut u8
}

#[lang="exchange_free"]
#[allow(unused_variables)]
pub unsafe fn exchange_free(ptr: *mut u8, size: usize, align: usize){
    memory::unalloc(ptr as usize);
}

/// A pointer type for heap allocation.
///
/// See the [module-level documentation](../../std/boxed/index.html) for more.
#[lang = "owned_box"]
#[fundamental]
pub struct Box<T>(Unique<T>);

impl<T> Box<T> {
    /// Allocates memory on the heap and then moves `x` into it.
    ///
    /// # Examples
    ///
    /// ```
    /// let x = Box::new(5);
    /// ```
    #[inline(always)]
    pub fn new(x: T) -> Box<T> {
        box x
    }
}

impl<T : ?Sized> Box<T> {
    /// Constructs a box from the raw pointer.
    ///
    /// After this function call, pointer is owned by resulting box.
    /// In particular, it means that `Box` destructor calls destructor
    /// of `T` and releases memory. Since the way `Box` allocates and
    /// releases memory is unspecified, the only valid pointer to pass
    /// to this function is the one taken from another `Box` with
    /// `boxed::into_raw` function.
    ///
    /// Function is unsafe, because improper use of this function may
    /// lead to memory problems like double-free, for example if the
    /// function is called twice on the same raw pointer.
    #[inline]
    pub unsafe fn from_raw(raw: *mut T) -> Self {
        mem::transmute(raw)
    }
}

/// Consumes the `Box`, returning the wrapped raw pointer.
///
/// After call to this function, caller is responsible for the memory
/// previously managed by `Box`, in particular caller should properly
/// destroy `T` and release memory. The proper way to do it is to
/// convert pointer back to `Box` with `Box::from_raw` function, because
/// `Box` does not specify, how memory is allocated.
///
/// Function is unsafe, because result of this function is no longer
/// automatically managed that may lead to memory or other resource
/// leak.
///
/// # Examples
/// ```
/// # #![feature(alloc)]
/// use std::boxed;
///
/// let seventeen = Box::new(17u32);
/// let raw = unsafe { boxed::into_raw(seventeen) };
/// let boxed_again = unsafe { Box::from_raw(raw) };
/// ```
#[inline]
pub unsafe fn into_raw<T : ?Sized>(b: Box<T>) -> *mut T {
    mem::transmute(b)
}

impl<T: Default> Default for Box<T> {
    fn default() -> Box<T> { box Default::default() }
}

impl<T> Default for Box<[T]> {
    fn default() -> Box<[T]> { Box::<[T; 0]>::new([]) }
}

impl<T: Clone> Clone for Box<T> {
    /// Returns a new box with a `clone()` of this box's contents.
    ///
    /// # Examples
    ///
    /// ```
    /// let x = Box::new(5);
    /// let y = x.clone();
    /// ```
    #[inline]
    fn clone(&self) -> Box<T> { box {(**self).clone()} }

    /// Copies `source`'s contents into `self` without creating a new allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// # #![feature(alloc, core)]
    /// let x = Box::new(5);
    /// let mut y = Box::new(10);
    ///
    /// y.clone_from(&x);
    ///
    /// assert_eq!(*y, 5);
    /// ```
    #[inline]
    fn clone_from(&mut self, source: &Box<T>) {
        (**self).clone_from(&(**source));
    }
}

impl<T: ?Sized + PartialEq> PartialEq for Box<T> {
    #[inline]
    fn eq(&self, other: &Box<T>) -> bool { PartialEq::eq(&**self, &**other) }
    #[inline]
    fn ne(&self, other: &Box<T>) -> bool { PartialEq::ne(&**self, &**other) }
}
impl<T: ?Sized + PartialOrd> PartialOrd for Box<T> {
    #[inline]
    fn partial_cmp(&self, other: &Box<T>) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
    #[inline]
    fn lt(&self, other: &Box<T>) -> bool { PartialOrd::lt(&**self, &**other) }
    #[inline]
    fn le(&self, other: &Box<T>) -> bool { PartialOrd::le(&**self, &**other) }
    #[inline]
    fn ge(&self, other: &Box<T>) -> bool { PartialOrd::ge(&**self, &**other) }
    #[inline]
    fn gt(&self, other: &Box<T>) -> bool { PartialOrd::gt(&**self, &**other) }
}
impl<T: ?Sized + Ord> Ord for Box<T> {
    #[inline]
    fn cmp(&self, other: &Box<T>) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}
impl<T: ?Sized + Eq> Eq for Box<T> {}

impl<T: ?Sized + Hash> Hash for Box<T> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl Box<Any> {
    #[inline]
    /// Attempt to downcast the box to a concrete type.
    pub fn downcast<T: Any>(self) -> Result<Box<T>, Box<Any>> {
        if self.is::<T>() {
            unsafe {
                // Get the raw representation of the trait object
                let raw = into_raw(self);
                let to: TraitObject =
                    mem::transmute::<*mut Any, TraitObject>(raw);

                // Extract the data pointer
                Ok(Box::from_raw(to.data as *mut T))
            }
        } else {
            Err(self)
        }
    }
}

impl Box<Any + Send> {
    #[inline]
    /// Attempt to downcast the box to a concrete type.
    pub fn downcast<T: Any>(self) -> Result<Box<T>, Box<Any + Send>> {
        <Box<Any>>::downcast(self).map_err(|s| unsafe {
            // reapply the Send marker
            mem::transmute::<Box<Any>, Box<Any + Send>>(s)
        })
    }
}

impl<T: fmt::Display + ?Sized> fmt::Display for Box<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Debug + ?Sized> fmt::Debug for Box<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T> fmt::Pointer for Box<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // It's not possible to extract the inner Uniq directly from the Box,
        // instead we cast it to a *const which aliases the Unique
        let ptr: *const T = &**self;
        fmt::Pointer::fmt(&ptr, f)
    }
}

impl<T: ?Sized> Deref for Box<T> {
    type Target = T;

    fn deref(&self) -> &T { &**self }
}

impl<T: ?Sized> DerefMut for Box<T> {
    fn deref_mut(&mut self) -> &mut T { &mut **self }
}

impl<I: Iterator + ?Sized> Iterator for Box<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> { (**self).next() }
    fn size_hint(&self) -> (usize, Option<usize>) { (**self).size_hint() }
}
impl<I: DoubleEndedIterator + ?Sized> DoubleEndedIterator for Box<I> {
    fn next_back(&mut self) -> Option<I::Item> { (**self).next_back() }
}
impl<I: ExactSizeIterator + ?Sized> ExactSizeIterator for Box<I> {}

#[cfg(not(stage0))]
impl<T: ?Sized+Unsize<U>, U: ?Sized> CoerceUnsized<Box<U>> for Box<T> {}