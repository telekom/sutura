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

/// A run that reported exactly `the_added_one` failing - the mutation KILLS the cell.
pub(super) const KILLED: &str = concat!(
    "    Starting 3 tests across 47 binaries\n",
    "        FAIL [   0.021s] (2/3) sutura-cli::bin/sutura audit::tests::the_added_one\n",
    "     Summary [   0.4s] 2 tests run: 1 passed, 1 failed, 0 skipped\n",
    "error: test run failed\n",
);

/// A run that reported a DIFFERENT test failing - the mutation does not kill this cell.
pub(super) const UNRELATED: &str = concat!(
    "        FAIL [   0.313s] (86/1810) sutura-app::differential tests::postgres::sums_by_month\n",
    "error: test run failed\n",
);
