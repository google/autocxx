//! Module to make Rust subclasses of C++ classes. See [`CppSubclass`]
//! for details.

// Copyright 2021 Google LLC
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

use std::{
    cell::RefCell,
    pin::Pin,
    rc::{Rc, Weak},
};

use cxx::{memory::UniquePtrTarget, UniquePtr};

pub use autocxx_macro::is_subclass;

#[doc(hidden)]
pub trait CppSubclassCppPeer: UniquePtrTarget {
    fn relinquish_ownership(&self);
}

#[doc(hidden)]
pub enum CppSubclassRustPeerHolder<T> {
    Owned(Rc<RefCell<T>>),
    Unowned(Weak<RefCell<T>>),
}

impl<T> CppSubclassRustPeerHolder<T> {
    pub fn get(&self) -> Option<Rc<RefCell<T>>> {
        match self {
            CppSubclassRustPeerHolder::Owned(strong) => Some(strong.clone()),
            CppSubclassRustPeerHolder::Unowned(weak) => weak.upgrade(),
        }
    }
    pub fn relinquish_ownership(self) -> Self {
        match self {
            CppSubclassRustPeerHolder::Owned(strong) => {
                CppSubclassRustPeerHolder::Unowned(Rc::downgrade(&strong))
            }
            _ => self,
        }
    }
}

#[doc(hidden)]
pub enum CppSubclassCppPeerHolder<CppPeer: CppSubclassCppPeer> {
    Empty,
    Owned(Box<UniquePtr<CppPeer>>),
    Unowned(*mut CppPeer),
}

impl<CppPeer: CppSubclassCppPeer> Default for CppSubclassCppPeerHolder<CppPeer> {
    fn default() -> Self {
        CppSubclassCppPeerHolder::Empty
    }
}

impl<CppPeer: CppSubclassCppPeer> CppSubclassCppPeerHolder<CppPeer> {
    fn pin_mut(&mut self) -> Pin<&mut CppPeer> {
        match self {
            CppSubclassCppPeerHolder::Empty => panic!("Peer not set up"),
            CppSubclassCppPeerHolder::Owned(peer) => peer.pin_mut(),
            CppSubclassCppPeerHolder::Unowned(peer) => unsafe {
                Pin::new_unchecked(peer.as_mut().unwrap())
            },
        }
    }
    fn get(&self) -> &CppPeer {
        match self {
            CppSubclassCppPeerHolder::Empty => panic!("Peer not set up"),
            CppSubclassCppPeerHolder::Owned(peer) => peer.as_ref(),
            CppSubclassCppPeerHolder::Unowned(peer) => unsafe { peer.as_ref().unwrap() },
        }
    }
    fn set_owned(&mut self, peer: UniquePtr<CppPeer>) {
        *self = Self::Owned(Box::new(peer));
    }
    fn set_unowned(&mut self, peer: &mut UniquePtr<CppPeer>) {
        *self = Self::Unowned(unsafe {
            std::pin::Pin::<&mut CppPeer>::into_inner_unchecked(peer.pin_mut())
        });
    }
}

fn make_owning_peer<CppPeer, PeerConstructor, Subclass, PeerBoxer>(
    me: Subclass,
    peer_constructor: PeerConstructor,
    peer_boxer: PeerBoxer,
) -> Rc<RefCell<Subclass>>
where
    CppPeer: CppSubclassCppPeer,
    Subclass: CppSubclass<CppPeer>,
    PeerConstructor: FnOnce(CppSubclassRustPeerHolder<Subclass>) -> UniquePtr<CppPeer>,
    PeerBoxer: FnOnce(Rc<RefCell<Subclass>>) -> CppSubclassRustPeerHolder<Subclass>,
{
    let me = Rc::new(RefCell::new(me));
    let holder = peer_boxer(me.clone());
    let cpp_side = peer_constructor(holder);
    me.as_ref()
        .borrow_mut()
        .peer_holder_mut()
        .set_owned(cpp_side);
    me
}

/// A subclass of a C++ type.
///
/// To create a Rust subclass of a C++ class, you must do three things:
/// * Use the `subclass` directive in your [`crate::include_cpp`] macro
/// * Create a `struct` to act as your subclass, and add the #[`is_subclass`] attribute.
/// * Use the [`CppSubclass`] trait, and instantiate the subclass using
///   [`CppSubclass::new_rust_owned`] or [`CppSubclass::new_cpp_owned`]
///   constructors. (You can use [`CppSubclassSelfOwned`] if you need that
///   instead.)
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
/// method will be called instead. Naturally, you must implement any virtual
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
///    object; it just has a weak pointer. (Otherwise we'd get a reference)
///    loop and nothing would ever be freed.
/// 3. The object pair is self-owned and will stay around forever until
///    [`CppSubclassSelfOwned::delete_self`] is called. In this case there's a strong reference
///    from the C++ to the Rust and from the Rust to the C++. This is useful
///    for cases where the subclass is listening for events, and needs to
///    stick around until a particular event occurs then delete itself.
pub trait CppSubclass<CppPeer: CppSubclassCppPeer> {
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
    /// returned [`cxx::UniquePtr`] and is thus suitable to be passed around
    /// in C++.
    fn new_cpp_owned<PeerConstructor, Subclass>(
        me: Subclass,
        peer_constructor: PeerConstructor,
    ) -> UniquePtr<CppPeer>
    where
        Subclass: CppSubclass<CppPeer>,
        PeerConstructor: FnOnce(CppSubclassRustPeerHolder<Subclass>) -> UniquePtr<CppPeer>,
    {
        let me = Rc::new(RefCell::new(me));
        let holder = CppSubclassRustPeerHolder::Owned(me.clone());
        let mut cpp_side = peer_constructor(holder);
        me.as_ref()
            .borrow_mut()
            .peer_holder_mut()
            .set_unowned(&mut cpp_side);
        cpp_side
    }

    /// Creates a new instance of this subclass. This instance is not owned
    /// by C++, and therefore will be deleted when it goes out of scope in
    /// Rust.
    fn new_rust_owned<PeerConstructor, Subclass>(
        me: Subclass,
        peer_constructor: PeerConstructor,
    ) -> Rc<RefCell<Subclass>>
    where
        Subclass: CppSubclass<CppPeer>,
        PeerConstructor: FnOnce(CppSubclassRustPeerHolder<Subclass>) -> UniquePtr<CppPeer>,
    {
        make_owning_peer(me, peer_constructor, |me| {
            CppSubclassRustPeerHolder::Unowned(Rc::downgrade(&me))
        })
    }
}

/// Trait to be implemented by subclasses which are self-owned, i.e. not owned
/// externally by either Rust or C++ code, and thus need the ability to delete
/// themselves when some virtual function is called.
pub trait CppSubclassSelfOwned<CppPeer: CppSubclassCppPeer>: CppSubclass<CppPeer> {
    /// Creates a new instance of this subclass which owns itself.
    /// This is useful
    /// for observers (etc.) which self-register to listen to events.
    /// If an event occurs which would cause this to want to unregister,
    /// use [`CppSubclassSelfOwned::delete_self`].
    /// The return value may be useful to register this, etc. but can ultimately
    /// be discarded without destroying this object.
    fn new_self_owned<PeerConstructor, Subclass>(
        me: Subclass,
        peer_constructor: PeerConstructor,
    ) -> Rc<RefCell<Subclass>>
    where
        CppPeer: CppSubclassCppPeer,
        Subclass: CppSubclass<CppPeer>,
        PeerConstructor: FnOnce(CppSubclassRustPeerHolder<Subclass>) -> UniquePtr<CppPeer>,
    {
        make_owning_peer(me, peer_constructor, CppSubclassRustPeerHolder::Owned)
    }

    /// Relinquishes ownership from the C++ side. If there are no outstanding
    /// references from the Rust side, this will result in the destruction
    /// of this subclass instance.
    fn delete_self(&self) {
        self.peer().relinquish_ownership()
    }
}
