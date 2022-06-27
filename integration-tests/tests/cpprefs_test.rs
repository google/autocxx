// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Tests specific to reference wrappers.

use autocxx_integration_tests::{directives_from_lists, do_run_test};
use indoc::indoc;
use proc_macro2::TokenStream;
use quote::quote;

/// A positive test, we expect to pass.
fn run_cpprefs_test(
    cxx_code: &str,
    header_code: &str,
    rust_code: TokenStream,
    generate: &[&str],
    generate_pods: &[&str],
) {
    do_run_test(
        cxx_code,
        header_code,
        rust_code,
        directives_from_lists(generate, generate_pods, None),
        None,
        None,
        None,
        "unsafe_references_wrapped",
    )
    .unwrap()
}

#[test]
fn test_method_call_mut() {
    run_cpprefs_test(
        "",
        indoc! {"
        #include <string>
        #include <sstream>

        class Goat {
            public:
                Goat() : horns(0) {}
                void add_a_horn();
            private:
                uint32_t horns;
        };

        inline void Goat::add_a_horn() { horns++; }
    "},
        quote! {
            let goat = ffi::Goat::new().within_unique_ptr();
            let mut goat = ffi::CppUniquePtrPin::new(goat);
            goat.as_cpp_mut_ref().add_a_horn();
        },
        &["Goat"],
        &[],
    )
}

#[test]
fn test_method_call_const() {
    run_cpprefs_test(
        "",
        indoc! {"
        #include <string>
        #include <sstream>

        class Goat {
            public:
                Goat() : horns(0) {}
                std::string describe() const;
            private:
                uint32_t horns;
        };

        inline std::string Goat::describe() const {
            std::ostringstream oss;
            std::string plural = horns == 1 ? \"\" : \"s\";
            oss << \"This goat has \" << horns << \" horn\" << plural << \".\";
            return oss.str();
        }
    "},
        quote! {
            let goat = ffi::Goat::new().within_unique_ptr();
            let goat = ffi::cpp_pin_uniqueptr(goat);
            goat.as_cpp_ref().describe();
        },
        &["Goat"],
        &[],
    )
}
