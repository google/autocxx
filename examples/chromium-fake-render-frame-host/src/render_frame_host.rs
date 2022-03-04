// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use autocxx::subclass::prelude::*;
use autocxx::{c_int, PinMut};
use std::cell::{Ref, RefCell, RefMut};
use std::ops::Deref;
use std::pin::Pin;
use std::rc::Rc;

use crate::ffi;

/// A memory-safe handle to a C++ RenderFrameHost.
///
/// This is a toy, hypothetical, example.
///
/// Creation: in this sample, the only option is to use [`RenderFrameHostHandle::from_id`]
/// which corresponds to the equivalent method in C++ `RenderFrameHost`. Unlike
/// the C++ version, you must pass a WebContents so that Rust wrappers can listen for
/// destruction events.
///
/// The returned handle is memory safe and can be used to access the methods
/// of [`ffi::content::RenderFrameHost`]. To use such a method, you have three options:
/// * If you believe there is no chance that the `RenderFrameHost` has been
///   destroyed, and if the method is const, you can just go ahead and call methods
///   on this object. As it implements [`std::ops::Deref`], that will just work -
///   but your code will panic if the `RenderFrameHost` was already destroyed.
/// * If the method is non-const, you'll have to call `.pin_mut().method()` instead
//    but otherwise this is functionally identical.
/// * If you believe that there is a chance that the `RenderFrameHost` was already
///   destroyed, use [`RenderFrameHostHandle::try_borrow`] or
///   [`RenderFrameHostHandle::try_borrow_mut`]. This will return
///   a guard object which guarantees the existence of the `RenderFrameHost`
///   during its lifetime.
///
/// # Performance characteristics
///
/// The existence of this object registers an observer with the `WebContents`
/// and deregisters it on destruction. That is, of course, an overhead, but
/// that's necessary to keep track of liveness. (A more efficient
/// implementation would use a single observer for multiple such handles - but
/// this is a toy implementation).
///
/// In addition, each time you extract the value from this
/// `RenderFrameHostHandle`, a liveness check is performed. This involves
/// not just a null check but also some reference count manipulation.
/// If you're going to access the `RenderFrameHost` multiple times, it's
/// advised that you call [`RenderFrameHostHandle::try_borrow`] or
/// [`RenderFrameHostHandle::try_borrow_mut`] and then use
/// the result multiple times. The liveness check for the `RenderFrameHost`
/// will be performed only once at runtime.
///
/// # Destruction of RenderFrameHosts while borrowed
///
/// If you have called [`RenderFrameHostHandle::try_borrow`] (or its mutable
/// equivalent) and still have an outstanding borrow, any code path - via C++
/// - which results it the destruction of the `RenderFrameHost` will result in
/// a runtime panic.
pub struct RenderFrameHostHandle<'wc> {
    obs: Rc<RefCell<RenderFrameHostForWebContents>>,
    web_contents: Pin<&'wc mut ffi::content::WebContents>,
}

impl<'wc> RenderFrameHostHandle<'wc> {
    /// Create a memory-safe handle to a RenderFrameHost using its
    /// process ID and frame ID.
    pub fn from_id(
        render_process_id: c_int,
        render_frame_id: c_int,
        mut web_contents: Pin<&'wc mut ffi::content::WebContents>,
    ) -> Self {
        // Instantiate our WebContentsObserver subclass.
        let obs = RenderFrameHostForWebContents::new_rust_owned(RenderFrameHostForWebContents {
            rfh: ffi::content::RenderFrameHost::FromId(render_process_id, render_frame_id),
            cpp_peer: Default::default(),
        });

        // And now register it.
        // This nasty line will go away when autocxx is a bit more sophisticated.
        let superclass_ptr = cast_to_superclass(obs.as_ref().borrow_mut().peer_mut());

        // But this will remain unsafe. cxx policy is that any raw pointer
        // passed into a C++ function requires an unsafe {} block and that
        // is sensible. We may of course provide an ergonomic Rust wrapper
        // around WebContents which provides safe Rust equivalents
        // (using references or similar rather than pointers) in which case
        // this unsafe block would go away.
        unsafe { web_contents.as_mut().AddObserver(superclass_ptr) };

        Self { obs, web_contents }
    }

    /// Tries to return a mutable reference to the RenderFrameHost.
    /// Because this requires `self` to be `&mut`, and that lifetime is
    /// applied to the returned `RenderFrameHost`, the compiler will prevent
    /// multiple such references existing in Rust at the same time.
    /// This will return `None` if the RenderFrameHost were already destroyed.
    pub fn try_borrow_mut<'a>(
        &'a mut self,
    ) -> Option<impl PinMut<ffi::content::RenderFrameHost> + 'a> {
        let ref_mut = self.obs.as_ref().borrow_mut();
        if ref_mut.rfh.is_null() {
            None
        } else {
            Some(RenderFrameHostRefMut(ref_mut))
        }
    }

    /// Tries to return a reference to the RenderFrameHost.
    /// The compiler will prevent calls to this if anyone has an outstanding
    /// mutable reference from [`RenderFrameHostHandle::try_borrow_mut`].
    /// This will return `None` if the RenderFrameHost were already destroyed.
    #[allow(dead_code)]
    pub fn try_borrow<'a>(&'a self) -> Option<impl AsRef<ffi::content::RenderFrameHost> + 'a> {
        let ref_non_mut = self.obs.as_ref().borrow();
        if ref_non_mut.rfh.is_null() {
            None
        } else {
            Some(RenderFrameHostRef(ref_non_mut))
        }
    }
}

impl<'wc> Drop for RenderFrameHostHandle<'wc> {
    fn drop(&mut self) {
        // Unregister our observer.
        let superclass_ptr = cast_to_superclass(self.obs.as_ref().borrow_mut().peer_mut());
        unsafe { self.web_contents.as_mut().RemoveObserver(superclass_ptr) };
    }
}

impl<'wc> AsRef<ffi::content::RenderFrameHost> for RenderFrameHostHandle<'wc> {
    fn as_ref(&self) -> &ffi::content::RenderFrameHost {
        let ref_non_mut = self.obs.as_ref().borrow();
        // Safety: the .rfh field is guaranteed to be a RenderFrameHost
        // and we are observing its lifetime so it will be reset to null
        // if destroyed.
        unsafe { ref_non_mut.rfh.as_ref() }.expect("This RenderFrameHost was already destroyed")
    }
}

impl<'wc> PinMut<ffi::content::RenderFrameHost> for RenderFrameHostHandle<'wc> {
    fn pin_mut(&mut self) -> Pin<&mut ffi::content::RenderFrameHost> {
        let ref_mut = self.obs.as_ref().borrow_mut();
        // Safety: the .rfh field is guaranteed to be a RenderFrameHost
        // and we are observing its lifetime so it will be reset to null
        // if destroyed.
        unsafe { ref_mut.rfh.as_mut().map(|p| Pin::new_unchecked(p)) }
            .expect("This RenderFrameHost was already destroyed")
    }
}

impl<'wc> Deref for RenderFrameHostHandle<'wc> {
    type Target = ffi::content::RenderFrameHost;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

#[doc(hidden)]
struct RenderFrameHostRefMut<'a>(RefMut<'a, RenderFrameHostForWebContents>);

#[doc(hidden)]
struct RenderFrameHostRef<'a>(Ref<'a, RenderFrameHostForWebContents>);

impl<'a> AsRef<ffi::content::RenderFrameHost> for RenderFrameHostRef<'a> {
    fn as_ref(&self) -> &ffi::content::RenderFrameHost {
        // Safety:
        // Creation precondition is that self.0.rfh is not null
        // and it can't be destroyed whilst this borrow exists.
        unsafe { self.0.rfh.as_ref().unwrap() }
    }
}

impl<'a> PinMut<ffi::content::RenderFrameHost> for RenderFrameHostRefMut<'a> {
    fn pin_mut(&mut self) -> Pin<&mut ffi::content::RenderFrameHost> {
        // Safety:
        // Creation precondition is that self.0.rfh is not null
        // and it can't be destroyed whilst this borrow exists.
        unsafe { Pin::new_unchecked(self.0.rfh.as_mut().unwrap()) }
    }
}

impl<'a> AsRef<ffi::content::RenderFrameHost> for RenderFrameHostRefMut<'a> {
    fn as_ref(&self) -> &ffi::content::RenderFrameHost {
        // Safety:
        // Creation precondition is that self.0.rfh is not null
        // and it can't be destroyed whilst this borrow exists.
        unsafe { self.0.rfh.as_ref().unwrap() }
    }
}

impl<'a> Deref for RenderFrameHostRef<'a> {
    type Target = ffi::content::RenderFrameHost;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'a> Deref for RenderFrameHostRefMut<'a> {
    type Target = ffi::content::RenderFrameHost;
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

#[is_subclass(superclass("content::WebContentsObserver"))]
#[doc(hidden)]
pub struct RenderFrameHostForWebContents {
    rfh: *mut ffi::content::RenderFrameHost,
}

impl ffi::content::WebContentsObserver_methods for RenderFrameHostForWebContents {
    unsafe fn RenderFrameDeleted(&mut self, destroyed_rfh: *mut ffi::content::RenderFrameHost) {
        if self.rfh == destroyed_rfh {
            self.rfh = std::ptr::null_mut()
        }
    }
}

fn cast_to_superclass(
    obs: Pin<&mut ffi::RenderFrameHostForWebContentsCpp>,
) -> *mut ffi::content::WebContentsObserver {
    // This horrid code will all go away once we implement
    // https://github.com/google/autocxx/issues/592; safe wrappers will
    // be automatically generated to allow upcasting to superclasses.
    // NB this code is probably actually _wrong_ too meanwhile; we need to cast
    // on the C++ side.
    let subclass_obs_ptr =
        unsafe { Pin::into_inner_unchecked(obs) } as *mut ffi::RenderFrameHostForWebContentsCpp;
    unsafe {
        std::mem::transmute::<
            *mut ffi::RenderFrameHostForWebContentsCpp,
            *mut ffi::content::WebContentsObserver,
        >(subclass_obs_ptr)
    }
}
