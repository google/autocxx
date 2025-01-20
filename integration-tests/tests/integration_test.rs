// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::{
    builder_modifiers::{
        make_clang_arg_adder, make_clang_optional_arg_adder, make_cpp17_adder, EnableAutodiscover,
        SetSuppressSystemHeaders,
    },
    code_checkers::{
        make_error_finder, make_rust_code_finder, make_string_finder, CppMatcher,
        NoSystemHeadersChecker,
    },
};
use autocxx_integration_tests::{
    directives_from_lists, do_run_test, do_run_test_manual, run_generate_all_test, run_test,
    run_test_ex, run_test_expect_fail, run_test_expect_fail_ex, BuilderModifier, TestError,
};
use indoc::indoc;
use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{parse_quote, Token};
use test_log::test;

#[test]
fn test_return_void() {
    let cxx = indoc! {"
        void do_nothing() {
        }
    "};
    let hdr = indoc! {"
        void do_nothing();
    "};
    let rs = quote! {
        ffi::do_nothing();
    };
    run_test(cxx, hdr, rs, &["do_nothing"], &[]);
}

#[test]
fn test_two_funcs() {
    let cxx = indoc! {"
        void do_nothing1() {
        }
        void do_nothing2() {
        }
    "};
    let hdr = indoc! {"
        void do_nothing1();
        void do_nothing2();
    "};
    let rs = quote! {
        ffi::do_nothing1();
        ffi::do_nothing2();
    };
    run_test(cxx, hdr, rs, &["do_nothing1", "do_nothing2"], &[]);
}

#[test]
fn test_two_funcs_with_definition() {
    // Test to ensure C++ header isn't included twice
    let cxx = indoc! {"
        void do_nothing1() {
        }
        void do_nothing2() {
        }
    "};
    let hdr = indoc! {"
        struct Bob {
            int a;
        };
        void do_nothing1();
        void do_nothing2();
    "};
    let rs = quote! {
        ffi::do_nothing1();
        ffi::do_nothing2();
    };
    run_test(cxx, hdr, rs, &["do_nothing1", "do_nothing2"], &[]);
}

#[test]
fn test_return_i32() {
    let cxx = indoc! {"
        uint32_t give_int() {
            return 5;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        uint32_t give_int();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_int(), 5);
    };
    run_test(cxx, hdr, rs, &["give_int"], &[]);
}

#[test]
fn test_take_i32() {
    let cxx = indoc! {"
        uint32_t take_int(uint32_t a) {
            return a + 3;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        uint32_t take_int(uint32_t a);
    "};
    let rs = quote! {
        assert_eq!(ffi::take_int(3), 6);
    };
    run_test(cxx, hdr, rs, &["take_int"], &[]);
}

#[test]
fn test_exception_pod() {
    let hdr = indoc! {r#"
        #include <stdint.h>
        #include <string>
        #include <stdexcept>

        struct B { uint32_t x; };

        void do_nothing();
        uint32_t get_integer();
        uint32_t get_wrapped_integer();
        B get_wrapped_struct();
        uint32_t throw_exception();
    "#};
    let cxx = indoc! {r#"
        void do_nothing() {
        }

        uint32_t get_integer() {
            return 5;
        }

        uint32_t get_wrapped_integer() {
            return 5;
        }

        B get_wrapped_struct() {
            return B { 5 };
        }

        uint32_t throw_exception() {
            throw std::invalid_argument("EXCEPTION");
        }
    "#};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        mod a {
            use autocxx::prelude::*;
            include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                pod!("B")
                generate!("do_nothing")
                generate!("get_integer")
                generate!("get_wrapped_integer")
                generate!("get_wrapped_struct")
                generate!("throw_exception")
                throws!("do_nothing")
                throws!("get_wrapped_integer")
                throws!("get_wrapped_struct")
                throws!("throw_exception")
            }
            pub use ffi::*;
        }
        fn main() {
            let maybe_nothing = a::do_nothing();
            assert!(maybe_nothing.is_ok());
            assert_eq!(maybe_nothing.unwrap(), ());

            assert_eq!(a::get_integer(), 5);

            let maybe_int = a::get_wrapped_integer();
            assert!(maybe_int.is_ok());
            assert_eq!(maybe_int.unwrap(), 5);

            let maybe_b = a::get_wrapped_struct();
            assert!(maybe_b.is_ok());
            let b = maybe_b.unwrap();
            assert_eq!(b.x, 5);

            assert!(a::throw_exception().is_err());
        }
    };
    do_run_test_manual(cxx, hdr, rs, None, None).unwrap();
}

#[test]
fn test_exception_uniqueptr() {
    let hdr = indoc! {r#"
        #include <string>
        #include <stdexcept>

        std::string get_str();
        std::string try_get_str();
        std::string throw_exception();
    "#};
    let cxx = indoc! {r#"
        std::string get_str() {
            return std::string("foobar");
        }

        std::string try_get_str() {
            return get_str();
        }

        std::string throw_exception() {
            throw std::invalid_argument("EXCEPTION");
        }
    "#};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        mod a {
            use autocxx::prelude::*;
            include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                generate!("get_str")
                generate!("try_get_str")
                generate!("throw_exception")
                throws!("try_get_str")
                throws!("throw_exception")
            }
            pub use ffi::*;
        }
        fn main() {
            assert!(a::try_get_str().is_ok());
            assert!(a::throw_exception().is_err());
        }
    };
    do_run_test_manual(cxx, hdr, rs, None, None).unwrap();
}

#[test]
fn test_exception_moveit() {
    let hdr = indoc! {r#"
        #include <string>
        class B {
        public:
            B(std::string s) : s(s) {}
        private:
            std::string s;
        };

        B get_b();
        B try_get_b();
    "#};
    let cxx = indoc! {r#"
        B get_b() {
            return B("foobar");
        }

        B try_get_b() {
            return get_b();
        }
    "#};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        mod a {
            use autocxx::prelude::*;
            include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                generate!("B")
                generate!("get_b")
                generate!("try_get_b")
                throws!("try_get_b")
            }
            pub use ffi::*;
        }
        fn main() {
            use autocxx::WithinBox;
            let _ = a::get_b().within_box();
            let maybe_b = a::try_get_b();
        }
    };
    do_run_test_manual(cxx, hdr, rs, None, None).unwrap();
}

#[test]
fn test_nested_module() {
    let cxx = indoc! {"
        void do_nothing() {
        }
    "};
    let hdr = indoc! {"
        void do_nothing();
    "};
    let hexathorpe = Token![#](Span::call_site());
    let unexpanded_rust = quote! {
        mod a {
            use autocxx::prelude::*;

            include_cpp!(
                #hexathorpe include "input.h"
                generate!("do_nothing")
                safety!(unsafe)
            );

            pub use ffi::*;
        }

        fn main() {
            a::do_nothing();
        }
    };

    do_run_test_manual(cxx, hdr, unexpanded_rust, None, None).unwrap();
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/681
#[cfg(target_pointer_width = "64")]
fn test_return_big_ints() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t give_u32() {
            return 5;
        }
        inline uint64_t give_u64() {
            return 5;
        }
        inline int32_t give_i32() {
            return 5;
        }
        inline int64_t give_i64() {
            return 5;
        }
        inline __int128 give_i128() {
            return 5;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::give_u32(), 5);
        assert_eq!(ffi::give_u64(), 5);
        assert_eq!(ffi::give_i32(), 5);
        assert_eq!(ffi::give_i64(), 5);
        assert_eq!(ffi::give_i128(), 5);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["give_u32", "give_u64", "give_i32", "give_i64", "give_i128"],
        &[],
    );
}

#[test]
#[ignore] // because cxx doesn't support unique_ptrs to primitives.
fn test_give_up_int() {
    let cxx = indoc! {"
        std::unique_ptr<uint32_t> give_up() {
            return std::make_unique<uint32_t>(12);
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        std::unique_ptr<uint32_t> give_up();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_up().as_ref().unwrap(), 12);
    };
    run_test(cxx, hdr, rs, &["give_up"], &[]);
}

#[test]
#[ignore] // because we don't yet implement UniquePtr etc. for autocxx::c_int and friends
fn test_give_up_ctype() {
    let cxx = indoc! {"
        std::unique_ptr<int> give_up() {
            return std::make_unique<int>(12);
        }
    "};
    let hdr = indoc! {"
        #include <memory>
        std::unique_ptr<int> give_up();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_up().as_ref().unwrap(), autocxx::c_int(12));
    };
    run_test(cxx, hdr, rs, &["give_up"], &[]);
}

#[test]
fn test_give_string_up() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str_up() {
            return std::make_unique<std::string>(\"Bob\");
        }
    "};
    let hdr = indoc! {"
        #include <memory>
        #include <string>
        std::unique_ptr<std::string> give_str_up();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_str_up().as_ref().unwrap().to_str().unwrap(), "Bob");
    };
    run_test(cxx, hdr, rs, &["give_str_up"], &[]);
}

#[test]
fn test_give_string_plain() {
    let cxx = indoc! {"
        std::string give_str() {
            return std::string(\"Bob\");
        }
    "};
    let hdr = indoc! {"
        #include <string>
        std::string give_str();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_str().as_ref().unwrap(), "Bob");
    };
    run_test(cxx, hdr, rs, &["give_str"], &[]);
}

#[test]
fn test_cycle_string_up() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str_up() {
            return std::make_unique<std::string>(\"Bob\");
        }
        uint32_t take_str_up(std::unique_ptr<std::string> a) {
            return a->length();
        }
    "};
    let hdr = indoc! {"
        #include <memory>
        #include <string>
        #include <cstdint>
        std::unique_ptr<std::string> give_str_up();
        uint32_t take_str_up(std::unique_ptr<std::string> a);
    "};
    let rs = quote! {
        let s = ffi::give_str_up();
        assert_eq!(ffi::take_str_up(s), 3);
    };
    run_test(cxx, hdr, rs, &["give_str_up", "take_str_up"], &[]);
}

#[test]
fn test_cycle_string() {
    let cxx = indoc! {"
        std::string give_str() {
            return std::string(\"Bob\");
        }
        uint32_t take_str(std::string a) {
            return a.length();
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        std::string give_str();
        uint32_t take_str(std::string a);
    "};
    let rs = quote! {
        let s = ffi::give_str();
        assert_eq!(ffi::take_str(s), 3);
    };
    let generate = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, generate, &[]);
}

#[test]
fn test_cycle_string_by_ref() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str() {
            return std::make_unique<std::string>(\"Bob\");
        }
        uint32_t take_str(const std::string& a) {
            return a.length();
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        #include <cstdint>
        std::unique_ptr<std::string> give_str();
        uint32_t take_str(const std::string& a);
    "};
    let rs = quote! {
        let s = ffi::give_str();
        assert_eq!(ffi::take_str(s.as_ref().unwrap()), 3);
    };
    let generate = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, generate, &[]);
}

#[test]
fn test_cycle_string_by_mut_ref() {
    let cxx = indoc! {"
        std::unique_ptr<std::string> give_str() {
            return std::make_unique<std::string>(\"Bob\");
        }
        uint32_t take_str(std::string& a) {
            return a.length();
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        #include <cstdint>
        std::unique_ptr<std::string> give_str();
        uint32_t take_str(std::string& a);
    "};
    let rs = quote! {
        let mut s = ffi::give_str();
        assert_eq!(ffi::take_str(s.as_mut().unwrap()), 3);
    };
    let generate = &["give_str", "take_str"];
    run_test(cxx, hdr, rs, generate, &[]);
}

#[test]
fn test_give_pod_by_value() {
    let cxx = indoc! {"
        Bob give_bob() {
            Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        Bob give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_give_pod_class_by_value() {
    let cxx = indoc! {"
        Bob give_bob() {
            Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            uint32_t a;
            uint32_t b;
        };
        Bob give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_give_pod_by_up() {
    let cxx = indoc! {"
        std::unique_ptr<Bob> give_bob() {
            auto a = std::make_unique<Bob>();
            a->a = 3;
            a->b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        std::unique_ptr<Bob> give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().as_ref().unwrap().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_take_pod_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_negative_take_as_pod_with_destructor() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
            inline ~Bob() {}
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test_expect_fail(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_negative_take_as_pod_with_move_constructor() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <type_traits>
        struct Bob {
            uint32_t a;
            uint32_t b;
            inline Bob(Bob&& other_bob) {}
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test_expect_fail(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[ignore] // https://github.com/google/autocxx/issues/1252
#[test]
fn test_take_as_pod_with_is_relocatable() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <type_traits>
        struct Bob {
            uint32_t a;
            uint32_t b;
            inline Bob() {}
            inline ~Bob() {}
            inline Bob(Bob&& other_bob) { a = other_bob.a; b = other_bob.b; }
            using IsRelocatable = std::true_type;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_pod_by_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_pod_by_ref_and_ptr() {
    let cxx = indoc! {"
        uint32_t take_bob_ref(const Bob& a) {
            return a.a;
        }
        uint32_t take_bob_ptr(const Bob* a) {
            return a->a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob_ref(const Bob& a);
        uint32_t take_bob_ptr(const Bob* a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob_ref(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob_ref", "take_bob_ptr"], &["Bob"]);
}

#[test]
fn test_return_pod_by_ref_and_ptr() {
    let hdr = indoc! {"
        #include <cstdint>
        struct B {
            uint32_t a;
        };
        struct A {
            B b;
        };
        inline const B& return_b_ref(const A& a) {
            return a.b;
        }
        inline const B* return_b_ptr(const A& a) {
            return &a.b;
        }
    "};
    let rs = quote! {
        let a = ffi::A { b: ffi::B { a: 3 } };
        assert_eq!(ffi::return_b_ref(&a).a, 3);
        let b_ptr = ffi::return_b_ptr(&a);
        assert_eq!(unsafe { b_ptr.as_ref() }.unwrap().a, 3);
    };
    run_test("", hdr, rs, &["return_b_ref", "return_b_ptr"], &["A", "B"]);
}

#[test]
fn test_take_pod_by_mut_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob& a) {
            a.b = 14;
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(Bob& a);
    "};
    let rs = quote! {
        let mut a = Box::pin(ffi::Bob { a: 12, b: 13 });
        assert_eq!(ffi::take_bob(a.as_mut()), 12);
        assert_eq!(a.b, 14);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_nested_pod_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Phil {
            uint32_t d;
        };
        struct Bob {
            uint32_t a;
            uint32_t b;
            Phil c;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13, c: ffi::Phil { d: 4 } };
        assert_eq!(ffi::take_bob(a), 12);
    };
    // Should be no need to allowlist Phil below
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_take_nonpod_by_value() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a0, uint32_t b0)
           : a(a0), b(b0) {}
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
            std::string reason_why_this_is_nonpod;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob::new(12, 13).within_unique_ptr();
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::make_bob(12);
        assert_eq!(ffi::take_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_take_nonpod_by_up() {
    let cxx = indoc! {"
        uint32_t take_bob(std::unique_ptr<Bob> a) {
            return a->a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };

        struct NOP { inline void take_bob(); };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(std::unique_ptr<Bob> a);
    "};
    let rs = quote! {
        let a = ffi::make_bob(12);
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob", "NOP"], &[]);
}

#[test]
fn test_take_nonpod_by_ptr_simple() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob* a) {
            return a->a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(const Bob* a);
    "};
    let rs = quote! {
        let a = ffi::make_bob(12);
        let a_ptr = a.into_raw();
        assert_eq!(unsafe { ffi::take_bob(a_ptr) }, 12);
        unsafe { cxx::UniquePtr::from_raw(a_ptr) }; // so we drop
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ptr_in_method() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        #include <cstdint>
        class A {
        public:
            A() {};
            uint32_t take_bob(const Bob* a) const {
                return a->a;
            }
            std::unique_ptr<Bob> make_bob(uint32_t a) const {
                auto b = std::make_unique<Bob>();
                b->a = a;
                return b;
            }
            uint16_t a;
        };

    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        let b = a.as_ref().unwrap().make_bob(12);
        let b_ptr = b.into_raw();
        assert_eq!(unsafe { a.as_ref().unwrap().take_bob(b_ptr) }, 12);
        unsafe { cxx::UniquePtr::from_raw(b_ptr) }; // so we drop
    };
    run_test("", hdr, rs, &["A", "Bob"], &[]);
}

#[test]
fn test_take_nonpod_by_ptr_in_wrapped_method() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct C {
            C() {}
            uint32_t a;
        };
        struct Bob {
            uint32_t a;
        };
        class A {
        public:
            A() {};
            uint32_t take_bob(const Bob* a, C) const {
                return a->a;
            }
            std::unique_ptr<Bob> make_bob(uint32_t a) const {
                auto b = std::make_unique<Bob>();
                b->a = a;
                return b;
            }
            uint16_t a;
        };

    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        let c = ffi::C::new().within_unique_ptr();
        let b = a.as_ref().unwrap().make_bob(12);
        let b_ptr = b.into_raw();
        assert_eq!(unsafe { a.as_ref().unwrap().take_bob(b_ptr, c) }, 12);
        unsafe { cxx::UniquePtr::from_raw(b_ptr) }; // so we drop
    };
    run_test("", hdr, rs, &["A", "Bob", "C"], &[]);
}

fn run_char_test(builder_modifier: Option<BuilderModifier>) {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct C {
            C() { test = \"hi\"; }
            uint32_t a;
            const char* test;
        };
        class A {
        public:
            A() {};
            uint32_t take_char(const char* a, C) const {
                return a[0];
            }
            const char* make_char(C extra) const {
                return extra.test;
            }
            uint16_t a;
        };

    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        let c1 = ffi::C::new().within_unique_ptr();
        let c2 = ffi::C::new().within_unique_ptr();
        let ch = a.as_ref().unwrap().make_char(c1);
        assert_eq!(unsafe { ch.as_ref()}.unwrap(), &104i8);
        assert_eq!(unsafe { a.as_ref().unwrap().take_char(ch, c2) }, 104);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["A", "C"], &[], None),
        builder_modifier,
        None,
        None,
    );
}

#[test]
fn test_take_char_by_ptr_in_wrapped_method() {
    run_char_test(None)
}

#[test]
fn test_take_char_by_ptr_in_wrapped_method_with_unsigned_chars() {
    run_char_test(make_clang_arg_adder(&["-funsigned-char"]))
}

#[test]
fn test_take_nonpod_by_mut_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(Bob& a);
    "};
    let rs = quote! {
        let mut a = ffi::make_bob(12);
        assert_eq!(ffi::take_bob(a.pin_mut()), 12);
    };
    // TODO confirm that the object really was mutated by C++ in this
    // and similar tests.
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_return_nonpod_by_value() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a0, uint32_t b0)
           : a(a0), b(b0) {}
        Bob give_bob(uint32_t a) {
            Bob c(a, 44);
            return c;
        }
        uint32_t take_bob(std::unique_ptr<Bob> a) {
            return a->a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
        };
        Bob give_bob(uint32_t a);
        uint32_t take_bob(std::unique_ptr<Bob> a);
    "};
    let rs = quote! {
        let a = ffi::give_bob(13).within_unique_ptr();
        assert_eq!(ffi::take_bob(a), 13);
    };
    run_test(cxx, hdr, rs, &["take_bob", "give_bob", "Bob"], &[]);
}

#[test]
fn test_get_str_by_up() {
    let cxx = indoc! {"
    std::unique_ptr<std::string> get_str() {
            return std::make_unique<std::string>(\"hello\");
        }
    "};
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        std::unique_ptr<std::string> get_str();
    "};
    let rs = quote! {
        assert_eq!(ffi::get_str().as_ref().unwrap(), "hello");
    };
    run_test(cxx, hdr, rs, &["get_str"], &[]);
}

#[test]
fn test_get_str_by_value() {
    let cxx = indoc! {"
        std::string get_str() {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <string>
        std::string get_str();
    "};
    let rs = quote! {
        assert_eq!(ffi::get_str().as_ref().unwrap(), "hello");
    };
    run_test(cxx, hdr, rs, &["get_str"], &[]);
}

#[test]
fn test_cycle_nonpod_with_str_by_ref() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob() {
            auto a = std::make_unique<Bob>();
            a->a = 32;
            a->b = \"hello\";
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        struct Bob {
            uint32_t a;
            std::string b;
        };
        uint32_t take_bob(const Bob& a);
        std::unique_ptr<Bob> make_bob();
    "};
    let rs = quote! {
        let a = ffi::make_bob();
        assert_eq!(ffi::take_bob(a.as_ref().unwrap()), 32);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_make_up() {
    let cxx = indoc! {"
        Bob::Bob() : a(3) {
        }
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            Bob();
            uint32_t a;
        };
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::Bob::new().within_unique_ptr(); // TODO test with all sorts of arguments.
        assert_eq!(ffi::take_bob(a.as_ref().unwrap()), 3);
    };
    run_test(cxx, hdr, rs, &["Bob", "take_bob"], &[]);
}

#[test]
fn test_make_up_with_args() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a0, uint32_t b0)
           : a(a0), b(b0) {}
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            Bob(uint32_t a, uint32_t b);
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        let a = ffi::Bob::new(12, 13).within_unique_ptr();
        assert_eq!(ffi::take_bob(a.as_ref().unwrap()), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob", "Bob"], &[]);
}

#[test]
#[ignore] // because we don't support unique_ptrs to primitives
fn test_make_up_int() {
    let cxx = indoc! {"
        Bob::Bob(uint32_t a) : b(a) {
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            Bob(uint32_t a);
            uint32_t b;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob::new(3).within_unique_ptr();
        assert_eq!(a.as_ref().unwrap().b, 3);
    };
    run_test(cxx, hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_enum_with_funcs() {
    let cxx = indoc! {"
        Bob give_bob() {
            return Bob::BOB_VALUE_2;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        enum Bob {
            BOB_VALUE_1,
            BOB_VALUE_2,
        };
        Bob give_bob();
    "};
    let rs = quote! {
        let a = ffi::Bob::BOB_VALUE_2;
        let b = ffi::give_bob();
        assert!(a == b);
    };
    run_test(cxx, hdr, rs, &["Bob", "give_bob"], &[]);
}

#[test]
fn test_re_export() {
    let cxx = indoc! {"
        Bob give_bob() {
            return Bob::BOB_VALUE_2;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        enum Bob {
            BOB_VALUE_1,
            BOB_VALUE_2,
        };
        Bob give_bob();
    "};
    let rs = quote! {
        let a = ffi::Bob::BOB_VALUE_2;
        let b = ffi::give_bob();
        assert!(a == b);
    };
    run_test_ex(
        cxx,
        hdr,
        rs,
        directives_from_lists(&["Bob", "give_bob"], &[], None),
        None,
        None,
        Some(quote! { pub use ffi::Bob; }),
    );
}

#[test]
fn test_enum_no_funcs() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        enum Bob {
            BOB_VALUE_1,
            BOB_VALUE_2,
        };
    "};
    let rs = quote! {
        let a = ffi::Bob::BOB_VALUE_1;
        let b = ffi::Bob::BOB_VALUE_2;
        assert!(a != b);
    };
    run_test(cxx, hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_enum_with_funcs_as_pod() {
    let cxx = indoc! {"
        Bob give_bob() {
            return Bob::BOB_VALUE_2;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        enum Bob {
            BOB_VALUE_1,
            BOB_VALUE_2,
        };
        Bob give_bob();
    "};
    let rs = quote! {
        let a = ffi::Bob::BOB_VALUE_2;
        let b = ffi::give_bob();
        assert!(a == b);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test] // works, but causes compile warnings
fn test_take_pod_class_by_value() {
    let cxx = indoc! {"
        uint32_t take_bob(Bob a) {
            return a.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        class Bob {
        public:
            uint32_t a;
            uint32_t b;
        };
        uint32_t take_bob(Bob a);
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["Bob"]);
}

#[test]
fn test_pod_method() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob() const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob() const;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12, b: 13 };
        assert_eq!(a.get_bob(), 12);
    };
    run_test(cxx, hdr, rs, &[], &["Bob"]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/723
fn test_constructors_for_specialized_types() {
    // bindgen sometimes makes such opaque types as type Bob = u32[2];
    let hdr = indoc! {"
        #include <cstdint>
        template<typename T>
        class A {
            uint32_t foo() { return 12; };
        private:
            T a[2];
        };

        typedef A<uint32_t> B;
        typedef B C;
    "};
    let rs = quote! {
        let a = ffi::C::new().within_unique_ptr();
        assert_eq!(a.foo(), 12);
    };
    run_test("", hdr, rs, &["C"], &[]);
}

#[test]
fn test_pod_mut_method() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob() {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob();
        };
    "};
    let rs = quote! {
        let mut a = Box::pin(ffi::Bob { a: 12, b: 13 });
        assert_eq!(a.as_mut().get_bob(), 12);
    };
    run_test(cxx, hdr, rs, &[], &["Bob"]);
}

#[test]
fn test_define_int() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #define BOB 3
    "};
    let rs = quote! {
        assert_eq!(ffi::BOB, 3);
    };
    run_test(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_define_str() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #define BOB \"foo\"
    "};
    let rs = quote! {
        assert_eq!(core::str::from_utf8(ffi::BOB).unwrap().trim_end_matches(char::from(0)), "foo");
    };
    run_test(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_i32_const() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        const uint32_t BOB = 3;
    "};
    let rs = quote! {
        assert_eq!(ffi::BOB, 3);
    };
    run_test(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_negative_rs_nonsense() {
    // Really just testing the test infrastructure.
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        const uint32_t BOB = 3;
    "};
    let rs = quote! {
        foo bar
    };
    run_test_expect_fail(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_negative_cpp_nonsense() {
    // Really just testing the test infrastructure.
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        const uint32_t BOB = CAT;
    "};
    let rs = quote! {
        assert_eq!(ffi::BOB, 3);
    };
    run_test_expect_fail(cxx, hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_negative_make_nonpod() {
    let cxx = indoc! {"
        uint32_t take_bob(const Bob& a) {
            return a.a;
        }
        std::unique_ptr<Bob> make_bob(uint32_t a) {
            auto b = std::make_unique<Bob>();
            b->a = a;
            return b;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        std::unique_ptr<Bob> make_bob(uint32_t a);
        uint32_t take_bob(const Bob& a);
    "};
    let rs = quote! {
        ffi::Bob {};
    };
    let rs2 = quote! {
        ffi::Bob { a: 12 };
    };
    let rs3 = quote! {
        ffi::Bob { do_not_attempt_to_allocate_nonpod_types: [] };
    };
    run_test_expect_fail(cxx, hdr, rs, &["take_bob", "Bob", "make_bob"], &[]);
    run_test_expect_fail(cxx, hdr, rs2, &["take_bob", "Bob", "make_bob"], &[]);
    run_test_expect_fail(cxx, hdr, rs3, &["take_bob", "Bob", "make_bob"], &[]);
}

#[test]
fn test_method_pass_pod_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Anna"]);
}

fn perform_asan_doom_test(into_raw: TokenStream, box_type: TokenStream) {
    if std::env::var_os("AUTOCXX_ASAN").is_none() {
        return;
    }
    // Testing that we get an asan fail when it's enabled.
    // Really just testing our CI is working to spot ASAN mistakes.
    let hdr = indoc! {"
        #include <cstddef>
        struct A {
            int a;
        };
        inline size_t how_big_is_a() {
            return sizeof(A);
        }
    "};
    let rs = quote! {
        let a = #box_type::emplace(ffi::A::new());
        unsafe {
            let a_raw = #into_raw;
            // Intentional memory unsafety. Don't @ me.
            let a_offset_into_doom = a_raw.offset(ffi::how_big_is_a().try_into().unwrap());
            a_offset_into_doom.write_bytes(0x69, 1);
            #box_type::from_raw(a_raw); // to delete. If we haven't yet crashed.
        }
    };
    run_test_expect_fail("", hdr, rs, &["A", "how_big_is_a"], &[]);
}

#[test]
fn test_asan_working_as_expected_for_cpp_allocations() {
    perform_asan_doom_test(quote! { a.into_raw() }, quote! { UniquePtr })
}

#[test]
fn test_asan_working_as_expected_for_rust_allocations() {
    perform_asan_doom_test(
        quote! { Box::into_raw(core::pin::Pin::into_inner_unchecked(a)) },
        quote! { Box },
    )
}

#[test]
fn test_inline_method() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna) const {
                return a;
            }
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test("", hdr, rs, &[], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_pod_by_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(const Anna&) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(const Anna& a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(&a), 12);
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_pod_by_mut_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna&) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna& a) const;
        };
    "};
    let rs = quote! {
        let mut a = Box::pin(ffi::Anna { a: 14 });
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a.as_mut()), 12);
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_pod_by_up() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(std::unique_ptr<Anna>) const {
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Anna {
            uint32_t a;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(std::unique_ptr<Anna> z) const;
        };
    "};
    let rs = quote! {
        let a = ffi::Anna { a: 14 };
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(cxx::UniquePtr::new(a)), 12);
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Anna"]);
}

#[test]
fn test_method_pass_nonpod_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna().within_box();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["Anna", "give_anna"], &["Bob"]);
}

#[test]
fn test_pass_two_nonpod_by_value() {
    let cxx = indoc! {"
        void take_a(A, A) {
        }
    "};
    let hdr = indoc! {"
        #include <string>
        struct A {
            std::string b;
        };
        void take_a(A, A);
    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        let a2 = ffi::A::new().within_unique_ptr();
        ffi::take_a(a, a2);
    };
    run_test(cxx, hdr, rs, &["A", "take_a"], &[]);
}

#[test]
fn test_issue_931() {
    let cxx = "";
    let hdr = indoc! {"
    namespace a {
        struct __cow_string {
          __cow_string();
        };
        class b {
        public:
          __cow_string c;
        };
        class j {
        public:
          b d;
        };
        template <typename> class e;
        } // namespace a
        template <typename> struct f {};
        namespace llvm {
        template <class> class g {
          union {
            f<a::j> h;
          };
        };
        class MemoryBuffer {
        public:
          g<a::e<MemoryBuffer>> i;
        };
        } // namespace llvm
    "};
    let rs = quote! {};
    run_test(cxx, hdr, rs, &["llvm::MemoryBuffer"], &[]);
}

#[test]
fn test_issue_936() {
    let cxx = "";
    let hdr = indoc! {"
    struct a;
    class B {
    public:
        B(a &, bool);
    };
    "};
    let rs = quote! {};
    run_test(cxx, hdr, rs, &["B"], &[]);
}

#[test]
fn test_method_pass_nonpod_by_value_with_up() {
    // Checks that existing UniquePtr params are not wrecked
    // by the conversion we do here.
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna, std::unique_ptr<Anna>) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna a, std::unique_ptr<Anna>) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna().within_unique_ptr();
        let a2 = ffi::give_anna().within_unique_ptr();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a, a2), 12);
    };
    run_test(cxx, hdr, rs, &["Anna", "give_anna"], &["Bob"]);
}

#[test]
fn test_issue_940() {
    let cxx = "";
    let hdr = indoc! {"
    template <class> class b;
    template <class = void> struct c;
    struct identity;
    template <class, class, class e, class> class f {
    using g = e;
    g h;
    };
    template <class i, class k = c<>, class l = b<i>>
    using j = f<i, identity, k, l>;
    class n;
    class RenderFrameHost {
    public:
    virtual void o(const j<n> &);
    virtual ~RenderFrameHost() {}
    };
    "};
    let rs = quote! {};
    run_test_ex(
        cxx,
        hdr,
        rs,
        directives_from_lists(&["RenderFrameHost"], &[], None),
        make_cpp17_adder(),
        None,
        None,
    );
}

#[test]
fn test_method_pass_nonpod_by_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(const Anna&) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(const Anna& a) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna().within_box();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a.as_ref().get_ref()), 12);
    };
    run_test(cxx, hdr, rs, &["Anna", "give_anna"], &["Bob"]);
}

#[test]
fn test_method_pass_nonpod_by_mut_reference() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(Anna&) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(Anna& a) const;
        };
    "};
    let rs = quote! {
        let mut a = ffi::give_anna().within_unique_ptr();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a.as_mut().unwrap()), 12);
    };
    run_test(cxx, hdr, rs, &["Anna", "give_anna"], &["Bob"]);
}

#[test]
fn test_method_pass_nonpod_by_up() {
    let cxx = indoc! {"
        uint32_t Bob::get_bob(std::unique_ptr<Anna>) const {
            return a;
        }
        Anna give_anna() {
            Anna a;
            a.a = 10;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        Anna give_anna();
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t get_bob(std::unique_ptr<Anna> z) const;
        };
    "};
    let rs = quote! {
        let a = ffi::give_anna().within_unique_ptr();
        let b = ffi::Bob { a: 12, b: 13 };
        assert_eq!(b.get_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["give_anna"], &["Bob"]);
}

#[test]
fn test_method_return_nonpod_by_value() {
    let cxx = indoc! {"
        Anna Bob::get_anna() const {
            Anna a;
            a.a = 12;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Anna {
            uint32_t a;
            std::string b;
        };
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            Anna get_anna() const;
        };
    "};
    let rs = quote! {
        let b = ffi::Bob { a: 12, b: 13 };
        let a = b.get_anna().within_unique_ptr();
        assert!(!a.is_null());
    };
    run_test(cxx, hdr, rs, &["Anna"], &["Bob"]);
}

#[test]
fn test_pass_string_by_value() {
    let cxx = indoc! {"
        uint32_t measure_string(std::string z) {
            return z.length();
        }
        std::unique_ptr<std::string> get_msg() {
            return std::make_unique<std::string>(\"hello\");
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        uint32_t measure_string(std::string a);
        std::unique_ptr<std::string> get_msg();
    "};
    let rs = quote! {
        let a = ffi::get_msg();
        let c = ffi::measure_string(a);
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string", "get_msg"], &[]);
}

#[test]
fn test_return_string_by_value() {
    let cxx = indoc! {"
        std::string get_msg() {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <string>
        std::string get_msg();
    "};
    let rs = quote! {
        let a = ffi::get_msg();
        assert!(a.as_ref().unwrap() == "hello");
    };
    run_test(cxx, hdr, rs, &["get_msg"], &[]);
}

#[test]
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
fn test_method_pass_string_by_value() {
    let cxx = indoc! {"
        uint32_t Bob::measure_string(std::string z) const {
            return z.length();
        }
        std::unique_ptr<std::string> get_msg() {
            return std::make_unique<std::string>(\"hello\");
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        #include <memory>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            uint32_t measure_string(std::string a) const;
        };
        std::unique_ptr<std::string> get_msg();
    "};
    let rs = quote! {
        let a = ffi::get_msg();
        let b = ffi::Bob { a: 12, b: 13 };
        let c = b.measure_string(a);
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["Bob", "get_msg"], &["Bob"]);
}

#[test]
fn test_method_return_string_by_value() {
    let cxx = indoc! {"
        std::string Bob::get_msg() const {
            return \"hello\";
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
        public:
            uint32_t a;
            uint32_t b;
            std::string get_msg() const;
        };
    "};
    let rs = quote! {
        let b = ffi::Bob { a: 12, b: 13 };
        let a = b.get_msg();
        assert!(a.as_ref().unwrap() == "hello");
    };
    run_test(cxx, hdr, rs, &[], &["Bob"]);
}

#[test]
fn test_pass_rust_string_by_ref() {
    let cxx = indoc! {"
        uint32_t measure_string(const rust::String& z) {
            return std::string(z).length();
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <cxx.h>
        uint32_t measure_string(const rust::String& z);
    "};
    let rs = quote! {
        let c = ffi::measure_string(&"hello".to_string());
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

#[test]
fn test_pass_rust_string_by_value() {
    let cxx = indoc! {"
        uint32_t measure_string(rust::String z) {
            return std::string(z).length();
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <cxx.h>
        uint32_t measure_string(rust::String z);
    "};
    let rs = quote! {
        let c = ffi::measure_string("hello".into());
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

#[test]
fn test_pass_rust_str() {
    // passing by value is the only legal option
    let cxx = indoc! {"
        uint32_t measure_string(rust::Str z) {
            return std::string(z).length();
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <cxx.h>
        uint32_t measure_string(rust::Str z);
    "};
    let rs = quote! {
        let c = ffi::measure_string("hello");
        assert_eq!(c, 5);
    };
    run_test(cxx, hdr, rs, &["measure_string"], &[]);
}

#[test]
fn test_multiple_classes_with_methods() {
    let hdr = indoc! {"
        #include <cstdint>

        struct TrivialStruct {
            uint32_t val = 0;

            uint32_t get() const;
            uint32_t inc();
        };
        TrivialStruct make_trivial_struct();

        class TrivialClass {
          public:
            uint32_t get() const;
            uint32_t inc();

          private:
            uint32_t val_ = 1;
        };
        TrivialClass make_trivial_class();

        struct OpaqueStruct {
            // ~OpaqueStruct();
            uint32_t val = 2;

            uint32_t get() const;
            uint32_t inc();
        };
        OpaqueStruct make_opaque_struct();

        class OpaqueClass {
          public:
            // ~OpaqueClass();
            uint32_t get() const;
            uint32_t inc();

          private:
            uint32_t val_ = 3;
        };
        OpaqueClass make_opaque_class();
    "};
    let cxx = indoc! {"
        TrivialStruct make_trivial_struct() { return {}; }
        TrivialClass make_trivial_class() { return {}; }
        OpaqueStruct make_opaque_struct() { return {}; }
        OpaqueClass make_opaque_class() { return {}; }

        uint32_t TrivialStruct::get() const { return val;}
        uint32_t TrivialClass::get() const { return val_; }
        uint32_t OpaqueStruct::get() const { return val;}
        uint32_t OpaqueClass::get() const { return val_; }

        uint32_t TrivialStruct::inc() { return ++val; }
        uint32_t TrivialClass::inc() { return ++val_; }
        uint32_t OpaqueStruct::inc() { return ++val; }
        uint32_t OpaqueClass::inc() { return ++val_; }
    "};
    let rs = quote! {
        use ffi::*;

        let mut ts = Box::pin(make_trivial_struct());
        assert_eq!(ts.get(), 0);
        assert_eq!(ts.as_mut().inc(), 1);
        assert_eq!(ts.as_mut().inc(), 2);

        let mut tc = Box::pin(make_trivial_class());
        assert_eq!(tc.get(), 1);
        assert_eq!(tc.as_mut().inc(), 2);
        assert_eq!(tc.as_mut().inc(), 3);

        let mut os= make_opaque_struct().within_unique_ptr();
        assert_eq!(os.get(), 2);
        assert_eq!(os.pin_mut().inc(), 3);
        assert_eq!(os.pin_mut().inc(), 4);

        let mut oc = make_opaque_class().within_unique_ptr();
        assert_eq!(oc.get(), 3);
        assert_eq!(oc.pin_mut().inc(), 4);
        assert_eq!(oc.pin_mut().inc(), 5);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &[
            "make_trivial_struct",
            "make_trivial_class",
            "make_opaque_struct",
            "make_opaque_class",
            "OpaqueStruct",
            "OpaqueClass",
        ],
        &["TrivialStruct", "TrivialClass"],
    );
}

#[test]
fn test_ns_return_struct() {
    let cxx = indoc! {"
        A::B::Bob give_bob() {
            A::B::Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            namespace B {
                struct Bob {
                    uint32_t a;
                    uint32_t b;
                };
            }
        }
        A::B::Bob give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["A::B::Bob"]);
}

#[test]
fn test_ns_take_struct() {
    let cxx = indoc! {"
    uint32_t take_bob(A::B::Bob a) {
        return a.a;
    }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            namespace B {
                struct Bob {
                    uint32_t a;
                    uint32_t b;
                };
            }
        }
        uint32_t take_bob(A::B::Bob a);
    "};
    let rs = quote! {
        let a = ffi::A::B::Bob { a: 12, b: 13 };
        assert_eq!(ffi::take_bob(a), 12);
    };
    run_test(cxx, hdr, rs, &["take_bob"], &["A::B::Bob"]);
}

#[test]
fn test_ns_func() {
    let cxx = indoc! {"
        using namespace C;
        A::B::Bob C::give_bob() {
            A::B::Bob a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            namespace B {
                struct Bob {
                    uint32_t a;
                    uint32_t b;
                };
            }
        }
        namespace C {
            ::A::B::Bob give_bob();
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::C::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["C::give_bob"], &["A::B::Bob"]);
}

#[test]
fn test_overload_constructors() {
    let cxx = indoc! {"
        Bob::Bob() {}
        Bob::Bob(uint32_t _a) :a(_a) {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            Bob();
            Bob(uint32_t a);
            uint32_t a;
            uint32_t b;
        };
    "};
    let rs = quote! {
        ffi::Bob::new().within_unique_ptr();
        ffi::Bob::new1(32).within_unique_ptr();
    };
    run_test(cxx, hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_overload_functions() {
    let cxx = indoc! {"
        void daft(uint32_t) {}
        void daft(uint8_t) {}
        void daft(std::string) {}
        void daft(Fred) {}
        void daft(Norma) {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Fred {
            uint32_t a;
        };
        struct Norma {
            Norma() {}
            uint32_t a;
        };
        void daft(uint32_t);
        void daft(uint8_t);
        void daft(std::string);
        void daft(Fred);
        void daft(Norma);
    "};
    let rs = quote! {
        use ffi::ToCppString;
        ffi::daft(32);
        ffi::daft1(8);
        ffi::daft2("hello".into_cpp());
        let b = ffi::Fred { a: 3 };
        ffi::daft3(b);
        let c = ffi::Norma::new().within_unique_ptr();
        ffi::daft4(c);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["Norma", "daft", "daft1", "daft2", "daft3", "daft4"],
        &["Fred"],
    );
}

#[test]
#[ignore] // At present, bindgen generates two separate 'daft1'
          // functions here, and there's not much we can do about that.
fn test_overload_numeric_functions() {
    // Because bindgen deals with conflicting overloaded functions by
    // appending a numeric suffix, let's see if we can cope.
    let cxx = indoc! {"
        void daft1(uint32_t) {}
        void daft2(uint8_t) {}
        void daft(std::string) {}
        void daft(Fred) {}
        void daft(Norma) {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Fred {
            uint32_t a;
        };
        struct Norma {
            uint32_t a;
        };
        void daft1(uint32_t a);
        void daft2(uint8_t a);
        void daft(std::string a);
        void daft(Fred a);
        void daft(Norma a);
    "};
    let rs = quote! {
        use ffi::ToCppString;
        ffi::daft(32);
        ffi::daft1(8);
        ffi::daft2("hello".into_cpp());
        let b = ffi::Fred { a: 3 };
        ffi::daft3(b);
        let c = ffi::Norma::new().within_unique_ptr();
        ffi::daft4(c);
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["Norma", "daft", "daft1", "daft2", "daft3", "daft4"],
        &["Fred"],
    );
}

#[test]
fn test_overload_methods() {
    let cxx = indoc! {"
        void Bob::daft(uint32_t) const {}
        void Bob::daft(uint8_t) const {}
        void Bob::daft(std::string) const {}
        void Bob::daft(Fred) const {}
        void Bob::daft(Norma) const {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Fred {
            uint32_t a;
        };
        struct Norma {
            Norma() {}
            uint32_t a;
        };
        struct Bob {
            uint32_t a;
            void daft(uint32_t) const;
            void daft(uint8_t) const;
            void daft(std::string) const;
            void daft(Fred) const;
            void daft(Norma) const;
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        let a = ffi::Bob { a: 12 };
        a.daft(32);
        a.daft1(8);
        a.daft2("hello".into_cpp());
        let b = ffi::Fred { a: 3 };
        a.daft3(b);
        let c = ffi::Norma::new().within_unique_ptr();
        a.daft4(c);
    };
    run_test(cxx, hdr, rs, &["Norma"], &["Fred", "Bob"]);
}

#[test]
fn test_ns_constructor() {
    let cxx = indoc! {"
        A::Bob::Bob() {}
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                Bob();
                uint32_t a;
                uint32_t b;
            };
        }
    "};
    let rs = quote! {
        ffi::A::Bob::new().within_unique_ptr();
    };
    run_test(cxx, hdr, rs, &["A::Bob"], &[]);
}

#[test]
fn test_ns_up_direct() {
    let cxx = indoc! {"
        std::unique_ptr<A::Bob> A::get_bob() {
            A::Bob b;
            b.a = 2;
            b.b = 3;
            return std::make_unique<A::Bob>(b);
        }
        uint32_t give_bob(A::Bob bob) {
            return bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t b;
            };
            std::unique_ptr<Bob> get_bob();
        }
        uint32_t give_bob(A::Bob bob);
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob(ffi::A::get_bob()), 2);
    };
    run_test(cxx, hdr, rs, &["give_bob", "A::get_bob"], &[]);
}

#[test]
fn test_ns_up_wrappers() {
    let cxx = indoc! {"
        A::Bob get_bob() {
            A::Bob b;
            b.a = 2;
            b.b = 3;
            return b;
        }
        uint32_t give_bob(A::Bob bob) {
            return bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t b;
            };
        }
        A::Bob get_bob();
        uint32_t give_bob(A::Bob bob);
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob(as_new(ffi::get_bob())), 2);
    };
    run_test(cxx, hdr, rs, &["give_bob", "get_bob"], &[]);
}

#[test]
fn test_ns_up_wrappers_in_up() {
    let cxx = indoc! {"
        A::Bob A::get_bob() {
            A::Bob b;
            b.a = 2;
            b.b = 3;
            return b;
        }
        uint32_t give_bob(A::Bob bob) {
            return bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t b;
            };
            Bob get_bob();
        }
        uint32_t give_bob(A::Bob bob);
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob(as_new(ffi::A::get_bob())), 2);
    };
    run_test(cxx, hdr, rs, &["give_bob", "A::get_bob"], &[]);
}

#[test]
fn test_return_reference() {
    let cxx = indoc! {"
        const Bob& give_bob(const Bob& input_bob) {
            return input_bob;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        const Bob& give_bob(const Bob& input_bob);
    "};
    let rs = quote! {
        let b = ffi::Bob { a: 3, b: 4 };
        assert_eq!(ffi::give_bob(&b).b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[test]
fn test_return_reference_non_pod() {
    let cxx = indoc! {"
        const Bob& give_bob(const Bob& input_bob) {
            return input_bob;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        namespace A {
            void give_bob(); // force wrapper generation
        }
        const Bob& give_bob(const Bob& input_bob);
    "};
    let rs = quote! {};
    run_test(cxx, hdr, rs, &["give_bob", "Bob", "A::give_bob"], &[]);
}

#[test]
fn test_return_reference_non_pod_string() {
    let cxx = indoc! {"
        const std::string& give_bob(const Bob& input_bob) {
            return input_bob.a;
        }
    "};
    let hdr = indoc! {"
        #include <string>
        struct Bob {
            std::string a;
        };
       // namespace A {
       //     void give_bob(); // force wrapper generation
       // }
        const std::string& give_bob(const Bob& input_bob);
    "};
    let rs = quote! {};
    run_test(cxx, hdr, rs, &["give_bob", "Bob"], &[]);
}

#[test]
fn test_member_return_reference() {
    let hdr = indoc! {"
        #include <string>
        class A {
        public:
            virtual const std::string& get_str() { return a; }
            virtual ~A() {}
            std::string a;
        };
    "};
    let rs = quote! {
        let mut b = ffi::A::new().within_unique_ptr();
        b.pin_mut().get_str();
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_destructor() {
    let hdr = indoc! {"
        struct WithDtor {
            ~WithDtor();
        };
        WithDtor make_with_dtor();
    "};
    let cxx = indoc! {"
        WithDtor::~WithDtor() {}
        WithDtor make_with_dtor() {
            return {};
        }
    "};
    let rs = quote! {
        use ffi::*;
        let with_dtor: cxx::UniquePtr<WithDtor> = make_with_dtor().within_unique_ptr();
        drop(with_dtor);
    };
    run_test(cxx, hdr, rs, &["WithDtor", "make_with_dtor"], &[]);
}

#[test]
fn test_nested_with_destructor() {
    // Regression test, naming the destructor in the generated C++ is a bit tricky.
    let hdr = indoc! {"
        struct A {
            struct B {
                B() = default;
                ~B() = default;
            };
        };
    "};
    let rs = quote! {
        ffi::A_B::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["A", "A_B"], &[]);
}

// Even without a `safety!`, we still need to generate a safe `fn drop`.
#[test]
fn test_destructor_no_safety() {
    let hdr = indoc! {"
        struct WithDtor {
            ~WithDtor();
        };
    "};
    let cxx = indoc! {"
        WithDtor::~WithDtor() {}
    "};
    let hexathorpe = Token![#](Span::call_site());
    let unexpanded_rust = quote! {
        use autocxx::prelude::*;

        include_cpp!(
            #hexathorpe include "input.h"
            generate!("WithDtor")
        );

        fn main() {}
    };

    do_run_test_manual(cxx, hdr, unexpanded_rust, None, None).unwrap();
}

#[test]
fn test_static_func() {
    let hdr = indoc! {"
        #include <cstdint>
        struct WithStaticMethod {
            static uint32_t call();
        };
    "};
    let cxx = indoc! {"
        uint32_t WithStaticMethod::call() {
            return 42;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::WithStaticMethod::call(), 42);
    };
    run_test(cxx, hdr, rs, &["WithStaticMethod"], &[]);
}

#[test]
fn test_static_func_wrapper() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct A {
            std::string a;
            static A CreateA(std::string a, std::string) {
                A c;
                c.a = a;
                return c;
            }
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        ffi::A::CreateA("a".into_cpp(), "b".into_cpp());
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_give_pod_typedef_by_value() {
    let cxx = indoc! {"
        Horace give_bob() {
            Horace a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
        Horace give_bob();
    "};
    let rs = quote! {
        assert_eq!(ffi::give_bob().b, 4);
    };
    run_test(cxx, hdr, rs, &["give_bob"], &["Bob"]);
}

#[ignore] // because we need to put some aliases in the output ffi mod.
#[test]
fn test_use_pod_typedef() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
    "};
    let rs = quote! {
        let h = Horace { a: 3, b: 4 };
        assert_eq!(h.b, 4);
    };
    run_test(cxx, hdr, rs, &[], &["Bob"]);
}

#[test]
fn test_typedef_to_ns() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            template<typename T>
            struct C {
                T* t;
            };
            typedef C<char> B;
        }
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A::B"], &[]);
}

#[ignore] // we don't yet allow typedefs to be listed in allow_pod
#[test]
fn test_use_pod_typedef_with_allowpod() {
    let cxx = indoc! {"
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
    "};
    let rs = quote! {
        let h = Horace { a: 3, b: 4 };
        assert_eq!(h.b, 4);
    };
    run_test(cxx, hdr, rs, &[], &["Horace"]);
}

#[test]
fn test_give_nonpod_typedef_by_value() {
    let cxx = indoc! {"
        Horace give_bob() {
            Horace a;
            a.a = 3;
            a.b = 4;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t b;
        };
        using Horace = Bob;
        Horace give_bob();
        inline uint32_t take_horace(const Horace& horace) { return horace.b; }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_horace(&moveit!(ffi::give_bob())), 4);
    };
    run_test(cxx, hdr, rs, &["give_bob", "take_horace"], &[]);
}

#[test]
fn test_conflicting_static_functions() {
    let cxx = indoc! {"
        Bob Bob::create() { Bob a; return a; }
        Fred Fred::create() { Fred b; return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            Bob() : a(0) {}
            uint32_t a;
            static Bob create();
        };
        struct Fred {
            Fred() : b(0) {}
            uint32_t b;
            static Fred create();
        };
    "};
    let rs = quote! {
        ffi::Bob::create();
        ffi::Fred::create();
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Fred"]);
}

#[test]
fn test_conflicting_ns_up_functions() {
    let cxx = indoc! {"
        uint32_t A::create(C) { return 3; }
        uint32_t B::create(C) { return 4; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct C {
            C() {}
            uint32_t a;
        };
        namespace A {
            uint32_t create(C c);
        };
        namespace B {
            uint32_t create(C c);
        };
    "};
    let rs = quote! {
        let c = ffi::C::new().within_unique_ptr();
        let c2 = ffi::C::new().within_unique_ptr();
        assert_eq!(ffi::A::create(c), 3);
        assert_eq!(ffi::B::create(c2), 4);
    };
    run_test(cxx, hdr, rs, &["A::create", "B::create", "C"], &[]);
}

#[test]
fn test_conflicting_methods() {
    let cxx = indoc! {"
        uint32_t Bob::get() const { return a; }
        uint32_t Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
            uint32_t get() const;
        };
        struct Fred {
            uint32_t b;
            uint32_t get() const;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 10 };
        let b = ffi::Fred { b: 20 };
        assert_eq!(a.get(), 10);
        assert_eq!(b.get(), 20);
    };
    run_test(cxx, hdr, rs, &[], &["Bob", "Fred"]);
}

#[test]
// There's a bindgen bug here. bindgen generates
// functions called 'get' and 'get1' but then generates impl
// blocks which call 'get' and 'get'. By luck, we currently
// should not be broken by this, but at some point we should take
// the time to create a minimal bindgen test case and submit it
// as a bindgen bug.
fn test_conflicting_up_wrapper_methods_not_in_ns() {
    // Ensures the two names 'get' do not conflict in the flat
    // cxx::bridge mod namespace.
    let cxx = indoc! {"
        Bob::Bob() : a(\"hello\") {}
        Fred::Fred() : b(\"goodbye\") {}
        std::string Bob::get() const { return a; }
        std::string Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            Bob();
            std::string a;
            std::string get() const;
        };
        struct Fred {
            Fred();
            std::string b;
            std::string get() const;
        };
    "};
    let rs = quote! {
        let a = ffi::Bob::new().within_unique_ptr();
        let b = ffi::Fred::new().within_unique_ptr();
        assert_eq!(a.get().as_ref().unwrap().to_str().unwrap(), "hello");
        assert_eq!(b.get().as_ref().unwrap().to_str().unwrap(), "goodbye");
    };
    run_test(cxx, hdr, rs, &["Bob", "Fred"], &[]);
}

#[test]
fn test_conflicting_methods_in_ns() {
    let cxx = indoc! {"
        uint32_t A::Bob::get() const { return a; }
        uint32_t B::Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
                uint32_t get() const;
            };
        }
        namespace B {
            struct Fred {
                uint32_t b;
                uint32_t get() const;
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 10 };
        let b = ffi::B::Fred { b: 20 };
        assert_eq!(a.get(), 10);
        assert_eq!(b.get(), 20);
    };
    run_test(cxx, hdr, rs, &[], &["A::Bob", "B::Fred"]);
}

#[test]
fn test_conflicting_up_wrapper_methods_in_ns() {
    let cxx = indoc! {"
        A::Bob::Bob() : a(\"hello\") {}
        B::Fred::Fred() : b(\"goodbye\") {}
        std::string A::Bob::get() const { return a; }
        std::string B::Fred::get() const { return b; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        namespace A {
            struct Bob {
                Bob();
                std::string a;
                std::string get() const;
            };
        }
        namespace B {
            struct Fred {
                Fred();
                std::string b;
                std::string get() const;
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::new().within_unique_ptr();
        let b = ffi::B::Fred::new().within_unique_ptr();
        assert_eq!(a.get().as_ref().unwrap().to_str().unwrap(), "hello");
        assert_eq!(b.get().as_ref().unwrap().to_str().unwrap(), "goodbye");
    };
    run_test(cxx, hdr, rs, &["A::Bob", "B::Fred"], &[]);
}

#[test]
fn test_ns_struct_pod_request() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
    "};
    let rs = quote! {
        ffi::A::Bob { a: 12 };
    };
    run_test("", hdr, rs, &[], &["A::Bob"]);
}

#[test]
fn test_conflicting_ns_funcs() {
    let cxx = indoc! {"
        uint32_t A::get() { return 10; }
        uint32_t B::get() { return 20; }
    "};
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            uint32_t get();
        }
        namespace B {
            uint32_t get();
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::A::get(), 10);
        assert_eq!(ffi::B::get(), 20);
    };
    run_test(cxx, hdr, rs, &["A::get", "B::get"], &[]);
}

#[ignore]
// because currently we feed a flat namespace to cxx
// This would be relatively easy to enable now that we have the facility
// to add aliases to the 'use' statements we generate, plus
// bridge_name_tracker to pick a unique name. TODO.
#[test]
fn test_conflicting_ns_structs() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct Bob {
                uint32_t a;
            };
        }
    "};
    let rs = quote! {
        ffi::A::Bob { a: 12 };
        ffi::B::Bob { b: 12 };
    };
    run_test("", hdr, rs, &[], &["A::Bob", "B::Bob"]);
}

#[test]
fn test_make_string() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        let a = "hello".into_cpp();
        assert_eq!(a.to_str().unwrap(), "hello");
    };
    run_test("", hdr, rs, &["Bob"], &[]);
}

#[test]
fn test_string_make_unique() {
    let hdr = indoc! {"
        #include <string>
        inline void take_string(const std::string*) {};
    "};
    let rs = quote! {
        let s = ffi::make_string("");
        unsafe { ffi::take_string(s.as_ref().unwrap()) };
    };
    run_test("", hdr, rs, &["take_string"], &[]);
}

#[test]
fn test_string_constant() {
    let hdr = indoc! {"
        #include <cstdint>
        const char* STRING = \"Foo\";
    "};
    let rs = quote! {
        let a = core::str::from_utf8(ffi::STRING).unwrap().trim_end_matches(char::from(0));
        assert_eq!(a, "Foo");
    };
    run_test("", hdr, rs, &["STRING"], &[]);
}

#[test]
fn test_string_let_cxx_string() {
    let hdr = indoc! {"
        #include <string>
        inline void take_string(const std::string&) {};
    "};
    let rs = quote! {
        autocxx::cxx::let_cxx_string!(s = "hello");
        ffi::take_string(&s);
    };
    run_test("", hdr, rs, &["take_string"], &[]);
}

#[test]
fn test_pod_constant_harmless_inside_type() {
    // Check that the presence of this constant doesn't break anything.
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        struct Anna {
            uint32_t a;
            const Bob BOB = Bob { 10 };
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["Anna"]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/93
fn test_pod_constant() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        const Bob BOB = Bob { 10 };
    "};
    let rs = quote! {
        let a = &ffi::BOB;
        assert_eq!(a.a, 10);
    };
    run_test("", hdr, rs, &["BOB"], &["Bob"]);
}

#[test]
fn test_pod_static_harmless_inside_type() {
    // Check that the presence of this constant doesn't break anything.
    // Remove this test when the following one is enabled.
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        struct Anna {
            uint32_t a;
            static Bob BOB;
        };
        Bob Anna::BOB = Bob { 10 };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["Anna"]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/93
fn test_pod_static() {
    let hdr = indoc! {"
        #include <cstdint>
        struct Bob {
            uint32_t a;
        };
        static Bob BOB = Bob { 10 };
    "};
    let rs = quote! {
        let a = &ffi::BOB;
        assert_eq!(a.a, 10);
    };
    run_test("", hdr, rs, &["BOB"], &["Bob"]);
}

#[test]
#[ignore] // this probably requires code generation on the C++
          // side. It's not at all clear how best to handle this.
fn test_non_pod_constant() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        struct Bob {
            std::string a;
            std::string get() { return a };
        };
        const Bob BOB = Bob { \"hello\" };
    "};
    let rs = quote! {
        let a = ffi::BOB;
        // following line assumes that 'a' is a &Bob
        // but who knows how we'll really do this.
        assert_eq!(a.get().as_ref().unwrap().to_str().unwrap(), "hello");
    };
    run_test("", hdr, rs, &["BOB"], &[]);
}

#[test]
fn test_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            const STRING_TYPE* ptr_;
            size_t length_;
        };
        typedef BasicStringPiece<uint8_t> StringPiece;

        struct Origin {
            Origin() {}
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_struct_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        struct Concrete {
            uint8_t a;
        };
        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            const STRING_TYPE* ptr_;
            size_t length_;
        };
        typedef BasicStringPiece<Concrete> StringPiece;

        struct Origin {
            Origin() {}
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_enum_typedef() {
    let hdr = indoc! {"
        enum ConstraintSolverParameters_TrailCompression : int {
            ConstraintSolverParameters_TrailCompression_NO_COMPRESSION = 0,
            ConstraintSolverParameters_TrailCompression_COMPRESS_WITH_ZLIB = 1
        };
        typedef ConstraintSolverParameters_TrailCompression TrailCompression;
    "};
    let rs = quote! {
        let _ = ffi::TrailCompression::ConstraintSolverParameters_TrailCompression_NO_COMPRESSION;
    };
    run_test("", hdr, rs, &["TrailCompression"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/264
fn test_conflicting_usings() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <cstddef>
        typedef size_t diff;
        struct A {
            using diff = diff;
            diff a;
        };
        struct B {
            using diff = diff;
            diff a;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["A", "B"]);
}

#[test]
fn test_conflicting_usings_with_self_declaration1() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <cstddef>
        struct common_params {
            using difference_type = ptrdiff_t;
        };
        template <typename Params>
        class btree_node {
            public:
            using difference_type = typename Params::difference_type;
            Params params;
        };
        template <typename Tree>
        class btree_container {
            public:
            using difference_type = typename Tree::difference_type;
            void clear() {}
            Tree b;
            uint32_t a;
        };
        typedef btree_container<btree_node<common_params>> my_tree;
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["my_tree"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/106
fn test_string_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            const STRING_TYPE* ptr_;
            size_t length_;
        };
        typedef BasicStringPiece<std::string> StringPiece;

        struct Origin {
            Origin() {}
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_associated_type_problem() {
    // Regression test for a potential bindgen bug
    let hdr = indoc! {"
        namespace a {
        template <typename> class b {};
        } // namespace a
        class bl {
        public:
          a::b<bl> bm;
        };
        struct B {
            int a;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_two_type_constructors() {
    // https://github.com/google/autocxx/issues/877
    let hdr = indoc! {"
        struct A {
            int a;
        };
        struct B {
            int B;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[ignore] // https://github.com/rust-lang/rust-bindgen/issues/1924
#[test]
fn test_associated_type_templated_typedef_in_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            typedef size_t size_type;
            typedef typename STRING_TYPE::value_type value_type;
            const value_type* ptr_;
            size_type length_;
        };

        typedef BasicStringPiece<std::string> StringPiece;

        struct Origin {
            // void SetHost(StringPiece host);
            StringPiece host;
        };
    "};
    let rs = quote! {
        ffi::Origin::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["Origin"], &[]);
}

#[test]
fn test_associated_type_templated_typedef() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            typedef size_t size_type;
            typedef typename STRING_TYPE::value_type value_type;
            const value_type* ptr_;
            size_type length_;
        };

        typedef BasicStringPiece<std::string> StringPiece;

        struct Container {
            Container() {}
            const StringPiece& get_string_piece() const { return sp; }
            StringPiece sp;
        };

        inline void take_string_piece(const StringPiece&) {}
    "};
    let rs = quote! {
        let sp = ffi::Container::new().within_box();
        ffi::take_string_piece(sp.get_string_piece());
    };
    run_test("", hdr, rs, &["take_string_piece", "Container"], &[]);
}

#[test]
fn test_associated_type_templated_typedef_by_value_regular() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            BasicStringPiece() : ptr_(nullptr), length_(0) {}
            typedef size_t size_type;
            typedef typename STRING_TYPE::value_type value_type;
            const value_type* ptr_;
            size_type length_;
        };

        typedef BasicStringPiece<std::string> StringPiece;

        inline StringPiece give_string_piece() {
            StringPiece s;
            return s;
        }
        inline void take_string_piece(StringPiece) {}
    "};
    let rs = quote! {
        let sp = ffi::give_string_piece();
        ffi::take_string_piece(sp);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            generate!("take_string_piece")
            generate!("give_string_piece")
            instantiable!("StringPiece")
        },
        None,
        None,
        None,
    );
}

#[test]
fn test_associated_type_templated_typedef_by_value_forward_declaration() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>

        template <typename STRING_TYPE> class BasicStringPiece;

        typedef BasicStringPiece<std::string> StringPiece;

        struct Container {
            StringPiece give_string_piece() const;
            void take_string_piece(StringPiece string_piece) const;
            const StringPiece& get_string_piece() const;
            uint32_t b;
        };

        inline void take_string_piece_by_ref(const StringPiece&) {}
    "};
    let cpp = indoc! {"
        template <typename STRING_TYPE> class BasicStringPiece {
        public:
            BasicStringPiece() : ptr_(nullptr), length_(0) {}
            typedef size_t size_type;
            typedef typename STRING_TYPE::value_type value_type;
            const value_type* ptr_;
            size_type length_;
        };

        StringPiece Container::give_string_piece() const {
            StringPiece s;
            return s;
        }
        void Container::take_string_piece(StringPiece) const {}

        StringPiece a;

        const StringPiece& Container::get_string_piece() const {
            return a;
        }
    "};
    // As this template is forward declared we shouldn't be able to pass it by
    // value, but we still want to be able to use it by reference.
    let rs = quote! {
        let cont = ffi::Container::new().within_box();
        ffi::take_string_piece_by_ref(cont.as_ref().get_string_piece());
    };
    run_test(
        cpp,
        hdr,
        rs,
        &["take_string_piece_by_ref", "Container"],
        &[],
    );
}

#[test]
fn test_remove_cv_t_pathological() {
    let hdr = indoc! {"
        template <class _Ty>
        struct remove_cv {
            using type = _Ty;
            template <template <class> class _Fn>
            using _Apply = _Fn<_Ty>;
        };

        template <class _Ty>
        struct remove_cv<const _Ty> {
            using type = _Ty;
            template <template <class> class _Fn>
            using _Apply = const _Fn<_Ty>;
        };

        template <class _Ty>
        struct remove_cv<volatile _Ty> {
            using type = _Ty;
            template <template <class> class _Fn>
            using _Apply = volatile _Fn<_Ty>;
        };

        template <class _Ty>
        struct remove_cv<const volatile _Ty> {
            using type = _Ty;
            template <template <class> class _Fn>
            using _Apply = const volatile _Fn<_Ty>;
        };

        template <class _Ty>
        using remove_cv_t = typename remove_cv<_Ty>::type;

        template <class _Ty>
        struct remove_reference {
            using type                 = _Ty;
            using _Const_thru_ref_type = const _Ty;
        };

        template <class _Ty>
        struct remove_reference<_Ty&> {
            using type                 = _Ty;
            using _Const_thru_ref_type = const _Ty&;
        };

        template <class _Ty>
        struct remove_reference<_Ty&&> {
            using type                 = _Ty;
            using _Const_thru_ref_type = const _Ty&&;
        };

        template <class _Ty>
        using remove_reference_t = typename remove_reference<_Ty>::type;

        template <class _Ty>
        using _Remove_cvref_t = remove_cv_t<remove_reference_t<_Ty>>;
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_foreign_ns_func_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            inline uint32_t daft(A::Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 12 };
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["A::Bob"]);
}

#[test]
fn test_foreign_ns_func_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
                Bob(uint32_t _a) :a(_a) {}
            };
        }
        namespace B {
            inline uint32_t daft(A::Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::new(12).within_unique_ptr();
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft", "A::Bob"], &[]);
}

#[test]
fn test_foreign_ns_meth_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(A::Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 12 };
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &[], &["A::Bob", "B::C"]);
}

#[test]
fn test_foreign_ns_meth_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
                Bob(uint32_t _a) :a(_a) {}
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(A::Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::new(12).within_unique_ptr();
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &["A::Bob"], &["B::C"]);
}

#[test]
fn test_foreign_ns_cons_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                C(const A::Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob { a: 12 };
        let b = ffi::B::C::new(&a).within_unique_ptr();
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &[], &["B::C", "A::Bob"]);
}

#[test]
fn test_foreign_ns_cons_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                Bob(uint32_t _a) :a(_a) {}
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                C(const A::Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::A::Bob::new(12).within_unique_ptr();
        let b = ffi::B::C::new(&a).within_unique_ptr();
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &["A::Bob"], &["B::C"]);
}

#[test]
fn test_foreign_ns_func_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            inline A::Bob daft() { A::Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::B::daft().a, 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["A::Bob"]);
}

#[test]
fn test_foreign_ns_func_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            inline A::Bob daft() { A::Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        ffi::B::daft().within_box().as_ref();
    };
    run_test("", hdr, rs, &["B::daft", "A::Bob"], &[]);
}

#[test]
fn test_foreign_ns_meth_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                A::Bob daft() const { A::Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft().a, 12);
    };
    run_test("", hdr, rs, &[], &["A::Bob", "B::C"]);
}

#[test]
fn test_foreign_ns_meth_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        namespace A {
            struct Bob {
                uint32_t a;
            };
        }
        namespace B {
            struct C {
                uint32_t a;
                A::Bob daft() const { A::Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 14 };
        b.daft().within_unique_ptr().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["A::Bob"], &["B::C"]);
}

#[test]
fn test_root_ns_func_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            inline uint32_t daft(Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12 };
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["Bob"]);
}

#[test]
fn test_root_ns_func_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
            Bob(uint32_t _a) :a(_a) {}
        };
        namespace B {
            inline uint32_t daft(Bob a) { return a.a; }
        }
    "};
    let rs = quote! {
        let a = ffi::Bob::new(12).within_unique_ptr();
        assert_eq!(ffi::B::daft(a), 12);
    };
    run_test("", hdr, rs, &["B::daft", "Bob"], &[]);
}

#[test]
fn test_root_ns_meth_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12 };
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &[], &["Bob", "B::C"]);
}

#[test]
fn test_root_ns_meth_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
            Bob(uint32_t _a) :a(_a) {}
        };
        namespace B {
            struct C {
                uint32_t a;
                uint32_t daft(Bob a) const { return a.a; }
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob::new(12).within_unique_ptr();
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft(a), 12);
    };
    run_test("", hdr, rs, &["Bob"], &["B::C"]);
}

#[test]
fn test_root_ns_cons_arg_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                C(const Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob { a: 12 };
        let b = ffi::B::C::new(&a).within_unique_ptr();
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &[], &["B::C", "Bob"]);
}

#[test]
fn test_root_ns_cons_arg_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            Bob(uint32_t _a) :a(_a) {}
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                C(const Bob& input) : a(input.a) {}
            };
        }
    "};
    let rs = quote! {
        let a = ffi::Bob::new(12).within_unique_ptr();
        let b = ffi::B::C::new(&a).within_unique_ptr();
        assert_eq!(b.as_ref().unwrap().a, 12);
    };
    run_test("", hdr, rs, &["Bob"], &["B::C"]);
}

#[test]
fn test_root_ns_func_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            inline Bob daft() { Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::B::daft().a, 12);
    };
    run_test("", hdr, rs, &["B::daft"], &["Bob"]);
}

#[test]
fn test_root_ns_func_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            inline Bob daft() { Bob bob; bob.a = 12; return bob; }
        }
    "};
    let rs = quote! {
        ffi::B::daft().within_unique_ptr().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["B::daft", "Bob"], &[]);
}

#[test]
fn test_root_ns_meth_ret_pod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                Bob daft() const { Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 12 };
        assert_eq!(b.daft().a, 12);
    };
    run_test("", hdr, rs, &[], &["Bob", "B::C"]);
}

#[test]
fn test_root_ns_meth_ret_nonpod() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct Bob {
            uint32_t a;
        };
        namespace B {
            struct C {
                uint32_t a;
                Bob daft() const { Bob bob; bob.a = 12; return bob; }
            };
        }
    "};
    let rs = quote! {
        let b = ffi::B::C { a: 12 };
        b.daft().within_unique_ptr().as_ref().unwrap();
    };
    run_test("", hdr, rs, &["Bob"], &["B::C"]);
}

#[test]
fn test_forward_declaration() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        struct A;
        struct B {
            B() : a(0) {}
            uint32_t a;
            void daft(const A&) const {}
            static B daft3(const A&) { B b; return b; }
            A daft4();
            std::unique_ptr<A> daft5();
            const std::unique_ptr<A>& daft6();
        };
        A* get_a();
        void delete_a(A*);
    "};
    let cpp = indoc! {"
        struct A {
            A() : a(0) {}
            uint32_t a;
        };
        A* get_a() {
            return new A();
        }
        void delete_a(A* a) {
            delete a;
        }
        A B::daft4() {
            A a;
            return a;
        }
        std::unique_ptr<A> B::daft5() {
            return std::make_unique<A>();
        }
        std::unique_ptr<A> fixed;
        const std::unique_ptr<A>& B::daft6() {
            return fixed;
        }
    "};
    let rs = quote! {
        let b = ffi::B::new().within_unique_ptr();
        let a = ffi::get_a();
        b.daft(unsafe { a.as_ref().unwrap() });
        unsafe { ffi::delete_a(a) };
    };
    run_test(cpp, hdr, rs, &["B", "get_a", "delete_a"], &[]);
}

#[test]
fn test_ulong() {
    let hdr = indoc! {"
    inline unsigned long daft(unsigned long a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(autocxx::c_ulong(34)), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[cfg_attr(skip_windows_msvc_failing_tests, ignore)]
#[test]
fn test_typedef_to_ulong() {
    let hdr = indoc! {"
        typedef unsigned long fiddly;
        inline fiddly daft(fiddly a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(autocxx::c_ulong(34)), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_generate_typedef_to_ulong() {
    let hdr = indoc! {"
        #include <cstdint>
        typedef uint32_t fish_t;
    "};
    let rs = quote! {
        let _: ffi::fish_t;
    };
    run_test("", hdr, rs, &[], &["fish_t"]);
}

#[test]
fn test_ulong_method() {
    let hdr = indoc! {"
    class A {
        public:
        A() {};
        unsigned long daft(unsigned long a) const { return a; }
    };
    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        assert_eq!(a.as_ref().unwrap().daft(autocxx::c_ulong(34)), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_ulong_wrapped_method() {
    let hdr = indoc! {"
    #include <cstdint>
    struct B {
        B() {};
        uint32_t a;
    };
    class A {
        public:
        A() {};
        unsigned long daft(unsigned long a, B) const { return a; }
    };
    "};
    let rs = quote! {
        let b = ffi::B::new().within_unique_ptr();
        let a = ffi::A::new().within_unique_ptr();
        assert_eq!(a.as_ref().unwrap().daft(autocxx::c_ulong(34), b), autocxx::c_ulong(34));
    };
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_reserved_name() {
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t async(uint32_t a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::async_(34), 34);
    };
    run_test("", hdr, rs, &["async_"], &[]);
}

#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[cfg_attr(skip_windows_msvc_failing_tests, ignore)]
#[test]
fn test_nested_type() {
    // Test that we can import APIs that use nested types.
    // As a regression test, we also test that the nested type `A::B` doesn't conflict with the
    // top-level type `B`. This used to cause compile errors.
    let hdr = indoc! {"
        struct A {
            A() {}
            struct B {
                B() {}
            };
            enum C {};
            using D = int;
        };
        struct B {
            B() {}
            void method_on_top_level_type() const {}
        };
        void take_A_B(A::B);
        void take_A_C(A::C);
        void take_A_D(A::D);
    "};
    let rs = quote! {
        let _ = ffi::A::new().within_unique_ptr();
        let b = ffi::B::new().within_unique_ptr();
        b.as_ref().unwrap().method_on_top_level_type();
    };
    run_test("", hdr, rs, &["A", "B", "take_A_B", "take_A_C"], &[]);
}

#[test]
fn test_nested_type_in_namespace() {
    // Test that we can import APIs that use nested types in a namespace.
    // We can't make this part of the previous test as autocxx drops the
    // namespace, so `A::B` and `N::A::B` would be imported as the same
    // type.
    let hdr = indoc! {"
        namespace N {
            struct A {
                A() {}
                struct B {
                    B() {}
                };
            };
        };
        void take_A_B(N::A::B);
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["take_A_B"], &[]);
}

#[test]
fn test_nested_enum_in_namespace() {
    let hdr = indoc! {"
        namespace N {
            struct A {
                A() {}
                enum B {
                    C,
                    D,
                };
            };
        };
        void take_A_B(N::A::B);
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["take_A_B"], &[]);
}

#[test]
fn test_abstract_nested_type() {
    let hdr = indoc! {"
        namespace N {
            class A {
            public:
                A() {}
                class B {
                private:
                    B() {}
                public:
                    virtual ~B() {}
                    virtual void Foo() = 0;
                };
            };
        };
        void take_A_B(const N::A::B&);
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["take_A_B", "N::A_B"], &[]);
}

#[test]
fn test_nested_unnamed_enum() {
    let hdr = indoc! {"
        namespace N {
            struct A {
                enum {
                    LOW_VAL = 1,
                    HIGH_VAL = 1000,
                };
            };
        }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! { generate_ns!("N")},
        None,
        None,
        None,
    );
}

#[test]
fn test_nested_type_constructor() {
    let hdr = indoc! {"
        #include <string>
        class A {
        public:
            class B {
            public:
                B(const std::string&) {}
                int b;
            };
            int a;
        };
    "};
    let rs = quote! {
        ffi::A_B::new(&ffi::make_string("Hello")).within_unique_ptr();
    };
    run_test("", hdr, rs, &["A_B"], &[]);
}

#[test]
fn test_generic_type() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <string>
        template<typename TY>
        struct Container {
            Container(TY a_) : a(a_) {}
            TY a;
        };
        struct Secondary {
            Secondary() {}
            void take_a(const Container<char>) const {}
            void take_b(const Container<uint16_t>) const {}
            uint16_t take_c(std::string a) const { return 10 + a.size(); }
        };
    "};
    let rs = quote! {
        use ffi::ToCppString;
        let item = ffi::Secondary::new().within_unique_ptr();
        assert_eq!(item.take_c("hello".into_cpp()), 15)
    };
    run_test("", hdr, rs, &["Secondary"], &[]);
}

#[test]
fn test_cycle_generic_type() {
    let hdr = indoc! {"
        #include <cstdint>
        template<typename TY>
        struct Container {
            Container(TY a_) : a(a_) {}
            TY a;
        };
        inline Container<char> make_thingy() {
            Container<char> a('a');
            return a;
        }
        typedef Container<char> Concrete;
        inline uint32_t take_thingy(Concrete a) {
            return a.a;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_thingy(ffi::make_thingy()), 'a' as u32)
    };
    run_test("", hdr, rs, &["take_thingy", "make_thingy"], &[]);
}

#[test]
fn test_virtual_fns() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            A(uint32_t num) : b(num) {}
            virtual uint32_t foo(uint32_t a) { return a+1; };
            virtual ~A() {}
            uint32_t b;
        };
        class B: public A {
        public:
            B() : A(3), c(4) {}
            virtual uint32_t foo(uint32_t a) { return a+2; };
            uint32_t c;
        };
    "};
    let rs = quote! {
        let mut a = ffi::A::new(12).within_unique_ptr();
        assert_eq!(a.pin_mut().foo(2), 3);
        let mut b = ffi::B::new().within_unique_ptr();
        assert_eq!(b.pin_mut().foo(2), 4);
    };
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_const_virtual_fns() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            A(uint32_t num) : b(num) {}
            virtual uint32_t foo(uint32_t a) const { return a+1; };
            virtual ~A() {}
            uint32_t b;
        };
        class B: public A {
        public:
            B() : A(3), c(4) {}
            virtual uint32_t foo(uint32_t a) const { return a+2; };
            uint32_t c;
        };
    "};
    let rs = quote! {
        let a = ffi::A::new(12).within_unique_ptr();
        assert_eq!(a.foo(2), 3);
        let b = ffi::B::new().within_unique_ptr();
        assert_eq!(b.foo(2), 4);
    };
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/197
fn test_virtual_fns_inheritance() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            A(uint32_t num) : b(num) {}
            virtual uint32_t foo(uint32_t a) { return a+1; };
            virtual ~A() {}
            uint32_t b;
        };
        class B: public A {
        public:
            B() : A(3), c(4) {}
            uint32_t c;
        };
    "};
    let rs = quote! {
        let mut b = ffi::B::new().within_unique_ptr();
        assert_eq!(b.pin_mut().foo(2), 3);
    };
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_vector_cycle_up() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <vector>
        #include <memory>
        struct A {
            uint32_t a;
        };
        inline uint32_t take_vec(std::unique_ptr<std::vector<A>> many_as) {
            return many_as->size();
        }
        inline std::unique_ptr<std::vector<A>> get_vec() {
            auto items = std::make_unique<std::vector<A>>();
            items->push_back(A { 3 });
            items->push_back(A { 4 });
            return items;
        }
    "};
    let rs = quote! {
        let v = ffi::get_vec();
        assert_eq!(v.as_ref().unwrap().is_empty(), false);
        assert_eq!(ffi::take_vec(v), 2);
    };
    run_test("", hdr, rs, &["take_vec", "get_vec"], &[]);
}

#[test]
fn test_vector_cycle_bare() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <vector>
        struct A {
            uint32_t a;
        };
        inline uint32_t take_vec(std::vector<A> many_as) {
            return many_as.size();
        }
        inline std::vector<A> get_vec() {
            std::vector<A> items;
            items.push_back(A { 3 });
            items.push_back(A { 4 });
            return items;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_vec(ffi::get_vec()), 2);
    };
    run_test("", hdr, rs, &["take_vec", "get_vec"], &[]);
}

#[test]
fn test_cycle_up_of_vec() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <vector>
        #include <memory>
        struct A {
            uint32_t a;
        };
        inline std::unique_ptr<std::vector<A>> take_vec(std::unique_ptr<std::vector<A>> a) {
            return a;
        }
        inline std::unique_ptr<std::vector<A>> get_vec() {
            std::unique_ptr<std::vector<A>> items = std::make_unique<std::vector<A>>();
            items->push_back(A { 3 });
            items->push_back(A { 4 });
            return items;
        }
    "};
    let rs = quote! {
        ffi::take_vec(ffi::get_vec());
    };
    run_test("", hdr, rs, &["take_vec", "get_vec"], &[]);
}

#[test]
fn test_typedef_to_std() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        typedef std::string my_string;
        inline uint32_t take_str(my_string a) {
            return a.size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_str("hello".into_cpp()), 5);
    };
    run_test("", hdr, rs, &["take_str"], &[]);
}

#[test]
fn test_typedef_to_up_in_fn_call() {
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        typedef std::unique_ptr<std::string> my_string;
        inline uint32_t take_str(my_string a) {
            return a->size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_str("hello".into_cpp()), 5);
    };
    run_test("", hdr, rs, &["take_str"], &[]);
}

#[test]
fn test_typedef_in_pod_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        typedef uint32_t my_int;
        struct A {
            my_int a;
        };
        inline uint32_t take_a(A a) {
            return a.a;
        }
    "};
    let rs = quote! {
        let a = ffi::A {
            a: 32,
        };
        assert_eq!(ffi::take_a(a), 32);
    };
    run_test("", hdr, rs, &["take_a"], &["A"]);
}

#[test]
fn test_cint_in_pod_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        struct A {
            int a;
        };
        inline uint32_t take_a(A a) {
            return a.a;
        }
    "};
    let rs = quote! {
        let a = ffi::A {
            a: 32,
        };
        assert_eq!(ffi::take_a(a), 32);
    };
    run_test("", hdr, rs, &["take_a"], &["A"]);
}

#[test]
fn test_string_in_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        struct A {
            std::string a;
        };
        inline A make_a(std::string b) {
            A bob;
            bob.a = b;
            return bob;
        }
        inline uint32_t take_a(A a) {
            return a.a.size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_a(as_new(ffi::make_a("hello".into_cpp()))), 5);
    };
    run_test("", hdr, rs, &["make_a", "take_a"], &[]);
}

#[test]
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
fn test_up_in_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        struct A {
            std::unique_ptr<std::string> a;
        };
        inline A make_a(std::string b) {
            A bob;
            bob.a = std::make_unique<std::string>(b);
            return bob;
        }
        inline uint32_t take_a(A a) {
            return a.a->size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_a(as_new(ffi::make_a("hello".into_cpp()))), 5);
    };
    run_test("", hdr, rs, &["make_a", "take_a"], &[]);
}

#[test]
fn test_typedef_to_std_in_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        typedef std::string my_string;
        struct A {
            my_string a;
        };
        inline A make_a(std::string b) {
            A bob;
            bob.a = b;
            return bob;
        }
        inline uint32_t take_a(A a) {
            return a.a.size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_a(as_new(ffi::make_a("hello".into_cpp()))), 5);
    };
    run_test("", hdr, rs, &["make_a", "take_a"], &[]);
}

#[test]
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
fn test_typedef_to_up_in_struct() {
    let hdr = indoc! {"
        #include <string>
        #include <memory>
        typedef std::unique_ptr<std::string> my_string;
        struct A {
            my_string a;
        };
        inline A make_a(std::string b) {
            A bob;
            bob.a = std::make_unique<std::string>(b);
            return bob;
        }
        inline uint32_t take_a(A a) {
            return a.a->size();
        }
    "};
    let rs = quote! {
        use ffi::ToCppString;
        assert_eq!(ffi::take_a(as_new(ffi::make_a("hello".into_cpp()))), 5);
    };
    run_test("", hdr, rs, &["make_a", "take_a"], &[]);
}

#[test]
fn test_float() {
    let hdr = indoc! {"
    inline float daft(float a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(34.0f32), 34.0f32);
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_double() {
    let hdr = indoc! {"
    inline double daft(double a) { return a; }
    "};
    let rs = quote! {
        assert_eq!(ffi::daft(34.0f64), 34.0f64);
    };
    run_test("", hdr, rs, &["daft"], &[]);
}

#[test]
fn test_issues_217_222() {
    let hdr = indoc! {"
    #include <string>
    #include <cstdint>
    #include <cstddef>

    template <typename STRING_TYPE> class BasicStringPiece {
        public:
         typedef size_t size_type;
         typedef typename STRING_TYPE::traits_type traits_type;
         typedef typename STRING_TYPE::value_type value_type;
         typedef const value_type* pointer;
         typedef const value_type& reference;
         typedef const value_type& const_reference;
         typedef ptrdiff_t difference_type;
         typedef const value_type* const_iterator;
         typedef std::reverse_iterator<const_iterator> const_reverse_iterator;
         static const size_type npos;
    };

    template<typename CHAR>
    class Replacements {
     public:
      Replacements() {
      }
      void SetScheme(const CHAR*) {
      }
      uint16_t a;
    };

    struct Component {
        uint16_t a;
    };

    template <typename STR>
    class StringPieceReplacements : public Replacements<typename STR::value_type> {
        private:
         using CharT = typename STR::value_type;
         using StringPieceT = BasicStringPiece<STR>;
         using ParentT = Replacements<CharT>;
         using SetterFun = void (ParentT::*)(const CharT*, const Component&);
         void SetImpl(SetterFun, StringPieceT) {
        }
        public:
        void SetSchemeStr(const CharT* str) { SetImpl(&ParentT::SetScheme, str); }
    };

    class GURL {
        public:
        typedef StringPieceReplacements<std::string> UrlReplacements;
        GURL() {}
        GURL ReplaceComponents(const Replacements<char>&) const {
            return GURL();
        }
        uint16_t a;
    };
    "};
    let rs = quote! {
        ffi::GURL::new().within_unique_ptr();
    };
    // The block! directives here are to avoid running into
    // https://github.com/rust-lang/rust-bindgen/pull/1975
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate!("GURL") block!("StringPiece") block!("Replacements") },
        None,
        None,
        None,
    );
}

#[test]
#[ignore] // https://github.com/rust-lang/rust-bindgen/pull/1975, https://github.com/google/autocxx/issues/106
fn test_dependent_qualified_type() {
    let hdr = indoc! {"
    #include <stddef.h>
    struct MyString {
        typedef char value_type;
    };
    template<typename T> struct MyStringView {
        typedef typename T::value_type view_value_type;
        const view_value_type* start;
        size_t length;
    };
    const char* HELLO = \"hello\";
    inline MyStringView<MyString> make_string_view() {
        MyStringView<MyString> r;
        r.start = HELLO;
        r.length = 2;
        return r;
    }
    inline size_t take_string_view(const MyStringView<MyString>& bit) {
        return bit.length;
    }
    "};
    let rs = quote! {
        let sv = ffi::make_string_view();
        assert_eq!(ffi::take_string_view(sv.as_ref().unwrap()), 2);
    };
    run_test("", hdr, rs, &["take_string_view", "make_string_view"], &[]);
}

#[test]
fn test_simple_dependent_qualified_type() {
    // bindgen seems to cope with this case just fine
    let hdr = indoc! {"
    #include <stddef.h>
    #include <stdint.h>
    struct MyString {
        typedef char value_type;
    };
    template<typename T> struct MyStringView {
        typedef typename T::value_type view_value_type;
        const view_value_type* start;
        size_t length;
    };
    typedef MyStringView<MyString>::view_value_type MyChar;
    inline MyChar make_char() {
        return 'a';
    }
    inline uint32_t take_char(MyChar c) {
        return static_cast<unsigned char>(c);
    }
    "};
    let rs = quote! {
        let c = ffi::make_char();
        assert_eq!(ffi::take_char(c), 97);
    };
    run_test("", hdr, rs, &["make_char", "take_char"], &[]);
}

#[test]
fn test_ignore_dependent_qualified_type() {
    let hdr = indoc! {"
    #include <stddef.h>
    struct MyString {
        typedef char value_type;
    };
    template<typename T> struct MyStringView {
        typedef typename T::value_type view_value_type;
        const view_value_type* start;
        size_t length;
    };
    MyStringView<MyString> make_string_view();
    struct B {
        B() {}
        inline size_t take_string_view(const MyStringView<MyString> bit) {
            return bit.length;
        }
    };
    "};
    let cpp = indoc! {"
    const char* HELLO = \"hello\";
    MyStringView<MyString> make_string_view() {
        MyStringView<MyString> r;
        r.start = HELLO;
        r.length = 2;
        return r;
    }
    "};
    let rs = quote! {
        ffi::B::new().within_unique_ptr();
    };
    run_test(cpp, hdr, rs, &["B"], &[]);
}

#[test]
fn test_ignore_dependent_qualified_type_reference() {
    let hdr = indoc! {"
    #include <stddef.h>
    struct MyString {
        typedef char value_type;
    };
    template<typename T> struct MyStringView {
        typedef typename T::value_type view_value_type;
        const view_value_type* start;
        size_t length;
    };
    MyStringView<MyString> make_string_view();
    struct B {
        B() {}
        inline size_t take_string_view(const MyStringView<MyString>& bit) {
            return bit.length;
        }
    };
    "};
    let cpp = indoc! {"
    const char* HELLO = \"hello\";
    MyStringView<MyString> make_string_view() {
        MyStringView<MyString> r;
        r.start = HELLO;
        r.length = 2;
        return r;
    }
    "};
    let rs = quote! {
        ffi::B::new().within_unique_ptr();
    };
    run_test(cpp, hdr, rs, &["B"], &[]);
}

#[test]
fn test_specialization() {
    let hdr = indoc! {"
    #include <stddef.h>
    #include <stdint.h>
    #include <string>
    #include <type_traits>

    template <typename T, bool = std::is_trivially_destructible<T>::value>
    struct OptionalStorageBase {
        T value_;
    };

    template <typename T,
    bool = std::is_trivially_copy_constructible<T>::value,
    bool = std::is_trivially_move_constructible<T>::value>
    struct OptionalStorage : OptionalStorageBase<T> {};

    template <typename T>
    struct OptionalStorage<T,
                       true /* trivially copy constructible */,
                       false /* trivially move constructible */>
    : OptionalStorageBase<T> {
    };

    template <typename T>
    struct OptionalStorage<T,
                       false /* trivially copy constructible */,
                       true /* trivially move constructible */>
    : OptionalStorageBase<T> {
    };

    template <typename T>
    struct OptionalStorage<T,
                       true /* trivially copy constructible */,
                       true /* trivially move constructible */>
    : OptionalStorageBase<T> {
    };

    template <typename T>
    class OptionalBase {
    private:
        OptionalStorage<T> storage_;
    };

    template <typename T>
    class Optional : public OptionalBase<T> {

    };

    struct B {
        B() {}
        void take_optional(Optional<std::string>) {}
        uint32_t a;
    };
    "};
    let rs = quote! {
        ffi::B::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_private_constructor_make_unique() {
    let hdr = indoc! {"
    #include <stdint.h>
    struct A {
    private:
        A() {};
    public:
        uint32_t a;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/266
fn test_take_array() {
    let hdr = indoc! {"
    #include <cstdint>
    uint32_t take_array(const uint32_t a[4]) {
        return a[0] + a[2];
    }
    "};
    let rs = quote! {
        let c: [u32; 4usize] = [ 10, 20, 30, 40 ];
        let c = c as *const [_];
        assert_eq!(ffi::take_array(&c), 40);
    };
    run_test("", hdr, rs, &["take_array"], &[]);
}

#[test]
fn test_take_array_in_struct() {
    let hdr = indoc! {"
    #include <cstdint>
    struct data {
        char a[4];
    };
    uint32_t take_array(const data a) {
        return a.a[0] + a.a[2];
    }
    "};
    let rs = quote! {
        let c = ffi::data { a: [ 10, 20, 30, 40 ] };
        assert_eq!(ffi::take_array(c), 40);
    };
    run_test("", hdr, rs, &["take_array"], &["data"]);
}

#[test]
fn test_take_struct_built_array_in_function() {
    let hdr = indoc! {"
    #include <cstdint>
    struct data {
        char a[4];
    };
    uint32_t take_array(char a[4]) {
        return a[0] + a[2];
    }
    "};
    let rs = quote! {
        let mut c = ffi::data { a: [ 10, 20, 30, 40 ] };
        unsafe {
            assert_eq!(ffi::take_array(c.a.as_mut_ptr()), 40);
        }
    };
    run_test("", hdr, rs, &["take_array"], &["data"]);
}

#[test]
fn test_take_array_in_function() {
    let hdr = indoc! {"
    #include <cstdint>
    uint32_t take_array(char a[4]) {
        return a[0] + a[2];
    }
    "};
    let rs = quote! {
        let mut a: [i8; 4] = [ 10, 20, 30, 40 ];
        unsafe {
            assert_eq!(ffi::take_array(a.as_mut_ptr()), 40);
        }
    };
    run_test("", hdr, rs, &["take_array"], &[]);
}

#[test]
fn test_union_ignored() {
    let hdr = indoc! {"
    #include <cstdint>
    union A {
        uint32_t a;
        float b;
    };
    struct B {
        B() :a(1) {}
        uint32_t take_union(A) const {
            return 3;
        }
        uint32_t get_a() const { return 2; }
        uint32_t a;
    };
    "};
    let rs = quote! {
        let b = ffi::B::new().within_unique_ptr();
        assert_eq!(b.get_a(), 2);
    };
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_union_nonpod() {
    let hdr = indoc! {"
    #include <cstdint>
    union A {
        uint32_t a;
        float b;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_union_pod() {
    let hdr = indoc! {"
    #include <cstdint>
    union A {
        uint32_t a;
        float b;
    };
    "};
    let rs = quote! {};
    run_test_expect_fail("", hdr, rs, &[], &["A"]);
}

#[test]
fn test_type_aliased_anonymous_union_ignored() {
    let hdr = indoc! {"
    #include <cstdint>
    namespace test {
        typedef union {
        int a;
        } Union;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["test::Union"], &[]);
}

#[test]
fn test_type_aliased_anonymous_struct_ignored() {
    let hdr = indoc! {"
    #include <cstdint>
    namespace test {
        typedef struct {
            int a;
        } Struct;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["test::Struct"], &[]);
}

#[test]
fn test_type_aliased_anonymous_nested_struct_ignored() {
    let hdr = indoc! {"
    #include <cstdint>
    namespace test {
        struct Outer {
            typedef struct {
                int a;
            } Struct;
            int b;
        };
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["test::Outer_Struct"], &[]);
}

#[ignore] // https://github.com/google/autocxx/issues/1251
#[test]
fn test_double_underscores_ignored() {
    let hdr = indoc! {"
    #include <cstdint>
    struct __FOO {
        uint32_t a;
    };
    struct B {
        B() :a(1) {}
        uint32_t take_foo(__FOO) const {
            return 3;
        }
        void do__something() const { }
        uint32_t get_a() const { return 2; }
        uint32_t a;
    };

    struct __default { __default() = default; };
    struct __destructor { ~__destructor() = default; };
    struct __copy { __copy(const __copy&) = default; };
    struct __copy_operator { __copy_operator &operator=(const __copy_operator&) = default; };
    struct __move { __move(__move&&) = default; };
    struct __move_operator { __move_operator &operator=(const __move_operator&) = default; };
    "};
    let rs = quote! {
        let b = ffi::B::new().within_unique_ptr();
        assert_eq!(b.get_a(), 2);
    };
    run_test(
        "",
        hdr,
        rs,
        &[
            "B",
            "__default",
            "__destructor",
            "__copy",
            "__copy_operator",
            "__move",
            "__move_operator",
        ],
        &[],
    );
}

// This test fails on Windows gnu but not on Windows msvc
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[test]
fn test_double_underscore_typedef_ignored() {
    let hdr = indoc! {"
    #include <cstdint>
    typedef int __int32_t;
    typedef __int32_t __darwin_pid_t;
    typedef __darwin_pid_t pid_t;
    struct B {
        B() :a(1) {}
        uint32_t take_foo(pid_t) const {
            return 3;
        }
        uint32_t get_a() const { return 2; }
        uint32_t a;
    };
    "};
    let rs = quote! {
        let b = ffi::B::new().within_unique_ptr();
        assert_eq!(b.get_a(), 2);
    };
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_double_underscores_fn_namespace() {
    let hdr = indoc! {"
    namespace __B {
        inline void a() {}
    };
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_typedef_to_ptr_is_marked_unsafe() {
    let hdr = indoc! {"
    struct _xlocalefoo; /* forward reference */
    typedef struct _xlocalefoo * locale_tfoo;
    extern \"C\" {
        locale_tfoo duplocalefoo(locale_tfoo);
    }
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["duplocalefoo"], &[]);
}

#[test]
fn test_issue_264() {
    let hdr = indoc! {"
    namespace a {
        typedef int b;
        //inline namespace c {}
        template <typename> class aa;
        inline namespace c {
        template <typename d, typename = d, typename = aa<d>> class e;
        }
        typedef e<char> f;
        template <typename g, typename, template <typename> typename> struct h {
          using i = g;
        };
        template <typename g, template <typename> class k> using j = h<g, void, k>;
        template <typename g, template <typename> class k>
        using m = typename j<g, k>::i;
        template <typename> struct l { typedef b ab; };
        template <typename p> class aa {
        public:
          typedef p n;
        };
        struct r {
          template <typename p> using o = typename p::c;
        };
        template <typename ad> struct u : r {
          typedef typename ad::n n;
          using ae = m<n, o>;
          template <typename af, typename> struct v { using i = typename l<f>::ab; };
          using ab = typename v<ad, ae>::i;
        };
        } // namespace a
        namespace q {
        template <typename ad> struct w : a::u<ad> {};
        } // namespace q
        namespace a {
        inline namespace c {
        template <typename, typename, typename ad> class e {
          typedef q::w<ad> s;
        public:
          typedef typename s::ab ab;
        };
        } // namespace c
        } // namespace a
        namespace ag {
        namespace ah {
        typedef a::f::ab t;
        class ai {
        public:
          t aj;
        };
        class al;
        namespace am {
        class an {
        public:
          void ao(ai);
        };
        } // namespace am
        class ap {
        public:
          al aq();
        };
        class ar {
        public:
          am::an as;
        };
        class al {
        public:
          ar at;
        };
        struct au {
          ap av;
        };
        } // namespace ah
        } // namespace ag
        namespace operations_research {
        class aw {
        public:
          ag::ah::au ax;
        };
        class Solver {
        public:
          aw ay;
        };
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["operations_research::Solver"], &[], None),
        make_cpp17_adder(),
        None,
        None,
    );
}

#[test]
fn test_unexpected_use() {
    // https://github.com/google/autocxx/issues/303
    let hdr = indoc! {"
        typedef int a;
        namespace b {
        namespace c {
        enum d : a;
        }
        } // namespace b
        namespace {
        using d = b::c::d;
        }
        namespace content {
        class RenderFrameHost {
        public:
            RenderFrameHost() {}
        d e;
        };
        } // namespace content
        "};
    let rs = quote! {
        let _ = ffi::content::RenderFrameHost::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["content::RenderFrameHost"], &[]);
}

#[test]
fn test_get_pure_virtual() {
    let hdr = indoc! {"
        #include <cstdint>
        class A {
        public:
            virtual ~A() {}
            virtual uint32_t get_val() const = 0;
        };
        class B : public A {
        public:
            virtual uint32_t get_val() const { return 3; }
        };
        const B b;
        inline const A* get_a() { return &b; };
    "};
    let rs = quote! {
        let a = ffi::get_a();
        let a_ref = unsafe { a.as_ref() }.unwrap();
        assert_eq!(a_ref.get_val(), 3);
    };
    run_test("", hdr, rs, &["A", "get_a"], &[]);
}

#[test]
fn test_abstract_class_no_make_unique() {
    // We shouldn't generate a new().within_unique_ptr() for abstract classes.
    // The test is successful if the bindings compile, i.e. if autocxx doesn't
    // attempt to instantiate the class.
    let hdr = indoc! {"
        class A {
        public:
            A() {}
            virtual ~A() {}
            virtual void foo() const = 0;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_derived_abstract_class_no_make_unique() {
    let hdr = indoc! {"
        class A {
        public:
            A();
            virtual ~A() {}
            virtual void foo() const = 0;
        };

        class B : public A {
        public:
            B();
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_recursive_derived_abstract_class_no_make_unique() {
    let hdr = indoc! {"
        class A {
        public:
            A() {}
            virtual ~A() {}
            virtual void foo() const = 0;
        };

        class B : public A {
        public:
            B() {};
        };

        class C : public B {
        public:
            C() {};
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B", "C"], &[]);
}

#[test]
fn test_derived_abstract_class_with_no_allowlisting_no_make_unique() {
    let hdr = indoc! {"
        class A {
        public:
            A();
            virtual ~A() {}
            virtual void foo() const = 0;
        };

        class B : public A {
        public:
            B();
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["B"], &[]);
}

#[test]
fn test_vector_of_pointers() {
    // Just ensures the troublesome API is ignored
    let hdr = indoc! {"
        #include <vector>
        namespace operations_research {
        class a;
        class Solver {
        public:
          struct b c(std::vector<a *>);
        };
        class a {};
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["operations_research::Solver"], &[]);
}

#[test]
fn test_vec_and_up_of_primitives() {
    let hdr = indoc! {"
        #include <vector>
        #include <memory>
        #include <cstdint>
        class Value {
        public:
            Value(std::vector<uint32_t>) {} // OK
            Value(std::unique_ptr<uint32_t>) {} // should be ignored
            Value(std::vector<int>) {} // should be ignored
            Value(std::unique_ptr<int>) {} // should be ignored
            Value(std::vector<char>) {} // should be ignored
            Value(std::unique_ptr<char>) {} // should be ignored
            Value(std::vector<float>) {} // OK
            Value(std::unique_ptr<float>) {} // should be ignored
            Value(std::vector<bool>) {} // should be ignored
            Value(std::unique_ptr<bool>) {} // should be ignored
            Value(std::vector<size_t>) {} // OK
            Value(std::unique_ptr<size_t>) {} // should be ignored
        };
        inline std::vector<uint32_t> make_vec_uint32_t() {
            std::vector<uint32_t> a;
            return a;
        }
        inline std::vector<float> make_vec_float() {
            std::vector<float> a;
            return a;
        }
        inline std::vector<size_t> make_vec_size_t() {
            std::vector<size_t> a;
            return a;
        }
    "};
    let rs = quote! {
        ffi::Value::new(ffi::make_vec_uint32_t()).within_box();
        ffi::Value::new6(ffi::make_vec_float()).within_box();
        ffi::Value::new10(ffi::make_vec_size_t()).within_box();
    };
    run_test(
        "",
        hdr,
        rs,
        &[
            "Value",
            "make_vec_uint32_t",
            "make_vec_float",
            "make_vec_size_t",
        ],
        &[],
    );
}

#[test]
fn test_pointer_to_pointer() {
    // Just ensures the troublesome API is ignored
    let hdr = indoc! {"
        namespace operations_research {
        class a;
        class Solver {
        public:
          struct b c(a **);
        };
        class a {};
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["operations_research::Solver"], &[]);
}

#[test]
fn test_defines_effective() {
    let hdr = indoc! {"
        #include <cstdint>
        #ifdef FOO
        inline uint32_t a() { return 4; }
        #endif
    "};
    let rs = quote! {
        ffi::a();
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate!("a") },
        make_clang_arg_adder(&["-DFOO"]),
        None,
        None,
    );
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/227
fn test_function_pointer_template() {
    let hdr = indoc! {"
        typedef int a;
        namespace std {
        template <typename> class b;
        }
        typedef a c;
        namespace operations_research {
        class d;
        class Solver {
        public:
            typedef std::b<c()> IndexEvaluator3;
            d e(IndexEvaluator3);
        };
        class d {};
        } // namespace operations_research
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["operations_research::Solver"], &[]);
}

#[test]
fn test_cvoid() {
    let hdr = indoc! {"
        #include <memory>
        #include <cstdint>
        inline void* a() {
            return static_cast<void*>(new int(3));
        }
        inline uint32_t b(void* p) {
            int* p_int = static_cast<int*>(p);
            auto val = *p_int;
            delete p_int;
            return val;
        }
    "};
    let rs = quote! {
        let ptr = ffi::a();
        let res = unsafe { ffi::b(ptr) };
        assert_eq!(res, 3);
    };
    run_test("", hdr, rs, &["a", "b"], &[]);
}

#[test]
fn test_c_schar() {
    let hdr = indoc! {"
        inline signed char a() {
            return 8;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::a(), 8);
    };
    run_test("", hdr, rs, &["a"], &[]);
}

#[test]
fn test_c_uchar() {
    let hdr = indoc! {"
        inline unsigned char a() {
            return 8;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::a(), 8);
    };
    run_test("", hdr, rs, &["a"], &[]);
}

#[test]
fn test_c_ulonglong() {
    // We don't test all the different variable-length integer types which we populate.
    // If one works, they probably all do. Hopefully.
    let hdr = indoc! {"
        inline unsigned long long a() {
            return 8;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::a(), autocxx::c_ulonglong(8));
    };
    run_test("", hdr, rs, &["a"], &[]);
}

#[test]
fn test_string_transparent_function() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        inline uint32_t take_string(std::string a) { return a.size(); }
    "};
    let rs = quote! {
        assert_eq!(ffi::take_string("hello"), 5);
    };
    run_test("", hdr, rs, &["take_string"], &[]);
}

#[test]
fn test_string_transparent_method() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        struct A {
            A() {}
            inline uint32_t take_string(std::string a) const { return a.size(); }
        };
    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        assert_eq!(a.take_string("hello"), 5);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_string_transparent_static_method() {
    let hdr = indoc! {"
        #include <string>
        #include <cstdint>
        struct A {
            A() {}
            static inline uint32_t take_string(std::string a) { return a.size(); }
        };
    "};
    let rs = quote! {
        assert_eq!(ffi::A::take_string("hello"), 5);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/490
fn test_issue_490() {
    let hdr = indoc! {"
        typedef int a;
        typedef long unsigned size_t;
        namespace std {
        namespace {
        using ::size_t;
        template <class b, b c> struct g { static const b value = c; };
        template <bool d> using e = g<bool, d>;
        typedef e<true> true_type;
        template <size_t, size_t> struct ag {};
        template <class b> typename b ::h move();
        template <class> class allocator;
        template <class> class vector;
        } // namespace
        } // namespace std
        void *operator new(size_t, void *);
        namespace std {
        namespace {
        template <class> struct iterator;
        template <class b, class> struct ay { using h = b *; };
        template <class b> struct bj { b bk; };
        template <class bm, class> class bn : bj<bm> {};
        template <class b, class i = b> class unique_ptr {
        typedef i bp;
        typedef typename ay<b, bp>::h bh;
        bn<bh, bp> bq;

        public:
        unique_ptr();
        unique_ptr(bh);
        bh get() const;
        bh release();
        };
        template <class = void> struct bt;
        } // namespace
        } // namespace std
        typedef a bv;
        namespace absl {
        template <typename ce> class cj {
        public:
        using bh = ce *;
        using iterator = bh;
        };
        namespace j {
        template <class ce> struct cp {
        using k = ce;
        using cq = std::bt<>;
        };
        template <class ce> using cr = typename cp<ce>::k;
        template <class ce> using cs = typename cp<ce>::cq;
        template <class, class, class, class> class ct {
        public:
        class iterator {};
        class cu {
            cu(iterator);
            iterator cv;
        };
        };
        template <typename> struct cw;
        } // namespace j
        template <class ce, class k = j::cr<ce>, class cq = j::cs<ce>,
                class cx = std::allocator<ce>>
        class cy : public j::ct<j::cw<ce>, k, cq, cx> {};
        } // namespace absl
        namespace cz {
        template <typename da> class db { std::ag<sizeof(a), alignof(da)> c; };
        } // namespace cz
        namespace spanner {
        class l;
        class ColumnList {
        public:
        typedef absl::cj<l>::iterator iterator;
        iterator begin();
        };
        class dd {
        union {
            cz::db<absl::cy<bv>::cu> e;
        };
        };
        class Row {
        public:
        bool f(dd);
        };
        } // namespace spanner
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["spanner::Row", "spanner::ColumnList"], &[]);
}

#[test]
fn test_immovable_object() {
    let hdr = indoc! {"
        class A {
        public:
            A();
            A(A&&) = delete;
        };

        class B{
        public:
            B();
            B(const B&) = delete;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_struct_with_reference() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <utility>
        struct A {
            uint32_t a;
        };
        struct B {
            B(const A& param) : a(param) {}
            const A& a;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_struct_with_rvalue() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <utility>
        struct A {
            uint32_t a;
        };
        struct B {
            B(A&& param) : a(std::move(param)) {}
            A&& a;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_immovable_nested_object() {
    let hdr = indoc! {"
        struct C {
            class A {
            public:
                A();
                A(A&&) = delete;
            };

            class B{
            public:
                B();
                B(const B&) = delete;
            };
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["C_A", "C_B"], &[]);
}

#[test]
fn test_type_called_type() {
    let hdr = indoc! {"
        namespace a {
            template<int _Len>
            struct b
            {
                union type
                {
                    unsigned char __data[_Len];
                    struct foo {
                        int a;
                    };
                };
            };
        }
        inline void take_type(a::b<4>::type) {}
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["take_type"], &[]);
}

#[test]
fn test_bridge_conflict_ty() {
    let hdr = indoc! {"
        namespace a {
            struct Key { int a; };
        }
        namespace b {
            struct Key { int a; };
        }
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["a::Key", "b::Key"], &[]);
}

#[test]
fn test_bridge_conflict_ty_fn() {
    let hdr = indoc! {"
        namespace a {
            struct Key { int a; };
        }
        namespace b {
            inline void Key() {}
        }
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["a::Key", "b::Key"], &[]);
}

#[test]
fn test_issue_506() {
    let hdr = indoc! {"
        namespace std {
            template <class, class> class am;
            typedef am<char, char> an;
        } // namespace std
        namespace be {
            class bf {
            virtual std::an bg() = 0;
            };
            class bh : bf {};
        } // namespace be
        namespace spanner {
            class Database;
            class Row {
            public:
            Row(be::bh *);
            };
        } // namespace spanner
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["spanner::Database", "spanner::Row"], &[], None),
        // This is normally a valid warning for generating bindings for this code, but we're doing
        // it on purpose as a regression test on minimized code so we'll just ignore it.
        make_clang_optional_arg_adder(&[], &["-Wno-delete-abstract-non-virtual-dtor"]),
        None,
        None,
    );
}

#[test]
fn test_private_inheritance() {
    let hdr = indoc! {"
        class A {
        public:
            void foo() {}
            int a;
        };
        class B : A {
        public:
            void bar() {}
            int b;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A", "B"], &[]);
}

#[test]
fn test_error_generated_for_static_data() {
    let hdr = indoc! {"
        #include <cstdint>
        struct A {
            A() {}
            uint32_t a;
        };
        static A FOO = A();
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate!("FOO")},
        None,
        Some(make_error_finder("FOO")),
        None,
    );
}

#[test]
fn test_error_generated_for_array_dependent_function() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <functional>
        inline void take_func(std::function<bool(const uint32_t number)>) {
        }
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate! ("take_func")},
        None,
        Some(make_error_finder("take_func")),
        None,
    );
}

#[test]
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[cfg_attr(skip_windows_msvc_failing_tests, ignore)]
fn test_error_generated_for_array_dependent_method() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <functional>
        struct A {
            void take_func(std::function<bool(const uint32_t number)>) {
            }
        };
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate! ("A")},
        None,
        Some(make_string_finder(
            ["take_func", "couldn't be generated"]
                .map(|s| s.to_string())
                .to_vec(),
        )),
        None,
    );
}

#[test]
fn test_error_generated_for_pod_with_nontrivial_destructor() {
    // take_a is necessary here because cxx won't generate the required
    // static assertions unless the type is actually used in some context
    // where cxx needs to decide it's trivial or non-trivial.
    let hdr = indoc! {"
        #include <cstdint>
        #include <functional>
        struct A {
            ~A() {}
        };
        inline void take_a(A) {}
    "};
    let rs = quote! {};
    run_test_expect_fail("", hdr, rs, &["take_a"], &["A"]);
}

#[test]
fn test_error_generated_for_pod_with_nontrivial_move_constructor() {
    // take_a is necessary here because cxx won't generate the required
    // static assertions unless the type is actually used in some context
    // where cxx needs to decide it's trivial or non-trivial.
    let hdr = indoc! {"
        #include <cstdint>
        #include <functional>
        struct A {
            A() = default;
            A(A&&) {}
        };
        inline void take_a(A) {}
    "};
    let rs = quote! {};
    run_test_expect_fail("", hdr, rs, &["take_a"], &["A"]);
}

#[test]
fn test_double_destruction() {
    let hdr = indoc! {"
        #include <stdio.h>
        #include <stdlib.h>
        // A simple type to let Rust verify the destructor is run.
        struct NotTriviallyDestructible {
            NotTriviallyDestructible() = default;
            NotTriviallyDestructible(const NotTriviallyDestructible&) = default;
            NotTriviallyDestructible(NotTriviallyDestructible&&) = default;

            ~NotTriviallyDestructible() {}
        };

        struct ExplicitlyDefaulted {
            ExplicitlyDefaulted() = default;
            ~ExplicitlyDefaulted() = default;

            NotTriviallyDestructible flag;
        };
    "};
    let rs = quote! {
        moveit! {
            let mut moveit_t = ffi::ExplicitlyDefaulted::new();
        }
    };
    match do_run_test(
        "",
        hdr,
        rs,
        directives_from_lists(
            &[],
            &["NotTriviallyDestructible", "ExplicitlyDefaulted"],
            None,
        ),
        None,
        None,
        None,
        "unsafe_ffi",
        None,
    ) {
        Err(TestError::CppBuild(_)) => {} // be sure this fails due to a static_assert
        // rather than some runtime problem
        _ => panic!("Test didn't fail as expected"),
    };
}

#[test]
fn test_keyword_function() {
    let hdr = indoc! {"
        inline void move(int) {};
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["move_"], &[]);
}

#[test]
fn test_keyword_method() {
    let hdr = indoc! {"
        struct A {
            int a;
            inline void move() {};
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_doc_passthru() {
    let hdr = indoc! {"
        #include <cstdint>
        /// Elephants!
        struct A {
            uint32_t a;
        };
        /// Giraffes!
        struct B {
            uint32_t a;
        };
        /// Rhinos!
        inline uint32_t get_a() { return 3; }
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["A", "get_a"], &["B"], None),
        None,
        Some(make_string_finder(
            ["Giraffes", "Elephants", "Rhinos"]
                .map(|s| s.to_string())
                .to_vec(),
        )),
        None,
    );
}

#[test]
fn test_closure() {
    // Ensuring presence of this closure doesn't break other things
    let hdr = indoc! {"
    #include <functional>
    #include <cstdint>

    inline bool take_closure(std::function<bool(const uint32_t number)> fn) {
        return fn(5);
    }
    inline uint32_t get_a() {
        return 3;
    }
    "};
    let rs = quote! {
        assert_eq!(ffi::get_a(), 3);
    };
    run_test("", hdr, rs, &["get_a"], &[]);
}

#[test]
fn test_multiply_nested_inner_type() {
    let hdr = indoc! {"
        struct Turkey {
            struct Duck {
                struct Hen {
                    int wings;
                };
                struct HenWithDefault {
                    HenWithDefault() = default;
                    int wings;
                };
                struct HenWithDestructor {
                    ~HenWithDestructor() = default;
                    int wings;
                };
                struct HenWithCopy {
                    HenWithCopy() = default;
                    HenWithCopy(const HenWithCopy&) = default;
                    int wings;
                };
                struct HenWithMove {
                    HenWithMove() = default;
                    HenWithMove(HenWithMove&&) = default;
                    int wings;
                };
            };
        };
        "};
    let rs = quote! {
        ffi::Turkey_Duck_Hen::new().within_unique_ptr();
        ffi::Turkey_Duck_HenWithDefault::new().within_unique_ptr();
        ffi::Turkey_Duck_HenWithDestructor::new().within_unique_ptr();
        ffi::Turkey_Duck_HenWithCopy::new().within_unique_ptr();
        ffi::Turkey_Duck_HenWithMove::new().within_unique_ptr();

        moveit! {
            let hen = ffi::Turkey_Duck_Hen::new();
            let moved_hen = autocxx::moveit::new::mov(hen);
            let _copied_hen = autocxx::moveit::new::copy(moved_hen);

            let hen = ffi::Turkey_Duck_HenWithDefault::new();
            let moved_hen = autocxx::moveit::new::mov(hen);
            let _copied_hen = autocxx::moveit::new::copy(moved_hen);

            let _hen = ffi::Turkey_Duck_HenWithDestructor::new();

            let hen = ffi::Turkey_Duck_HenWithCopy::new();
            let _copied_hen = autocxx::moveit::new::copy(hen);

            let hen = ffi::Turkey_Duck_HenWithMove::new();
            let _moved_hen = autocxx::moveit::new::mov(hen);
        }
    };
    run_test(
        "",
        hdr,
        rs,
        &[],
        &[
            "Turkey_Duck_Hen",
            "Turkey_Duck_HenWithDefault",
            "Turkey_Duck_HenWithDestructor",
            "Turkey_Duck_HenWithCopy",
            "Turkey_Duck_HenWithMove",
        ],
    );
}

#[test]
fn test_underscored_namespace_for_inner_type() {
    let hdr = indoc! {"
        namespace __foo {
            struct daft {
                struct bob {
                    int a;
                };
                int a;
            };
        }
        inline void bar(__foo::daft::bob) {}
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["bar"], &[]);
}

#[test]
fn test_blocklist_not_overly_broad() {
    // This is a regression test. We used to block anything that starts with "rust" or "std",
    // not just items in the "rust" and "std" namespaces. We therefore test that functions starting
    // with "rust" or "std" get imported.
    let hdr = indoc! {"
    inline void rust_func() { }
    inline void std_func() { }
    "};
    let rs = quote! {
        ffi::rust_func();
        ffi::std_func();
    };
    run_test("", hdr, rs, &["rust_func", "std_func"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/837
fn test_ref_qualified_method() {
    let hdr = indoc! {"
        struct A {
            void foo() & {}
        };
    "};
    let rs = quote! {
        A::new().within_unique_ptr().foo();
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[cfg_attr(skip_windows_msvc_failing_tests, ignore)]
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[test]
fn test_stringview() {
    // Test that APIs using std::string_view do not otherwise cause errors.
    let hdr = indoc! {"
        #include <string_view>
        #include <string>
        void take_string_view(std::string_view) {}
        std::string_view return_string_view(const std::string& a) { return std::string_view(a); }
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["take_string_view", "return_string_view"], &[], None),
        make_cpp17_adder(),
        None,
        None,
    );
}

#[test]
fn test_include_cpp_alone() {
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t give_int() {
            return 5;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        use autocxx::include_cpp;
        include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("give_int")
        }
        fn main() {
            assert_eq!(ffi::give_int(), 5);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_include_cpp_in_path() {
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t give_int() {
            return 5;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
            autocxx::include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                generate!("give_int")
            }
            fn main() {
                assert_eq!(ffi::give_int(), 5);
            }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_bitset() {
    let hdr = indoc! {"
        #include <cstddef>
        template <size_t _N_words, size_t _Size>
        class __bitset
        {
        public:
            typedef size_t              __storage_type;
            __storage_type __first_[_N_words];
            inline bool all() {
                return false;
            }
        };

        template <size_t _Size>
        class bitset
            : private __bitset<_Size == 0 ? 0 : (_Size - 1) / (sizeof(size_t) * 8) + 1, _Size>
        {
        public:
            static const unsigned __n_words = _Size == 0 ? 0 : (_Size - 1) / (sizeof(size_t) * 8) + 1;
            typedef __bitset<__n_words, _Size> base;
            bool all() const noexcept;
        };


        typedef bitset<1> mybitset;
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_cint_vector() {
    let hdr = indoc! {"
        #include <vector>
        #include <cstdint>
        inline std::vector<int32_t> give_vec() {
            return std::vector<int32_t> {1,2};
        }
    "};

    let rs = quote! {
        assert_eq!(ffi::give_vec().as_ref().unwrap().as_slice(), &[1,2]);
    };

    run_test("", hdr, rs, &["give_vec"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/422
fn test_int_vector() {
    let hdr = indoc! {"
        #include <vector>
        std::vector<int> give_vec() {
            return std::vector<int> {1,2};
        }
    "};

    let rs = quote! {
        assert_eq!(ffi::give_vec().as_ref().unwrap().as_slice(), &[autocxx::c_int(1),autocxx::c_int(2)]);
    };

    run_test("", hdr, rs, &["give_vec"], &[]);
}

#[test]
fn test_size_t() {
    let hdr = indoc! {"
        #include <cstddef>
        inline size_t get_count() { return 7; }
    "};

    let rs = quote! {
        ffi::get_count();
    };

    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["get_count"], &[], None),
        None,
        Some(make_rust_code_finder(vec![
            quote! {fn get_count() -> usize},
        ])),
        None,
    );
}

#[test]
fn test_deleted_function() {
    // We shouldn't generate bindings for deleted functions.
    // The test is successful if the bindings compile, i.e. if autocxx doesn't
    // attempt to call the deleted function.
    let hdr = indoc! {"
        class A {
        public:
            void foo() = delete;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_ignore_move_constructor() {
    let hdr = indoc! {"
        class A {
        public:
            A() {}
            A(A&&) {};
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_ignore_function_with_rvalue_ref() {
    let hdr = indoc! {"
        #include <string>

        void moveme(std::string &&);
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["moveme"], &[]);
}

#[test]
fn test_take_nonpod_rvalue_from_up() {
    let hdr = indoc! {"
        #include <string>
        struct A {
            std::string a;
        };
        inline void take_a(A&&) {};
    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        ffi::take_a(a);

        let a2 = ffi::A::new().within_box();
        ffi::take_a(a2);
    };
    run_test("", hdr, rs, &["A", "take_a"], &[]);
}

#[test]
fn test_take_nonpod_rvalue_from_stack() {
    let hdr = indoc! {"
        #include <string>
        struct A {
            std::string a;
        };
        inline void take_a(A&&) {};
    "};
    let rs = quote! {
        moveit! { let a = ffi::A::new() };
        ffi::take_a(a);
    };
    run_test("", hdr, rs, &["A", "take_a"], &[]);
}

#[test]
fn test_overloaded_ignored_function() {
    // When overloaded functions are ignored during import, the placeholder
    // functions generated for them should have unique names, just as they
    // would have if they had been imported successfully.
    // The test is successful if the bindings compile.
    let hdr = indoc! {"
        struct Blocked {};
        class A {
        public:
            void take_blocked(Blocked);
            void take_blocked(Blocked, int);
        };
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            generate!("A")
            block!("Blocked")
        },
        None,
        None,
        None,
    );
}

#[test]
fn test_namespaced_constant() {
    let hdr = indoc! {"
        namespace A {
            const int kConstant = 3;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::A::kConstant, 3);
    };
    run_test("", hdr, rs, &["A::kConstant"], &[]);
}

#[test]
fn test_issue_470_492() {
    let hdr = indoc! {"
        namespace std {
            template <bool, typename _Iftrue, typename _Iffalse> struct a;
        }
        template <typename> struct b;
        template <typename d> struct c {
            typedef std::a<b<d>::c, int, int> e;
        };
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_no_impl() {
    let hdr = indoc! {"
        struct A {
            int a;
        };
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            exclude_impls!()
            exclude_utilities!()
            generate!("A")
        },
        None,
        None,
        None,
    );
}

#[test]
fn test_generate_all() {
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t give_int() {
            return 5;
        }
    "};
    let rs = quote! {
        assert_eq!(ffi::give_int(), 5);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            generate_all!()
        },
        None,
        None,
        None,
    );
}

#[test]
fn test_std_thing() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace std {
            struct A {
                uint8_t a;
            };
        }
        typedef char daft;
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_two_mods() {
    let hdr = indoc! {"
        #include <cstdint>
        struct A {
            uint32_t a;
        };
        inline A give_a() {
            A a;
            a.a = 5;
            return a;
        }
        inline uint32_t get_a(A a) {
            return a.a;
        }
        struct B {
            uint32_t a;
        };
        inline B give_b() {
            B a;
            a.a = 8;
            return a;
        }
        inline uint32_t get_b(B a) {
            return a.a;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        use autocxx::prelude::*;
        include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("give_a")
            generate!("get_a")
        }
        include_cpp! {
            #hexathorpe include "input.h"
            name!(ffi2)
            generate!("give_b")
            generate!("get_b")
        }
        fn main() {
            let a = ffi::give_a().within_unique_ptr();
            assert_eq!(ffi::get_a(a), 5);
            let b = unsafe { ffi2::give_b().within_unique_ptr() };
            assert_eq!(unsafe { ffi2::get_b(b) }, 8);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_manual_bridge() {
    let hdr = indoc! {"
        #include <cstdint>
        inline uint32_t give_int() {
            return 5;
        }
        inline uint32_t give_int2() {
            return 5;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("give_int")
        }
        #[cxx::bridge]
        mod ffi2 {
            unsafe extern "C++" {
                include!("input.h");
                fn give_int2() -> u32;
            }
        }
        fn main() {
            assert_eq!(ffi::give_int(), 5);
            assert_eq!(ffi2::give_int2(), 5);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_manual_bridge_mixed_types() {
    let hdr = indoc! {"
        #include <memory>
        struct A {
            int a;
        };
        inline int take_A(const A& a) {
            return a.a;
        }
        inline std::unique_ptr<A> give_A() {
            auto a = std::make_unique<A>();
            a->a = 5;
            return a;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        use autocxx::prelude::*;
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("take_A")
            generate!("A")
        }
        #[cxx::bridge]
        mod ffi2 {
            unsafe extern "C++" {
                include!("input.h");
                type A = crate::ffi::A;
                fn give_A() -> UniquePtr<A>;
            }
        }
        fn main() {
            let a = ffi2::give_A();
            assert_eq!(ffi::take_A(&a), autocxx::c_int(5));
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_extern_cpp_type_cxx_bridge() {
    let hdr = indoc! {"
        #include <cstdint>
        struct A {
            A() : a(0) {}
            int a;
        };
        inline void handle_a(const A&) {
        }
        inline A create_a() {
            A a;
            return a;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        use autocxx::prelude::*;
        include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("handle_a")
            generate!("create_a")
            extern_cpp_opaque_type!("A", crate::ffi2::A)
        }
        #[cxx::bridge]
        pub mod ffi2 {
            unsafe extern "C++" {
                include!("input.h");
                type A;
            }
            impl UniquePtr<A> {}
        }
        fn main() {
            let a = ffi::create_a();
            ffi::handle_a(&a);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_extern_cpp_type_different_name() {
    let hdr = indoc! {"
        #include <cstdint>
        struct A {
            A() : a(0) {}
            int a;
        };
        inline void handle_a(const A&) {
        }
        inline A create_a() {
            A a;
            return a;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        use autocxx::prelude::*;
        include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("handle_a")
            generate!("create_a")
            extern_cpp_opaque_type!("A", crate::DifferentA)
        }
        #[cxx::bridge]
        pub mod ffi2 {
            unsafe extern "C++" {
                include!("input.h");
                type A;
            }
            impl UniquePtr<A> {}
        }
        pub use ffi2::A as DifferentA;
        fn main() {
            let a = ffi::create_a();
            ffi::handle_a(&a);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_extern_cpp_type_two_include_cpp() {
    let hdr = indoc! {"
        #include <cstdint>
        struct A {
            A() : a(0) {}
            int a;
        };
        enum B {
            VARIANT,
        };
        inline void handle_a(const A&) {
        }
        inline A create_a(B) {
            A a;
            return a;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        pub mod base {
            autocxx::include_cpp! {
                #hexathorpe include "input.h"
                name!(ffi2)
                safety!(unsafe_ffi)
                generate!("A")
                generate!("B")
            }
            pub use ffi2::*;
        }
        pub mod dependent {
            autocxx::include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                generate!("handle_a")
                generate!("create_a")
                extern_cpp_type!("A", crate::base::A)
                extern_cpp_type!("B", super::super::base::B)
                pod!("B")
            }
            pub use ffi::*;
        }
        fn main() {
            use autocxx::prelude::*;
            let a = dependent::create_a(base::B::VARIANT).within_box();
            dependent::handle_a(&a);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
/// Tests extern_cpp_type with a type inside a namespace.
fn test_extern_cpp_type_namespace() {
    let hdr = indoc! {"
        #include <cstdint>
        namespace b {
        struct B {
            B() {}
        };
        }  // namespace b
        struct A {
            A() {}
            b::B make_b() { return b::B(); }
        };
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        pub mod b {
            autocxx::include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                name!(ffi_b)
                generate_pod!("b::B")
            }
            pub use ffi_b::b::B;
        }
        pub mod a {
            autocxx::include_cpp! {
                #hexathorpe include "input.h"
                safety!(unsafe_ffi)
                name!(ffi_a)
                generate_pod!("A")
                extern_cpp_type!("b::B", crate::b::B)
            }
            pub use ffi_a::A;
        }
        fn main() {
            use autocxx::prelude::*;
            let _ = crate::a::A::new().within_unique_ptr().as_mut().unwrap().make_b();
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
#[ignore] // because we currently require UniquePtrTarget which this can't implement
fn test_extern_cpp_type_manual() {
    let hdr = indoc! {"
        #include <cstdint>
        struct A {
            int a;
        };
        inline void handle_a(const A& a) {
        }
        inline A create_a() {
            A a;
            return a;
        }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("handle_a")
            generate!("create_a")
            extern_cpp_type!("A", crate::ffi2::A)
        }
        pub mod ffi2 {
            use autocxx::cxx::{type_id, ExternType};
            #[repr(C)]
            pub struct A {
                a: std::os::raw::c_int
            }
            unsafe impl ExternType for A {
                type Kind = autocxx::cxx::kind::Opaque;
                type Id = type_id!("A");
            }

        }
        fn main() {
            let a = ffi2::A { a: 3 };
            ffi::handle_a(&a);
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_issue486() {
    let hdr = indoc! {"
        namespace a {
            namespace spanner {
                class Key;
            }
        } // namespace a
        namespace spanner {
            class Key {
                public:
                    bool b(a::spanner::Key &);
            };
        } // namespace spanner
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["spanner::Key"], &[]);
}

#[test]
#[ignore]
fn test_issue616() {
    let hdr = indoc! {"
        namespace N {
            template <typename> class B{};
            template <typename c> class C {
            public:
            using U = B<c>;
            };
            }
            class A : N::C<A> {
            U u;
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_shared_ptr() {
    let hdr = indoc! {"
        #include <memory>
        struct A {
            int a;
        };
        inline std::shared_ptr<A> make_shared_int() {
            return std::make_shared<A>(A { 3 });
        }
        inline int take_shared_int(std::shared_ptr<A> a) {
            return a->a;
        }
        inline std::weak_ptr<A> shared_to_weak(std::shared_ptr<A> a) {
            return std::weak_ptr<A>(a);
        }
    "};
    let rs = quote! {
        let a = ffi::make_shared_int();
        assert_eq!(ffi::take_shared_int(a.clone()), autocxx::c_int(3));
        ffi::shared_to_weak(a).upgrade();
    };
    run_test(
        "",
        hdr,
        rs,
        &["make_shared_int", "take_shared_int", "shared_to_weak"],
        &[],
    );
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/799
fn test_shared_ptr_const() {
    let hdr = indoc! {"
        #include <memory>
        inline std::shared_ptr<const int> make_shared_int() {
            return std::make_shared<const int>(3);
        }
        inline int take_shared_int(std::shared_ptr<const int> a) {
            return *a;
        }
    "};
    let rs = quote! {
        let a = ffi::make_shared_int();
        assert_eq!(ffi::take_shared_int(a.clone()), autocxx::c_int(3));
    };
    run_test("", hdr, rs, &["make_shared_int", "take_shared_int"], &[]);
}

#[test]
fn test_rust_reference() {
    let hdr = indoc! {"
    #include <cstdint>

    struct RustType;
    inline uint32_t take_rust_reference(const RustType&) {
        return 4;
    }
    "};
    let rs = quote! {
        let foo = RustType(3);
        assert_eq!(ffi::take_rust_reference(&foo), 4);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            generate!("take_rust_reference")
            extern_rust_type!(RustType)
        },
        None,
        None,
        Some(quote! {
            pub struct RustType(i32);
        }),
    );
}

#[test]
fn test_rust_reference_autodiscover() {
    let hdr = indoc! {"
    #include <cstdint>

    struct RustType;
    inline uint32_t take_rust_reference(const RustType&) {
        return 4;
    }
    "};
    let rs = quote! {
        let foo = RustType(3);
        let result = ffi::take_rust_reference(&foo);
        assert_eq!(result, 4);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {},
        Some(Box::new(EnableAutodiscover)),
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct RustType(i32);
        }),
    );
}

#[test]
fn test_pass_thru_rust_reference() {
    let hdr = indoc! {"
    #include <cstdint>

    struct RustType;
    inline const RustType& pass_rust_reference(const RustType& a) {
        return a;
    }
    "};
    let rs = quote! {
        let foo = RustType(3);
        assert_eq!(ffi::pass_rust_reference(&foo).0, 3);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            generate!("pass_rust_reference")
            extern_rust_type!(RustType)
        },
        None,
        None,
        Some(quote! {
            pub struct RustType(i32);
        }),
    );
}

#[test]
fn test_extern_rust_method() {
    let hdr = indoc! {"
        #include <cstdint>
        struct RustType;
        uint32_t examine(const RustType& foo);
    "};
    let cxx = indoc! {"
        uint32_t examine(const RustType& foo) {
            return foo.get();
        }"};
    let rs = quote! {
        let a = RustType(74);
        assert_eq!(ffi::examine(&a), 74);
    };
    run_test_ex(
        cxx,
        hdr,
        rs,
        directives_from_lists(&["examine"], &[], None),
        Some(Box::new(EnableAutodiscover)),
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct RustType(i32);
            impl RustType {
                #[autocxx::extern_rust::extern_rust_function]
                pub fn get(&self) -> i32 {
                    return self.0
                }
            }
        }),
    );
}

#[test]
fn test_extern_rust_fn_callback() {
    let hdr = indoc! {"
        struct a {};
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("a")
        }

        use ffi::a;
        use std::pin::Pin;

        #[autocxx::extern_rust::extern_rust_function]
        pub fn called_from_cpp(_a: Pin<&mut a>) {}

        fn main() {}
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

// TODO: there are various other tests for extern_rust_fn we should add:
// 1) taking mutable and immutable references
// 2) ensuring that types on which the signature depends as receiver,
//    parameters and return are not garbage collected

#[test]
fn test_rust_reference_no_autodiscover() {
    let hdr = indoc! {"
    #include <cstdint>

    struct RustType;
    inline uint32_t take_rust_reference(const RustType&) {
        return 4;
    }
    "};
    let rs = quote! {
        let foo = RustType(3);
        let result = ffi::take_rust_reference(&foo);
        assert_eq!(result, 4);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["take_rust_reference"], &[], None),
        None,
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct RustType(i32);
        }),
    );
}

#[test]
fn test_rust_box() {
    let hdr = indoc! {"
    #include <cstdint>
    #include <cxx.h>

    struct RustType;
    inline uint32_t take_rust_box(rust::Box<RustType>) {
        return 4;
    }
    "};
    let rs = quote! {
        let foo = Box::new(RustType(3));
        let result = ffi::take_rust_box(foo);
        assert_eq!(result, 4);
    };
    run_test_ex(
        "",
        hdr,
        rs,
        directives_from_lists(&["take_rust_box"], &[], None),
        None,
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct RustType(i32);
        }),
    );
}

#[test]
fn test_rust_reference_no_autodiscover_no_usage() {
    let rs = quote! {
        let _ = RustType(3);
    };
    run_test_ex(
        "",
        "",
        rs,
        directives_from_lists(&[], &[], None),
        None,
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct RustType(i32);
        }),
    );
}

#[test]
#[cfg_attr(skip_windows_msvc_failing_tests, ignore)]
// TODO - replace make_clang_arg_adder with something that knows how to add an MSVC-suitable
// directive for the cc build.
fn test_cpp17() {
    let hdr = indoc! {"
        static_assert(__cplusplus >= 201703L, \"This file expects a C++17 compatible compiler.\");
        inline void foo() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            ffi::foo();
        },
        quote! {
            generate!("foo")
        },
        make_cpp17_adder(),
        None,
        None,
    );
}

#[test]
fn test_box_extern_rust_type() {
    let hdr = indoc! {"
        #include <cxx.h>
        struct Foo;
        inline void take_box(rust::Box<Foo>) {
        }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            ffi::take_box(Box::new(Foo { a: "Hello".into() }))
        },
        quote! {
            generate!("take_box")
            extern_rust_type!(Foo)
        },
        None,
        None,
        Some(quote! {
            pub struct Foo {
                a: String,
            }
        }),
    );
}

#[test]
fn test_box_return_placement_new() {
    let hdr = indoc! {"
        #include <cxx.h>
        struct Foo;
        struct Foo2;
        struct Ret {};
        inline Ret take_box(rust::Box<Foo>, rust::Box<Foo2>) {
            return Ret{};
        }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let _ = ffi::take_box(
                Box::new(Foo { a: "Hello".into() }),
                Box::new(bar::Foo2 { a: "Goodbye".into() })
            );
        },
        quote! {
            generate!("take_box")
            extern_rust_type!(Foo)
            generate!("Ret")
        },
        None,
        None,
        Some(quote! {
            pub struct Foo {
                a: String,
            }
            mod bar {
                #[autocxx::extern_rust::extern_rust_type]
                pub struct Foo2 {
                    pub a: String,
                }
            }
        }),
    );
}

#[test]
fn test_box_via_extern_rust() {
    let hdr = indoc! {"
        #include <cxx.h>
        struct Foo;
        inline void take_box(rust::Box<Foo>) {
        }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            ffi::take_box(Box::new(Foo { a: "Hello".into() }))
        },
        quote! {},
        Some(Box::new(EnableAutodiscover)),
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct Foo {
                a: String,
            }
        }),
    );
}

#[test]
fn test_box_via_extern_rust_no_include_cpp() {
    let hdr = indoc! {"
        #include <cxx.h>
        struct Foo;
        inline void take_box(rust::Box<Foo>) {
        }
    "};
    do_run_test_manual(
        "",
        hdr,
        quote! {
            #[autocxx::extern_rust::extern_rust_type]
            pub struct Foo {
                a: String,
            }

            fn main() {
            }
        },
        Some(Box::new(EnableAutodiscover)),
        None,
    )
    .unwrap();
}

#[test]
fn test_box_via_extern_rust_in_mod() {
    let hdr = indoc! {"
        #include <cxx.h>
        struct Foo;
        inline void take_box(rust::Box<Foo>) {
        }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            ffi::take_box(Box::new(bar::Foo { a: "Hello".into() }))
        },
        quote! {},
        Some(Box::new(EnableAutodiscover)),
        None,
        Some(quote! {
            mod bar {
                #[autocxx::extern_rust::extern_rust_type]
                pub struct Foo {
                    pub a: String,
                }
            }
        }),
    );
}

#[test]
fn test_extern_rust_fn_simple() {
    let cpp = indoc! {"
        void foo() {
            my_rust_fun();
        }
    "};
    let hdr = indoc! {"
        #include <cxx.h>
        inline void do_thing() {}
    "};
    run_test_ex(
        cpp,
        hdr,
        quote! {
            ffi::do_thing();
        },
        quote! {
            generate!("do_thing")
        },
        Some(Box::new(EnableAutodiscover)),
        None,
        Some(quote! {
            #[autocxx::extern_rust::extern_rust_function]
            fn my_rust_fun() {
            }
        }),
    );
}

#[test]
fn test_extern_rust_fn_in_mod() {
    let hdr = indoc! {"
        #include <cxx.h>
        inline void do_thing() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! {
            generate!("do_thing")
        },
        Some(Box::new(EnableAutodiscover)),
        None,
        Some(quote! {
            mod bar {
                #[autocxx::extern_rust::extern_rust_function]
                pub fn my_rust_fun() {

                }
            }
        }),
    );
}

#[test]
fn test_issue_956() {
    let hdr = indoc! {"
        #include <cstdint>
        inline void take_int(int&) {}
        inline void take_uint16(uint16_t) {}
        inline void take_us(unsigned short) {}
        inline void take_char16(char16_t) {}
        inline void take_uint16_ref(uint16_t&) {}
        inline void take_char16_ref(char16_t &) {}
    "};
    run_test(
        "",
        hdr,
        quote! {},
        &[
            "take_int",
            "take_uint16",
            "take_char16",
            "take_uint16_ref",
            "take_char16_ref",
            "take_us",
        ],
        &[],
    );
}

#[test]
fn test_extern_rust_fn_no_autodiscover() {
    let hdr = indoc! {"
        #include <cxx.h>
    "};
    let cpp = indoc! {"
        void call_it() {
            my_rust_fun();
        }
    "};
    run_test_ex(
        cpp,
        hdr,
        quote! {},
        quote! {},
        None,
        None,
        Some(quote! {
            mod bar {
                #[autocxx::extern_rust::extern_rust_function]
                pub fn my_rust_fun() {

                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_mut() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() = 0;
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&mut self) {
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_const() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const = 0;
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&self) {
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_calls_impossible() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const = 0;
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_expect_fail_ex(
        "",
        hdr,
        quote! {
            MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&self) {
                    use ffi::Observer_supers;
                    self.foo_super()
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_not_pub() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const = 0;
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_expect_fail_ex(
        "",
        hdr,
        quote! {
            MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&self) {
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_ptr_param() {
    let hdr = indoc! {"
    #include <cstdint>
    struct A {
        uint8_t a;
    };

    class Observer {
    public:
        Observer() {}
        virtual void foo(const A*) const {};
        virtual ~Observer() {}
    };
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            generate!("A")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                unsafe fn foo(&self, a: *const ffi::A) {
                    use ffi::Observer_supers;
                    self.foo_super(a)
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_opaque_param() {
    let hdr = indoc! {"
    #include <cstdint>

    typedef uint32_t MyUnsupportedType[4];

    struct MySupportedType {
        uint32_t a;
    };

    class MySuperType {
    public:
        virtual void foo(const MyUnsupportedType* foo, const MySupportedType* bar) const = 0;
        virtual ~MySuperType() = default;
    };
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            MySubType::new_rust_owned(MySubType { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            subclass!("MySuperType",MySubType)
            extern_cpp_opaque_type!("MyUnsupportedType", crate::ffi2::MyUnsupportedType)
        },
        None,
        None,
        Some(quote! {

            #[cxx::bridge]
            pub mod ffi2 {
                unsafe extern "C++" {
                    include!("input.h");
                    type MyUnsupportedType;
                }
            }
            use autocxx::subclass::CppSubclass;
            use ffi::MySuperType_methods;
            #[autocxx::subclass::subclass]
            pub struct MySubType {
                a: u32
            }
            impl MySuperType_methods for MySubType {
                unsafe fn foo(&self, _foo: *const ffi2::MyUnsupportedType, _bar: *const ffi::MySupportedType) {
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_return() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual uint32_t foo() const = 0;
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&self) -> u32 {
                    4
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_passed_to_fn() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual uint32_t foo() const = 0;
        virtual ~Observer() {}
    };
    inline void take_observer(const Observer&) {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let o = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
            ffi::take_observer(o.borrow().as_ref());
        },
        quote! {
            generate!("take_observer")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&self) -> u32 {
                    4
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_derive_defaults() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual uint32_t foo() const = 0;
        virtual ~Observer() {}
    };
    inline void take_observer(const Observer&) {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            use autocxx::subclass::CppSubclassDefault;
            let o = MyObserver::default_rust_owned();
            ffi::take_observer(o.borrow().as_ref());
        },
        quote! {
            generate!("take_observer")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            #[autocxx::subclass::subclass]
            #[derive(Default)]
            pub struct MyObserver {
                a: u32
            }
            impl ffi::Observer_methods for MyObserver {
                fn foo(&self) -> u32 {
                    4
                }
            }
        }),
    );
}

#[test]
fn test_non_pv_subclass_simple() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const {}
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let obs = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
            }
        }),
    );
}

#[test]
/// Tests the Rust code generated for subclasses when there's a `std` module in scope representing
/// the C++ `std` namespace. This breaks if any of the generated Rust code fails to fully qualify
/// its references to the Rust `std`.
fn test_subclass_with_std() {
    let hdr = indoc! {"
    #include <cstdint>
    #include <chrono>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const {}
        virtual ~Observer() {}

        void unused(std::chrono::nanoseconds) {}
    };
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let obs = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
        },
        quote! {
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
            }
        }),
    );
}

#[test]
fn test_two_subclasses() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const {}
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let obs = MyObserverA::new_rust_owned(MyObserverA { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
            let obs = MyObserverB::new_rust_owned(MyObserverB { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserverA)
            subclass!("Observer",MyObserverB)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserverA {
                a: u32
            }
            impl Observer_methods for MyObserverA {
            }
            #[autocxx::subclass::subclass]
            pub struct MyObserverB {
                a: u32
            }
            impl Observer_methods for MyObserverB {
            }
        }),
    );
}

#[test]
fn test_two_superclasses_with_same_name_method() {
    let hdr = indoc! {"
    #include <cstdint>

    class ObserverA {
    public:
        ObserverA() {}
        virtual void foo() const {}
        virtual ~ObserverA() {}
    };

    class ObserverB {
        public:
            ObserverB() {}
            virtual void foo() const {}
            virtual ~ObserverB() {}
        };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let obs = MyObserverA::new_rust_owned(MyObserverA { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
            let obs = MyObserverB::new_rust_owned(MyObserverB { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
        },
        quote! {
            generate!("bar")
            subclass!("ObserverA",MyObserverA)
            subclass!("ObserverB",MyObserverB)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::ObserverA_methods;
            use ffi::ObserverB_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserverA {
                a: u32
            }
            impl ObserverA_methods for MyObserverA {
            }
            #[autocxx::subclass::subclass]
            pub struct MyObserverB {
                a: u32
            }
            impl ObserverB_methods for MyObserverB {
            }
        }),
    );
}

#[test]
fn test_subclass_no_safety() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() = 0;
        virtual ~Observer() {}
    };
    "};
    let hexathorpe = Token![#](Span::call_site());
    let unexpanded_rust = quote! {
        use autocxx::prelude::*;

        include_cpp!(
            #hexathorpe include "input.h"
            subclass!("Observer",MyObserver)
        );

        use ffi::Observer_methods;
        #hexathorpe [autocxx::subclass::subclass]
        pub struct MyObserver;
        impl Observer_methods for MyObserver {
            unsafe fn foo(&mut self) {}
        }

        use autocxx::subclass::{CppSubclass, CppPeerConstructor, CppSubclassRustPeerHolder};
        use cxx::UniquePtr;
        impl CppPeerConstructor<ffi::MyObserverCpp> for MyObserver {
            fn make_peer(
                &mut self,
                peer_holder: CppSubclassRustPeerHolder<Self>,
            ) -> UniquePtr<ffi::MyObserverCpp> {
                UniquePtr::emplace(unsafe { ffi::MyObserverCpp::new(peer_holder) })
            }
        }

        fn main() {
            let obs = MyObserver::new_rust_owned(MyObserver { cpp_peer: Default::default() });
            unsafe { obs.borrow_mut().foo() };
        }
    };

    do_run_test_manual("", hdr, unexpanded_rust, None, None).unwrap()
}

#[test]
fn test_pv_protected_constructor() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    protected:
        Observer() {}
    public:
        virtual void foo() const {}
        virtual ~Observer() {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let obs = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
            }
        }),
    );
}

#[test]
fn test_pv_protected_method() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer() {}
        virtual void foo() const {}
        virtual ~Observer() {}
    protected:
        virtual void baz() const {}
    };
    inline void bar() {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let obs = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
            obs.borrow().foo();
        },
        quote! {
            generate!("bar")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn baz(&self) {
                }

                fn foo(&self) {
                    use ffi::Observer_supers;
                    self.baz_super()
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_allocation_not_self_owned() {
    let hdr = indoc! {"
    #include <cstdint>
    extern \"C\" void mark_freed() noexcept;
    extern \"C\" void mark_allocated() noexcept;

    class TestObserver {
    public:
        TestObserver() {
            mark_allocated();
        }
        virtual void a() const = 0;
        virtual ~TestObserver() {
            mark_freed();
        }
    };
    inline void TriggerTestObserverA(const TestObserver& obs) {
        obs.a();
    }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);

            // Test when owned by C++
            let obs = MyTestObserver::new_cpp_owned(
                MyTestObserver::new()
            );
            assert!(Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);
            let obs_superclass = obs.as_ref().unwrap(); // &subclass
            let obs_superclass = unsafe { core::mem::transmute::<&ffi::MyTestObserverCpp, &ffi::TestObserver>(obs_superclass) };
            ffi::TriggerTestObserverA(obs_superclass);
            assert!(Lazy::force(&STATUS).lock().unwrap().a_called);
            core::mem::drop(obs);
            Lazy::force(&STATUS).lock().unwrap().a_called = false;
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);

            // Test when owned by Rust
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::new()
            );
            //let cpp_peer_ptr = unsafe { obs.borrow_mut().peer_mut().get_unchecked_mut() as *mut ffi::MyTestObserverCpp };
            assert!(Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);
            ffi::TriggerTestObserverA(obs.as_ref().borrow().as_ref());
            assert!(Lazy::force(&STATUS).lock().unwrap().a_called);
            Lazy::force(&STATUS).lock().unwrap().a_called = false;
            core::mem::drop(obs);
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);
        },
        quote! {
            generate!("TriggerTestObserverA")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use once_cell::sync::Lazy;
            use std::sync::Mutex;

            use autocxx::subclass::CppSubclass;
            use ffi::TestObserver_methods;
            #[autocxx::subclass::subclass]
            pub struct MyTestObserver {
                data: ExternalEngine,
            }
            impl TestObserver_methods for MyTestObserver {
                fn a(&self) {
                    self.data.do_something();
                }
            }
            impl MyTestObserver {
                fn new() -> Self {
                    Self {
                        cpp_peer: Default::default(),
                        data: ExternalEngine::default(),
                    }
                }
            }

            #[no_mangle]
            pub fn mark_allocated() {
                Lazy::force(&STATUS).lock().unwrap().cpp_allocated = true;
            }

            #[no_mangle]
            pub fn mark_freed() {
                Lazy::force(&STATUS).lock().unwrap().cpp_allocated = false;
            }

            #[derive(Default)]
            struct Status {
                cpp_allocated: bool,
                rust_allocated: bool,
                a_called: bool,
            }

            static STATUS: Lazy<Mutex<Status>> = Lazy::new(|| Mutex::new(Status::default()));

            pub struct ExternalEngine;

            impl ExternalEngine {
                fn do_something(&self) {
                    Lazy::force(&STATUS).lock().unwrap().a_called = true;
                }
            }

            impl Default for ExternalEngine {
                fn default() -> Self {
                    Lazy::force(&STATUS).lock().unwrap().rust_allocated = true;
                    ExternalEngine
                }
            }

            impl Drop for ExternalEngine {
                fn drop(&mut self) {
                    Lazy::force(&STATUS).lock().unwrap().rust_allocated = false;
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_allocation_self_owned() {
    let hdr = indoc! {"
    #include <cstdint>
    extern \"C\" void mark_freed() noexcept;
    extern \"C\" void mark_allocated() noexcept;

    class TestObserver {
    public:
        TestObserver() {
            mark_allocated();
        }
        virtual void a() const = 0;
        virtual ~TestObserver() {
            mark_freed();
        }
    };
    inline void TriggerTestObserverA(const TestObserver& obs) {
        const_cast<TestObserver&>(obs).a();
    }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);

            // Test when owned by C++
            let obs = MyTestObserver::new_cpp_owned(
                MyTestObserver::new(false)
            );
            assert!(Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);
            let obs_superclass = obs.as_ref().unwrap(); // &subclass
            let obs_superclass = unsafe { core::mem::transmute::<&ffi::MyTestObserverCpp, &ffi::TestObserver>(obs_superclass) };

            ffi::TriggerTestObserverA(obs_superclass);
            assert!(Lazy::force(&STATUS).lock().unwrap().a_called);
            core::mem::drop(obs);
            Lazy::force(&STATUS).lock().unwrap().a_called = false;
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);

            // Test when owned by Rust
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::new(false)
            );
            assert!(Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);
            ffi::TriggerTestObserverA(obs.as_ref().borrow().as_ref());

            assert!(Lazy::force(&STATUS).lock().unwrap().a_called);
            Lazy::force(&STATUS).lock().unwrap().a_called = false;
            core::mem::drop(obs);
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);

            // Test when self-owned
            let obs = MyTestObserver::new_self_owned(
                MyTestObserver::new(true)
            );
            let obs_superclass_ptr: *const ffi::TestObserver = obs.as_ref().borrow().as_ref();
            // Retain just a pointer on the Rust side, so there is no Rust-side
            // ownership.
            core::mem::drop(obs);
            assert!(Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
            assert!(Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().a_called);
            ffi::TriggerTestObserverA(unsafe { obs_superclass_ptr.as_ref().unwrap() });

            assert!(Lazy::force(&STATUS).lock().unwrap().a_called);
            assert!(!Lazy::force(&STATUS).lock().unwrap().rust_allocated);
            assert!(!Lazy::force(&STATUS).lock().unwrap().cpp_allocated);
        },
        quote! {
            generate!("TriggerTestObserverA")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use once_cell::sync::Lazy;
            use std::sync::Mutex;

            use autocxx::subclass::CppSubclass;
            use autocxx::subclass::CppSubclassSelfOwned;
            use ffi::TestObserver_methods;
            #[autocxx::subclass::subclass(self_owned)]
            pub struct MyTestObserver {
                data: ExternalEngine,
                self_owning: bool,
            }
            impl TestObserver_methods for MyTestObserver {
                fn a(&self) {
                    self.data.do_something();
                    if self.self_owning {
                        self.delete_self();
                    }
                }
            }
            impl MyTestObserver {
                fn new(self_owning: bool) -> Self {
                    Self {
                        cpp_peer: Default::default(),
                        data: ExternalEngine::default(),
                        self_owning,
                    }
                }
            }

            #[no_mangle]
            pub fn mark_allocated() {
                Lazy::force(&STATUS).lock().unwrap().cpp_allocated = true;
            }

            #[no_mangle]
            pub fn mark_freed() {
                Lazy::force(&STATUS).lock().unwrap().cpp_allocated = false;
            }

            #[derive(Default)]
            struct Status {
                cpp_allocated: bool,
                rust_allocated: bool,
                a_called: bool,
            }

            static STATUS: Lazy<Mutex<Status>> = Lazy::new(|| Mutex::new(Status::default()));

            pub struct ExternalEngine;

            impl ExternalEngine {
                fn do_something(&self) {
                    Lazy::force(&STATUS).lock().unwrap().a_called = true;
                }
            }

            impl Default for ExternalEngine {
                fn default() -> Self {
                    Lazy::force(&STATUS).lock().unwrap().rust_allocated = true;
                    ExternalEngine
                }
            }

            impl Drop for ExternalEngine {
                fn drop(&mut self) {
                    Lazy::force(&STATUS).lock().unwrap().rust_allocated = false;
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_calls() {
    let hdr = indoc! {"
    #include <cstdint>
    extern \"C\" void mark_c_called() noexcept;
    extern \"C\" void mark_d_called() noexcept;
    extern \"C\" void mark_e_called() noexcept;
    extern \"C\" void mark_f_called() noexcept;
    extern \"C\" void mark_g_called() noexcept;
    extern \"C\" void mark_h_called() noexcept;

    class TestObserver {
    public:
        TestObserver() {}
        virtual uint32_t a(uint32_t) const = 0;
        virtual uint32_t b(uint32_t) = 0;
        virtual uint32_t c(uint32_t) const { mark_c_called(); return 0; };
        virtual uint32_t d(uint32_t) { mark_d_called(); return 0; };
        virtual uint32_t e(uint32_t) const { mark_e_called(); return 0; };
        virtual uint32_t f(uint32_t) { mark_f_called(); return 0; };
        virtual uint32_t g(uint32_t) const { mark_g_called(); return 0; };
        virtual uint32_t h(uint32_t) { mark_h_called(); return 0; };
        virtual ~TestObserver() {}
    };

    extern TestObserver* obs;

    inline void register_observer(TestObserver& a) {
        obs = &a;
    }
    inline uint32_t call_a(uint32_t param) {
        return obs->a(param);
    }
    inline uint32_t call_b(uint32_t param) {
        return obs->b(param);
    }
    inline uint32_t call_c(uint32_t param) {
        return obs->c(param);
    }
    inline uint32_t call_d(uint32_t param) {
        return obs->d(param);
    }
    inline uint32_t call_e(uint32_t param) {
        return obs->e(param);
    }
    inline uint32_t call_f(uint32_t param) {
        return obs->f(param);
    }
    inline uint32_t call_g(uint32_t param) {
        return obs->g(param);
    }
    inline uint32_t call_h(uint32_t param) {
        return obs->h(param);
    }
    "};
    run_test_ex(
        "TestObserver* obs;",
        hdr,
        quote! {
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::default()
            );
            ffi::register_observer(obs.as_ref().borrow_mut().pin_mut());
            assert_eq!(ffi::call_a(1), 2);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_a_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_b(1), 3);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_b_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_c(1), 4);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_c_called);
            assert!(!Lazy::force(&STATUS).lock().unwrap().super_c_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_d(1), 5);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_d_called);
            assert!(!Lazy::force(&STATUS).lock().unwrap().super_d_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_e(1), 0);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_e_called);
            assert!(Lazy::force(&STATUS).lock().unwrap().super_e_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_f(1), 0);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_f_called);
            assert!(Lazy::force(&STATUS).lock().unwrap().super_f_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_g(1), 0);
            assert!(Lazy::force(&STATUS).lock().unwrap().super_g_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();

            assert_eq!(ffi::call_h(1), 0);
            assert!(Lazy::force(&STATUS).lock().unwrap().super_h_called);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();
        },
        quote! {
            generate!("register_observer")
            generate!("call_a")
            generate!("call_b")
            generate!("call_c")
            generate!("call_d")
            generate!("call_e")
            generate!("call_f")
            generate!("call_g")
            generate!("call_h")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use once_cell::sync::Lazy;
            use std::sync::Mutex;

            use autocxx::subclass::CppSubclass;
            use ffi::TestObserver_methods;
            #[autocxx::subclass::subclass]
            #[derive(Default)]
            pub struct MyTestObserver {
            }
            impl TestObserver_methods for MyTestObserver {

                // a and b are pure virtual
                fn a(&self, param: u32) -> u32 {
                    Lazy::force(&STATUS).lock().unwrap().sub_a_called = true;
                    param + 1
                }
                fn b(&mut self, param: u32) -> u32 {
                    Lazy::force(&STATUS).lock().unwrap().sub_b_called = true;
                    param + 2
                }

                // c and d we override the superclass
                fn c(&self, param: u32) -> u32 {
                    Lazy::force(&STATUS).lock().unwrap().sub_c_called = true;
                    param + 3
                }
                fn d(&mut self, param: u32) -> u32 {
                    Lazy::force(&STATUS).lock().unwrap().sub_d_called = true;
                    param + 4
                }

                // e and f we call through to the superclass
                fn e(&self, param: u32) -> u32 {
                    Lazy::force(&STATUS).lock().unwrap().sub_e_called = true;
                    self.peer().e_super(param)
                }
                fn f(&mut self, param: u32) -> u32 {
                    Lazy::force(&STATUS).lock().unwrap().sub_f_called = true;
                    self.peer_mut().f_super(param)
                }

                // g and h we do not do anything, so calls should only call
                // the superclass
            }

            #[no_mangle]
            pub fn mark_c_called() {
                Lazy::force(&STATUS).lock().unwrap().super_c_called = true;
            }
            #[no_mangle]
            pub fn mark_d_called() {
                Lazy::force(&STATUS).lock().unwrap().super_d_called = true;
            }
            #[no_mangle]
            pub fn mark_e_called() {
                Lazy::force(&STATUS).lock().unwrap().super_e_called = true;
            }
            #[no_mangle]
            pub fn mark_f_called() {
                Lazy::force(&STATUS).lock().unwrap().super_f_called = true;
            }
            #[no_mangle]
            pub fn mark_g_called() {
                Lazy::force(&STATUS).lock().unwrap().super_g_called = true;
            }
            #[no_mangle]
            pub fn mark_h_called() {
                Lazy::force(&STATUS).lock().unwrap().super_h_called = true;
            }

            #[derive(Default)]
            struct Status {
                super_c_called: bool,
                super_d_called: bool,
                super_e_called: bool,
                super_f_called: bool,
                super_g_called: bool,
                super_h_called: bool,
                sub_a_called: bool,
                sub_b_called: bool,
                sub_c_called: bool,
                sub_d_called: bool,
                sub_e_called: bool,
                sub_f_called: bool,
            }

            static STATUS: Lazy<Mutex<Status>> = Lazy::new(|| Mutex::new(Status::default()));
        }),
    );
}

#[test]
fn test_pv_subclass_as_superclass() {
    let hdr = indoc! {"
    #include <cstdint>
    #include <memory>

    class TestObserver {
    public:
        TestObserver() {}
        virtual void a() const = 0;
        virtual ~TestObserver() {}
    };

    inline void call_observer(std::unique_ptr<TestObserver> obs) { obs->a(); }
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            use autocxx::subclass::CppSubclass;
            let obs = MyTestObserver::new_cpp_owned(
                MyTestObserver::default()
            );
            let obs = MyTestObserver::as_TestObserver_unique_ptr(obs);
            assert!(!Lazy::force(&STATUS).lock().unwrap().dropped);
            ffi::call_observer(obs);
            assert!(Lazy::force(&STATUS).lock().unwrap().sub_a_called);
            assert!(Lazy::force(&STATUS).lock().unwrap().dropped);
            *Lazy::force(&STATUS).lock().unwrap() = Default::default();
        },
        quote! {
            generate!("call_observer")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use once_cell::sync::Lazy;
            use std::sync::Mutex;

            use ffi::TestObserver_methods;
            #[autocxx::subclass::subclass]
            #[derive(Default)]
            pub struct MyTestObserver {
            }
            impl TestObserver_methods for MyTestObserver {
                fn a(&self) {
                    assert!(!Lazy::force(&STATUS).lock().unwrap().dropped);
                    Lazy::force(&STATUS).lock().unwrap().sub_a_called = true;
                }
            }
            impl Drop for MyTestObserver {
                fn drop(&mut self) {
                    Lazy::force(&STATUS).lock().unwrap().dropped = true;
                }
            }

            #[derive(Default)]
            struct Status {
                sub_a_called: bool,
                dropped: bool,
            }

            static STATUS: Lazy<Mutex<Status>> = Lazy::new(|| Mutex::new(Status::default()));
        }),
    );
}

#[test]
fn test_cycle_nonpod_simple() {
    let hdr = indoc! {"
    #include <string>
    struct NonPod {
        std::string a;
    };
    inline NonPod make_non_pod(std::string a) {
        NonPod p;
        p.a = a;
        return p;
    }
    inline NonPod call_n(NonPod param) {
        return param;
    }
    "};
    let rs = quote! {
        let nonpod = ffi::make_non_pod("hello").within_unique_ptr();
        ffi::call_n(nonpod).within_unique_ptr();
    };
    run_test("", hdr, rs, &["NonPod", "make_non_pod", "call_n"], &[])
}

#[test]
fn test_pv_subclass_types() {
    let hdr = indoc! {"
    #include <cstdint>
    #include <string>
    #include <vector>

    struct Fwd;
    struct Pod {
        uint32_t a;
    };
    struct NonPod {
        std::string a;
    };
    class TestObserver {
    public:
        TestObserver() {}
        virtual std::string s(std::string p) const { return p; }
        virtual Pod p(Pod p) const { return p; }
        virtual NonPod n(NonPod p) const { return p; }
        virtual void f(const Fwd&) const { }
        virtual std::vector<NonPod> v(std::vector<NonPod> v) const { return v; }
        virtual const std::vector<NonPod>& vr(const std::vector<NonPod>& vr) const { return vr; }
        virtual const std::vector<Fwd>& vfr(const std::vector<Fwd>& vfr) const { return vfr; }
        virtual ~TestObserver() {}
    };

    extern TestObserver* obs;

    inline void register_observer(TestObserver& a) {
        obs = &a;
    }
    inline std::string call_s(std::string param) {
        return obs->s(param);
    }
    inline Pod call_p(Pod param) {
        return obs->p(param);
    }
    inline NonPod call_n(NonPod param) {
        return obs->n(param);
    }
    inline NonPod make_non_pod(std::string a) {
        NonPod p;
        p.a = a;
        return p;
    }
    "};
    run_test_ex(
        "TestObserver* obs;",
        hdr,
        quote! {
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::default()
            );
            ffi::register_observer(obs.as_ref().borrow_mut().pin_mut());
            ffi::call_p(ffi::Pod { a: 3 });
            ffi::call_s("hello");
            ffi::call_n(ffi::make_non_pod("goodbye").within_unique_ptr());
        },
        quote! {
            generate!("register_observer")
            generate!("call_s")
            generate!("call_n")
            generate!("call_p")
            generate!("NonPod")
            generate!("make_non_pod")
            generate_pod!("Pod")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::TestObserver_methods;
            #[autocxx::subclass::subclass]
            #[derive(Default)]
            pub struct MyTestObserver {
            }
            impl TestObserver_methods for MyTestObserver {
                fn s(&self, p: cxx::UniquePtr<cxx::CxxString>) -> cxx::UniquePtr<cxx::CxxString> {
                    self.peer().s_super(p)
                }

                fn p(&self, p: ffi::Pod) -> ffi::Pod {
                    self.peer().p_super(p)
                }

                fn n(&self, p: cxx::UniquePtr<ffi::NonPod>) -> cxx::UniquePtr<ffi::NonPod> {
                    self.peer().n_super(p)
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_constructors() {
    // Also tests a Rust-side subclass type which is an empty struct
    let hdr = indoc! {"
    #include <cstdint>
    #include <string>

    class TestObserver {
    public:
        TestObserver() {}
        TestObserver(uint8_t) {}
        TestObserver(std::string) {}
        virtual void call() const { }
        virtual ~TestObserver() {}
    };

    extern TestObserver* obs;

    inline void register_observer(TestObserver& a) {
        obs = &a;
    }
    inline void do_a_thing() {
        return obs->call();
    }
    "};
    run_test_ex(
        "TestObserver* obs;",
        hdr,
        quote! {
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::default()
            );
            ffi::register_observer(obs.as_ref().borrow_mut().pin_mut());
            ffi::do_a_thing();
        },
        quote! {
            generate!("register_observer")
            generate!("do_a_thing")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::prelude::*;
            #[subclass]
            #[derive(Default)]
            pub struct MyTestObserver;
            impl ffi::TestObserver_methods for MyTestObserver {
                fn call(&self) {
                    self.peer().call_super()
                }
            }
            impl CppPeerConstructor<ffi::MyTestObserverCpp> for MyTestObserver {
                fn make_peer(&mut self, peer_holder: CppSubclassRustPeerHolder<Self>) -> cxx::UniquePtr<ffi::MyTestObserverCpp> {
                    ffi::MyTestObserverCpp::new1(peer_holder, 3u8).within_unique_ptr()
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_fancy_constructor() {
    let hdr = indoc! {"
    #include <cstdint>

    class Observer {
    public:
        Observer(uint8_t) {}
        virtual uint32_t foo() const = 0;
        virtual ~Observer() {}
    };
    inline void take_observer(const Observer&) {}
    "};
    run_test_expect_fail_ex(
        "",
        hdr,
        quote! {
            let o = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() }, ffi::MyObserverCpp::make_unique);
            ffi::take_observer(o.borrow().as_ref());
        },
        quote! {
            generate!("take_observer")
            subclass!("Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            use ffi::Observer_methods;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl Observer_methods for MyObserver {
                fn foo(&self) -> u32 {
                    4
                }
            }
        }),
    );
}

#[test]
fn test_non_pv_subclass_overloads() {
    let hdr = indoc! {"
    #include <cstdint>
    #include <string>

    class TestObserver {
    public:
        TestObserver() {}
        virtual void call(uint8_t) const {}
        virtual void call(std::string) const {}
        virtual ~TestObserver() {}
    };

    extern TestObserver* obs;

    inline void register_observer(TestObserver& a) {
        obs = &a;
    }
    inline void do_a_thing() {
        return obs->call(8);
    }
    "};
    run_test_ex(
        "TestObserver* obs;",
        hdr,
        quote! {
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::default()
            );
            ffi::register_observer(obs.as_ref().borrow_mut().pin_mut());
            ffi::do_a_thing();
        },
        quote! {
            generate!("register_observer")
            generate!("do_a_thing")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::prelude::*;
            #[subclass]
            #[derive(Default)]
            pub struct MyTestObserver;
            impl ffi::TestObserver_methods for MyTestObserver {
                fn call(&self, a: u8) {
                    self.peer().call_super(a)
                }
                fn call1(&self, a: cxx::UniquePtr<cxx::CxxString>) {
                    self.peer().call1_super(a)
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_overrides() {
    let hdr = indoc! {"
    #include <cstdint>
    #include <string>

    class TestObserver {
    public:
        TestObserver() {}
        virtual void call(uint8_t) const = 0;
        virtual void call(std::string) const = 0;
        virtual ~TestObserver() {}
    };

    extern TestObserver* obs;

    inline void register_observer(TestObserver& a) {
        obs = &a;
    }
    inline void do_a_thing() {
        return obs->call(8);
    }
    "};
    run_test_ex(
        "TestObserver* obs;",
        hdr,
        quote! {
            let obs = MyTestObserver::new_rust_owned(
                MyTestObserver::default()
            );
            ffi::register_observer(obs.as_ref().borrow_mut().pin_mut());
            ffi::do_a_thing();
        },
        quote! {
            generate!("register_observer")
            generate!("do_a_thing")
            subclass!("TestObserver",MyTestObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::prelude::*;
            #[subclass]
            #[derive(Default)]
            pub struct MyTestObserver;
            impl ffi::TestObserver_methods for MyTestObserver {
                fn call(&self, _a: u8) {
                }
                fn call1(&self, _a: cxx::UniquePtr<cxx::CxxString>) {
                }
            }
        }),
    );
}

#[test]
fn test_pv_subclass_namespaced_superclass() {
    let hdr = indoc! {"
    #include <cstdint>

    namespace a {
    class Observer {
    public:
        Observer() {}
        virtual uint32_t foo() const = 0;
        virtual ~Observer() {}
    };
    }
    inline void take_observer(const a::Observer&) {}
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            let o = MyObserver::new_rust_owned(MyObserver { a: 3, cpp_peer: Default::default() });
            ffi::take_observer(o.borrow().as_ref());
        },
        quote! {
            generate!("take_observer")
            subclass!("a::Observer",MyObserver)
        },
        None,
        None,
        Some(quote! {
            use autocxx::subclass::CppSubclass;
            #[autocxx::subclass::subclass]
            pub struct MyObserver {
                a: u32
            }
            impl ffi::a::Observer_methods for MyObserver {
                fn foo(&self) -> u32 {
                    4
                }
            }
        }),
    );
}

#[test]
fn test_no_constructor_make_unique() {
    let hdr = indoc! {"
    #include <stdint.h>
    struct A {
        uint32_t a;
    };
    "};
    let rs = quote! {
        ffi::A::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_constructor_moveit() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {}
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        moveit! {
            let mut stack_obj = ffi::A::new();
        }
        stack_obj.as_mut().set(42);
        assert_eq!(stack_obj.get(), 42);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_move_out_of_uniqueptr() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {}
        std::string so_we_are_non_trivial;
    };
    inline A get_a() {
        A a;
        return a;
    }
    "};
    let rs = quote! {
        let a = ffi::get_a().within_unique_ptr();
        moveit! {
            let _stack_obj = autocxx::moveit::new::mov(a);
        }
    };
    run_test("", hdr, rs, &["A", "get_a"], &[]);
}

#[test]
fn test_implicit_constructor_with_typedef_field() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct B {
        uint32_t b;
    };
    typedef struct B C;
    struct A {
        B field;
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        moveit! {
            let mut stack_obj = ffi::A::new();
        }
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_implicit_constructor_with_array_field() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        uint32_t a[3];
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        moveit! {
            let mut _stack_obj = ffi::A::new();
        }
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_implicit_constructor_moveit() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        moveit! {
            let mut stack_obj = ffi::A::new();
        }
        stack_obj.as_mut().set(42);
        assert_eq!(stack_obj.get(), 42);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_pass_by_value_moveit() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        void set(uint32_t val) { a = val; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    inline void take_a(A) {}
    struct B {
        B() {}
        B(const B&) {}
        B(B&&) {}
        std::string so_we_are_non_trivial;
    };
    inline void take_b(B) {}
    "};
    let rs = quote! {
        moveit! {
            let mut stack_obj = ffi::A::new();
        }
        stack_obj.as_mut().set(42);
        ffi::take_a(&*stack_obj);
        ffi::take_a(as_copy(stack_obj.as_ref()));
        ffi::take_a(as_copy(stack_obj.as_ref()));
        // A has no move constructor so we can't consume it.

        let heap_obj = ffi::A::new().within_unique_ptr();
        ffi::take_a(heap_obj.as_ref().unwrap());
        ffi::take_a(&heap_obj);
        ffi::take_a(autocxx::as_copy(heap_obj.as_ref().unwrap()));
        ffi::take_a(heap_obj); // consume

        let heap_obj2 = ffi::A::new().within_box();
        ffi::take_a(heap_obj2.as_ref().get_ref());
        ffi::take_a(&heap_obj2);
        ffi::take_a(autocxx::as_copy(heap_obj2.as_ref().get_ref()));
        ffi::take_a(heap_obj2); // consume

        moveit! {
            let mut stack_obj = ffi::B::new();
        }
        ffi::take_b(&*stack_obj);
        ffi::take_b(as_copy(stack_obj.as_ref()));
        ffi::take_b(as_copy(stack_obj.as_ref()));
        ffi::take_b(as_mov(stack_obj)); // due to move constructor

        // Test direct-from-New-to-param.
        ffi::take_b(as_new(ffi::B::new()));
    };
    run_test("", hdr, rs, &["A", "take_a", "B", "take_b"], &[]);
}

#[test]
fn test_nonconst_reference_parameter() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>

    // Force generating a wrapper for the second `take_a`.
    struct NOP { void take_a() {}; };

    struct A {
        std::string so_we_are_non_trivial;
    };
    inline void take_a(A&) {}
    "};
    let rs = quote! {
        let mut heap_obj = ffi::A::new().within_unique_ptr();
        ffi::take_a(heap_obj.pin_mut());
    };
    run_test("", hdr, rs, &["NOP", "A", "take_a"], &[]);
}

#[test]
fn test_nonconst_reference_method_parameter() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>

    // Force generating a wrapper for the second `take_a`.
    struct NOP { void take_a() {}; };

    struct A {
        std::string so_we_are_non_trivial;
    };
    struct B {
        inline void take_a(A&) const {}
    };
    "};
    let rs = quote! {
        let mut a = ffi::A::new().within_unique_ptr();
        let b = ffi::B::new().within_unique_ptr();
        b.take_a(a.pin_mut());
    };
    run_test("", hdr, rs, &["NOP", "A", "B"], &[]);
}

fn destruction_test(ident: proc_macro2::Ident, extra_bit: Option<TokenStream>) {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    extern bool gConstructed;
    struct A {
        A() { gConstructed = true; }
        virtual ~A() { gConstructed = false; }
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    inline bool is_constructed() { return gConstructed; }
    struct B: public A {
        uint32_t b;
    };
    "};
    let cpp = indoc! {"
        bool gConstructed = false;
    "};
    let rs = quote! {
        assert!(!ffi::is_constructed());
        {
            moveit! {
                let mut _stack_obj = ffi::#ident::new();
            }
            assert!(ffi::is_constructed());
            #extra_bit
        }
        assert!(!ffi::is_constructed());
    };
    run_test(cpp, hdr, rs, &[&ident.to_string(), "is_constructed"], &[]);
}

#[test]
fn test_destructor_moveit() {
    destruction_test(
        parse_quote! { A },
        Some(quote! {
            _stack_obj.as_mut().set(42);
            assert_eq!(_stack_obj.get(), 42);
        }),
    );
}

#[test]
fn test_destructor_derived_moveit() {
    destruction_test(parse_quote! { B }, None);
}

#[test]
fn test_copy_and_move_constructor_moveit() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {}
        A(const A& other) : a(other.a+1) {}
        A(A&& other) : a(other.a+2) { other.a = 666; }
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        moveit! {
            let mut stack_obj = ffi::A::new();
        }
        stack_obj.as_mut().set(42);
        moveit! {
            let stack_obj2 = autocxx::moveit::new::copy(stack_obj.as_ref());
        }
        assert_eq!(stack_obj2.get(), 43);
        assert_eq!(stack_obj.get(), 42);
        moveit! {
            let stack_obj3 = autocxx::moveit::new::mov(stack_obj);
        }
        assert_eq!(stack_obj3.get(), 44);
        // Following line prevented by moveit, even though it would
        // be possible in C++.
        // assert_eq!(stack_obj.get(), 666);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

// This test fails on Windows gnu but not on Windows msvc
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[test]
fn test_uniqueptr_moveit() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {}
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        use autocxx::moveit::Emplace;
        let mut up_obj = cxx::UniquePtr::emplace(ffi::A::new());
        up_obj.as_mut().unwrap().set(42);
        assert_eq!(up_obj.get(), 42);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

// This test fails on Windows gnu but not on Windows msvc
#[cfg_attr(skip_windows_gnu_failing_tests, ignore)]
#[test]
fn test_various_emplacement() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {}
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {
        use autocxx::moveit::Emplace;
        let mut up_obj = cxx::UniquePtr::emplace(ffi::A::new());
        up_obj.pin_mut().set(666);
        // Can't current move out of a UniquePtr
        let mut box_obj = Box::emplace(ffi::A::new());
        box_obj.as_mut().set(667);
        let box_obj2 = Box::emplace(autocxx::moveit::new::mov(box_obj));
        moveit! { let back_on_stack = autocxx::moveit::new::mov(box_obj2); }
        assert_eq!(back_on_stack.get(), 667);
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_emplace_uses_overridden_new_and_delete() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {}
        void* operator new(size_t count);
        void operator delete(void* ptr) noexcept;
        void* operator new(size_t count, void* ptr);
        std::string so_we_are_non_trivial;
    };
    void reset_flags();
    bool was_new_called();
    bool was_delete_called();
    "};
    let cxx = indoc! {"
        bool new_called;
        bool delete_called;
        void reset_flags() {
            new_called = false;
            delete_called = false;
        }
        void* A::operator new(size_t count) {
            new_called = true;
            return ::operator new(count);
        }
        void* A::operator new(size_t count, void* ptr) {
            return ::operator new(count, ptr);
        }
        void A::operator delete(void* ptr) noexcept {
            delete_called = true;
            ::operator delete(ptr);
        }
        bool was_new_called() {
            return new_called;
        }
        bool was_delete_called() {
            return delete_called;
        }
    "};
    let rs = quote! {
        ffi::reset_flags();
        {
            let _ = ffi::A::new().within_unique_ptr();
            assert!(ffi::was_new_called());
        }
        assert!(ffi::was_delete_called());
        ffi::reset_flags();
        {
            use autocxx::moveit::Emplace;
            let _ = cxx::UniquePtr::emplace(ffi::A::new());
        }
        assert!(ffi::was_delete_called());
    };
    run_test(
        cxx,
        hdr,
        rs,
        &["A", "reset_flags", "was_new_called", "was_delete_called"],
        &[],
    );
}

#[test]
fn test_pass_by_reference_to_value_param() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() : count(0) {}
        std::string so_we_are_non_trivial;
        uint32_t count;
    };
    void take_a(A a) {
        a.count++;
    }
    uint32_t report_on_a(const A& a) {
        return a.count;
    }
    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        ffi::take_a(a.as_ref().unwrap());
        ffi::take_a(&a); // syntactic sugar
        assert_eq!(ffi::report_on_a(&a), 0); // should have acted upon copies
    };
    run_test("", hdr, rs, &["A", "take_a", "report_on_a"], &[]);
}

#[test]
fn test_explicit_everything() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    struct A {
        A() {} // default constructor
        A(A&&) {} // move constructor
        A(const A&) {} // copy constructor
        A& operator=(const A&) { return *this; } // copy assignment operator
        A& operator=(A&&) { return *this; } // move assignment operator
        ~A() {} // destructor
        void set(uint32_t val) { a = val; }
        uint32_t get() const { return a; }
        uint32_t a;
        std::string so_we_are_non_trivial;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_generate_ns() {
    let hdr = indoc! {"
    namespace A {
        inline void foo() {}
        inline void bar() {}
    }
    namespace B {
        inline void baz() {}
    }
    "};
    let rs = quote! {
        ffi::A::foo();
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! {
            generate_ns!("A")
            safety!(unsafe_ffi)
        },
        None,
        None,
        None,
    );
}

#[test]
fn test_no_constructor_make_unique_ns() {
    let hdr = indoc! {"
    #include <stdint.h>
    namespace B {
    struct A {
        uint32_t a;
    };
    }
    "};
    let rs = quote! {
        ffi::B::A::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &["B::A"], &[]);
}

#[test]
fn test_no_constructor_pod_make_unique() {
    let hdr = indoc! {"
    #include <stdint.h>
    struct A {
        uint32_t a;
    };
    "};
    let rs = quote! {
        ffi::A::new().within_unique_ptr();
    };
    run_test("", hdr, rs, &[], &["A"]);
}

#[test]
fn test_no_constructor_pv() {
    let hdr = indoc! {"
    #include <stdint.h>
    class A {
    public:
        virtual ~A() {}
        virtual void foo() = 0;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_suppress_system_includes() {
    let hdr = indoc! {"
    #include <stdint.h>
    #include <string>
    inline void a() {};
    "};
    let rs = quote! {};
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate("a")},
        Some(Box::new(SetSuppressSystemHeaders)),
        Some(Box::new(NoSystemHeadersChecker)),
        None,
    );
}

#[test]
fn test_no_rvo_move() {
    let hdr = indoc! {"
    #include <memory>
    class A {
    public:
        static std::unique_ptr<A> create() { return std::make_unique<A>(); }
    };
    "};
    let rs = quote! {
        ffi::A::create();
    };
    run_test_ex(
        "",
        hdr,
        rs,
        quote! { generate!("A") },
        None,
        Some(Box::new(CppMatcher::new(
            &["return A::create();"],
            &["return std::move(A::create());"],
        ))),
        None,
    );
}

#[test]
fn test_abstract_up() {
    let hdr = indoc! {"
    #include <memory>
    class A {
    public:
        virtual void foo() const = 0;
        virtual ~A() {}
    };
    class B : public A {
    public:
        void foo() const {}
    };
    inline std::unique_ptr<A> get_a() { return std::make_unique<B>(); }
    "};
    let rs = quote! {
        let a = ffi::get_a();
        a.foo();
    };
    run_test("", hdr, rs, &["A", "get_a"], &[]);
}

#[test]
fn test_abstract_up_multiple_bridge() {
    let hdr = indoc! {"
    #include <memory>
    class A {
    public:
        virtual void foo() const = 0;
        virtual ~A() {}
    };
    class B : public A {
    public:
        void foo() const {}
    };
    inline std::unique_ptr<A> get_a() { return std::make_unique<B>(); }
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            generate!("A")
        }
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            safety!(unsafe_ffi)
            name!(ffi2)
            extern_cpp_type!("A", crate::ffi::A)
            generate!("get_a")
        }
        fn main() {
            let a = ffi2::get_a();
            a.foo();
        }
    };
    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
fn test_abstract_private() {
    let hdr = indoc! {"
    #include <memory>
    class A {
        virtual void foo() const = 0;
    public:
        virtual ~A() {}
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_abstract_issue_979() {
    let hdr = indoc! {"
    class Test {
        virtual ~Test() {}
        virtual void TestBody() = 0;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["Test"], &[]);
}

#[test]
fn test_class_having_protected_method() {
    let hdr = indoc! {"
    #include <cstdint>
    class A {
    protected:
        inline uint32_t protected_method() { return 0; }
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["A"]);
}

#[test]
fn test_protected_inner_class() {
    let hdr = indoc! {"
    #include <cstdint>
    inline uint32_t DoMath(uint32_t a)  {
        return a * 3;
    }

    class A {
    protected:
        inline uint32_t protected_method() { return 0; }

        struct B {
            int x;
        };

        inline B protected_method_2() {
            return { 0 };
        }
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_private_inner_class() {
    let hdr = indoc! {"
    #include <cstdint>
    inline uint32_t DoMath(uint32_t a)  {
        return a * 3;
    }

    class A {
    protected:
        inline uint32_t protected_method() { return 0; }

    private:
        struct B {
            int x;
        };

        inline B private_method_2() {
            return { 0 };
        }
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_class_having_private_method() {
    let hdr = indoc! {"
    #include <cstdint>
    class A {
    private:
        inline uint32_t private_method() { return 0; }
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["A"]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/787
fn test_chrono_problem() {
    let hdr = indoc! {"
    #include <chrono>
    struct Clock {
      typedef std::chrono::nanoseconds duration;
    };
    struct Class {
      int a() { return 42; }
      std::chrono::time_point<Clock> b();
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &[], &["Class"]);
}

fn size_and_alignment_test(pod: bool) {
    static TYPES: [(&str, &str); 6] = [
        ("A", "struct A { uint8_t a; };"),
        ("B", "struct B { uint32_t a; };"),
        ("C", "struct C { uint64_t a; };"),
        ("D", "enum D { Z, X };"),
        ("E", "struct E { uint8_t a; uint32_t b; };"),
        ("F", "struct F { uint32_t a; uint8_t b; };"),
    ];
    let type_definitions = TYPES.iter().map(|(_, def)| *def).join("\n");
    let function_definitions = TYPES.iter().map(|(name, _)| format!("inline size_t get_sizeof_{name}() {{ return sizeof({name}); }}\ninline size_t get_alignof_{name}() {{ return alignof({name}); }}\n")).join("\n");
    let hdr = format!(
        indoc! {"
        #include <cstdint>
        #include <cstddef>
        {}
        {}
    "},
        type_definitions, function_definitions
    );
    #[allow(clippy::unnecessary_to_owned)] // wrongly triggers on into_iter() below
    let allowlist_fns: Vec<String> = TYPES
        .iter()
        .flat_map(|(name, _)| {
            [format!("get_sizeof_{name}"), format!("get_alignof_{name}")]
                .to_vec()
                .into_iter()
        })
        .collect_vec();
    let allowlist_types: Vec<String> = TYPES.iter().map(|(name, _)| name.to_string()).collect_vec();
    let allowlist_both = allowlist_types
        .iter()
        .cloned()
        .chain(allowlist_fns.iter().cloned())
        .collect_vec();
    let allowlist_types: Vec<&str> = allowlist_types.iter().map(AsRef::as_ref).collect_vec();
    let allowlist_fns: Vec<&str> = allowlist_fns.iter().map(AsRef::as_ref).collect_vec();
    let allowlist_both: Vec<&str> = allowlist_both.iter().map(AsRef::as_ref).collect_vec();
    let rs = TYPES.iter().fold(quote! {}, |mut accumulator, (name, _)| {
        let get_align_symbol =
            proc_macro2::Ident::new(&format!("get_alignof_{name}"), Span::call_site());
        let get_size_symbol =
            proc_macro2::Ident::new(&format!("get_sizeof_{name}"), Span::call_site());
        let type_symbol = proc_macro2::Ident::new(name, Span::call_site());
        accumulator.extend(quote! {
            let c_size = ffi::#get_size_symbol();
            let c_align = ffi::#get_align_symbol();
            assert_eq!(core::mem::size_of::<ffi::#type_symbol>(), c_size);
            assert_eq!(core::mem::align_of::<ffi::#type_symbol>(), c_align);
        });
        accumulator
    });
    if pod {
        run_test("", &hdr, rs, &allowlist_fns, &allowlist_types);
    } else {
        run_test("", &hdr, rs, &allowlist_both, &[]);
    }
}

#[test]
fn test_sizes_and_alignment_nonpod() {
    size_and_alignment_test(false)
}

#[test]
fn test_sizes_and_alignment_pod() {
    size_and_alignment_test(true)
}

#[test]
fn test_nested_class_methods() {
    let hdr = indoc! {"
    #include <cstdint>
    class A {
    public:
        virtual ~A() {}
        struct B {
            virtual void b() const {}
        };
        virtual void a() const {}
        struct C {
            virtual void b() const {}
        };
        virtual void c() const {}
        struct D {
            virtual void b() const {}
        };
    };
    "};
    let rs = quote! {
        let a = ffi::A::new().within_unique_ptr();
        a.a();
        a.c();
    };
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_call_superclass() {
    let hdr = indoc! {"
    #include <memory>
    class A {
    public:
        virtual void foo() const {};
        virtual ~A() {}
    };
    class B : public A {
    public:
        void bar() const {}
    };
    inline std::unique_ptr<B> get_b() { return std::make_unique<B>(); }
    "};
    let rs = quote! {
        let b = ffi::get_b();
        b.as_ref().unwrap().as_ref().foo();
    };
    run_test("", hdr, rs, &["A", "B", "get_b"], &[]);
}

#[test]
fn test_pass_superclass() {
    let hdr = indoc! {"
    #include <memory>
    class A {
    public:
        virtual void foo() const {};
        virtual ~A() {}
    };
    class B : public A {
    public:
        void bar() const {}
    };
    inline std::unique_ptr<B> get_b() { return std::make_unique<B>(); }
    inline void take_a(const A&) {}
    "};
    let rs = quote! {
        let b = ffi::get_b();
        ffi::take_a(b.as_ref().unwrap().as_ref());
    };
    run_test("", hdr, rs, &["A", "B", "get_b", "take_a"], &[]);
}

#[test]
fn test_issue_1238() {
    let hdr = indoc! {"
    class b;
    class c;
    class f {
        b d();
    };
    class S2E {
    public:
        f e;
        b &d(c *) const;
    };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["S2E"], &[]);
}

#[test]
fn test_issue486_multi_types() {
    let hdr = indoc! {"
        namespace a {
            namespace spanner {
                struct Key {};
            }
        } // namespace a
        namespace b {
            namespace spanner {
                typedef int Key;
            }
        } // namespace b
        namespace c {
            namespace spanner {
                enum Key { A, B };
            }
        } // namespace c
        namespace spanner {
            class Key {
                public:
                    bool a(a::spanner::Key &);
                    bool b(b::spanner::Key &);
                    bool c(c::spanner::Key &);
            };
        } // namespace spanner
    "};
    let rs = quote! {};
    run_test(
        "",
        hdr,
        rs,
        &["spanner::Key", "a::spanner::Key", "b::spanner::Key"],
        &[],
    );
}

#[test]
/// Tests types with various forms of copy, move, and default constructors. Calls the things which
/// should be generated, and will produce C++ compile failures if other wrappers are generated.
///
/// Specifically, we can have the cross product of any of these:
///   * Explicitly deleted
///   * Implicitly defaulted
///   * User declared
///   * Explicitly defaulted
///     Not handled yet: https://github.com/google/autocxx/issues/815.
///     Once this is handled, add equivalents of all the implicitly defaulted cases, at all
///     visibility levels.
/// applied to each of these:
///   * Default constructor
///   * Copy constructor
///   * Move constructor
/// in any of these:
///   * The class itself
///   * A base class
///   * A field of the class
///   * A field of a base class
/// with any of these access modifiers:
///   * private (impossible for implicitly defaulted)
///   * protected (impossible for implicitly defaulted)
///   * public
///
/// Various combinations of these lead to the default versions being deleted. The move and copy
/// ones also interact with each other in various ways.
///
/// TODO: Remove all the `int x` members after https://github.com/google/autocxx/issues/832 is
/// fixed.
fn test_implicit_constructor_rules() {
    let cxx = "";
    let hdr = indoc! {"
        struct AllImplicitlyDefaulted {
            void a() const {}
        };

        struct AllExplicitlyDefaulted {
            AllExplicitlyDefaulted() = default;
            AllExplicitlyDefaulted(const AllExplicitlyDefaulted&) = default;
            AllExplicitlyDefaulted(AllExplicitlyDefaulted&&) = default;
            void a() const {};
        };

        struct PublicDeleted {
            PublicDeleted() = delete;
            PublicDeleted(const PublicDeleted&) = delete;
            PublicDeleted(PublicDeleted&&) = delete;

            void a() const {}

            int x;
        };
        struct PublicDeletedDefault {
            PublicDeletedDefault() = delete;

            void a() const {}

            int x;
        };
        struct PublicDeletedCopy {
            PublicDeletedCopy() = default;
            PublicDeletedCopy(const PublicDeletedCopy&) = delete;

            void a() const {}

            int x;
        };
        struct PublicDeletedCopyNoDefault {
            PublicDeletedCopyNoDefault(const PublicDeletedCopyNoDefault&) = delete;

            void a() const {}

            int x;
        };
        struct PublicMoveDeletedCopy {
            PublicMoveDeletedCopy() = default;
            PublicMoveDeletedCopy(const PublicMoveDeletedCopy&) = delete;
            PublicMoveDeletedCopy(PublicMoveDeletedCopy&&) = default;

            void a() const {}

            int x;
        };
        struct PublicDeletedMove {
            PublicDeletedMove() = default;
            PublicDeletedMove(PublicDeletedMove&&) = delete;

            void a() const {}

            int x;
        };
        struct PublicDeletedDestructor {
            PublicDeletedDestructor() = default;
            ~PublicDeletedDestructor() = delete;

            void a() const {}

            int x;
        };
        struct PublicDestructor {
            PublicDestructor() = default;
            ~PublicDestructor() = default;

            void a() const {}

            int x;
        };

        struct ProtectedDeleted {
            void a() const {}

            int x;

          protected:
            ProtectedDeleted() = delete;
            ProtectedDeleted(const ProtectedDeleted&) = delete;
            ProtectedDeleted(ProtectedDeleted&&) = delete;
        };
        struct ProtectedDeletedDefault {
            void a() const {}

            int x;

          protected:
            ProtectedDeletedDefault() = delete;
        };
        struct ProtectedDeletedCopy {
            ProtectedDeletedCopy() = default;

            void a() const {}

            int x;

          protected:
            ProtectedDeletedCopy(const ProtectedDeletedCopy&) = delete;
        };
        struct ProtectedDeletedCopyNoDefault {
            void a() const {}

            int x;

          protected:
            ProtectedDeletedCopyNoDefault(const ProtectedDeletedCopyNoDefault&) = delete;
        };
        struct ProtectedMoveDeletedCopy {
            ProtectedMoveDeletedCopy() = default;

            void a() const {}

            int x;

          protected:
            ProtectedMoveDeletedCopy(const ProtectedMoveDeletedCopy&) = delete;
            ProtectedMoveDeletedCopy(ProtectedMoveDeletedCopy&&) = default;
        };
        struct ProtectedDeletedMove {
            ProtectedDeletedMove() = default;

            void a() const {}

            int x;

          protected:
            ProtectedDeletedMove(ProtectedDeletedMove&&) = delete;
        };
        struct ProtectedDeletedDestructor {
            ProtectedDeletedDestructor() = default;

            void a() const {}

            int x;

          protected:
            ~ProtectedDeletedDestructor() = delete;
        };
        struct ProtectedDestructor {
            ProtectedDestructor() = default;

            void a() const {}

            int x;

          protected:
            ~ProtectedDestructor() = default;
        };

        struct PrivateDeleted {
            void a() const {}

            int x;

          private:
            PrivateDeleted() = delete;
            PrivateDeleted(const PrivateDeleted&) = delete;
            PrivateDeleted(PrivateDeleted&&) = delete;
        };
        struct PrivateDeletedDefault {
            void a() const {}

            int x;

          private:
            PrivateDeletedDefault() = delete;
        };
        struct PrivateDeletedCopy {
            PrivateDeletedCopy() = default;

            void a() const {}

            int x;

          private:
            PrivateDeletedCopy(const PrivateDeletedCopy&) = delete;
        };
        struct PrivateDeletedCopyNoDefault {
            void a() const {}

            int x;

          private:
            PrivateDeletedCopyNoDefault(const PrivateDeletedCopyNoDefault&) = delete;
        };
        struct PrivateMoveDeletedCopy {
            PrivateMoveDeletedCopy() = default;

            void a() const {}

            int x;

          private:
            PrivateMoveDeletedCopy(const PrivateMoveDeletedCopy&) = delete;
            PrivateMoveDeletedCopy(PrivateMoveDeletedCopy&&) = default;
        };
        struct PrivateDeletedMove {
            PrivateDeletedMove() = default;

            void a() const {}

            int x;

          private:
            PrivateDeletedMove(PrivateDeletedMove&&) = delete;
        };
        struct PrivateDeletedDestructor {
            PrivateDeletedDestructor() = default;

            void a() const {}

            int x;

          private:
            ~PrivateDeletedDestructor() = delete;
        };
        struct PrivateDestructor {
            PrivateDestructor() = default;

            void a() const {}

            int x;

          private:
            ~PrivateDestructor() = default;
        };

        struct NonConstCopy {
            NonConstCopy() = default;

            NonConstCopy(NonConstCopy&) {}
            NonConstCopy(NonConstCopy&&) = default;

            void a() const {}
        };
        struct TwoCopy {
            TwoCopy() = default;

            TwoCopy(TwoCopy&) {}
            TwoCopy(const TwoCopy&) {}
            TwoCopy(TwoCopy&&) = default;

            void a() const {}
        };

        struct MemberPointerDeleted {
            PublicDeleted *x;

            void a() const {}
        };

        struct MemberConstPointerDeleted {
            PublicDeleted *const x;

            void a() const {}
        };

        struct MemberConst {
            const int x;

            void a() const {}
        };

        struct MemberReferenceDeleted {
            PublicDeleted &x;

            void a() const {}
        };

        struct MemberConstReferenceDeleted {
            const PublicDeleted &x;

            void a() const {}
        };

        struct MemberReference {
            int &x;

            void a() const {}
        };

        struct MemberConstReference {
            const int &x;

            void a() const {}
        };

        struct MemberRvalueReferenceDeleted {
            PublicDeleted &&x;

            void a() const {}
        };

        struct MemberRvalueReference {
            int &&x;

            void a() const {}
        };

        struct BasePublicDeleted : public PublicDeleted {};
        struct BasePublicDeletedDefault : public PublicDeletedDefault {};
        struct BasePublicDeletedCopy : public PublicDeletedCopy {};
        struct BasePublicDeletedCopyNoDefault : public PublicDeletedCopyNoDefault { };
        struct BasePublicMoveDeletedCopy : public PublicMoveDeletedCopy {};
        struct BasePublicDeletedMove : public PublicDeletedMove {};
        struct BasePublicDeletedDestructor : public PublicDeletedDestructor {};
        struct BasePublicDestructor : public PublicDestructor {};

        struct MemberPublicDeleted {
            void a() const {}

            PublicDeleted member;
        };
        struct MemberPublicDeletedDefault {
            void a() const {}

            PublicDeletedDefault member;
        };
        struct MemberPublicDeletedCopy {
            void a() const {}

            PublicDeletedCopy member;
        };
        struct MemberPublicDeletedCopyNoDefault {
            void a() const {}

            PublicDeletedCopyNoDefault member;
        };
        struct MemberPublicMoveDeletedCopy {
            void a() const {}

            PublicMoveDeletedCopy member;
        };
        struct MemberPublicDeletedMove {
            void a() const {}

            PublicDeletedMove member;
        };
        struct MemberPublicDeletedDestructor {
            void a() const {}

            PublicDeletedDestructor member;
        };
        struct MemberPublicDestructor {
            void a() const {}

            PublicDestructor member;
        };

        struct BaseMemberPublicDeleted : public MemberPublicDeleted {};
        struct BaseMemberPublicDeletedDefault : public MemberPublicDeletedDefault {};
        struct BaseMemberPublicDeletedCopy : public MemberPublicDeletedCopy {};
        struct BaseMemberPublicDeletedCopyNoDefault : public MemberPublicDeletedCopyNoDefault {};
        struct BaseMemberPublicMoveDeletedCopy : public MemberPublicMoveDeletedCopy {};
        struct BaseMemberPublicDeletedMove : public MemberPublicDeletedMove {};
        struct BaseMemberPublicDeletedDestructor : public MemberPublicDeletedDestructor {};
        struct BaseMemberPublicDestructor : public MemberPublicDestructor {};

        struct BaseProtectedDeleted : public ProtectedDeleted {};
        struct BaseProtectedDeletedDefault : public ProtectedDeletedDefault {};
        struct BaseProtectedDeletedCopy : public ProtectedDeletedCopy {};
        struct BaseProtectedDeletedCopyNoDefault : public ProtectedDeletedCopyNoDefault {};
        struct BaseProtectedMoveDeletedCopy : public ProtectedMoveDeletedCopy {};
        struct BaseProtectedDeletedMove : public ProtectedDeletedMove {};
        struct BaseProtectedDeletedDestructor : public ProtectedDeletedDestructor {};
        struct BaseProtectedDestructor : public ProtectedDestructor {};

        struct MemberProtectedDeleted {
            void a() const {}

            ProtectedDeleted member;
        };
        struct MemberProtectedDeletedDefault {
            void a() const {}

            ProtectedDeletedDefault member;
        };
        struct MemberProtectedDeletedCopy {
            void a() const {}

            ProtectedDeletedCopy member;
        };
        struct MemberProtectedDeletedCopyNoDefault {
            void a() const {}

            ProtectedDeletedCopyNoDefault member;
        };
        struct MemberProtectedMoveDeletedCopy {
            void a() const {}

            ProtectedMoveDeletedCopy member;
        };
        struct MemberProtectedDeletedMove {
            void a() const {}

            ProtectedDeletedMove member;
        };
        struct MemberProtectedDeletedDestructor {
            void a() const {}

            ProtectedDeletedDestructor member;
        };
        struct MemberProtectedDestructor {
            void a() const {}

            ProtectedDestructor member;
        };

        struct BaseMemberProtectedDeleted : public MemberProtectedDeleted {};
        struct BaseMemberProtectedDeletedDefault : public MemberProtectedDeletedDefault {};
        struct BaseMemberProtectedDeletedCopy : public MemberProtectedDeletedCopy {};
        struct BaseMemberProtectedDeletedCopyNoDefault : public MemberProtectedDeletedCopyNoDefault {};
        struct BaseMemberProtectedMoveDeletedCopy : public MemberProtectedMoveDeletedCopy {};
        struct BaseMemberProtectedDeletedMove : public MemberProtectedDeletedMove {};
        struct BaseMemberProtectedDeletedDestructor : public MemberProtectedDeletedDestructor {};
        struct BaseMemberProtectedDestructor : public MemberProtectedDestructor {};

        struct BasePrivateDeleted : public PrivateDeleted {};
        struct BasePrivateDeletedDefault : public PrivateDeletedDefault {};
        struct BasePrivateDeletedCopy : public PrivateDeletedCopy {};
        struct BasePrivateDeletedCopyNoDefault : public PrivateDeletedCopyNoDefault {};
        struct BasePrivateMoveDeletedCopy : public PrivateMoveDeletedCopy {};
        struct BasePrivateDeletedMove : public PrivateDeletedMove {};
        struct BasePrivateDeletedDestructor : public PrivateDeletedDestructor {};
        struct BasePrivateDestructor : public PrivateDestructor {};

        struct MemberPrivateDeleted {
            void a() const {}

            PrivateDeleted member;
        };
        struct MemberPrivateDeletedDefault {
            void a() const {}

            PrivateDeletedDefault member;
        };
        struct MemberPrivateDeletedCopy {
            void a() const {}

            PrivateDeletedCopy member;
        };
        struct MemberPrivateDeletedCopyNoDefault {
            void a() const {}

            PrivateDeletedCopyNoDefault member;
        };
        struct MemberPrivateMoveDeletedCopy {
            void a() const {}

            PrivateMoveDeletedCopy member;
        };
        struct MemberPrivateDeletedMove {
            void a() const {}

            PrivateDeletedMove member;
        };
        struct MemberPrivateDeletedDestructor {
            void a() const {}

            PrivateDeletedDestructor member;
        };
        struct MemberPrivateDestructor {
            void a() const {}

            PrivateDestructor member;
        };

        struct BaseMemberPrivateDeleted : public MemberPrivateDeleted {};
        struct BaseMemberPrivateDeletedDefault : public MemberPrivateDeletedDefault {};
        struct BaseMemberPrivateDeletedCopy : public MemberPrivateDeletedCopy {};
        struct BaseMemberPrivateDeletedCopyNoDefault : public MemberPrivateDeletedCopyNoDefault {};
        struct BaseMemberPrivateMoveDeletedCopy : public MemberPrivateMoveDeletedCopy {};
        struct BaseMemberPrivateDeletedMove : public MemberPrivateDeletedMove {};
        struct BaseMemberPrivateDeletedDestructor : public MemberPrivateDeletedDestructor {};
        struct BaseMemberPrivateDestructor : public MemberPrivateDestructor {};
    "};
    let rs = quote! {
        // Some macros to test various operations on our types. Note that some of them define
        // functions which take arguments that the APIs defined in this test have no way to
        // produce, because we have C++ types which can't be constructed (for example). In a real
        // program, there might be other C++ APIs which can instantiate these types.

        // TODO: https://github.com/google/autocxx/issues/829: Should this be merged with
        // `test_make_unique`? Currently types where the Rust wrappers permit this but not that
        // aren't running C++ destructors.
        macro_rules! test_constructible {
            [$t:ty] => {
                moveit! {
                    let _moveit_t = <$t>::new();
                }
            }
        }
        macro_rules! test_make_unique {
            [$t:ty] => {
                let _unique_t = <$t>::new().within_unique_ptr();
            }
        }
        macro_rules! test_copyable {
            [$t:ty] => {
                {
                    fn test_copyable(moveit_t: impl autocxx::moveit::new::New<Output = $t>) {
                        moveit! {
                            let moveit_t = moveit_t;
                            let _copied_t = autocxx::moveit::new::copy(moveit_t);
                        }
                    }
                }
            }
        }
        macro_rules! test_movable {
            [$t:ty] => {
                {
                    fn test_movable(moveit_t: impl autocxx::moveit::new::New<Output = $t>) {
                        moveit! {
                            let moveit_t = moveit_t;
                            let _moved_t = autocxx::moveit::new::mov(moveit_t);
                        }
                    }
                }
            }
        }
        macro_rules! test_call_a {
            [$t:ty] => {
                {
                    fn test_call_a(t: &$t) {
                        t.a();
                    }
                }
            }
        }
        macro_rules! test_call_a_as {
            [$t:ty, $parent:ty] => {
                {
                    fn test_call_a(t: &$t) {
                        let t: &$parent = t.as_ref();
                        t.a();
                    }
                }
            }
        }

        test_constructible![ffi::AllImplicitlyDefaulted];
        test_make_unique![ffi::AllImplicitlyDefaulted];
        test_copyable![ffi::AllImplicitlyDefaulted];
        test_movable![ffi::AllImplicitlyDefaulted];
        test_call_a![ffi::AllImplicitlyDefaulted];

        test_constructible![ffi::AllExplicitlyDefaulted];
        test_make_unique![ffi::AllExplicitlyDefaulted];
        test_copyable![ffi::AllExplicitlyDefaulted];
        test_movable![ffi::AllExplicitlyDefaulted];
        test_call_a![ffi::AllExplicitlyDefaulted];

        test_call_a![ffi::PublicDeleted];

        test_copyable![ffi::PublicDeletedDefault];
        test_movable![ffi::PublicDeletedDefault];
        test_call_a![ffi::PublicDeletedDefault];

        test_constructible![ffi::PublicDeletedCopy];
        test_make_unique![ffi::PublicDeletedCopy];
        test_call_a![ffi::PublicDeletedCopy];

        test_call_a![ffi::PublicDeletedCopyNoDefault];

        test_constructible![ffi::PublicMoveDeletedCopy];
        test_make_unique![ffi::PublicMoveDeletedCopy];
        test_movable![ffi::PublicMoveDeletedCopy];
        test_call_a![ffi::PublicMoveDeletedCopy];

        test_constructible![ffi::PublicDeletedMove];
        test_make_unique![ffi::PublicDeletedMove];
        test_call_a![ffi::PublicDeletedMove];

        test_constructible![ffi::PublicDeletedDestructor];
        test_copyable![ffi::PublicDeletedDestructor];
        test_call_a![ffi::PublicDeletedDestructor];

        test_constructible![ffi::PublicDestructor];
        test_make_unique![ffi::PublicDestructor];
        test_copyable![ffi::PublicDestructor];
        test_call_a![ffi::PublicDestructor];

        test_call_a![ffi::ProtectedDeleted];

        test_copyable![ffi::ProtectedDeletedDefault];
        test_movable![ffi::ProtectedDeletedDefault];
        test_call_a![ffi::ProtectedDeletedDefault];

        test_constructible![ffi::ProtectedDeletedCopy];
        test_make_unique![ffi::ProtectedDeletedCopy];
        test_call_a![ffi::ProtectedDeletedCopy];

        test_call_a![ffi::ProtectedDeletedCopyNoDefault];

        test_constructible![ffi::ProtectedMoveDeletedCopy];
        test_make_unique![ffi::ProtectedMoveDeletedCopy];
        test_call_a![ffi::ProtectedMoveDeletedCopy];

        test_constructible![ffi::ProtectedDeletedMove];
        test_make_unique![ffi::ProtectedDeletedMove];
        test_call_a![ffi::ProtectedDeletedMove];

        test_constructible![ffi::ProtectedDeletedDestructor];
        test_copyable![ffi::ProtectedDeletedDestructor];
        test_call_a![ffi::ProtectedDeletedDestructor];

        test_constructible![ffi::ProtectedDestructor];
        test_copyable![ffi::ProtectedDestructor];
        test_call_a![ffi::ProtectedDestructor];

        test_call_a![ffi::PrivateDeleted];

        test_copyable![ffi::PrivateDeletedDefault];
        test_movable![ffi::PrivateDeletedDefault];
        test_call_a![ffi::PrivateDeletedDefault];

        test_constructible![ffi::PrivateDeletedCopy];
        test_make_unique![ffi::PrivateDeletedCopy];
        test_call_a![ffi::PrivateDeletedCopy];

        test_call_a![ffi::PrivateDeletedCopyNoDefault];

        test_constructible![ffi::PrivateMoveDeletedCopy];
        test_make_unique![ffi::PrivateMoveDeletedCopy];
        test_call_a![ffi::PrivateMoveDeletedCopy];

        test_constructible![ffi::PrivateDeletedMove];
        test_make_unique![ffi::PrivateDeletedMove];
        test_call_a![ffi::PrivateDeletedMove];

        test_constructible![ffi::PrivateDeletedDestructor];
        test_copyable![ffi::PrivateDeletedDestructor];
        test_call_a![ffi::PrivateDeletedDestructor];

        test_constructible![ffi::PrivateDestructor];
        test_copyable![ffi::PrivateDestructor];
        test_call_a![ffi::PrivateDestructor];

        test_constructible![ffi::NonConstCopy];
        test_make_unique![ffi::NonConstCopy];
        test_movable![ffi::NonConstCopy];
        test_call_a![ffi::NonConstCopy];

        test_constructible![ffi::TwoCopy];
        test_make_unique![ffi::TwoCopy];
        test_copyable![ffi::TwoCopy];
        test_movable![ffi::TwoCopy];
        test_call_a![ffi::TwoCopy];

        // TODO: https://github.com/google/autocxx/issues/865
        // Treat pointers and references differently so this has a default constructor.
        //test_constructible![ffi::MemberPointerDeleted];
        //test_make_unique![ffi::MemberPointerDeleted];
        test_copyable![ffi::MemberPointerDeleted];
        test_movable![ffi::MemberPointerDeleted];
        test_call_a![ffi::MemberPointerDeleted];

        test_copyable![ffi::MemberConstPointerDeleted];
        test_movable![ffi::MemberConstPointerDeleted];
        test_call_a![ffi::MemberConstPointerDeleted];

        //test_copyable![ffi::MemberConst];
        //test_movable![ffi::MemberConst];
        //test_call_a![ffi::MemberConst];

        test_copyable![ffi::MemberReferenceDeleted];
        test_movable![ffi::MemberReferenceDeleted];
        test_call_a![ffi::MemberReferenceDeleted];

        test_copyable![ffi::MemberConstReferenceDeleted];
        test_movable![ffi::MemberConstReferenceDeleted];
        test_call_a![ffi::MemberConstReferenceDeleted];

        test_copyable![ffi::MemberReference];
        test_movable![ffi::MemberReference];
        test_call_a![ffi::MemberReference];

        test_copyable![ffi::MemberConstReference];
        test_movable![ffi::MemberConstReference];
        test_call_a![ffi::MemberConstReference];

        test_movable![ffi::MemberRvalueReferenceDeleted];
        test_call_a![ffi::MemberRvalueReferenceDeleted];

        test_movable![ffi::MemberRvalueReference];
        test_call_a![ffi::MemberRvalueReference];

        test_call_a_as![ffi::BasePublicDeleted, ffi::PublicDeleted];

        test_copyable![ffi::BasePublicDeletedDefault];
        test_movable![ffi::BasePublicDeletedDefault];
        test_call_a_as![ffi::BasePublicDeletedDefault, ffi::PublicDeletedDefault];

        test_constructible![ffi::BasePublicDeletedCopy];
        test_make_unique![ffi::BasePublicDeletedCopy];
        test_call_a_as![ffi::BasePublicDeletedCopy, ffi::PublicDeletedCopy];

        test_call_a_as![ffi::BasePublicDeletedCopyNoDefault, ffi::PublicDeletedCopyNoDefault];

        test_constructible![ffi::BasePublicMoveDeletedCopy];
        test_make_unique![ffi::BasePublicMoveDeletedCopy];
        test_movable![ffi::BasePublicMoveDeletedCopy];
        test_call_a_as![ffi::BasePublicMoveDeletedCopy, ffi::PublicMoveDeletedCopy];

        test_constructible![ffi::BasePublicDeletedMove];
        test_make_unique![ffi::BasePublicDeletedMove];
        test_call_a_as![ffi::BasePublicDeletedMove, ffi::PublicDeletedMove];

        test_call_a_as![ffi::BasePublicDeletedDestructor, ffi::PublicDeletedDestructor];

        test_constructible![ffi::BasePublicDestructor];
        test_make_unique![ffi::BasePublicDestructor];
        test_copyable![ffi::BasePublicDestructor];
        test_call_a_as![ffi::BasePublicDestructor, ffi::PublicDestructor];

        test_call_a![ffi::MemberPublicDeleted];

        test_copyable![ffi::MemberPublicDeletedDefault];
        test_movable![ffi::MemberPublicDeletedDefault];
        test_call_a![ffi::MemberPublicDeletedDefault];

        test_constructible![ffi::MemberPublicDeletedCopy];
        test_make_unique![ffi::MemberPublicDeletedCopy];
        test_call_a![ffi::MemberPublicDeletedCopy];

        test_call_a![ffi::MemberPublicDeletedCopyNoDefault];

        test_constructible![ffi::MemberPublicMoveDeletedCopy];
        test_make_unique![ffi::MemberPublicMoveDeletedCopy];
        test_movable![ffi::MemberPublicMoveDeletedCopy];
        test_call_a![ffi::MemberPublicMoveDeletedCopy];

        test_constructible![ffi::MemberPublicDeletedMove];
        test_make_unique![ffi::MemberPublicDeletedMove];
        test_call_a![ffi::MemberPublicDeletedMove];

        test_call_a![ffi::MemberPublicDeletedDestructor];

        test_constructible![ffi::MemberPublicDestructor];
        test_make_unique![ffi::MemberPublicDestructor];
        test_copyable![ffi::MemberPublicDestructor];
        test_call_a![ffi::MemberPublicDestructor];

        test_call_a_as![ffi::BaseMemberPublicDeleted, ffi::MemberPublicDeleted];

        test_copyable![ffi::BaseMemberPublicDeletedDefault];
        test_movable![ffi::BaseMemberPublicDeletedDefault];
        test_call_a_as![ffi::BaseMemberPublicDeletedDefault, ffi::MemberPublicDeletedDefault];

        test_constructible![ffi::BaseMemberPublicDeletedCopy];
        test_make_unique![ffi::BaseMemberPublicDeletedCopy];
        test_call_a_as![ffi::BaseMemberPublicDeletedCopy, ffi::MemberPublicDeletedCopy];

        test_call_a_as![ffi::BaseMemberPublicDeletedCopyNoDefault, ffi::MemberPublicDeletedCopyNoDefault];

        test_constructible![ffi::BaseMemberPublicMoveDeletedCopy];
        test_make_unique![ffi::BaseMemberPublicMoveDeletedCopy];
        test_movable![ffi::BaseMemberPublicMoveDeletedCopy];
        test_call_a_as![ffi::BaseMemberPublicMoveDeletedCopy, ffi::MemberPublicMoveDeletedCopy];

        test_constructible![ffi::BaseMemberPublicDeletedMove];
        test_make_unique![ffi::BaseMemberPublicDeletedMove];
        test_call_a_as![ffi::BaseMemberPublicDeletedMove, ffi::MemberPublicDeletedMove];

        test_call_a_as![ffi::BaseMemberPublicDeletedDestructor, ffi::MemberPublicDeletedDestructor];

        test_constructible![ffi::BaseMemberPublicDestructor];
        test_make_unique![ffi::BaseMemberPublicDestructor];
        test_copyable![ffi::BaseMemberPublicDestructor];
        test_call_a_as![ffi::BaseMemberPublicDestructor, ffi::MemberPublicDestructor];

        test_call_a_as![ffi::BaseProtectedDeleted, ffi::ProtectedDeleted];

        test_copyable![ffi::BaseProtectedDeletedDefault];
        test_movable![ffi::BaseProtectedDeletedDefault];
        test_call_a_as![ffi::BaseProtectedDeletedDefault, ffi::ProtectedDeletedDefault];

        test_constructible![ffi::BaseProtectedDeletedCopy];
        test_make_unique![ffi::BaseProtectedDeletedCopy];
        test_call_a_as![ffi::BaseProtectedDeletedCopy, ffi::ProtectedDeletedCopy];

        test_call_a_as![ffi::BaseProtectedDeletedCopyNoDefault, ffi::ProtectedDeletedCopyNoDefault];

        test_constructible![ffi::BaseProtectedMoveDeletedCopy];
        test_make_unique![ffi::BaseProtectedMoveDeletedCopy];
        test_movable![ffi::BaseProtectedMoveDeletedCopy];
        test_call_a_as![ffi::BaseProtectedMoveDeletedCopy, ffi::ProtectedMoveDeletedCopy];

        test_constructible![ffi::BaseProtectedDeletedMove];
        test_make_unique![ffi::BaseProtectedDeletedMove];
        test_call_a_as![ffi::BaseProtectedDeletedMove, ffi::ProtectedDeletedMove];

        test_call_a_as![ffi::BaseProtectedDeletedDestructor, ffi::ProtectedDeletedDestructor];

        test_constructible![ffi::BaseProtectedDestructor];
        test_make_unique![ffi::BaseProtectedDestructor];
        test_copyable![ffi::BaseProtectedDestructor];
        test_call_a_as![ffi::BaseProtectedDestructor, ffi::ProtectedDestructor];

        test_call_a![ffi::MemberProtectedDeleted];

        test_copyable![ffi::MemberProtectedDeletedDefault];
        test_movable![ffi::MemberProtectedDeletedDefault];
        test_call_a![ffi::MemberProtectedDeletedDefault];

        test_constructible![ffi::MemberProtectedDeletedCopy];
        test_make_unique![ffi::MemberProtectedDeletedCopy];
        test_call_a![ffi::MemberProtectedDeletedCopy];

        test_call_a![ffi::MemberProtectedDeletedCopyNoDefault];

        test_constructible![ffi::MemberProtectedMoveDeletedCopy];
        test_make_unique![ffi::MemberProtectedMoveDeletedCopy];
        test_call_a![ffi::MemberProtectedMoveDeletedCopy];

        test_constructible![ffi::MemberProtectedDeletedMove];
        test_make_unique![ffi::MemberProtectedDeletedMove];
        test_call_a![ffi::MemberProtectedDeletedMove];

        test_call_a![ffi::MemberProtectedDeletedDestructor];

        test_call_a![ffi::MemberProtectedDestructor];

        test_call_a_as![ffi::BaseMemberProtectedDeleted, ffi::MemberProtectedDeleted];

        test_copyable![ffi::BaseMemberProtectedDeletedDefault];
        test_movable![ffi::BaseMemberProtectedDeletedDefault];
        test_call_a_as![ffi::BaseMemberProtectedDeletedDefault, ffi::MemberProtectedDeletedDefault];

        test_constructible![ffi::BaseMemberProtectedDeletedCopy];
        test_make_unique![ffi::BaseMemberProtectedDeletedCopy];
        test_call_a_as![ffi::BaseMemberProtectedDeletedCopy, ffi::MemberProtectedDeletedCopy];

        test_call_a_as![ffi::BaseMemberProtectedDeletedCopyNoDefault, ffi::MemberProtectedDeletedCopyNoDefault];

        test_constructible![ffi::BaseMemberProtectedMoveDeletedCopy];
        test_make_unique![ffi::BaseMemberProtectedMoveDeletedCopy];
        test_call_a_as![ffi::BaseMemberProtectedMoveDeletedCopy, ffi::MemberProtectedMoveDeletedCopy];

        test_constructible![ffi::BaseMemberProtectedDeletedMove];
        test_make_unique![ffi::BaseMemberProtectedDeletedMove];
        test_call_a_as![ffi::BaseMemberProtectedDeletedMove, ffi::MemberProtectedDeletedMove];

        test_call_a_as![ffi::BaseMemberProtectedDeletedDestructor, ffi::MemberProtectedDeletedDestructor];

        test_call_a_as![ffi::BaseMemberProtectedDestructor, ffi::MemberProtectedDestructor];

        test_call_a_as![ffi::BasePrivateDeleted, ffi::PrivateDeleted];

        test_copyable![ffi::BasePrivateDeletedDefault];
        test_movable![ffi::BasePrivateDeletedDefault];
        test_call_a_as![ffi::BasePrivateDeletedDefault, ffi::PrivateDeletedDefault];

        test_constructible![ffi::BasePrivateDeletedCopy];
        test_make_unique![ffi::BasePrivateDeletedCopy];
        test_call_a_as![ffi::BasePrivateDeletedCopy, ffi::PrivateDeletedCopy];

        test_call_a_as![ffi::BasePrivateDeletedCopyNoDefault, ffi::PrivateDeletedCopyNoDefault];

        test_constructible![ffi::BasePrivateMoveDeletedCopy];
        test_make_unique![ffi::BasePrivateMoveDeletedCopy];
        test_call_a_as![ffi::BasePrivateMoveDeletedCopy, ffi::PrivateMoveDeletedCopy];

        test_constructible![ffi::BasePrivateDeletedMove];
        test_make_unique![ffi::BasePrivateDeletedMove];
        test_call_a_as![ffi::BasePrivateDeletedMove, ffi::PrivateDeletedMove];

        test_call_a_as![ffi::BasePrivateDeletedDestructor, ffi::PrivateDeletedDestructor];

        test_call_a_as![ffi::BasePrivateDestructor, ffi::PrivateDestructor];

        test_call_a![ffi::MemberPrivateDeleted];

        test_copyable![ffi::MemberPrivateDeletedDefault];
        test_movable![ffi::MemberPrivateDeletedDefault];
        test_call_a![ffi::MemberPrivateDeletedDefault];

        test_constructible![ffi::MemberPrivateDeletedCopy];
        test_make_unique![ffi::MemberPrivateDeletedCopy];
        test_call_a![ffi::MemberPrivateDeletedCopy];

        test_call_a![ffi::MemberPrivateDeletedCopyNoDefault];

        test_constructible![ffi::MemberPrivateMoveDeletedCopy];
        test_make_unique![ffi::MemberPrivateMoveDeletedCopy];
        test_call_a![ffi::MemberPrivateMoveDeletedCopy];

        test_constructible![ffi::MemberPrivateDeletedMove];
        test_make_unique![ffi::MemberPrivateDeletedMove];
        test_call_a![ffi::MemberPrivateDeletedMove];

        test_call_a![ffi::MemberPrivateDeletedDestructor];

        test_call_a![ffi::MemberPrivateDestructor];

        test_call_a_as![ffi::BaseMemberPrivateDeleted, ffi::MemberPrivateDeleted];

        test_copyable![ffi::BaseMemberPrivateDeletedDefault];
        test_movable![ffi::BaseMemberPrivateDeletedDefault];
        test_call_a_as![ffi::BaseMemberPrivateDeletedDefault, ffi::MemberPrivateDeletedDefault];

        test_constructible![ffi::BaseMemberPrivateDeletedCopy];
        test_make_unique![ffi::BaseMemberPrivateDeletedCopy];
        test_call_a_as![ffi::BaseMemberPrivateDeletedCopy, ffi::MemberPrivateDeletedCopy];

        test_call_a_as![ffi::BaseMemberPrivateDeletedCopyNoDefault, ffi::MemberPrivateDeletedCopyNoDefault];

        test_constructible![ffi::BaseMemberPrivateMoveDeletedCopy];
        test_make_unique![ffi::BaseMemberPrivateMoveDeletedCopy];
        test_call_a_as![ffi::BaseMemberPrivateMoveDeletedCopy, ffi::MemberPrivateMoveDeletedCopy];

        test_constructible![ffi::BaseMemberPrivateDeletedMove];
        test_make_unique![ffi::BaseMemberPrivateDeletedMove];
        test_call_a_as![ffi::BaseMemberPrivateDeletedMove, ffi::MemberPrivateDeletedMove];

        test_call_a_as![ffi::BaseMemberPrivateDeletedDestructor, ffi::MemberPrivateDeletedDestructor];

        test_call_a_as![ffi::BaseMemberPrivateDestructor, ffi::MemberPrivateDestructor];
    };
    run_test(
        cxx,
        hdr,
        rs,
        &[
            "AllImplicitlyDefaulted",
            "AllExplicitlyDefaulted",
            "PublicDeleted",
            "PublicDeletedDefault",
            "PublicDeletedCopy",
            "PublicDeletedCopyNoDefault",
            "PublicMoveDeletedCopy",
            "PublicDeletedMove",
            "PublicDeletedDestructor",
            "PublicDestructor",
            "ProtectedDeleted",
            "ProtectedDeletedDefault",
            "ProtectedDeletedCopy",
            "ProtectedDeletedCopyNoDefault",
            "ProtectedMoveDeletedCopy",
            "ProtectedDeletedMove",
            "ProtectedDeletedDestructor",
            "ProtectedDestructor",
            "PrivateDeleted",
            "PrivateDeletedDefault",
            "PrivateDeletedCopy",
            "PrivateDeletedCopyNoDefault",
            "PrivateMoveDeletedCopy",
            "PrivateDeletedMove",
            "PrivateDeletedDestructor",
            "PrivateDestructor",
            "NonConstCopy",
            "TwoCopy",
            "MemberPointerDeleted",
            "MemberConstPointerDeleted",
            // TODO: Handle top-level const on C++ members correctly.
            //"MemberConst",
            "MemberReferenceDeleted",
            "MemberConstReferenceDeleted",
            "MemberReference",
            "MemberConstReference",
            "MemberRvalueReferenceDeleted",
            "MemberRvalueReference",
            "BasePublicDeleted",
            "BasePublicDeletedDefault",
            "BasePublicDeletedCopy",
            "BasePublicDeletedCopyNoDefault",
            "BasePublicMoveDeletedCopy",
            "BasePublicDeletedMove",
            "BasePublicDeletedDestructor",
            "BasePublicDestructor",
            "MemberPublicDeleted",
            "MemberPublicDeletedDefault",
            "MemberPublicDeletedCopy",
            "MemberPublicDeletedCopyNoDefault",
            "MemberPublicMoveDeletedCopy",
            "MemberPublicDeletedMove",
            "MemberPublicDeletedDestructor",
            "MemberPublicDestructor",
            "BaseMemberPublicDeleted",
            "BaseMemberPublicDeletedDefault",
            "BaseMemberPublicDeletedCopy",
            "BaseMemberPublicDeletedCopyNoDefault",
            "BaseMemberPublicMoveDeletedCopy",
            "BaseMemberPublicDeletedMove",
            "BaseMemberPublicDeletedDestructor",
            "BaseMemberPublicDestructor",
            "BaseProtectedDeleted",
            "BaseProtectedDeletedDefault",
            "BaseProtectedDeletedCopy",
            "BaseProtectedDeletedCopyNoDefault",
            "BaseProtectedMoveDeletedCopy",
            "BaseProtectedDeletedMove",
            "BaseProtectedDeletedDestructor",
            "BaseProtectedDestructor",
            "MemberProtectedDeleted",
            "MemberProtectedDeletedDefault",
            "MemberProtectedDeletedCopy",
            "MemberProtectedDeletedCopyNoDefault",
            "MemberProtectedMoveDeletedCopy",
            "MemberProtectedDeletedMove",
            "MemberProtectedDeletedDestructor",
            "MemberProtectedDestructor",
            "BaseMemberProtectedDeleted",
            "BaseMemberProtectedDeletedDefault",
            "BaseMemberProtectedDeletedCopy",
            "BaseMemberProtectedDeletedCopyNoDefault",
            "BaseMemberProtectedMoveDeletedCopy",
            "BaseMemberProtectedDeletedMove",
            "BaseMemberProtectedDeletedDestructor",
            "BaseMemberProtectedDestructor",
            "BasePrivateDeleted",
            "BasePrivateDeletedDefault",
            "BasePrivateDeletedCopy",
            "BasePrivateDeletedCopyNoDefault",
            "BasePrivateMoveDeletedCopy",
            "BasePrivateDeletedMove",
            "BasePrivateDeletedDestructor",
            "BasePrivateDestructor",
            "MemberPrivateDeleted",
            "MemberPrivateDeletedDefault",
            "MemberPrivateDeletedCopy",
            "MemberPrivateDeletedCopyNoDefault",
            "MemberPrivateMoveDeletedCopy",
            "MemberPrivateDeletedMove",
            "MemberPrivateDeletedDestructor",
            "MemberPrivateDestructor",
            "BaseMemberPrivateDeleted",
            "BaseMemberPrivateDeletedDefault",
            "BaseMemberPrivateDeletedCopy",
            "BaseMemberPrivateDeletedCopyNoDefault",
            "BaseMemberPrivateMoveDeletedCopy",
            "BaseMemberPrivateDeletedMove",
            "BaseMemberPrivateDeletedDestructor",
            "BaseMemberPrivateDestructor",
        ],
        &[],
    );
}

#[test]
/// Test that destructors hidden in various places are correctly called.
///
/// Some types are excluded because we know they behave poorly due to
/// https://github.com/google/autocxx/issues/829.
fn test_tricky_destructors() {
    let cxx = "";
    let hdr = indoc! {"
        #include <stdio.h>
        #include <stdlib.h>
        // A simple type to let Rust verify the destructor is run.
        struct DestructorFlag {
            DestructorFlag() = default;
            DestructorFlag(const DestructorFlag&) = default;
            DestructorFlag(DestructorFlag&&) = default;

            ~DestructorFlag() {
                if (!flag) return;
                if (*flag) {
                    fprintf(stderr, \"DestructorFlag is already set\\n\");
                    abort();
                }
                *flag = true;
                // Note we deliberately do NOT clear the value of `flag`, to catch Rust calling
                // this destructor twice.
            }

            bool *flag = nullptr;
        };

        struct ImplicitlyDefaulted {
            DestructorFlag flag;

            void set_flag(bool *flag_pointer) { flag.flag = flag_pointer; }
        };
        struct ExplicitlyDefaulted {
            ExplicitlyDefaulted() = default;
            ~ExplicitlyDefaulted() = default;

            DestructorFlag flag;

            void set_flag(bool *flag_pointer) { flag.flag = flag_pointer; }
        };
        struct Explicit {
            Explicit() = default;
            ~Explicit() {}

            DestructorFlag flag;

            void set_flag(bool *flag_pointer) { flag.flag = flag_pointer; }
        };

        struct BaseImplicitlyDefaulted : public ImplicitlyDefaulted {
            void set_flag(bool *flag_pointer) { ImplicitlyDefaulted::set_flag(flag_pointer); }
        };
        struct BaseExplicitlyDefaulted : public ExplicitlyDefaulted {
            void set_flag(bool *flag_pointer) { ExplicitlyDefaulted::set_flag(flag_pointer); }
        };
        struct BaseExplicit : public Explicit {
            void set_flag(bool *flag_pointer) { Explicit::set_flag(flag_pointer); }
        };

        struct MemberImplicitlyDefaulted {
            ImplicitlyDefaulted member;

            void set_flag(bool *flag_pointer) { member.set_flag(flag_pointer); }
        };
        struct MemberExplicitlyDefaulted {
            ExplicitlyDefaulted member;

            void set_flag(bool *flag_pointer) { member.set_flag(flag_pointer); }
        };
        struct MemberExplicit {
            Explicit member;

            void set_flag(bool *flag_pointer) { member.set_flag(flag_pointer); }
        };

        struct BaseMemberImplicitlyDefaulted : public MemberImplicitlyDefaulted {
            void set_flag(bool *flag_pointer) { MemberImplicitlyDefaulted::set_flag(flag_pointer); }
        };
        struct BaseMemberExplicitlyDefaulted : public MemberExplicitlyDefaulted {
            void set_flag(bool *flag_pointer) { MemberExplicitlyDefaulted::set_flag(flag_pointer); }
        };
        struct BaseMemberExplicit : public MemberExplicit {
            void set_flag(bool *flag_pointer) { MemberExplicit::set_flag(flag_pointer); }
        };
    "};
    let rs = quote! {
        macro_rules! test_type {
            [$t:ty] => {
                let mut unique_t = <$t>::new().within_unique_ptr();
                let mut destructor_flag = false;
                unsafe {
                    unique_t.pin_mut().set_flag(&mut destructor_flag);
                }
                std::mem::drop(unique_t);
                assert!(destructor_flag, "Destructor did not run with make_unique for {}", quote::quote!{$t});

                moveit! {
                    let mut moveit_t = <$t>::new();
                }
                let mut destructor_flag = false;
                unsafe {
                    moveit_t.as_mut().set_flag(&mut destructor_flag);
                }
                std::mem::drop(moveit_t);
                assert!(destructor_flag, "Destructor did not run with moveit for {}", quote::quote!{$t});
            }
        }

        test_type![ffi::ImplicitlyDefaulted];
        test_type![ffi::ExplicitlyDefaulted];
        test_type![ffi::Explicit];
        test_type![ffi::BaseImplicitlyDefaulted];
        test_type![ffi::BaseExplicitlyDefaulted];
        test_type![ffi::BaseExplicit];
        test_type![ffi::MemberImplicitlyDefaulted];
        test_type![ffi::MemberExplicitlyDefaulted];
        test_type![ffi::MemberExplicit];
        test_type![ffi::BaseMemberImplicitlyDefaulted];
        test_type![ffi::BaseMemberExplicitlyDefaulted];
        test_type![ffi::BaseMemberExplicit];
    };
    run_test(
        cxx,
        hdr,
        rs,
        &[
            "DestructorFlag",
            "ImplicitlyDefaulted",
            "ExplicitlyDefaulted",
            "Explicit",
            "BaseImplicitlyDefaulted",
            "BaseExplicitlyDefaulted",
            "BaseExplicit",
            "MemberImplicitlyDefaulted",
            "MemberExplicitlyDefaulted",
            "MemberExplicit",
            "BaseMemberImplicitlyDefaulted",
            "BaseMemberExplicitlyDefaulted",
            "BaseMemberExplicit",
        ],
        &[],
    );
}

#[test]
fn test_concretize() {
    let hdr = indoc! {"
        #include <string>
        template<typename CONTENTS>
        class Container {
        private:
            CONTENTS* contents;
        };
        struct B {
            std::string a;
        };
    "};
    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! {
            concrete!("Container<B>", ContainerOfB)
            generate!("B")
        },
        None,
        None,
        Some(quote! {
            struct HasAField {
                contents: ffi::ContainerOfB
            }
        }),
    );
}

#[test]
fn test_doc_comments_survive() {
    let hdr = indoc! {"
        #include <cstdint>
        /// Struct line A
        /// Struct line B
        struct A { int b; };

        /// POD struct line A
        /// POD struct line B
        struct B {
            /// Field line A
            /// Field line B
            uint32_t b;

            /// Method line A
            /// Method line B
            void foo() {}
        };

        /// Enum line A
        /// Enum line B
        enum C {
            /// Variant line A
            /// Variant line B
            VARIANT,
        };

        /// Function line A
        /// Function line B
        inline void D() {}
    "};

    let expected_messages = [
        "Struct",
        "POD struct",
        "Field",
        "Method",
        "Enum",
        "Variant",
        "Function",
    ]
    .into_iter()
    .flat_map(|l| [format!("{l} line A"), format!("{l} line B")])
    .collect_vec();

    run_test_ex(
        "",
        hdr,
        quote! {},
        directives_from_lists(&["A", "C", "D"], &["B"], None),
        None,
        Some(make_string_finder(expected_messages)),
        None,
    );
}

#[test]
fn optional_param_in_copy_constructor() {
    let hdr = indoc! {"
        struct A {
            A(const A &other, bool optional_arg = false);
        };
    "};
    run_test("", hdr, quote! {}, &["A"], &[]);
}

#[test]
fn param_in_copy_constructor() {
    let hdr = indoc! {"
        struct A {
            A(const A &other, bool arg);
        };
    "};
    run_test("", hdr, quote! {}, &["A"], &[]);
}

#[test]
fn test_variadic() {
    let hdr = indoc! {"
        class SomeClass{
        public:
            inline void foo(int, ... ) {}
        };
    "};
    run_test("", hdr, quote! {}, &["SomeClass"], &[]);
}

#[test]
fn test_typedef_to_enum() {
    let hdr = indoc! {"
        enum b {};
        class c {
        public:
          typedef b d;
          d e();
        };
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_typedef_to_ns_enum() {
    let hdr = indoc! {"
        namespace a {
        enum b {};
        class c {
        public:
          typedef b d;
          d e();
        };
        } // namespace
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_enum_in_ns() {
    let hdr = indoc! {"
        namespace a {
        enum b {};
        } // namespace
    "};
    run_test("", hdr, quote! {}, &["a::b"], &[]);
}

#[test]
fn test_recursive_field() {
    let hdr = indoc! {"
        #include <memory>
        struct A {
            std::unique_ptr<A> a;
        };
    "};
    run_test("", hdr, quote! {}, &["A"], &[]);
}

#[test]
fn test_recursive_field_indirect() {
    let hdr = indoc! {"
        #include <memory>
        struct B;
        struct A {
            std::unique_ptr<B> a;
        };
        struct B {
            std::unique_ptr<A> a1;
            A a2;
        };
    "};
    run_test("", hdr, quote! {}, &["A", "B"], &[]);
}

#[test]
fn test_typedef_unsupported_type_pub() {
    let hdr = indoc! {"
        #include <set>
        namespace NS{
            class cls{
                public:
                    typedef std::set<int> InnerType;
                };
        }
    "};

    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! { generate_ns!("NS") },
        None,
        None,
        None,
    );
}

#[test]
fn test_typedef_unsupported_type_pri() {
    let hdr = indoc! {"
        #include <set>
        namespace NS{
            class cls{
                private:
                    typedef std::set<int> InnerType;
                };
        }
    "};

    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! { generate_ns!("NS") },
        None,
        None,
        None,
    );
}

#[test]
fn test_array_trouble1() {
    let hdr = indoc! {"
        namespace a {
        template <typename b> struct array {
          typedef b c;
          typedef c d;
        };
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_array_trouble2() {
    let hdr = indoc! {"
        template <typename b> struct array {
          typedef b c;
          typedef c d;
        };
    "};
    run_test("", hdr, quote! {}, &["array_d"], &[]);
}

#[test]
fn test_issue_1087a() {
    let hdr = indoc! {"
        template <typename _CharT> class a {
          _CharT b;
        };
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1087b() {
    let hdr = indoc! {"
        template <typename _CharT> class a {
          typedef _CharT b;
          b c;
        };
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1087c() {
    let hdr = indoc! {"
        namespace {
        namespace {
        template <typename _CharT> class a {
          typedef _CharT b;
          b c;
        };
        }
        }
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1089() {
    let hdr = indoc! {"
        namespace a {
        template <typename c, c> struct d;
        template <bool, typename, typename> struct ab;
        inline namespace {
        namespace ac {
        template <typename, template <typename> class, typename> struct bh;
        template <template <typename> class ad, typename... bi>
        using bj = bh<void, ad, bi...>;
        template <typename ad> using bk = typename ad::b;
        template <typename> struct bm;
        } // namespace ac
        template <typename ad>
        struct b : ab<ac::bj<ac::bk, ad>::e, ac::bm<ad>, d<bool, ad ::e>>::bg {};
        } // namespace
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

/// The problem here is that 'g' doesn't get annotated with
/// the unused_template semantic attribute.
/// This seems to be because both g and f have template
/// parameters, so they're all "used", but effectively cancel
/// out and thus bindgen generates
///   pub type g = root::b::f;
/// So, what we should do here is spot any typedef depending
/// on a template which takes template args, and reject that too.
/// Probably.
#[test]
#[ignore] // https://github.com/google/autocxx/pull/1094
fn test_issue_1094() {
    let hdr = indoc! {"
        namespace {
        typedef int a;
        }
        namespace b {
        template <typename> struct c;
        template <typename d, d e> using f = __make_integer_seq<c, d, e>;
        template <a e> using g = f<a, e>;
        } // namespace b
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1096a() {
    let hdr = indoc! {"
        namespace a {
        class b {
          class c;
        };
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1096b() {
    let hdr = indoc! {"
        namespace a {
        class b {
        public:
          class c;
        };
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1096c() {
    let hdr = indoc! {"
        namespace a {
        class b {
        public:
          class c {
          public:
            int d;
          };
        };
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1096d() {
    let hdr = indoc! {"
        namespace a {
        class b {
        private:
          class c {
          public:
            int d;
          };
        };
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1096e() {
    let hdr = indoc! {"
        namespace a {
        class b {
        private:
          enum c {
              D,
          };
        };
        } // namespace a
    "};
    run_generate_all_test(hdr);
}

/// Unclear why minimization resulted in this particular test case.
#[test]
#[ignore] // https://github.com/google/autocxx/pull/1097
fn test_issue_1097() {
    let hdr = indoc! {"
        namespace rust {
        inline namespace a {
        class Str {
        public:
          ~Str();
        };
        } // namespace a
        } // namespace rust
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1098a() {
    let hdr = indoc! {"
        namespace {
        namespace {
        template <typename _CharT> class a {
          typedef _CharT b;
          b c;
        };
        template <typename _CharT> class d : a<_CharT> {};
        } // namespace
        } // namespace
    "};
    run_generate_all_test(hdr);
}

/// Need to spot structs like this:
/// pub struct d<_CharT> {
///  _base: root::a<_CharT>,
/// }
/// and not create concrete types where the inner type is something from
/// the outer context.
#[test]
fn test_issue_1098b() {
    let hdr = indoc! {"
        template <typename _CharT> class a {
          typedef _CharT b;
          b c;
        };
        template <typename _CharT> class d : a<_CharT> {};
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_issue_1098c() {
    let hdr = indoc! {"
        namespace {
        namespace {
        struct A {
            int a;
        };
        typedef A B;
        } // namespace
        } // namespace
        inline void take_b(const B&) {}
    "};
    run_generate_all_test(hdr);
}

#[test]
fn test_pass_rust_str_and_return_struct() {
    let cxx = indoc! {"
        A take_str_return_struct(rust::Str) {
            A a;
            return a;
        }
    "};
    let hdr = indoc! {"
        #include <cxx.h>
        struct A {};
        A take_str_return_struct(rust::Str);
    "};
    let rs = quote! {
        ffi::take_str_return_struct("hi");
    };
    run_test(cxx, hdr, rs, &["take_str_return_struct"], &[]);
}

#[test]
fn test_issue_1065a() {
    let hdr = indoc! {"
        #include <memory>
        #include <vector>

        template <typename at> class au {
        std::unique_ptr<at> aw;
        };
        class bb;
        using bc = au<bb>;
        class RenderFrameHost {
        public:
        virtual std::vector<bc> &bd() = 0;
        virtual ~RenderFrameHost() {}
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["RenderFrameHost"], &[]);
}

#[test]
fn test_issue_1065b() {
    let hdr = indoc! {"
        #include <memory>
        #include <vector>

        class bb;
        using bc = std::unique_ptr<bb>;
        class RenderFrameHost {
        public:
        virtual std::vector<bc> &bd() = 0;
        virtual ~RenderFrameHost() {}
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["RenderFrameHost"], &[]);
}

#[test]
fn test_issue_1081() {
    let hdr = indoc! {"
        namespace libtorrent {
        char version;
        }
        namespace libtorrent {
        struct session;
        }
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["libtorrent::session"], &[]);
}

#[test]
#[ignore] // This test passes under all normal builds. However
          // it triggers a stack use-after-return in older versions of
          // libclang which is only detected under ASAN (obviously it
          // sometimes causes crashes the rest of the time).
          // This UaR does not occur when the same code is processed
          // with a HEAD version of clang itself as of June 2022. This
          // may mean that the UaR has been fixed in later versions of
          // the clang code, or that it only occurs when the code is used
          // in a libclang context (not a plain clang compilation context).
          // If the problem recurs, we should work out which of these is
          // the case.
fn test_issue_1125() {
    let hdr = indoc! {"
        namespace {
        namespace {
        template <class a> class b {
          typedef a c;
          struct {
            c : sizeof(c);
          };
        };
        } // namespace
        } // namespace
    "};
    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! {
            generate_all!()
        },
        make_cpp17_adder(),
        None,
        None,
    );
}

#[test]
fn test_issue_1143() {
    let hdr = indoc! {
        "namespace mapnik {
            class Map {
            public:
              int &a(long);
            };
        }"
    };

    run_test("", hdr, quote! {}, &["mapnik::Map"], &[]);
}

#[test]
fn test_issue_1170() {
    let hdr = indoc! {
        "#include <vector>
        struct a {
            enum b {} c;
        } Loc;
        struct Arch {
            std::vector<a> d();
        } DeterministicRNG;"
    };
    run_test("", hdr, quote! {}, &["Arch"], &[]);
}

// https://github.com/google/autocxx/issues/774
#[test]
fn test_virtual_methods() {
    let hdr = indoc! {"
        #include <cstdint>
        #include <memory>
        class Base {
        public:
            Base() {}
            virtual ~Base() {}

            virtual int a() = 0;

            virtual void b(int) = 0;
            virtual void b(bool) = 0;

            virtual int c() const = 0;
            virtual int c() = 0;
        };
        class FullyDefined : public Base {
        public:
            int a() { return 0; }

            void b(int) { }
            void b(bool) { }

            int c() const { return 1; }
            int c() { return 2; }
        };
        class Partial1 : public Base {
        public:
            int a() { return 0; }

            void b(bool) {}
        };

        class Partial2 : public Base {
        public:
            int a() { return 0; }

            void b(int) { }
            void b(bool) { }

            int c() const { return 1; }
        };

        class Partial3 : public Base {
        public:
            int a() { return 0; }

            void b(int) { }

            int c() const { return 1; }
            int c() { return 2; }
        };

        class Partial4 : public Base {
        public:
            int a() { return 0; }

            void b(int) { }
            void b(bool) = 0;

            int c() const { return 1; }
            int c() { return 2; }
        };

        // TODO: currently this class cannot be detected as virtual as there
        // is no metadata captured to show that this destructor is virtual
        // uncommenting this (as well as corresponding sections below) gives a 
        // 'instantiation of abstract class' error.
        // class Partial5 : public Base {
        // public:
        //     ~Partial5() = 0;

        //     int a() { return 0; }

        //     void b(int) { }
        //     void b(bool) { }

        //     int c() const { return 1; }
        //     int c() { return 2; }
        // };

    "};
    let rs = quote! {
        static_assertions::assert_impl_all!(ffi::FullyDefined: moveit::CopyNew);
        static_assertions::assert_not_impl_any!(ffi::Partial1: moveit::CopyNew);
        static_assertions::assert_not_impl_any!(ffi::Partial2: moveit::CopyNew);
        static_assertions::assert_not_impl_any!(ffi::Partial3: moveit::CopyNew);
        static_assertions::assert_not_impl_any!(ffi::Partial4: moveit::CopyNew);
        // static_assertions::assert_not_impl_any!(ffi::Partial5: moveit::CopyNew);
        let _c1 = ffi::FullyDefined::new().within_unique_ptr();
    };
    run_test(
        "",
        hdr,
        rs,
        &[
            "FullyDefined",
            "Partial1",
            "Partial2",
            "Partial3",
            "Partial4",
            // "Partial5"
        ],
        &[],
    );
}

#[test]
fn test_issue_1192() {
    let hdr = indoc! {
        "#include <vector>
        #include <cstdint>
        template <typename B>
        struct A {
            B a;
        };
        struct VecThingy {
            A<uint32_t> contents[2];
        };
        struct MyStruct {
            VecThingy vec;
        };"
    };
    run_test_ex(
        "",
        hdr,
        quote! {},
        quote! {

            extern_cpp_type!("VecThingy", crate::VecThingy)
            pod!("VecThingy")

            generate_pod!("MyStruct")
        },
        None,
        None,
        Some(quote! {
            // VecThingy isn't necessarily 128 bits long.
            // This test doesn't actually allocate one.
            #[repr(transparent)]
            pub struct VecThingy(pub u128);

            unsafe impl cxx::ExternType for VecThingy {
                type Id = cxx::type_id!("VecThingy");
                type Kind = cxx::kind::Trivial;
            }
        }),
    );
}

#[test]
fn test_issue_1214() {
    let hdr = indoc! {"
        #include <cstdint>
        enum class C: uint16_t {
            A,
            B,
        };
    "};
    run_test("", hdr, quote! {}, &["C"], &[]);
}

#[test]
fn test_issue_1229() {
    let hdr = indoc! {"
    struct Thing {
        float id;
    
        Thing(float id) : id(id) {}
    };

    struct Item {
        float id;
    
        Item(float id) : id(id) {}
    };
    "};
    let hexathorpe = Token![#](Span::call_site());
    let rs = quote! {
        use autocxx::WithinUniquePtr;

        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            name!(thing)
            safety!(unsafe)
            generate!("Thing")
        }
        autocxx::include_cpp! {
            #hexathorpe include "input.h"
            name!(item)
            safety!(unsafe)
            generate!("Item")
        }

        fn main() {
            let _thing = thing::Thing::new(15.).within_unique_ptr();
            let _item = item::Item::new(15.).within_unique_ptr();
        }
    };

    do_run_test_manual("", hdr, rs, None, None).unwrap();
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/1265
fn test_issue_1265() {
    let hdr = indoc! {"
        #include <string>

        class Test
        {
        public:
          explicit Test(std::string string)
            : string(std::move(string))
          {
          }

          Test() = delete;

          [[nodiscard]] auto get_string() const -> std::string const& { return this->string; }

        private:
          std::string string;
        };
    "};
    run_test_ex(
        "",
        hdr,
        quote! {
            run();
        },
        directives_from_lists(&["Test"], &[], None),
        None,
        None,
        Some(quote! {
            fn run() {
                let str0 = "string";
                let str1 = "another string";
                let ptr0 = UniquePtr::emplace(ffi::Test::new(str0));
                let ptr1 = UniquePtr::emplace(ffi::Test::new(str1));
                println!("0: {}", ptr0.get_string());
                println!("1: {}", ptr1.get_string());
                moveit!(let mut ref0 = &move *ptr0);
                moveit!(let mut ref1 = &move *ptr1);
                println!("0: {}", ref0.get_string());
                println!("1: {}", ref1.get_string());
                println!("swap");
                core::mem::swap(&mut *ref0, &mut *ref1);
                println!("0: {}", ref0.get_string());
                println!("1: {}", ref1.get_string());
            }
        }),
    )
}

#[test]
fn test_ignore_va_list() {
    let hdr = indoc! {"
        #include <stdarg.h>
        class A {
        public:
            A() {}
            void fn(va_list) {}
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_badly_named_alloc() {
    let hdr = indoc! {"
        #include <stdarg.h>
        class A {
        public:
            void alloc();
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["A"], &[]);
}

#[test]
fn test_cpp_union_pod() {
    let hdr = indoc! {"
        typedef unsigned long long UInt64_t;
        struct ManagedPtr_t_;
        typedef struct ManagedPtr_t_ ManagedPtr_t;
        
        typedef int (*ManagedPtr_ManagerFunction_t)(
                ManagedPtr_t *managedPtr,
                const ManagedPtr_t *srcPtr,
                int operation);
        
        typedef union {
            int intValue;
            void *ptr;
        } ManagedPtr_t_data_;
        
        struct ManagedPtr_t_ {
            void *pointer;
            ManagedPtr_t_data_ userData[4];
            ManagedPtr_ManagerFunction_t manager;
        };
        
        typedef struct CorrelationId_t_ {
            unsigned int size : 8;
            unsigned int valueType : 4;
            unsigned int classId : 16;
            unsigned int reserved : 4;
        
            union {
                UInt64_t intValue;
                ManagedPtr_t ptrValue;
            } value;
        } CorrelationId_t;
    "};
    run_test("", hdr, quote! {}, &["CorrelationId_t_"], &[]);
    run_test_expect_fail("", hdr, quote! {}, &[], &["CorrelationId_t_"]);
}

#[test]
fn test_using_string_function() {
    let hdr = indoc! {"
        #include <string>
        using std::string;
        void foo(const string &a);
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["foo"], &[]);
}

#[test]
fn test_using_string_method() {
    let hdr = indoc! {"
        #include <string>
        using std::string;
        class Foo
        {
        public:
            Foo bar(const string &a);
        };
    "};
    let rs = quote! {};
    run_test("", hdr, rs, &["Foo"], &[]);
}

#[test]
#[ignore] // https://github.com/google/autocxx/issues/1382
fn test_override_typedef_fn() {
    let hdr = indoc! {"
        #include <map>
        #include <memory>
        typedef std::shared_ptr<std::map<int, int>> Arg;
            // bindgen currently outputs  pub type Arg = u8;
        class Foo {
        public:
          void *createFoo(const int, Arg &arg);
        //   void *createFoo(const int, std::shared_ptr<std::map<int, int>> &arg); // works
        };
    "};
    run_test("", hdr, quote! {}, &["Foo"], &[]);
}

#[test]
fn test_double_template_w_default() {
    let hdr = indoc! {"
        class Widget {};

        template <class T>
        class RefPtr {
        private:
            T* m_ptr;
        };

        class FakeAlloc {};

        template <typename T, typename A=FakeAlloc>
        class Holder {
            A alloc;
        };

        typedef Holder<RefPtr<Widget>> WidgetRefHolder;
        class Problem {
        public:
            WidgetRefHolder& getWidgets();
        };
    "};
    run_test("", hdr, quote! {}, &["Problem"], &[]);
}

// Yet to test:
// - Ifdef
// - Out param pointers
// - ExcludeUtilities
// - Struct fields which are typedefs
// Negative tests:
// - Private methods
// - Private fields
