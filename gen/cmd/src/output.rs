// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Write},
    path::Path,
    rc::Rc,
};

use crate::depfile::Depfile;

pub(crate) trait Output {
    fn write_cpp(&mut self, filename: String, content: &[u8]);

    fn write_rs(&mut self, filename: String, content: &[u8]);

    fn finalize(&mut self) {}
}

pub(crate) fn write_atomic(depfile: &Option<Rc<RefCell<Depfile>>>, path: &Path, content: &[u8]) {
    let f = File::open(&path);
    if let Some(depfile) = depfile {
        depfile.borrow_mut().add_output(path);
    }
    if let Ok(mut f) = f {
        let mut existing_content = Vec::new();
        let r = f.read_to_end(&mut existing_content);
        if r.is_ok() && existing_content == content {
            return; // don't change timestamp on existing file unnecessarily
        }
    }
    let mut f = File::create(&path).expect("Unable to create file");
    f.write_all(content).expect("Unable to write file");
}
