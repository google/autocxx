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

use indoc::indoc;

/// This is logic to call either an overloaded operator new/delete
/// or the standard one.
/// The SFINAE magic here is: int is a better match than long,
/// and so the versions which match class-specific operator new/delete
/// will be used in preference to the general global ::operator new/delete.
pub(super) static NEW_AND_DELETE_PRELUDE: &str = indoc! {"
    #include <stddef.h>
    #ifndef AUTOCXX_NEW_AND_DELETE_PRELUDE
    #define AUTOCXX_NEW_AND_DELETE_PRELUDE
    // Mechanics to call custom operator new and delete
    template <typename T>
    auto delete_imp(T *ptr, int) -> decltype((void)T::operator delete(ptr)) {
      T::operator delete(ptr);
    }
    template <typename T> void delete_imp(T *ptr, long) { ::operator delete(ptr); }
    template <typename T> void delete_appropriately(T *obj) {
      // 0 is a better match for the first 'delete_imp' so will match
      // preferentially.
      delete_imp(obj, 0);
    }
    template <typename T>
    auto new_imp(size_t count, int) -> decltype(T::operator new(count)) {
      return T::operator new(count);
    }
    template <typename T> void *new_imp(size_t count, long) {
      return ::operator new(count);
    }
    template <typename T> T *new_appropriately(size_t count) {
      // 0 is a better match for the first 'delete_imp' so will match
      // preferentially.
      return static_cast<T *>(new_imp<T>(count, 0));
    }
    #endif // AUTOCXX_NEW_AND_DELETE_PRELUDE
"};
