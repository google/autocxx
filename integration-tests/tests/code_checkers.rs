// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::Item;

use autocxx_integration_tests::{CodeChecker, CodeCheckerFns, TestError};

/// Generates a closure which can be used to ensure that the given symbol
/// is mentioned in the output and has documentation attached.
/// The idea is that this is what we do in cases where we can't generate code properly.
pub(crate) fn make_error_finder(error_symbol: &'static str) -> CodeChecker {
    Box::new(ErrorFinder(error_symbol))
}
struct ErrorFinder(&'static str);

impl CodeCheckerFns for ErrorFinder {
    fn check_rust(&self, rs: syn::File) -> Result<(), TestError> {
        let ffi_items = find_ffi_items(rs)?;
        // Ensure there's some kind of struct entry for this symbol
        let error_item = ffi_items
            .into_iter()
            .filter_map(|i| match i {
                Item::Struct(its) if its.ident == self.0 => Some(its),
                _ => None,
            })
            .next()
            .ok_or(TestError::RsCodeExaminationFail)?;
        // Ensure doc attribute
        error_item
            .attrs
            .into_iter()
            .find(|a| a.path.get_ident().filter(|p| *p == "doc").is_some())
            .ok_or(TestError::RsCodeExaminationFail)?;
        Ok(())
    }
}

fn find_ffi_items(f: syn::File) -> Result<Vec<Item>, TestError> {
    Ok(f.items
        .into_iter()
        .filter_map(|i| match i {
            Item::Mod(itm) => Some(itm),
            _ => None,
        })
        .next()
        .ok_or(TestError::RsCodeExaminationFail)?
        .content
        .ok_or(TestError::RsCodeExaminationFail)?
        .1)
}

struct StringFinder(Vec<&'static str>);

impl CodeCheckerFns for StringFinder {
    fn check_rust(&self, rs: syn::File) -> Result<(), TestError> {
        let mut ts = TokenStream::new();
        rs.to_tokens(&mut ts);
        let toks = ts.to_string();
        for msg in &self.0 {
            if !toks.contains(msg) {
                return Err(TestError::RsCodeExaminationFail);
            };
        }
        Ok(())
    }
}

/// Returns a code checker which simply hunts for a given string in the results
pub(crate) fn make_string_finder(error_texts: Vec<&'static str>) -> CodeChecker {
    Box::new(StringFinder(error_texts))
}

/// Counts the number of generated C++ files.
pub(crate) struct CppCounter {
    cpp_count: usize,
}

impl CppCounter {
    pub(crate) fn new(cpp_count: usize) -> Self {
        Self { cpp_count }
    }
}

impl CodeCheckerFns for CppCounter {
    fn check_cpp(&self, cpp: &[PathBuf]) -> Result<(), TestError> {
        if cpp.len() == self.cpp_count {
            Ok(())
        } else {
            Err(TestError::CppCodeExaminationFail)
        }
    }

    fn skip_build(&self) -> bool {
        true
    }
}

/// Searches generated C++ for strings we want to find, or want _not_ to find,
/// or both.
pub(crate) struct CppMatcher<'a> {
    positive_matches: &'a [&'a str],
    negative_matches: &'a [&'a str],
}

impl<'a> CppMatcher<'a> {
    pub(crate) fn new(positive_matches: &'a [&'a str], negative_matches: &'a [&'a str]) -> Self {
        Self {
            positive_matches,
            negative_matches,
        }
    }
}

impl<'a> CodeCheckerFns for CppMatcher<'a> {
    fn check_cpp(&self, cpp: &[PathBuf]) -> Result<(), TestError> {
        let mut positives_needed = self.positive_matches.to_vec();
        for filename in cpp {
            let file = File::open(filename).unwrap();
            let lines = BufReader::new(file).lines();
            for l in lines.filter_map(|l| l.ok()) {
                if self.negative_matches.iter().any(|neg| l.contains(neg)) {
                    return Err(TestError::CppCodeExaminationFail);
                }
                positives_needed.retain(|pos| !l.contains(pos));
            }
        }
        if positives_needed.is_empty() {
            Ok(())
        } else {
            Err(TestError::CppCodeExaminationFail)
        }
    }
}

pub(crate) struct NoSystemHeadersChecker;

impl CodeCheckerFns for NoSystemHeadersChecker {
    fn check_cpp(&self, cpp: &[PathBuf]) -> Result<(), TestError> {
        for filename in cpp {
            let file = File::open(filename).unwrap();
            if BufReader::new(file)
                .lines()
                .any(|l| l.as_ref().unwrap().starts_with("#include <"))
            {
                return Err(TestError::CppCodeExaminationFail);
            }
        }
        Ok(())
    }
    fn skip_build(&self) -> bool {
        true
    }
}
