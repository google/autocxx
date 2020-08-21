
// Copyright 2020 Google LLC
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

use std::path::{Path, PathBuf};
use std::fs;
use std::fs::File;
use std::io::Write;
use tempfile::{tempdir, TempDir};
use syn::Item;

#[derive(Debug)]
pub enum Error {
    FileReadError(std::io::Error),
    Syntax(syn::Error),
    NoIdent,
    InvalidCxx(String),
    FileWriteFail(std::io::Error),
    TempDirCreationFailed(std::io::Error),
}

pub struct Builder {
    build: cc::Build,
    tdir: TempDir,
}

impl Builder {
    pub fn new(rs_file: impl AsRef<Path>) -> Result<Self, Error> {
        let tdir = tempdir().map_err(|e| Error::TempDirCreationFailed(e))?;
        let mut builder = cc::Build::new();
        builder.cpp(true);
        let source = fs::read_to_string(rs_file).map_err(|e| Error::FileReadError(e))?;
        let source = syn::parse_file(&source).map_err(|e| Error::Syntax(e))?;
        let mut counter = 0;
        for item in source.items {
            if let Item::Macro(mac) = item {
                if let Some(ref name) = mac.ident {
                    if name.to_string() == "include_cxx" {
                        let include_cpp = autocxx_engine::IncludeCpp::fromSyn(mac);
                        let (cxx, _) = include_cpp.generate().map_err(|e| Error::InvalidCxx(e))?;
                        let fname = format!("gen{}.cxx", counter);
                        counter += 1;
                        let gen_cxx_path = Self::write_to_file(&tdir, &fname, &cxx).map_err(|e| Error::FileWriteFail(e))?;
                        builder.file(gen_cxx_path);
                    }
                }
            }
        }
        Ok(Builder {
            build: builder,
            tdir: tdir,
        })
    }

    pub fn builder(&mut self) -> &mut cc::Build {
        &mut self.build
    }

    fn write_to_file(tdir: &TempDir, filename: &str, content: &[u8]) -> std::io::Result<PathBuf> {
        let path = tdir.path().join(filename);
        let mut f = File::create(&path)?;
        f.write_all(content)?;
        Ok(path)
    }
}
