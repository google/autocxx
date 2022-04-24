// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::marker::PhantomData;

/// A C++ const reference. These are different from Rust's `&T` in that
/// these may exist even while the object is nutated elsewhere.
#[repr(transparent)]
pub struct CppRef<'a, T>(*const T, ::std::marker::PhantomData<&'a T>);

// Implement manually so that there's no need for the inner type to implement Clone
impl<'a, T> Clone for CppRef<'a, T> {
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}

impl<'a, T> Copy for CppRef<'a, T> {}

impl<'a, T> CppRef<'a, T> {
    #[doc(hidden)]
    pub fn new(ptr: *const T) -> Self {
        Self(ptr, PhantomData)
    }

    /// Get a regular Rust reference out of this C++ reference.
    ///
    /// # Safety
    ///
    /// Callers must guarantee that the referent is not modified by any other
    /// C++ or Rust code while the returned reference exists. Callers must
    /// also guarantee that no mutable Rust reference is created to the
    /// referent while the returned reference exists.
    pub unsafe fn as_ref(&self) -> &T {
        &*self.0
    }

    pub fn as_ptr(&self) -> *const T {
        self.0
    }
}

/// A C++ non-const reference. These are different from Rust's `&mut T` in that
/// several C++ references can exist to the same underlying data ("aliasing")
/// and that's not permitted in Rust.
#[repr(transparent)]
pub struct CppMutRef<'a, T>(*mut T, PhantomData<&'a T>);

// Implement manually so that there's no need for the inner type to implement Clone
impl<'a, T> Clone for CppMutRef<'a, T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<'a, T> Copy for CppMutRef<'a, T> {}

impl<'a, T> CppMutRef<'a, T> {
    #[doc(hidden)]
    pub fn new(ptr: *mut T) -> Self {
        Self(ptr, PhantomData)
    }

    /// Get a regular Rust reference out of this C++ reference.
    ///
    /// # Safety
    ///
    /// Callers must guarantee that the referent is not modified by any other
    /// C++ or Rust code while the returned reference exists. Callers must
    /// also guarantee that no mutable Rust reference is created to the
    /// referent while the returned reference exists.
    pub unsafe fn as_ref(&self) -> &T {
        &*self.0
    }

    pub fn as_ptr(&self) -> *const T {
        self.0
    }

    /// Get a regular Rust mutable reference out of this C++ reference.
    ///
    /// # Safety
    ///
    /// Callers must guarantee that the referent is not modified by any other
    /// C++ or Rust code while the returned reference exists. Callers must
    /// also guarantee that no other Rust reference is created to the referent
    /// while the returned reference exists.
    pub unsafe fn as_mut(&mut self) -> &mut T {
        &mut *self.0
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.0
    }
}

/// A newtype wrapper which causes the contained object to obey C++ reference
/// semantics rather than Rust reference semantics.
///
/// C++ references are permitted to alias one another, and commonly do.
/// Rust references must alias according only to the narrow rules of the
/// borrow checker.
///
/// If you need C++ to access your Rust object, first imprison it in one of these
/// objects, then use [`as_cpp_ref`] to obtain C++ references to it.
#[repr(transparent)]
pub struct CppPin<T>(T);

impl<T> CppPin<T> {
    /// Wrap a pre-existing Rust type such that we can vend references to
    /// C++.
    pub fn new(item: T) -> Self {
        Self(item)
    }

    /// Get an immutable pointer to the underlying object.
    pub fn as_ptr(&self) -> *const T {
        std::ptr::addr_of!(self.0)
    }

    /// Get a mutable pointer to the underlying object.
    pub fn as_mut_ptr(&mut self) -> *mut T {
        std::ptr::addr_of_mut!(self.0)
    }

    /// Get a normal Rust reference to the underlying object. This is unsafe.
    ///
    /// # Safety
    ///
    /// You must guarantee that C++ will not mutate the object while the
    /// reference exists.
    pub unsafe fn as_ref(&self) -> &T {
        &self.0
    }

    /// Get a normal Rust mutable reference to the underlying object. This is unsafe.
    ///
    /// # Safety
    ///
    /// You must guarantee that C++ will not mutate the object while the
    /// reference exists.
    pub unsafe fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }

    /// Return a C++ reference to the underlying object. A "C++ reference" in this
    /// context means a reference which obeys C++ semantics as opposed to Rust
    /// semantics; that is, multiple aliasing references are allowed.
    pub fn as_cpp_ref(&self) -> CppRef<T> {
        CppRef::new(self.as_ptr())
    }

    /// Return a C++ mutable reference to the underlying object. A "C++ reference" in this
    /// context means a reference which obeys C++ semantics as opposed to Rust
    /// semantics; that is, multiple aliasing references are allowed.
    pub fn as_cpp_mut_ref(&mut self) -> CppMutRef<T> {
        CppMutRef::new(self.as_mut_ptr())
    }
}
