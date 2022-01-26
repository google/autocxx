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

#[cfg_attr(test, derive(Eq, PartialEq))]
pub(super) struct ImplicitConstructorsNeeded {
    pub(super) default_constructor: bool,
    pub(super) copy_constructor_taking_t: bool,
    pub(super) copy_constructor_taking_const_t: bool,
    pub(super) move_constructor: bool,
}

#[derive(Default)]
pub(super) struct ExplicitItemsFound {
    pub(super) move_constructor: bool,
    pub(super) copy_constructor: bool,
    pub(super) any_other_constructor: bool,
    pub(super) any_bases_or_fields_lack_const_copy_constructors: bool,
    pub(super) destructor: bool,
    pub(super) copy_assignment_operator: bool,
    pub(super) move_assignment_operator: bool,
}

pub(super) fn determine_implicit_constructors(
    explicits: ExplicitItemsFound,
) -> ImplicitConstructorsNeeded {
    let any_constructor =
        explicits.copy_constructor || explicits.move_constructor || explicits.any_other_constructor;
    // If no user-declared constructors of any kind are provided for a class type (struct, class, or union),
    // the compiler will always declare a default constructor as an inline public member of its class.
    let default_constructor = !any_constructor;

    // If no user-defined copy constructors are provided for a class type (struct, class, or union),
    // the compiler will always declare a copy constructor as a non-explicit inline public member of its class.
    // This implicitly-declared copy constructor has the form T::T(const T&) if all of the following are true:
    //  each direct and virtual base B of T has a copy constructor whose parameters are const B& or const volatile B&;
    //  each non-static data member M of T of class type or array of class type has a copy constructor whose parameters are const M& or const volatile M&.
    let (copy_constructor_taking_const_t, copy_constructor_taking_t) = if explicits.copy_constructor
    {
        (false, false)
    } else if explicits.any_bases_or_fields_lack_const_copy_constructors {
        (false, true)
    } else {
        (true, false)
    };

    // If no user-defined move constructors are provided for a class type (struct, class, or union), and all of the following is true:
    // there are no user-declared copy constructors;
    // there are no user-declared copy assignment operators;
    // there are no user-declared move assignment operators;
    // there is no user-declared destructor.
    // then the compiler will declare a move constructor
    let move_constructor = !(explicits.move_constructor
        || explicits.copy_constructor
        || explicits.destructor
        || explicits.copy_assignment_operator
        || explicits.move_assignment_operator);

    ImplicitConstructorsNeeded {
        default_constructor,
        copy_constructor_taking_t,
        copy_constructor_taking_const_t,
        move_constructor,
    }
}

#[cfg(test)]
mod tests {
    use super::determine_implicit_constructors;

    use super::ExplicitItemsFound;

    #[test]
    fn test_simple() {
        let inputs = ExplicitItemsFound::default();
        let outputs = determine_implicit_constructors(inputs);
        assert_eq!(true, outputs.default_constructor);
        assert_eq!(true, outputs.copy_constructor_taking_const_t);
        assert_eq!(false, outputs.copy_constructor_taking_t);
        assert_eq!(true, outputs.move_constructor);
    }

    #[test]
    fn test_with_destructor() {
        let inputs = ExplicitItemsFound {
            destructor: true,
            ..Default::default()
        };
        let outputs = determine_implicit_constructors(inputs);
        assert_eq!(true, outputs.default_constructor);
        assert_eq!(true, outputs.copy_constructor_taking_const_t);
        assert_eq!(false, outputs.copy_constructor_taking_t);
        assert_eq!(false, outputs.move_constructor);
    }

    #[test]
    fn test_with_pesky_base() {
        let inputs = ExplicitItemsFound {
            any_bases_or_fields_lack_const_copy_constructors: true,
            ..Default::default()
        };
        let outputs = determine_implicit_constructors(inputs);
        assert_eq!(true, outputs.default_constructor);
        assert_eq!(false, outputs.copy_constructor_taking_const_t);
        assert_eq!(true, outputs.copy_constructor_taking_t);
        assert_eq!(true, outputs.move_constructor);
    }

    #[test]
    fn test_with_user_defined_move_constructor() {
        let inputs = ExplicitItemsFound {
            move_constructor: true,
            ..Default::default()
        };
        let outputs = determine_implicit_constructors(inputs);
        assert_eq!(false, outputs.default_constructor);
        assert_eq!(true, outputs.copy_constructor_taking_const_t);
        assert_eq!(false, outputs.copy_constructor_taking_t);
        assert_eq!(false, outputs.move_constructor);
    }

    #[test]
    fn test_with_user_defined_misc_constructor() {
        let inputs = ExplicitItemsFound {
            any_other_constructor: true,
            ..Default::default()
        };
        let outputs = determine_implicit_constructors(inputs);
        assert_eq!(false, outputs.default_constructor);
        assert_eq!(true, outputs.copy_constructor_taking_const_t);
        assert_eq!(false, outputs.copy_constructor_taking_t);
        assert_eq!(true, outputs.move_constructor);
    }
}
