// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![forbid(unsafe_code)]

use autocxx_engine::{BuilderContext, RebuildDependencyRecorder};
use std::{collections::HashSet, io::Write, sync::Mutex};

pub type Builder = autocxx_engine::Builder<'static, CargoBuilderContext>;

#[doc(hidden)]
pub struct CargoBuilderContext;

impl BuilderContext for CargoBuilderContext {
    fn setup() {
        env_logger::builder()
            .format(|buf, record| writeln!(buf, "cargo:warning=MESSAGE:{}", record.args()))
            .init();
    }
    fn get_dependency_recorder() -> Option<Box<dyn RebuildDependencyRecorder>> {
        Some(Box::new(CargoRebuildDependencyRecorder::new()))
    }
}

#[derive(Debug)]
struct CargoRebuildDependencyRecorder {
    printed_already: Mutex<HashSet<String>>,
}

impl CargoRebuildDependencyRecorder {
    fn new() -> Self {
        Self {
            printed_already: Mutex::new(HashSet::new()),
        }
    }
}

impl RebuildDependencyRecorder for CargoRebuildDependencyRecorder {
    fn record_header_file_dependency(&self, filename: &str) {
        let mut already = self.printed_already.lock().unwrap();
        if already.insert(filename.into()) {
            println!("cargo:rerun-if-changed={}", filename);
        }
    }
}
