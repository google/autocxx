// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! It would be highly desirable to share a lot of this code with `value_param.rs`
//! but this proves to be surprisingly fiddly.

use cxx::{memory::UniquePtrTarget, UniquePtr};
use moveit::MoveRef;
use std::pin::Pin;

/// A trait representing a parameter to a C++ function which is received
/// by rvalue (i.e. by move).
///
/// # Panics
///
/// The implementations of this trait which take a [`cxx::UniquePtr`] will
/// panic if the pointer is NULL.
///
/// # Safety
///
/// Implementers must guarantee that the pointer returned by `get_ptr`
/// is of the correct size and alignment of `T`.
pub unsafe trait RValueParam<T>: Sized {
    /// Retrieve the pointer to the underlying item, to be passed to C++.
    /// Note that on the C++ side this is currently passed to `std::move`
    /// and therefore may be mutated.
    #[doc(hidden)]
    fn get_ptr(stack: Pin<&mut Self>) -> *mut T;
}

unsafe impl<T> RValueParam<T> for UniquePtr<T>
where
    T: UniquePtrTarget,
{
    fn get_ptr(stack: Pin<&mut Self>) -> *mut T {
        // Safety: we won't move/swap the contents of the outer pin, nor of the
        // type stored within the UniquePtr.
        unsafe {
            (Pin::into_inner_unchecked(
                (*Pin::into_inner_unchecked(stack))
                    .as_mut()
                    .expect("Passed a NULL UniquePtr as a C++ rvalue parameter"),
            )) as *mut T
        }
    }
}

unsafe impl<T> RValueParam<T> for Pin<Box<T>> {
    fn get_ptr(stack: Pin<&mut Self>) -> *mut T {
        // Safety: we won't move/swap the contents of the outer pin, nor of the
        // type stored within the UniquePtr.
        unsafe {
            (Pin::into_inner_unchecked((*Pin::into_inner_unchecked(stack)).as_mut())) as *mut T
        }
    }
}

unsafe impl<'a, T> RValueParam<T> for Pin<MoveRef<'a, T>> {
    fn get_ptr(stack: Pin<&mut Self>) -> *mut T {
        // Safety: we won't move/swap the contents of the outer pin, nor of the
        // type stored within the UniquePtr.
        unsafe {
            (Pin::into_inner_unchecked((*Pin::into_inner_unchecked(stack)).as_mut())) as *mut T
        }
    }
}
