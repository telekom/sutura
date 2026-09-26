//! The depth bound on the recursive origin walk over the merged value tree.

use super::{Environment, Settings, SettingsError, Sources};

/// An overlay nesting past `MERGE_DEPTH_LIMIT` is refused with `MergeDepth`.
#[test]
fn a_pathologically_nested_configuration_is_refused_at_the_depth_bound() {
    // Seventy single-key maps, past the limit of 64. The origin walk runs before deserialisation,
    // so the key names never meet `deny_unknown_fields`.
    let mut overlay = String::new();
    for level in 0..70 {
        overlay.push_str(&"  ".repeat(level));
        overlay.push_str("k:\n");
    }
    overlay.push_str(&"  ".repeat(70));
    overlay.push_str("leaf: true\n");

    let sources = Sources::defaults(Environment::Development).with_overlay(overlay);
    let error = Settings::load(&sources).expect_err("an over-depth overlay refuses startup");
    assert!(
        matches!(*error.reason(), SettingsError::MergeDepth { limit, .. } if limit == 64),
        "expected SettingsError::MergeDepth with limit=64, got {error:?}",
    );
}

/// A real configuration's depth is nowhere near the bound.
#[test]
fn a_shallow_configuration_is_unaffected_by_the_depth_bound() {
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  port: 9000\n");
    let settings = Settings::load(&sources).expect("a shallow overlay loads");
    assert_eq!(settings.server().bind().port(), 9000);
}
