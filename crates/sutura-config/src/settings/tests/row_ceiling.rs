//! `governance.top_row_ceiling` reaching `Settings::row_ceiling()`.
//!
//! A file of its own, matching `governance.rs` beside it: this is the settings-layer half of the
//! review finding that `LocalService::with_row_ceiling` (`telekom/sutura#870`) was wired to
//! nothing - no settings key existed, so the refusal text telling a caller to "ask the operator to
//! raise this ceiling" named a lever no deployment could move.

use sutura_domain::plan::RowCeiling;

use crate::settings::{Environment, Settings, SettingsError, Sources};

#[test]
fn a_written_ceiling_reaches_settings_as_a_parsed_row_ceiling() {
    let sources = Sources::defaults(Environment::Development).with_overlay("governance:\n  top_row_ceiling: 500\n");
    let settings = Settings::load(&sources).expect("a positive row ceiling loads");
    assert_eq!(settings.row_ceiling(), RowCeiling::parse(500).expect("500 is a row ceiling"));
}

#[test]
fn no_written_ceiling_defaults_to_the_compiled_bound() {
    let settings =
        Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load with no ceiling written");
    assert_eq!(settings.row_ceiling(), RowCeiling::DEFAULT);
}

#[test]
fn a_zero_ceiling_is_refused_rather_than_read_as_unlimited() {
    let sources = Sources::defaults(Environment::Development).with_overlay("governance:\n  top_row_ceiling: 0\n");
    let error = Settings::load(&sources).expect_err("a zero row ceiling does not load");
    assert!(matches!(error.reason(), SettingsError::RowCeiling { .. }), "{error:?}");
}
