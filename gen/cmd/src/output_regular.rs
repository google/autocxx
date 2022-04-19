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

use crate::{
    depfile::Depfile,
    output::{write_atomic, Output},
};

pub(crate) struct RegularOutput {
    depfile: Option<Rc<RefCell<Depfile>>>,
    dir: PathBuf,
}

impl Output for RegularOutput {
    fn write_cpp(&mut self, filename: String, content: &[u8]) {
        self.write_to_file(filename, content)
    }

    fn write_rs(&mut self, filename: String, content: &[u8]) {
        self.write_to_file(filename, content)
    }
}

impl RegularOutput {
    pub(crate) fn new(depfile: Option<Rc<RefCell<Depfile>>>, dir: &Path) -> Self {
        Self {
            depfile,
            dir: dir.into(),
        }
    }

    fn write_to_file(&self, filename: String, content: &[u8]) {
        let path = self.dir.join(filename);
        write_atomic(&self.depfile, &path, content);
    }
}
