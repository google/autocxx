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

use std::collections::{HashSet, VecDeque};

use crate::{conversion::api::Api, types::QualifiedName};

use super::fun::FnPhase;

/// Return APIs in a depth-first order, i.e. those with no dependencies first.
pub(super) fn depth_first(apis: &[Api<FnPhase>]) -> impl Iterator<Item = &Api<FnPhase>> {
    DepthFirstIter {
        queue: apis.iter().collect(),
        done: HashSet::new(),
    }
}

struct DepthFirstIter<'a> {
    queue: VecDeque<&'a Api<FnPhase>>,
    done: HashSet<QualifiedName>,
}

impl<'a> Iterator for DepthFirstIter<'a> {
    type Item = &'a Api<FnPhase>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(candidate) = self.queue.pop_front() {
            if candidate.deps().all(|d| self.done.contains(&d)) {
                self.done.insert(candidate.name().clone());
                return Some(candidate);
            }
            self.queue.push_back(candidate);
        }
        None
    }
}
