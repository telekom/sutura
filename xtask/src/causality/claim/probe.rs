//! The doctored run outputs and fixtures `super`'s assertions are about.
//!
//! **HARNESS ONLY, AND THAT IS THE RULE THIS FILE IS AN INSTANCE OF.** Nothing here carries a
//! `#[test]`, deliberately: this gate never reverts a file that adds a `#[test]`, so moving
//! ASSERTIONS out of a test-bearing file orphans them the moment the declaring file is reverted,
//! while moving the HARNESS out leaves every assertion where it was. `super` carries the cells;
//! this carries what they read.
//!
//! The doctored outputs are the SHAPE [`super::base::failures`] actually reads, copied from that
//! module's own fixtures so the mutation classifier reuses a key already proven to classify - a
//! doctored line that shared no syntax with nextest's would be a test of nothing.

use crate::causality::fixtures::scoped;
use crate::causality::place::AddedTest;

/// One added claim cell, as the gate keys it (the same shape `causality::base`'s fixtures use).
pub(super) fn cell() -> AddedTest {
    scoped("sutura-cli", "crates/sutura-cli/src/audit.rs", &["the_added_one"])
        .into_iter()
        .next()
        .expect("one added test")
}

/// The post-image reader the classify fixtures judge panic sites against.
///
/// `crates/sutura-cli/src/audit.rs` is a MIXED file - production on top, `the_added_one` inside an
/// inline `#[cfg(test)] mod tests { .. }` at the bottom - which is the realistic shape of a claim
/// cell and the case the over-refusal finding was about: its own assertion panics at a line inside
/// the file's test fn, and a patch to the SAME file's production lines must not make that read as
/// a production panic. The two `crates/x/src/*` files are production with no test region, the
/// shape a downstream `.expect()` or a panic that stops at a production caller lands in.
pub(super) fn reader() -> impl Fn(&str) -> Option<String> {
    crate::causality::fixtures::tree(&[
        (
            "crates/sutura-cli/src/audit.rs",
            "pub fn audit() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn the_added_one() {\n        assert_eq!(1, 2);\n    }\n}\n",
        ),
        ("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        ("crates/x/src/other.rs", "pub fn g() -> u8 { 1 }\n"),
    ])
}

/// The same cell (`the_added_one`, same file), but declared `#[tokio::test] async fn` - the shape
/// BOTH of #761's cells actually use (`gh pr diff 761` lines 303-304, 347-348) and the shape a
/// bare-`fn` matcher never locates: `test_fn_region` would answer `None` and a real assertion kill
/// would misread as `NotByAssertion`. The assertion stays on the same line (7) as [`reader`]'s.
pub(super) fn async_reader() -> impl Fn(&str) -> Option<String> {
    crate::causality::fixtures::tree(&[
        (
            "crates/sutura-cli/src/audit.rs",
            "pub fn audit() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[tokio::test]\n    async fn the_added_one() {\n        assert_eq!(1, 2);\n    }\n}\n",
        ),
        ("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        ("crates/x/src/other.rs", "pub fn g() -> u8 { 1 }\n"),
    ])
}

/// The same cell again, declared `pub fn` - the other shape a bare-`fn` matcher never locates.
pub(super) fn pub_fn_reader() -> impl Fn(&str) -> Option<String> {
    crate::causality::fixtures::tree(&[
        (
            "crates/sutura-cli/src/audit.rs",
            "pub fn audit() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    pub fn the_added_one() {\n        assert_eq!(1, 2);\n    }\n}\n",
        ),
        ("crates/x/src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        ("crates/x/src/other.rs", "pub fn g() -> u8 { 1 }\n"),
    ])
}

/// A run that reported exactly `the_added_one` failing by its OWN assertion - the panic
/// (`crates/sutura-cli/src/audit.rs:7`) is inside the cell's test region, so the mutation KILLS.
pub(super) const KILLED: &str = concat!(
    "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
    "thread 'audit::tests::the_added_one' panicked at crates/sutura-cli/src/audit.rs:7:9:\n",
    "     Summary [   0.4s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
    "error: test run failed\n",
);

/// A run that reported a DIFFERENT test failing - the mutation does not kill this cell.
pub(super) const UNRELATED: &str = concat!(
    "        FAIL [   0.313s] (86/1810) sutura-app::differential tests::postgres::sums_by_month\n",
    "error: test run failed\n",
);

/// A run that reports the cell failing but NEVER carries a `panicked at` site - a kill by
/// `std::process::exit(n)` or an abort/signal death. No site lands inside the cell's test region,
/// so it is not an assertion kill.
pub(super) const EXIT_NO_SITE: &str = concat!(
    "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
    "     Summary [   0.4s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
    "error: test run failed\n",
);

/// A run whose panic landed in an UNPATCHED PRODUCTION file - the ordinary careless mutation, a
/// patch that makes `lib.rs` return `None`/`Err` and an existing `.expect()` in `other.rs` fires.
/// The site is production, not the cell's test region, so it is not an assertion kill.
pub(super) const DOWNSTREAM_EXPECT: &str = concat!(
    "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
    "thread 'audit::tests::the_added_one' panicked at crates/x/src/other.rs:1:44:\n",
    "error: test run failed\n",
);

/// A run whose panic landed in a PRODUCTION caller - `crates/x/src/lib.rs:3`, not the cell's own
/// test fn - so it reads as a production panic, not the cell's own assertion.
///
/// NAMED FOR WHAT THE RULE REFUSES, not for `#[track_caller]`: this SHOULD be refused because the
/// site is production. It deliberately does NOT claim a `#[track_caller]` relocation is refused
/// generally - a `track_caller` panic called DIRECTLY from the cell panics at the cell's own line
/// (which this fixture does not model), and no site-based rule can refuse that; it is review-held.
pub(super) const PRODUCTION_CALLER: &str = concat!(
    "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
    "thread 'audit::tests::the_added_one' panicked at crates/x/src/lib.rs:3:5:\n",
    "error: test run failed\n",
);

/// A run whose text is a genuine COMPILE failure: the tree under the mutation never reached any
/// test, so nothing here attests either way about `the_added_one` - `#855`'s leading hypothesis
/// for a wrong verdict was a nested build compiling nothing, and this is that shape.
pub(super) const DID_NOT_COMPILE: &str = concat!(
    "error[E0061]: this function takes 1 argument but 0 arguments were supplied\n",
    "error: could not compile `sutura-cli` (lib) due to 1 previous error\n",
);

/// A run whose filter matched no test at all - orphaned by the patch (the mutation moved or
/// deleted the cell's own item) rather than killed by it, the fourth shape `could_not_attest`
/// covers alongside a compile or manifest failure.
pub(super) const NO_TESTS_TO_RUN: &str = "error: no tests to run\n";
