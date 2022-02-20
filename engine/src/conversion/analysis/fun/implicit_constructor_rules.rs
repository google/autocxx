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

//! Module which understands C++ constructor synthesis rules.

use crate::conversion::api::CppVisibility;

/// Indicates what we found out about a category of special member function.
///
/// In the end, we only care whether it's public and exists, but we track a bit more information to
/// support determining the information for dependent classes.
#[derive(Debug, Copy, Clone)]
pub(super) enum SpecialMemberFound {
    /// This covers being deleted in any way:
    ///   * Explicitly deleted
    ///   * Implicitly defaulted when that means being deleted
    ///   * Explicitly defaulted when that means being deleted
    ///
    /// It also covers not being either user declared or implicitly defaulted.
    NotPresent,
    /// Implicit special member functions, indicated by this, are always public.
    Implicit,
    /// This covers being explicitly defaulted (when that is not deleted) or being user-defined.
    Explicit(CppVisibility),
}

impl SpecialMemberFound {
    /// Returns whether code outside of subclasses can call this special member function.
    pub fn callable_any(&self) -> bool {
        matches!(self, Self::Explicit(CppVisibility::Public) | Self::Implicit)
    }

    /// Returns whether code in a subclass can call this special member function.
    pub fn callable_subclass(&self) -> bool {
        matches!(
            self,
            Self::Explicit(CppVisibility::Public)
                | Self::Explicit(CppVisibility::Protected)
                | Self::Implicit
        )
    }

    /// Returns whether this exists at all. Note that this will return true even if it's private,
    /// which is generally not very useful, but does come into play for some rules around which
    /// default special member functions are deleted vs don't exist.
    pub fn exists(&self) -> bool {
        matches!(self, Self::Explicit(_) | Self::Implicit)
    }

    pub fn exists_implicit(&self) -> bool {
        matches!(self, Self::Implicit)
    }

    pub fn exists_explicit(&self) -> bool {
        matches!(self, Self::Explicit(_))
    }
}

/// Information about which special member functions exist based on the C++ rules.
///
/// Not all of this information is used directly, but we need to track it to determine the
/// information we do need for classes which are used as members or base classes.
#[derive(Debug, Copy, Clone)]
pub(super) struct ItemsFound {
    pub(super) default_constructor: SpecialMemberFound,
    pub(super) destructor: SpecialMemberFound,
    pub(super) const_copy_constructor: SpecialMemberFound,
    /// Remember that [`const_copy_constructor`] may be used in place of this if it exists.
    pub(super) non_const_copy_constructor: SpecialMemberFound,
    pub(super) move_constructor: SpecialMemberFound,
}

impl ItemsFound {
    /// Returns whether we should generate a default constructor wrapper, because bindgen won't do
    /// one for the implicit default constructor which exists.
    pub(super) fn implicit_default_constructor_needed(&self) -> bool {
        self.default_constructor.exists_implicit()
    }

    /// Returns whether we should generate a copy constructor wrapper, because bindgen won't do one
    /// for the implicit copy constructor which exists.
    pub(super) fn implicit_copy_constructor_needed(&self) -> bool {
        let any_implicit_copy = self.const_copy_constructor.exists_implicit()
            || self.non_const_copy_constructor.exists_implicit();
        let no_explicit_copy = !(self.const_copy_constructor.exists_explicit()
            || self.non_const_copy_constructor.exists_explicit());
        any_implicit_copy && no_explicit_copy
    }

    /// Returns whether we should generate a move constructor wrapper, because bindgen won't do one
    /// for the implicit move constructor which exists.
    pub(super) fn implicit_move_constructor_needed(&self) -> bool {
        self.move_constructor.exists_implicit()
    }

    /// Returns whether we should generate a destructor wrapper, because bindgen won't do one for
    /// the implicit destructor which exists.
    pub(super) fn implicit_destructor_needed(&self) -> bool {
        self.destructor.exists_implicit()
    }
}
