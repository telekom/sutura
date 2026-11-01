//! `vendor/` is out of the gate's scope: upstream's attributes are upstream's to change.

use super::rust_source;

#[test]
fn a_threshold_expect_in_vendored_code_is_not_judged() {
    assert!(rust_source("crates/sutura-domain/src/lib.rs"));
    assert!(!rust_source("vendor/datafusion-physical-plan/src/joins/utils.rs"));
    assert!(!rust_source("AGENTS.md"));
}
