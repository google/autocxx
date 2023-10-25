//! Module to make Rust subclasses of C++ classes. See [`CppSubclass`]
//! for details.

// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::cell::{Ref, RefMut};
use std::error::Error;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, RwLock};
use std::{
    cell::RefCell,
    pin::Pin,
    rc::{Rc, Weak},
};

use cxx::{memory::UniquePtrTarget, UniquePtr};

/// Deprecated - use [`subclass`] instead.
#[deprecated]
pub use autocxx_macro::subclass as is_subclass;

/// Declare a Rust subclass of a C++ class.
/// You can use this in two ways:
/// * As an attribute macro on a struct which is to be a subclass.
///   In this case, you must specify the superclass as described below.
///   For instance,
///   ```nocompile
///   # use autocxx_macro::subclass as subclass;
///   #[subclass(superclass("MyCppSuperclass"))]
///   struct Bar {};
///   ```
/// * as a directive within the [crate::include_cpp] macro, in which case you
///   must provide two arguments of the superclass and then the
///   subclass:
///   ```
///   # use autocxx_macro::include_cpp_impl as include_cpp;
///   include_cpp!(
///   #   parse_only!()
///       #include "input.h"
///       subclass!("MyCppSuperclass",Bar)
///       safety!(unsafe)
///   );
///   struct Bar {
///     // ...
///   }
///   ```
///   In this latter case, you'll need to implement the trait
///   [`CppSubclass`] for the struct, so it's
///   generally easier to use the former option.
///
/// See [`CppSubclass`] for information about the
/// multiple steps you need to take to be able to make Rust
/// subclasses of a C++ class.
pub use autocxx_macro::subclass;
pub use autocxx_macro::subclass_multithreaded;

/// A prelude containing all the traits and macros required to create
/// Rust subclasses of C++ classes. It's recommended that you:
///
/// ```rust
/// use autocxx::subclass::prelude::*;
/// ```
pub mod prelude {
    pub use super::{
        is_subclass, subclass, subclass_multithreaded, CppPeerConstructor, CppSubclass,
        CppSubclassDefault, CppSubclassRustPeerHolder, CppSubclassSelfOwned,
        CppSubclassSelfOwnedDefault,
    };
}

/// A trait representing the C++ side of a Rust/C++ subclass pair.
#[doc(hidden)]
pub trait CppSubclassCppPeer: UniquePtrTarget {
    fn relinquish_ownership(&self);
}

/// A type used for how the C++ side of a Rust/C++ subclass pair refers to
/// the Rust side.
#[doc(hidden)]
pub enum CppSubclassRustPeerHolder<T, CppPeerPtr>
where
    CppPeerPtr: CppSubclassPeerPtr<T>,
{
    Owned(CppPeerPtr::Ptr),
    Unowned(CppPeerPtr::WeakPtr),
}

impl<T, CppPeerPtr> CppSubclassRustPeerHolder<T, CppPeerPtr>
where
    CppPeerPtr: CppSubclassPeerPtr<T>,
{
    pub fn get(&self) -> Option<CppPeerPtr::Ptr> {
        match self {
            CppSubclassRustPeerHolder::Owned(strong) => Some(strong.clone()),
            CppSubclassRustPeerHolder::Unowned(weak) => CppPeerPtr::upgrade(weak),
        }
    }
    pub fn relinquish_ownership(self) -> Self {
        match self {
            CppSubclassRustPeerHolder::Owned(strong) => {
                CppSubclassRustPeerHolder::Unowned(CppPeerPtr::downgrade(&strong))
            }
            _ => self,
        }
    }
}

/// A type showing how the Rust side of a Rust/C++ subclass pair refers to
/// the C++ side.
#[doc(hidden)]
#[derive(Default)]
pub enum CppSubclassCppPeerHolder<CppPeer: CppSubclassCppPeer> {
    #[default]
    Empty,
    Owned(Box<UniquePtr<CppPeer>>),
    Unowned(*mut CppPeer),
}

impl<CppPeer: CppSubclassCppPeer> CppSubclassCppPeerHolder<CppPeer> {
    fn pin_mut(&mut self) -> Pin<&mut CppPeer> {
        match self {
            CppSubclassCppPeerHolder::Empty => panic!("Peer not set up"),
            CppSubclassCppPeerHolder::Owned(peer) => peer.pin_mut(),
            CppSubclassCppPeerHolder::Unowned(peer) => unsafe {
                // Safety: guaranteed safe because this is a pointer to a C++ object,
                // and C++ never moves things in memory.
                Pin::new_unchecked(peer.as_mut().unwrap())
            },
        }
    }
    fn get(&self) -> &CppPeer {
        match self {
            CppSubclassCppPeerHolder::Empty => panic!("Peer not set up"),
            CppSubclassCppPeerHolder::Owned(peer) => peer.as_ref(),
            // Safety: guaranteed safe because this is a pointer to a C++ object,
            // and C++ never moves things in memory.
            CppSubclassCppPeerHolder::Unowned(peer) => unsafe { peer.as_ref().unwrap() },
        }
    }
    fn set_owned(&mut self, peer: UniquePtr<CppPeer>) {
        *self = Self::Owned(Box::new(peer));
    }
    fn set_unowned(&mut self, peer: &mut UniquePtr<CppPeer>) {
        // Safety: guaranteed safe because this is a pointer to a C++ object,
        // and C++ never moves things in memory.
        *self = Self::Unowned(unsafe {
            std::pin::Pin::<&mut CppPeer>::into_inner_unchecked(peer.pin_mut())
        });
    }
}

fn make_owning_peer<CppPeer, CppPeerPtr, PeerConstructor, Subclass, PeerBoxer>(
    me: Subclass,
    peer_constructor: PeerConstructor,
    peer_boxer: PeerBoxer,
) -> CppPeerPtr::Ptr
where
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Subclass>,
    Subclass: CppSubclass<CppPeer, CppPeerPtr>,
    PeerConstructor: FnOnce(
        &mut Subclass,
        CppSubclassRustPeerHolder<Subclass, CppPeerPtr>,
    ) -> UniquePtr<CppPeer>,
    PeerBoxer: FnOnce(CppPeerPtr::Ptr) -> CppSubclassRustPeerHolder<Subclass, CppPeerPtr>,
{
    let mut me = CppPeerPtr::new(me);
    let holder = peer_boxer(me.clone());
    let cpp_side = peer_constructor(&mut me.get_mut(), holder);
    me.get_mut().peer_holder_mut().set_owned(cpp_side);
    me.clone()
}

/// A trait to be implemented by a subclass which knows how to construct
/// its C++ peer object. Specifically, the implementation here will
/// arrange to call one or other of the `make_unique` methods to be
/// found on the superclass of the C++ object. If the superclass
/// has a single trivial constructor, then this is implemented
/// automatically for you. If there are multiple constructors, or
/// a single constructor which takes parameters, you'll need to implement
/// this trait for your subclass in order to call the correct
/// constructor.
pub trait CppPeerConstructor<CppPeer, CppPeerPtr>: Sized
where
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    /// Create the C++ peer. This method will be automatically generated
    /// for you *except* in cases where the superclass has multiple constructors,
    /// or its only constructor takes parameters. In such a case you'll need
    /// to implement this by calling a `make_unique` method on the
    /// `<my subclass name>Cpp` type, passing `peer_holder` as the first
    /// argument.
    fn make_peer(
        &mut self,
        peer_holder: CppSubclassRustPeerHolder<Self, CppPeerPtr>,
    ) -> UniquePtr<CppPeer>;
}

/// A subclass of a C++ type.
///
/// To create a Rust subclass of a C++ class, you must do these things:
/// * Create a `struct` to act as your subclass, and add the #[`macro@crate::subclass`] attribute.
///   This adds a field to your struct for autocxx record-keeping. You can
///   instead choose to implement [`CppSubclass`] a different way, in which case
///   you must provide the [`macro@crate::subclass`] inside your [`crate::include_cpp`]
///   macro. (`autocxx` will do the required codegen for your subclass
///   whether it discovers a [`macro@crate::subclass`] directive inside your
///   [`crate::include_cpp`], or elsewhere used as an attribute macro,
///   or both.)
/// * Use the [`CppSubclass`] trait, and instantiate the subclass using
///   [`CppSubclass::new_rust_owned`] or [`CppSubclass::new_cpp_owned`]
///   constructors. (You can use [`CppSubclassSelfOwned`] if you need that
///   instead; also, see [`CppSubclassSelfOwnedDefault`] and [`CppSubclassDefault`]
///   to arrange for easier constructors to exist.
/// * You _may_ need to implement [`CppPeerConstructor`] for your subclass,
///   but only if autocxx determines that there are multiple possible superclass
///   constructors so you need to call one explicitly (or if there's a single
///   non-trivial superclass constructor.) autocxx will implement this trait
///   for you if there's no ambiguity and FFI functions are safe to call due to
///   `autocxx::safety!` being used.
///
/// # How to access your Rust structure from outside
///
/// Use [`CppSubclass::new_rust_owned`] then use [`std::cell::RefCell::borrow`]
/// or [`std::cell::RefCell::borrow_mut`] to obtain the underlying Rust struct.
///
/// # How to call C++ methods on the subclass
///
/// Do the same. You should find that your subclass struct `impl`s all the
/// C++ methods belonging to the superclass.
///
/// # How to implement virtual methods
///
/// Simply add an `impl` for the `struct`, implementing the relevant method.
/// The C++ virtual function call will be redirected to your Rust implementation.
///
/// # How _not_ to implement virtual methods
///
/// If you don't want to implement a virtual method, don't: the superclass
/// method will be called instead. Naturally, you must implement any pure virtual
/// methods.
///
/// # How it works
///
/// This actually consists of two objects: this object itself and a C++-side
/// peer. The ownership relationship between those two things can work in three
/// different ways:
/// 1. Neither object is owned by Rust. The C++ peer is owned by a C++
///    [`UniquePtr`] held elsewhere in C++. That C++ peer then owns
///    this Rust-side object via a strong [`Rc`] reference. This is the
///    ownership relationship set up by [`CppSubclass::new_cpp_owned`].
/// 2. The object pair is owned by Rust. Specifically, by a strong
///    [`Rc`] reference to this Rust-side object. In turn, the Rust-side object
///    owns the C++-side peer via a [`UniquePtr`]. This is what's set up by
///    [`CppSubclass::new_rust_owned`]. The C++-side peer _does not_ own the Rust
///    object; it just has a weak pointer. (Otherwise we'd get a reference
///    loop and nothing would ever be freed.)
/// 3. The object pair is self-owned and will stay around forever until
///    [`CppSubclassSelfOwned::delete_self`] is called. In this case there's a strong reference
///    from the C++ to the Rust and from the Rust to the C++. This is useful
///    for cases where the subclass is listening for events, and needs to
///    stick around until a particular event occurs then delete itself.
///
/// # Limitations
///
/// * *Re-entrancy*. The main thing to look out for is re-entrancy. If a
///   (non-const) virtual method is called on your type, which then causes you
///   to call back into C++, which results in a _second_ call into a (non-const)
///   virtual method, we will try to create two mutable references to your
///   subclass which isn't allowed in Rust and will therefore panic.
///
///   A future version of autocxx may provide the option of treating all
///   non-const methods (in C++) as const methods on the Rust side, which will
///   give the option of using interior mutability ([`std::cell::RefCell`])
///   for you to safely handle this situation, whilst remaining compatible
///   with existing C++ interfaces. If you need this, indicate support on
///   [this issue](https://github.com/google/autocxx/issues/622).
///
/// * *Thread safety*. The subclass object is not thread-safe and shouldn't
///   be passed to different threads in C++. A future version of this code
///   will give the option to use `Arc` and `Mutex` internally rather than
///   `Rc` and `RefCell`, solving this problem.
///
/// * *Protected methods.* We don't do anything clever here - they're public.
///
/// * *Non-trivial class hierarchies*. We don't yet consider virtual methods
///   on base classes of base classes. This is a temporary limitation,
///   [see this issue](https://github.com/google/autocxx/issues/610).
pub trait CppSubclass<CppPeer, CppPeerPtr>: CppPeerConstructor<CppPeer, CppPeerPtr>
where
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    /// Return the field which holds the C++ peer object. This is normally
    /// implemented by the #[`is_subclass`] macro, but you're welcome to
    /// implement it yourself if you prefer.
    fn peer_holder(&self) -> &CppSubclassCppPeerHolder<CppPeer>;

    /// Return the field which holds the C++ peer object. This is normally
    /// implemented by the #[`is_subclass`] macro, but you're welcome to
    /// implement it yourself if you prefer.
    fn peer_holder_mut(&mut self) -> &mut CppSubclassCppPeerHolder<CppPeer>;

    /// Return a reference to the C++ part of this object pair.
    /// This can be used to register listeners, etc.
    fn peer(&self) -> &CppPeer {
        self.peer_holder().get()
    }

    /// Return a mutable reference to the C++ part of this object pair.
    /// This can be used to register listeners, etc.
    fn peer_mut(&mut self) -> Pin<&mut CppPeer> {
        self.peer_holder_mut().pin_mut()
    }

    /// Creates a new instance of this subclass. This instance is owned by the
    /// returned [`cxx::UniquePtr`] and thus would typically be returned immediately
    /// to C++ such that it can be owned on the C++ side.
    fn new_cpp_owned(me: Self) -> UniquePtr<CppPeer> {
        let mut me = CppPeerPtr::new(me);
        let holder = CppSubclassRustPeerHolder::Owned(me.clone());
        let mut borrowed = CppPeerPtr::get_mut(&mut me);
        let mut cpp_side = borrowed.make_peer(holder);
        borrowed.peer_holder_mut().set_unowned(&mut cpp_side);
        cpp_side
    }

    /// Creates a new instance of this subclass. This instance is not owned
    /// by C++, and therefore will be deleted when it goes out of scope in
    /// Rust.
    fn new_rust_owned(me: Self) -> CppPeerPtr::Ptr {
        make_owning_peer(
            me,
            |obj, holder| obj.make_peer(holder),
            |me| CppSubclassRustPeerHolder::Unowned(CppPeerPtr::downgrade(&me)),
        )
    }
}

pub trait CppSubclassPeerPtr<T> {
    type Ptr: Clone;
    type WeakPtr;
    type Ref<'a>: Deref<Target = T>
    where
        T: 'a,
        Self: 'a;
    type RefMut<'a>: DerefMut<Target = T>
    where
        T: 'a,
        Self: 'a;

    fn new(val: T) -> Self;
    fn upgrade(ptr: &Self::WeakPtr) -> Option<Self::Ptr>;
    fn downgrade(ptr: &Self::Ptr) -> Self::WeakPtr;
    fn get_ref(&self) -> Self::Ref<'_>;
    fn get_mut(&mut self) -> Self::RefMut<'_>;
    fn try_get_ref<'a>(&'a self) -> Result<Self::Ref<'a>, Box<dyn Error + 'a>>;
    fn try_get_mut<'a>(&'a mut self) -> Result<Self::RefMut<'a>, Box<dyn Error + 'a>>;
    fn clone(&self) -> Self::Ptr;
}

impl<T> CppSubclassPeerPtr<T> for Rc<RefCell<T>> {
    type Ptr = Self;
    type WeakPtr = Weak<RefCell<T>>;
    type Ref<'a> = Ref<'a, T> where T: 'a;
    type RefMut<'a> = RefMut<'a, T> where T: 'a;

    fn new(val: T) -> Self::Ptr {
        Rc::new(RefCell::new(val))
    }

    fn upgrade(ptr: &Self::WeakPtr) -> Option<Self::Ptr> {
        ptr.upgrade()
    }

    fn downgrade(ptr: &Self::Ptr) -> Self::WeakPtr {
        Rc::downgrade(ptr)
    }

    fn get_ref(&self) -> Self::Ref<'_> {
        self.borrow()
    }

    fn get_mut(&mut self) -> Self::RefMut<'_> {
        self.borrow_mut()
    }

    fn try_get_ref<'a>(&'a self) -> Result<Self::Ref<'a>, Box<dyn Error + 'a>> {
        self.try_borrow().map_err(|e| Box::new(e) as Box<dyn Error>)
    }

    fn try_get_mut<'a>(&'a mut self) -> Result<Self::RefMut<'a>, Box<dyn Error + 'a>> {
        self.try_borrow_mut()
            .map_err(move |e| Box::new(e) as Box<dyn Error>)
    }

    fn clone(&self) -> Self::Ptr {
        <Rc<RefCell<T>> as Clone>::clone(self)
    }
}

impl<T> CppSubclassPeerPtr<T> for Arc<RwLock<T>> {
    type Ptr = Self;
    type WeakPtr = std::sync::Weak<RwLock<T>>;
    type Ref<'a> = std::sync::RwLockReadGuard<'a, T> where T: 'a;
    type RefMut<'a> = std::sync::RwLockWriteGuard<'a, T> where T: 'a;

    fn new(val: T) -> Self::Ptr {
        Arc::new(RwLock::new(val))
    }

    fn upgrade(ptr: &Self::WeakPtr) -> Option<Self::Ptr> {
        ptr.upgrade()
    }

    fn downgrade(ptr: &Self::Ptr) -> Self::WeakPtr {
        Arc::downgrade(ptr)
    }

    fn get_ref(&self) -> Self::Ref<'_> {
        self.read().unwrap()
    }

    fn get_mut(&mut self) -> Self::RefMut<'_> {
        self.write().unwrap()
    }

    fn try_get_ref<'a>(&'a self) -> Result<Self::Ref<'a>, Box<dyn Error + 'a>> {
        self.read().map_err(move |e| Box::new(e) as Box<dyn Error>)
    }

    fn try_get_mut<'a>(&'a mut self) -> Result<Self::RefMut<'a>, Box<dyn Error + 'a>> {
        self.write().map_err(move |e| Box::new(e) as Box<dyn Error>)
    }

    fn clone(&self) -> Self::Ptr {
        <Arc<RwLock<T>> as Clone>::clone(self)
    }
}

/// Trait to be implemented by subclasses which are self-owned, i.e. not owned
/// externally by either Rust or C++ code, and thus need the ability to delete
/// themselves when some virtual function is called.
pub trait CppSubclassSelfOwned<CppPeer, CppPeerPtr>: CppSubclass<CppPeer, CppPeerPtr>
where
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    /// Creates a new instance of this subclass which owns itself.
    /// This is useful
    /// for observers (etc.) which self-register to listen to events.
    /// If an event occurs which would cause this to want to unregister,
    /// use [`CppSubclassSelfOwned::delete_self`].
    /// The return value may be useful to register this, etc. but can ultimately
    /// be discarded without destroying this object.
    fn new_self_owned(me: Self) -> CppPeerPtr::Ptr {
        make_owning_peer(
            me,
            |obj, holder| obj.make_peer(holder),
            CppSubclassRustPeerHolder::Owned,
        )
    }

    /// Relinquishes ownership from the C++ side. If there are no outstanding
    /// references from the Rust side, this will result in the destruction
    /// of this subclass instance.
    fn delete_self(&self) {
        self.peer().relinquish_ownership()
    }
}

/// Provides default constructors for subclasses which implement `Default`.
pub trait CppSubclassDefault<CppPeer, CppPeerPtr>:
    CppSubclass<CppPeer, CppPeerPtr> + Default
where
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    /// Create a Rust-owned instance of this subclass, initializing with default values. See
    /// [`CppSubclass`] for more details of the ownership models available.
    fn default_rust_owned() -> CppPeerPtr::Ptr;

    /// Create a C++-owned instance of this subclass, initializing with default values. See
    /// [`CppSubclass`] for more details of the ownership models available.
    fn default_cpp_owned() -> UniquePtr<CppPeer>;
}

impl<T, CppPeer, CppPeerPtr> CppSubclassDefault<CppPeer, CppPeerPtr> for T
where
    T: CppSubclass<CppPeer, CppPeerPtr> + Default,
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    fn default_rust_owned() -> CppPeerPtr::Ptr {
        Self::new_rust_owned(Self::default())
    }

    fn default_cpp_owned() -> UniquePtr<CppPeer> {
        Self::new_cpp_owned(Self::default())
    }
}

/// Provides default constructors for subclasses which implement `Default`
/// and are self-owning.
pub trait CppSubclassSelfOwnedDefault<CppPeer, CppPeerPtr>:
    CppSubclassSelfOwned<CppPeer, CppPeerPtr> + Default
where
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    /// Create a self-owned instance of this subclass, initializing with default values. See
    /// [`CppSubclass`] for more details of the ownership models available.
    fn default_self_owned() -> CppPeerPtr::Ptr;
}

impl<T, CppPeer, CppPeerPtr> CppSubclassSelfOwnedDefault<CppPeer, CppPeerPtr> for T
where
    T: CppSubclassSelfOwned<CppPeer, CppPeerPtr> + Default,
    CppPeer: CppSubclassCppPeer,
    CppPeerPtr: CppSubclassPeerPtr<Self>,
{
    fn default_self_owned() -> CppPeerPtr::Ptr {
        Self::new_self_owned(Self::default())
    }
}
