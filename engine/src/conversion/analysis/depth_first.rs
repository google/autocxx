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
    depth_first_impl(apis)
}

fn depth_first_impl<T: HasDependencies>(items: &[T]) -> impl Iterator<Item = &T> {
    DepthFirstIter {
        queue: items.iter().collect(),
        done: HashSet::new(),
    }
}

trait HasDependencies {
    fn name(&self) -> &QualifiedName;
    fn deps(&self) -> Box<dyn Iterator<Item = QualifiedName> + '_>;
}

struct DepthFirstIter<'a, T: HasDependencies> {
    queue: VecDeque<&'a T>,
    done: HashSet<QualifiedName>,
}

impl<'a, T: HasDependencies> Iterator for DepthFirstIter<'a, T> {
    type Item = &'a T;

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

impl HasDependencies for Api<FnPhase> {
    fn name(&self) -> &QualifiedName {
        self.name()
    }

    fn deps(&self) -> Box<dyn Iterator<Item = QualifiedName> + '_> {
        self.deps()
    }
}

#[cfg(test)]
mod test {
    use crate::types::QualifiedName;

    use super::{depth_first_impl, HasDependencies};

    struct Thing(QualifiedName, Vec<QualifiedName>);

    impl HasDependencies for Thing {
        fn name(&self) -> &QualifiedName {
            &self.0
        }

        fn deps(&self) -> Box<dyn Iterator<Item = QualifiedName> + '_> {
            Box::new(self.1.iter().cloned())
        }
    }

    #[test]
    fn test() {
        let a = Thing(QualifiedName::new_from_cpp_name("a"), vec![]);
        let b = Thing(
            QualifiedName::new_from_cpp_name("b"),
            vec![
                QualifiedName::new_from_cpp_name("a"),
                QualifiedName::new_from_cpp_name("c"),
            ],
        );
        let c = Thing(
            QualifiedName::new_from_cpp_name("c"),
            vec![QualifiedName::new_from_cpp_name("a")],
        );
        let api_list = vec![a, b, c];
        let mut it = depth_first_impl(&api_list);
        assert_eq!(it.next().unwrap().0, QualifiedName::new_from_cpp_name("a"));
        assert_eq!(it.next().unwrap().0, QualifiedName::new_from_cpp_name("c"));
        assert_eq!(it.next().unwrap().0, QualifiedName::new_from_cpp_name("b"));
        assert!(it.next().is_none());
    }
}
