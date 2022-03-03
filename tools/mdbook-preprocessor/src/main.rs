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

#![forbid(unsafe_code)]

use std::{borrow::Cow, collections::HashSet, fmt::Display, io, path::PathBuf, process};

use anyhow::Error;
use clap::{crate_authors, crate_version, Arg, ArgMatches, Command};
use itertools::Itertools;
use mdbook::{book::Book, preprocess::CmdPreprocessor};
use proc_macro2::Span;
use syn::{spanned::Spanned, Expr, __private::ToTokens};

static LONG_ABOUT: &str =
    "This is an mdbook preprocessor tailored for autocxx code examples. Autocxx
code examples don't fit well 'mdbook test' or even alternatives such as
'skeptic' or 'doc_comment' for these reasons:

a) A single code example consists of both Rust and C++ code. They must be
   linked into a separate executable, i.e. we must make one executable per
   doc test.
b) The code examples must be presented/formatted nicely with suitable
   separate blocks for the Rust and C++ code.
c) mdbook test is not good at handling doctests which have dependencies.

This preprocessor will find code snippets like this:
```rust,autocxx
autocxx_integration_tests::doctest(
\" /* any C++ implementation code */\",
\" /* C++ header code */\",
{
/* complete Rust code including 'main' */
)
```

and will build and run them, while emitting better formatted markdown blocks
for subsequent preprocessors and renderers.
";

fn main() {
    let matches = Command::new("autocxx-mdbook-preprocessor")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Expands and tests code examples in the autocxx book.")
        .long_about(LONG_ABOUT)
        .subcommand(
            Command::new("supports")
                .arg(Arg::new("renderer").required(true))
                .about("Whether a given renderer is supported by this preprocessor"),
        )
        .arg(
            Arg::new("skip_tests")
                .short('s')
                .help("Skip running doctests"),
        )
        .arg(
            Arg::new("manifest_dir")
            .long("manifest-dir")
            .help("Path to directory containing outermost autocxx Cargo.toml; necessary for trybuild to build test code successfully")
            .default_value_os(calculate_cargo_dir().as_os_str())
        )
        .get_matches();
    if let Some(supports_matches) = matches.subcommand_matches("supports") {
        // Only do our preprocessing and testing for the html renderer, not linkcheck.
        if supports_matches.value_of("renderer") == Some("html") {
            process::exit(0);
        } else {
            process::exit(1);
        }
    }
    preprocess(&matches).unwrap();
}

fn calculate_cargo_dir() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    for _ in 0..3 {
        path = path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
    }
    path.join("integration-tests")
}

fn preprocess(args: &ArgMatches) -> Result<(), Error> {
    let (_, mut book) = CmdPreprocessor::parse_input(io::stdin())?;

    let mut test_cases = Vec::new();

    Book::for_each_mut(&mut book, |sec| {
        if let mdbook::BookItem::Chapter(chapter) = sec {
            let filename = chapter
                .path
                .as_ref()
                .map(|pb| pb.to_string_lossy())
                .unwrap_or_default()
                .to_string();
            chapter.content = substitute_chapter(&chapter.content, &filename, &mut test_cases);
        }
    });

    // Now run any test cases we accumulated.
    let mut any_fails = false;
    if !args.is_present("skip_tests") {
        let num_tests = test_cases.len();
        for (counter, case) in test_cases.into_iter().enumerate() {
            eprintln!(
                "Running doctest {}/{} at {}",
                counter + 1,
                num_tests,
                &case.location
            );
            let passed = autocxx_integration_tests::doctest(
                &case.cpp,
                &case.hdr,
                case.rs.to_token_stream(),
                args.value_of_os("manifest_dir").unwrap(),
            )
            .is_ok();
            eprintln!(
                "Doctest {}/{} at {} {}.",
                counter + 1,
                num_tests,
                &case.location,
                if passed { "passed" } else { "failed" }
            );
            any_fails = any_fails || !passed;
        }
    }
    if any_fails {
        panic!("One or more tests failed.");
    }

    serde_json::to_writer(io::stdout(), &book)?;

    Ok(())
}

fn substitute_chapter(chapter: &str, filename: &str, test_cases: &mut Vec<TestCase>) -> String {
    let mut state = ChapterParseState::Start;
    let mut out = Vec::new();
    for (line_no, line) in chapter.lines().enumerate() {
        let line_type = recognize_line(line);
        let mut push_line = true;
        state = match state {
            ChapterParseState::Start => match line_type {
                LineType::CodeBlockStart | LineType::CodeBlockEnd => {
                    ChapterParseState::OtherCodeBlock
                }
                LineType::CodeBlockStartAutocxx(block_flags) => {
                    push_line = false;
                    ChapterParseState::OurCodeBlock(block_flags, Vec::new())
                }
                LineType::Misc => ChapterParseState::Start,
            },
            ChapterParseState::OtherCodeBlock => match line_type {
                LineType::CodeBlockEnd => ChapterParseState::Start,
                LineType::Misc => ChapterParseState::OtherCodeBlock,
                _ => panic!("Found confusing conflicting block markers"),
            },
            ChapterParseState::OurCodeBlock(flags, mut lines) => match line_type {
                LineType::Misc => {
                    push_line = false;
                    lines.push(line.to_string());
                    ChapterParseState::OurCodeBlock(flags, lines)
                }
                LineType::CodeBlockEnd => {
                    let location = MiniSpan {
                        filename: filename.to_string(),
                        start_line: line_no - lines.len(),
                    };
                    out.extend(handle_code_block(flags, lines, location, test_cases));
                    push_line = false;
                    ChapterParseState::Start
                }
                _ => panic!("Found something unexpected in one of our code blocks"),
            },
        };
        if push_line {
            out.push(line.to_string());
        }
    }

    out.join("\n")
}

/// Like `proc_macro2::Span` but only has the starting line. For basic
/// diagnostics.
struct MiniSpan {
    filename: String,
    start_line: usize,
}

impl Display for MiniSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} line {}", self.filename, self.start_line)
    }
}

struct TestCase {
    cpp: String,
    hdr: String,
    rs: syn::File,
    location: MiniSpan,
}

enum ChapterParseState {
    Start,
    OtherCodeBlock,
    OurCodeBlock(HashSet<String>, Vec<String>), // have found rust,autocxx
}

enum LineType {
    CodeBlockStart,
    CodeBlockStartAutocxx(HashSet<String>),
    CodeBlockEnd,
    Misc,
}

fn code_block_flags(line: &str) -> HashSet<String> {
    let line = &line[3..];
    line.split(',').map(|s| s.to_string()).collect()
}

fn recognize_line(line: &str) -> LineType {
    if line.starts_with("```") && line.len() > 3 {
        let flags = code_block_flags(line);
        if flags.contains("autocxx") {
            LineType::CodeBlockStartAutocxx(flags)
        } else {
            LineType::CodeBlockStart
        }
    } else if line == "```" {
        LineType::CodeBlockEnd
    } else {
        LineType::Misc
    }
}

fn handle_code_block(
    flags: HashSet<String>,
    lines: Vec<String>,
    location: MiniSpan,
    test_cases: &mut Vec<TestCase>,
) -> impl Iterator<Item = String> {
    let input_str = lines.join("\n");
    let fn_call = syn::parse_str::<syn::Expr>(&input_str)
        .unwrap_or_else(|_| panic!("Unable to parse outer function at {}", location));
    let fn_call = match fn_call {
        Expr::Call(expr) => expr,
        _ => panic!("Parsing unexpected"),
    };
    let mut args_iter = fn_call.args.iter();
    let cpp = extract_span(&lines, args_iter.next().unwrap().span());
    let hdr = extract_span(&lines, args_iter.next().unwrap().span());
    let rs = extract_span(&lines, args_iter.next().unwrap().span());
    let mut output = vec![
        "#### C++ header:".to_string(),
        "```cpp".to_string(),
        hdr.to_string(),
        "```".to_string(),
    ];
    if !cpp.is_empty() && !flags.contains("hidecpp") {
        output.push("#### C++ implementation:".to_string());
        output.push("```cpp".to_string());
        output.push(cpp.to_string());
        output.push("```".to_string());
    }
    output.push("#### Rust:".to_string());
    output.push("```rust,noplayground".to_string());
    output.push(escape_hexathorpes(&rs).to_string());
    output.push("```".to_string());

    // Don't run the test cases yet, because we want the preprocessor to spot
    // basic formatting errors before getting into the time consuming business of
    // running tests.
    if !flags.contains("nocompile") {
        test_cases.push(TestCase {
            cpp: unescape_quotes(&cpp),
            hdr: unescape_quotes(&hdr),
            rs: syn::parse_file(&rs)
                .unwrap_or_else(|_| panic!("Unable to parse code at {}", location)),
            location,
        });
    }

    output.into_iter()
}

fn extract_span(text: &[String], span: Span) -> Cow<str> {
    let start_line = span.start().line - 1;
    let start_col = span.start().column;
    let end_line = span.end().line - 1;
    let end_col = span.end().column;
    if start_line == end_line {
        Cow::Borrowed(&text[start_line][start_col + 1..end_col - 1])
    } else {
        let start_subset = &text[start_line][start_col + 1..];
        let end_subset = &text[end_line][..end_col - 1];
        let mid_lines = &text[start_line + 1..end_line];
        Cow::Owned(
            std::iter::once(start_subset.to_string())
                .chain(mid_lines.iter().cloned())
                .chain(std::iter::once(end_subset.to_string()))
                .join("\n"),
        )
    }
}

fn escape_hexathorpes(input: &str) -> Cow<str> {
    let re = regex::Regex::new(r"(?m)^(?P<ws>\s*)#(?P<c>.*)").unwrap();
    re.replace_all(input, "$ws##$c")
}

fn unescape_quotes(input: &str) -> String {
    input.replace("\\\"", "\"")
}
