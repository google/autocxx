// Copyright 2021 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

fn main() -> miette::Result<()> {
    let path = std::path::PathBuf::from("src");
    let mut b = autocxx_build::Builder::new("src/main.rs", &[&path])
        .auto_allowlist(true)
        .build()?;
    b.flag_if_supported("-std=c++17")
        .file("src/messages.cc")
        .compile("autocxx-subclass-example");
    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/messages.cc");
    println!("cargo:rerun-if-changed=src/messages.h");

    // The following line is *unrelated* to autocxx builds and is
    // just designed to ensure that example code doesn't get out of sync
    // from copies in comments.
    ensure_comments_match_real_code(&std::path::PathBuf::from("src/main.rs"));
    Ok(())
}

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Lines;
use std::path::Path;

enum CommentMatcherState {
    Searching,
    EatingBacktickLine(Lines<BufReader<File>>),
    SearchingForFirstLine(Lines<BufReader<File>>),
    Found(Lines<BufReader<File>>),
}

fn ensure_comments_match_real_code(rs_file: &Path) {
    use regex::Regex;
    let start_re = Regex::new(r"// .*from ([\w/]+\.\w+).*").unwrap();
    let strip_comment_re = Regex::new(r"// (.*)").unwrap();
    let file = File::open(rs_file).unwrap();
    let lines = BufReader::new(file).lines();
    let mut state = CommentMatcherState::Searching;
    for line in lines {
        let line = line.unwrap();
        state = match state {
            CommentMatcherState::Searching => match start_re.captures(&line) {
                Some(captures) => {
                    let fname = captures.get(1).unwrap().as_str();
                    let srcfile = File::open(fname).unwrap();
                    let srclines = BufReader::new(srcfile).lines();
                    CommentMatcherState::EatingBacktickLine(srclines)
                }
                None => CommentMatcherState::Searching,
            },
            CommentMatcherState::EatingBacktickLine(srclines) => {
                CommentMatcherState::SearchingForFirstLine(srclines)
            }
            CommentMatcherState::SearchingForFirstLine(mut srclines) => {
                match strip_comment_re.captures(&line) {
                    Some(captures) => {
                        let mut found = false;
                        while !found {
                            let srcline = srclines.next().unwrap().unwrap();
                            found = captures.get(1).unwrap().as_str() == srcline;
                        }
                        CommentMatcherState::Found(srclines)
                    }
                    None => CommentMatcherState::Searching,
                }
            }
            CommentMatcherState::Found(mut srclines) => {
                if line == "// ```" {
                    CommentMatcherState::Searching
                } else {
                    match strip_comment_re.captures(&line) {
                        Some(captures) => {
                            let actual = captures.get(1).unwrap().as_str();
                            let expected = srclines.next().unwrap().unwrap();
                            assert_eq!(expected, actual);
                            CommentMatcherState::Found(srclines)
                        }
                        None => CommentMatcherState::Searching,
                    }
                }
            }
        }
    }
}
