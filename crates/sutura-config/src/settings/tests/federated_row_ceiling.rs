//! `governance.federated_row_ceiling` reaching `Settings::federated_row_ceiling()`
//! (`github.com/telekom/sutura#828`).

use sutura_domain::plan::{FederatedRowCeiling, InvalidFederatedRowCeiling, RowCeiling};

use crate::settings::{Environment, Settings, SettingsError, SettingsLoadError, Sources};

fn loaded(rows: &str) -> Result<Settings, SettingsLoadError> {
    Settings::load(
        &Sources::defaults(Environment::Development).with_overlay(format!("governance:\n  federated_row_ceiling: {rows}\n")),
    )
}

#[test]
fn a_written_ceiling_reaches_settings_as_a_parsed_federated_row_ceiling() {
    let settings = loaded("500").expect("a positive federated row ceiling loads");
    assert_eq!(
        settings.federated_row_ceiling(),
        FederatedRowCeiling::parse(500).expect("500 is a federated row ceiling")
    );
    assert_eq!(settings.row_ceilings().federated(), settings.federated_row_ceiling());
}

#[test]
fn no_written_ceiling_defaults_to_the_compiled_bound() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert_eq!(settings.federated_row_ceiling(), FederatedRowCeiling::DEFAULT);
    assert_eq!(settings.row_ceilings().top(), RowCeiling::DEFAULT);
}

#[test]
fn zero_is_refused_rather_than_read_as_unlimited() {
    let error = loaded("0").expect_err("a zero federated row ceiling does not load");
    assert!(
        matches!(
            error.reason(),
            SettingsError::FederatedRowCeiling {
                cause: InvalidFederatedRowCeiling::Zero
            }
        ),
        "{error:?}"
    );
}

#[test]
fn the_maximum_loads_and_one_above_it_is_refused_naming_the_key() {
    loaded(&FederatedRowCeiling::MAX.to_string()).expect("the maximum is a federated row ceiling");
    let error = loaded(&(FederatedRowCeiling::MAX + 1).to_string()).expect_err("above the maximum does not load");
    assert!(
        matches!(
            error.reason(),
            SettingsError::FederatedRowCeiling {
                cause: InvalidFederatedRowCeiling::AboveTheMaximum { .. }
            }
        ),
        "{error:?}"
    );
}

#[test]
fn the_top_ceiling_is_still_unbounded_above() {
    let sources = Sources::defaults(Environment::Development).with_overlay("governance:\n  top_row_ceiling: 4000000000\n");
    let settings = Settings::load(&sources).expect("an existing key does not gain a maximum");
    assert_eq!(settings.row_ceiling().get(), 4_000_000_000);
}
