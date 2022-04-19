// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use indexmap::map::IndexMap as HashMap;
use indexmap::set::IndexSet as HashSet;

use itertools::Itertools;
use regex::Regex;
use serde::Serialize;

use crate::{
    depfile::Depfile,
    output::{write_atomic, Output},
};

pub(crate) struct SingleFileOutput {
    depfile: Option<Rc<RefCell<Depfile>>>,
    dir: PathBuf,
    contents_rs: RustFiles,
    contents_cpp: HashMap<String, String>,
}

#[derive(Serialize, Default)]
struct RustFiles {
    mapping: HashMap<String, String>,
}

impl Output for SingleFileOutput {
    fn write_cpp(&mut self, filename: String, content: &[u8]) {
        self.contents_cpp.insert(
            filename,
            String::from_utf8(content.to_vec()).expect("C++ code was not valid UTF8"),
        );
    }

    fn write_rs(&mut self, filename: String, content: &[u8]) {
        self.contents_rs.mapping.insert(
            filename,
            String::from_utf8(content.to_vec()).expect("Rust code was not valid UTF8"),
        );
    }

    fn finalize(&mut self) {
        // Write out the Rust as a big JSON blob.
        let json_path = self.dir.join("autocxx_gen_combined.rs.json");
        let json =
            serde_json::to_string(&self.contents_rs).expect("Unable to serialize JSON for Rust");
        write_atomic(&self.depfile, &json_path, json.as_bytes());

        // For C++... we combine all headers into a single header, and all .cc files into a single .cc file.
        let combined_h_content = create_combined_file(&mut self.contents_cpp, "h", None);
        let combined_h_path = self.dir.join("autocxx_gen_combined.h");
        write_atomic(
            &self.depfile,
            &combined_h_path,
            combined_h_content.as_bytes(),
        );

        let combined_cc_content =
            create_combined_file(&mut self.contents_cpp, "cc", Some("autocxx_gen_combined.h"));
        let combined_cc_path = self.dir.join("autocxx_gen_combined.cc");
        write_atomic(
            &self.depfile,
            &combined_cc_path,
            combined_cc_content.as_bytes(),
        );
    }
}

impl SingleFileOutput {
    pub(crate) fn new(depfile: Option<Rc<RefCell<Depfile>>>, dir: &Path) -> Self {
        Self {
            depfile,
            dir: dir.into(),
            contents_rs: Default::default(),
            contents_cpp: HashMap::new(),
        }
    }
}

fn create_combined_file(
    contents_cpp: &HashMap<String, String>,
    filename_suffix: &str,
    other_file_to_include: Option<&str>,
) -> String {
    let mut done = HashSet::new();
    let all_filenames: HashSet<_> = contents_cpp.keys().cloned().collect();
    let mut to_do: HashSet<String> = all_filenames
        .iter()
        .filter(|filename| filename.ends_with(filename_suffix))
        .cloned()
        .collect();
    let mut output_lines = Vec::new();
    loop {
        if to_do.is_empty() {
            break;
        }
        let mut this_filename = None;
        for candidate in &to_do {
            let candidate_deps =
                find_dependencies(contents_cpp.get(&candidate.to_string()).unwrap());
            let candidate_deps: HashSet<String> =
                candidate_deps.into_iter().map(|s| s.to_string()).collect();
            //println!("Deps from {} are {:?}", candidate, candidate_deps);
            if candidate_deps.is_disjoint(&to_do) {
                this_filename = Some(candidate.to_string());
                break;
            }
        }
        let this_filename = this_filename
            .expect("All remaining files depend on other files we have yet to process");
        to_do.remove(this_filename.as_str());
        done.insert(this_filename.to_string());
        let content = contents_cpp.get(this_filename.as_str()).unwrap();
        for line in content.lines() {
            let line_deps = find_dependencies(line);
            let line_deps: HashSet<String> = line_deps.into_iter().map(|s| s.to_string()).collect();
            if line_deps.is_disjoint(&all_filenames) {
                output_lines.push(line.to_string());
            }
        }
    }
    let mut output_lines = other_file_to_include
        .map(|s| format!("#include \"{}\"\n", s))
        .into_iter()
        .chain(output_lines.into_iter());
    output_lines.join("\n")
}

#[test]
fn test_create_combined_file() {
    let mut contents: HashMap<String, String> = HashMap::new();
    contents.insert("a.h".into(), "foo\n#include <b.h>\nbar\n".into());
    contents.insert("b.h".into(), "foo2\n\nbar2\n".into());
    contents.insert("a.cc".into(), "foo3\n#include <b.h>\nbar4\n".into());
    contents.insert(
        "b.cc".into(),
        "foo5\n#include <a.h>\n#include <b.h>\n\nbar6\n".into(),
    );
    let combined = create_combined_file(&mut contents.clone(), "h", None);
    assert_eq!(combined, "foo2\n\nbar2\nfoo\nbar");
    let combined = create_combined_file(&mut contents.clone(), "cc", Some("combined.h"));
    assert_eq!(
        combined,
        "#include \"combined.h\"\n\nfoo3\nbar4\nfoo5\n\nbar6"
    );
}

fn find_dependencies(cpp_code: &str) -> HashSet<&str> {
    let re = Regex::new("#include [<\"](.*)[>\"]").unwrap();
    re.captures_iter(cpp_code)
        .map(|captures| captures.get(1).unwrap().as_str())
        .collect()
}

#[test]
fn test_find_dependencies() {
    assert_eq!(find_dependencies(""), HashSet::new());
    assert_eq!(
        find_dependencies("#include <a.h>"),
        ["a.h"].into_iter().collect::<HashSet<_>>()
    );
    assert_eq!(
        find_dependencies("#include \"a.h\""),
        ["a.h"].into_iter().collect::<HashSet<_>>()
    );
    assert_eq!(
        find_dependencies("#include \"a.h\"\nfoo\n#include <b.h>\n\nbar"),
        ["a.h", "b.h"].into_iter().collect::<HashSet<_>>()
    );
}
