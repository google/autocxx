# How to Contribute

We'd love to accept your patches and contributions to this project. There are
just a few small guidelines you need to follow.

## Contributor License Agreement

Contributions to this project must be accompanied by a Contributor License
Agreement. You (or your employer) retain the copyright to your contribution;
this simply gives us permission to use and redistribute your contributions as
part of the project. Head over to <https://cla.developers.google.com/> to see
your current agreements on file or to sign a new one.

You generally only need to submit a CLA once, so if you've already submitted one
(even if it was for a different project), you probably don't need to do it
again.

## Code reviews

All submissions, including submissions by project members, require review. We
use GitHub pull requests for this purpose. Consult
[GitHub Help](https://help.github.com/articles/about-pull-requests/) for more
information on using pull requests.

## Community Guidelines

This project follows [Google's Open Source Community
Guidelines](https://opensource.google/conduct/).

## Directory structure

* `book` - you're reading it!
* `demo` - a very simple demo example
* `examples` - will gradually fill with more complex examples
* `parser` - code which parses a single `include_cpp!` macro. Used by both the macro
  (which doesn't do much) and the code generator (which does much more, by means of
  `engine` below)
* `engine` - all the core code for actual code generation.
* `macro` - the procedural macro which expands the Rust code.
* `gen/build` - a library to be used from `build.rs` scripts to generate .cc and .h
  files from an `include_cxx` section.
* `gen/cmd` - a command-line tool which does the same.
* `src` (outermost project) - a wrapper crate which imports the procedural macro and
  a few other things.

## Where to start reading

The main algorithm is in `engine/src/lib.rs`, in the function `generate()`. This asks
`bindgen` to generate a heap of Rust code and then passes it into
`engine/src/conversion` to convert it to be a format suitable for input
to `cxx`.

However, most of the actual code is in `engine/src/conversion/mod.rs`.

At the moment we're using a slightly branched version of `bindgen` called `autocxx-bindgen`.
It's hoped this is temporary (see [here](https://github.com/google/autocxx/issues/124)
for status.)

## How to develop

If you're making a change, here's what you need to do to get useful diagnostics etc.
First of all, `cargo run` in the `demo` directory. If it breaks, you don't get much
in the way of useful diagnostics, because `stdout` is swallowed by cargo build scripts.
So, practically speaking, you would almost always move onto running one of the tests
in the test suite. With suitable options, you can get plenty of output. For instance:

```ignore
RUST_BACKTRACE=1 RUST_LOG=autocxx_engine=info cargo test --all test_cycle_string_full_pipeline -- --nocapture
```

This is especially valuable to see the `bindgen` output Rust code, and then the converted Rust code which we pass into cxx. Usually, most problems are due to some mis-conversion somewhere
in `engine/src/conversion`. See [here](https://docs.rs/autocxx-engine/latest/autocxx_engine/struct.IncludeCppEngine.html) for documentation and diagrams on how the engine works.

You may also wish to set `AUTOCXX_ASAN=1` on Linux when running tests. To exercise all
the code paths related to generating both C++ and Rust side shims, you can set
`AUTOCXX_FORCE_WRAPPER_GENERATION=1`. The test suite doesn't do this by default because
we also want to test the normal code paths. (In the future we might want to
parameterize the test suite to do both.)

## Reporting bugs

Moved to [its own document](reporting_bugs.md).

## How to contribute to this manual

More examples in this manual are _very_ welcome!

Because `autocxx` examples require both Rust and C++ code to be linked together,
a custom preprocessor is used for this manual. See one of the existing examples
such as in `index.md` to see how to do this.

## Maintenance responsibilities

autocxx is currently in maintenance mode. In future, it may undergo further
feature development, but for now the job is just to keep it stable
and functional.

Events may occur which nonetheless require changes:

* Changes to Rust
* Changes to `bindgen`
* Pull requests raised against autocxx
* Issues reported against autocxx

Here's what to do. If issues are reported, encourage the reporter
to raise a PR with a minimized test case by pointing them at the
["reporting bugs"](reporting_bugs.md) page. This resolves all concerns about
reproducibility. autocxx is quite sensitive to the environment in which it
runs, e.g. standard library header files in use, and it's rare to be able
to reproduce a reporter's bug without them raising a reproducible test case
like this.

If CI starts to fail due to a Rust change (or,
if we [manage to unfork bindgen, a bindgen change](https://github.com/google/autocxx/issues/124))
then raise a pull request to fix it. (Most commonly CI starts to fail
because of more aggressive clippy lints).

For user-contributed pull requests, merge them if you can! For reproducible
bug reports, try to investigate them. A fair proportion of the bug reports
boil down to certain key known areas of technical debt or limitations,
described below, and you can mark them as a duplicate or similar.

autocxx tends to make a release every month or two, dependent on what changes
have been made. Ideally, autocxx would release after every single pull
request but the process takes about 15 minutes (see below) so it's not
that frequent.

## Release process

To make a new release of autocxx,

* First ensure there's green CI on github.
* Check out `main` locally and ensure it's up to date with `origin/main`.
* Make a new branch
* Run `tools/upgrade-version.sh OLD NEW` where `OLD` is the previous
  released version number, and `NEW` is the new version number.
* Commit that and make a PR; ensure it passes tests on github CI.
* If so, merge that PR, and update locally.
* Run `tools/publish-all.sh`. This will do the actual `cargo publish`
  for all the various crates.
* On [github releases](https://github.com/google/autocxx/releases),
  choose Draft a new release. Add a tag for `v0.X.Y`. Go through
  the process to automatically create release notes.

## Rolling bindgen

autocxx currently depends upon a fork of bindgen called `autocxx-bindgen`.
[This issue](https://github.com/google/autocxx/issues/124) is an attempt
at entirely unforking bindgen. There are about 14 upstream PRs required;
we should work hard to make required changes to get them accepted,
because then this section becomes irrelevant and you'll never need to follow
these steps.

Otherwise, periodically, you'll need to merge bindgen into autocxx-bindgen.
[`autocxx-bindgen` is kept in this repository](https://github.com/adetaylor/rust-bindgen)
in the `master` branch (which should be renamed eventually if we don't
unfork bindgen).

To update it,

* Check out the master branch of that repo
* Make a new branch
* `git pull <upstream repo> main` (note that we use merge rather than
  rebase)
* Resolve conflicts and commit.
* Push this branch to [the autocxx-bindgen repo](https://github.com/adetaylor/rust-bindgen)
* Make a new branch of autocxx
* Amend [`engine/Cargo.toml`](https://github.com/google/autocxx/blob/main/engine/Cargo.toml#L34)
  to point to the new git branch of autocxx-bindgen instead of using
  a published version (see the commented-out line)
* Maybe `cd integration-tests; cargo test` to ensure things build and seem to
  work... but you won't really know until you push to github CI
* So, push your autocxx and make a pull request
* If everything passes on CI, bump the `autocxx-bindgen` version number.
  This needs to be done in `bindgen/Cargo.toml` and `Cargo.toml`. Commit that.
* Push directly onto the master branch of
  [the autocxx-bindgen repo](https://github.com/adetaylor/rust-bindgen).
  You can't raise this roll as a pull request because Google's CLA tooling
  will reject it.
* `cargo publish` the new autocxx-bindgen version.
* Amend your `autocxx` PR to switch to the published `autocxx-bindgen` version,
  and push that.
* Ensure CI still passes.
* If so, merge your `autocxx` PR.

## Major areas of tech debt

The core of autocxx is the `Api` enum. That's solid. It's created by parsing
bindgen output, and exists all the way through to Rust and C++ codegen.
It is annotated by various analysis phases; it's parameterized by
the analysis phase so it's literally impossible to access those annotations
before they exist.

Because this core is solid, most of the areas of tech debt are discrete and
don't really overlap with each other. If you wish to tackle them, they
can be tackled fairly independently.

Without further ado, they are:

1. **Naming**. autocxx deals with lots of names - the original C++ name,
   the name chosen by bindgen, the name we want to give to cxx, etc.
   When one kind of name is used for another purpose, certain invariants
   need to be applied, e.g. avoiding conflicting overloads, or avoiding
   built-in cxx names such as `UniquePtr`. So, these different kinds of
   names really need to be encapsulated in newtype wrappers which
   enforce these invariants. This has been [started here](https://github.com/google/autocxx/issues/520).
   The other tricky aspect here is that we deal with name conflicts in
   two different ways. What are name conflicts? bindgen gives us
   a hierarchical namespace which we have to map to a flat namespace to give
   to cxx. For types, we just reject any duplicate names. For funtions, though,
   we go to some efforts to rename them to be non-conflicting. We should
   become uniform here by abstracting the name deconfliction stuff from the
   function analysis.

2. **Fork of bindgen**. As noted under [rolling bindgen, above](#rolling-bindgen),
   we currently use a fork of bindgen called `autocxx-bindgen`. This carries
   a maintenance burden as we roll upstream. Also as noted above, we're
   making good progress on undoing this. Tracked
   [here](https://github.com/google/autocxx/issues/124).

3. **Sensitivity to bindgen bugs caused by constructor calculations**.
   `bindgen` isn't perfect. When it mis-generates a type, users can
   ask bindgen to mark that type as "opaque", in which case it's represented
   as just an array of bytes. This is especially useful for complex templated
   C++ types where bindgen still has a number of bugs. Unfortunately, users
   of autocxx don't get that luxury. In order to calculate whether types
   are movable, autocxx requires visibility into all the fields.
   That means that any types marked opaque by bindgen are pretty useless,
   and we never currently ask bindgen for opaque types.
   `bindgen` might be in a better place to request implicit constructors,
   then we can solve these problems. This is tracked
   [here](https://github.com/google/autocxx/issues/1461).

4. **`CppRef` and friends.** `cxx` (and therefore `autocxx`) can be used
   to create multiple mutable references to the same Rust memory.
   This is UB. In cxx this problem applies only to types marked `Trivial`
   since others are zero-sized. In `autocxx`, we give all types a size
   so they can be allocated on the stack, and so the problem is worse.
   (This is explained more
   [here](https://github.com/google/autocxx/blob/main/engine/src/conversion/codegen_rs/non_pod_struct.rs#L41)).
   The intended solution here is to cease to use Rust references to
   represent C++ references - instead using a newtype wrapper called
   `CppRef<T>`. `autocxx` can already work in this mode using
   [`unsafe_references_wrapped`](https://docs.rs/autocxx/latest/autocxx/macro.safety.html)
   but for now it works well only on nightly Rust, pending the merge of the
   [arbitrary self types v2 RFC](https://github.com/rust-lang/rust/pull/135881).
   Even then, this isn't quite perfect because `cxx` expects to use
   Rust references, and its key types (such as `cxx::UniquePtr`) provide
   `Deref` implementations which point in that direction. This isn't a
   huge problem but I mention it here because it's one of the very few
   occasions in which our dependence on `cxx` is limiting for us.