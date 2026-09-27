//! The depth bound on the recursive origin walk over the merged value tree.

use super::{Environment, Settings, SettingsError, Sources};

/// `levels` single-key maps around one leaf: the walk meets the leaf at depth `levels + 1`.
fn nested(levels: usize) -> Sources {
    let mut overlay = String::new();
    for level in 0..levels {
        overlay.push_str(&"  ".repeat(level));
        overlay.push_str("k:\n");
    }
    overlay.push_str(&"  ".repeat(levels));
    overlay.push_str("leaf: true\n");
    Sources::defaults(Environment::Development).with_overlay(overlay)
}

/// An overlay nesting well past `MERGE_DEPTH_LIMIT` is refused with `MergeDepth`.
#[test]
fn a_pathologically_nested_configuration_is_refused_at_the_depth_bound() {
    let error = Settings::load(&nested(70)).expect_err("an over-depth overlay refuses startup");
    assert!(
        matches!(*error.reason(), SettingsError::MergeDepth { limit, .. } if limit == 64),
        "expected SettingsError::MergeDepth with limit=64, got {error:?}",
    );
}

/// A tree reaching exactly depth 64 passes the walk. `k` is not a setting, so the load still
/// refuses, but at deserialisation and not at the bound.
#[test]
fn a_configuration_at_the_depth_limit_passes_the_walk() {
    let error = Settings::load(&nested(63)).expect_err("`k` is not a setting");
    assert!(!matches!(*error.reason(), SettingsError::MergeDepth { .. }), "{error:?}");
}

/// One level deeper, depth 65, is the first the walk refuses.
#[test]
fn a_configuration_one_past_the_depth_limit_is_refused() {
    let error = Settings::load(&nested(64)).expect_err("depth 65 refuses startup");
    assert!(
        matches!(*error.reason(), SettingsError::MergeDepth { found: 65, limit: 64 }),
        "{error:?}"
    );
}

/// A real configuration's depth is nowhere near the bound.
#[test]
fn a_shallow_configuration_is_unaffected_by_the_depth_bound() {
    let sources = Sources::defaults(Environment::Development).with_overlay("server:\n  port: 9000\n");
    let settings = Settings::load(&sources).expect("a shallow overlay loads");
    assert_eq!(settings.server().bind().port(), 9000);
}
