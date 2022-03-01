// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use cxx::{memory::UniquePtrTarget, UniquePtr};
use moveit::{CopyNew, New};
use std::{marker::PhantomPinned, mem::MaybeUninit, pin::Pin};

/// A trait representing a parameter to a C++ function which is received
/// by value.
///
/// Rust has the concept of receiving parameters by _move_ or by _reference_.
/// C++ has the concept of receiving a parameter by 'value', which means
/// the parameter gets copied.
///
/// To make it easy to pass such parameters from Rust, this trait exists.
/// It is implemented both for references `&T` and for `UniquePtr<T>`,
/// subject to the presence or absence of suitable copy and move constructors.
/// This allows you to pass in parameters by copy (as is ergonomic and normal
/// in C++) retaining the original parameter; or by move semantics thus
/// destroying the object you're passing in. Simply use a reference if you want
/// copy semantics, or the item itself if you want move semantics.
///
/// It is not recommended that you implement this trait, nor that you directly
/// use its methods, which are for use by `autocxx` generated code only.
///
/// # Use of `moveit` traits
///
/// Most of the implementations of this trait require the type to implement
/// [`CopyNew`], which is simply the `autocxx`/`moveit` way of saying that
/// the type has a copy constructor in C++.
///
/// # Performance
///
/// At present, some additional copying occurs for all implementations of
/// this trait other than that for [`cxx::UniquePtr`]. In the future it's
/// hoped that the implementation for `&T where T: CopyNew` can also avoid
/// this extra copying.
///
/// # Panics
///
/// The implementations of this trait which take a [`cxx::UniquePtr`] will
/// panic if the pointer is NULL.
///
/// # Safety
///
/// Implementations of this trait must guarantee that `StackStorage` is the
/// same alignment and size as T, so long as `needs_stack_space` returns true.
/// Otherwise `StackStorage` is unused.
pub unsafe trait ValueParam<T> {
    /// Any stack storage required. If, as part of passing to C++,
    /// we need to store a temporary copy of the value, this will be `T`,
    /// otherwise `()`.
    #[doc(hidden)]
    type StackStorage;
    /// Whether this `ValueParam` requires temporary Rust-side storage of
    /// an extra copy of this value.
    #[doc(hidden)]
    fn needs_stack_space(&self) -> bool;
    /// Populate the stack storage given as a parameter. Only called if you
    /// return `true` from `needs_stack_space`.
    ///
    /// # Safety
    ///
    /// This is unsafe because callers must call this exactly once, and once
    /// only, to avoid reinitializing the 'this' parameter.
    #[doc(hidden)]
    unsafe fn populate_stack_space(&self, this: Pin<&mut MaybeUninit<Self::StackStorage>>);
    /// Return a pointer to the storage.
    /// Only called if `needs_stack_space` returns `false`, otherwise the pointer
    /// to the stack space will be used.
    /// Note that this returns a _mutable_ pointer. This is a big deal. That's
    /// because, on the C++ side, we'll call `std::move(*ptr)` on this pointer.
    /// This is unlikely to be semantically acceptable long-term, but currently
    /// makes it 'quicker' when a [`cxx::UniquePtr`] is used as a value parameter.
    #[doc(hidden)]
    fn get_ptr(&mut self) -> *mut T;
}

unsafe impl<T> ValueParam<T> for &T
where
    T: CopyNew,
{
    type StackStorage = T;

    fn needs_stack_space(&self) -> bool {
        true
    }

    unsafe fn populate_stack_space(&self, this: Pin<&mut MaybeUninit<Self::StackStorage>>) {
        crate::moveit::new::copy(*self).new(this)
    }

    fn get_ptr(&mut self) -> *mut T {
        unreachable!()
    }
}

unsafe impl<T> ValueParam<T> for UniquePtr<T>
where
    T: UniquePtrTarget,
{
    type StackStorage = ();

    fn needs_stack_space(&self) -> bool {
        false
    }

    unsafe fn populate_stack_space(&self, _: Pin<&mut MaybeUninit<Self::StackStorage>>) {}

    fn get_ptr(&mut self) -> *mut T {
        (unsafe {
            Pin::into_inner_unchecked(
                self.as_mut()
                    .expect("Passed a NULL UniquePtr as a C++ value parameter"),
            )
        }) as *mut T
    }
}

unsafe impl<T> ValueParam<T> for &UniquePtr<T>
where
    T: UniquePtrTarget + CopyNew,
{
    type StackStorage = T;

    fn needs_stack_space(&self) -> bool {
        true
    }

    unsafe fn populate_stack_space(&self, this: Pin<&mut MaybeUninit<Self::StackStorage>>) {
        // Invoke a copy constructor on the Rust side, because we'll use std::move on the C++
        // side.
        crate::moveit::new::copy(
            self.as_ref()
                .expect("Passed a NULL &UniquePtr as a C++ value parameter"),
        )
        .new(this)
    }

    fn get_ptr(&mut self) -> *mut T {
        unreachable!()
    }
}

/// Implementation detail for how we pass value parameters into C++.
/// This type is instantiated by auto-generated autocxx code each time we
/// need to pass a value parameter into C++, and will take responsibility
/// for extracting that value parameter from the [`ValueParam`] and doing
/// any later cleanup.
#[doc(hidden)]
pub struct ValueParamHandler<T, VP: ValueParam<T>> {
    param: VP,
    space: Option<MaybeUninit<VP::StackStorage>>,
    _pinned: PhantomPinned,
}

impl<T, VP: ValueParam<T>> ValueParamHandler<T, VP> {
    /// Create a new storage space for something that's about to be passed
    /// by value to C++. Depending on the `ValueParam` type passed in,
    /// this may be largely a no-op or it may involve storing a whole
    /// extra copy of the type.
    pub fn new(param: VP) -> Self {
        let mut this = Self {
            param,
            space: None,
            _pinned: PhantomPinned,
        };
        if this.param.needs_stack_space() {
            this.space = Some(MaybeUninit::uninit());
        }
        this
    }

    /// Populate this stack space if needs be.
    ///
    /// # Safety
    ///
    /// Callers must guarantee that this type will not move
    /// in memory between calls to [`populate`] and [`get_ptr`].
    /// Callers must call this exactly once prior to calling [`get_ptr`].
    pub unsafe fn populate(&mut self) {
        if self.param.needs_stack_space() {
            self.param
                .populate_stack_space(Pin::new_unchecked(self.space.as_mut().unwrap()));
        }
    }

    /// Return a pointer to the underlying value which can be passed to C++.
    /// Per the unsafety contract of [`populate`], the object must not have moved
    /// since it was created, and [`populate`] has been called exactly once
    /// prior to this call.
    pub fn get_ptr(&mut self) -> *mut T {
        if let Some(ref mut space) = self.space {
            // Safety: 'space' is guaranteed to be populated due to the unsafety
            // contract of 'populate'.
            let ptr =
                unsafe { space.assume_init_mut() } as *mut <VP as ValueParam<T>>::StackStorage;
            // Safety: per the unsafety contract of ValueParam, <VP as ValueParam<T>>::StackStorage
            // is guanteed to == T in the case that needs_stack_space returns true.
            unsafe { std::mem::transmute(ptr) }
        } else {
            self.param.get_ptr()
        }
    }
}

impl<T, VP: ValueParam<T>> Drop for ValueParamHandler<T, VP> {
    fn drop(&mut self) {
        if let Some(space) = self.space.take() {
            unsafe { space.assume_init() };
        }
    }
}
