//! `governance.source_ceiling` reaching `Settings::ceilings()`, and its two startup refusals.

use sutura_domain::plan::{InvalidPlanCeiling, SourceCeiling};

use crate::settings::{Environment, Settings, SettingsError, SettingsLoadError, Sources};

const KEY: &str = "governance.source_ceiling";

fn loaded(sources: &str) -> Result<Settings, SettingsLoadError> {
    Settings::load(
        &Sources::defaults(Environment::Development).with_overlay(format!("governance:\n  source_ceiling: {sources}\n")),
    )
}

#[test]
fn a_written_ceiling_reaches_settings_as_a_parsed_source_ceiling() {
    let settings = loaded("2").expect("a positive source ceiling loads");
    assert_eq!(
        settings.ceilings().plan().sources(),
        SourceCeiling::parse(2).expect("2 is a source ceiling")
    );
}

#[test]
fn no_written_ceiling_defaults_to_four_data_systems() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert_eq!(settings.ceilings().plan().sources(), SourceCeiling::DEFAULT);
}

#[test]
fn zero_is_refused_naming_the_key() {
    let error = loaded("0").expect_err("a zero source ceiling does not load");
    assert!(
        matches!(
            error.reason(),
            SettingsError::SourceCeiling {
                cause: InvalidPlanCeiling::Zero
            }
        ),
        "{error:?}"
    );
    assert!(
        error.reason().to_string().contains(KEY),
        "the refusal must name the key: {}",
        error.reason()
    );
}

#[test]
fn the_maximum_loads_and_one_above_it_is_refused_naming_the_key() {
    loaded(&SourceCeiling::MAX.to_string()).expect("the maximum is a source ceiling");
    let error = loaded(&(SourceCeiling::MAX + 1).to_string()).expect_err("above the maximum does not load");
    assert!(
        matches!(
            error.reason(),
            SettingsError::SourceCeiling {
                cause: InvalidPlanCeiling::AboveTheMaximum { .. }
            }
        ),
        "{error:?}"
    );
    assert!(
        error.reason().to_string().contains(KEY),
        "the refusal must name the key: {}",
        error.reason()
    );
}
