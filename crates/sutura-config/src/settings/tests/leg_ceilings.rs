//! `governance.lookup_leg_ceiling` and `governance.leg_ceiling` reaching `Settings::ceilings()`,
//! their defaults, and their startup refusals.

use sutura_domain::plan::{InvalidPlanCeiling, LegCeiling, LookupLegCeiling};

use crate::settings::{Environment, Settings, SettingsError, SettingsLoadError, Sources};

const LOOKUP_KEY: &str = "lookup_leg_ceiling";
const LEG_KEY: &str = "leg_ceiling";

/// Settings with `governance.<key>` written as `value`.
fn loaded(key: &str, value: usize) -> Result<Settings, SettingsLoadError> {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("governance:\n  {key}: {value}\n")))
}

#[test]
fn no_written_leg_ceilings_default_to_three_lookup_legs_and_six_legs() {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert_eq!(settings.ceilings().plan().lookup_legs(), LookupLegCeiling::DEFAULT);
    assert_eq!(settings.ceilings().plan().lookup_legs().get(), 3);
    assert_eq!(settings.ceilings().plan().legs(), LegCeiling::DEFAULT);
    assert_eq!(settings.ceilings().plan().legs().get(), 6);
}

#[test]
fn a_written_lookup_leg_ceiling_lower_or_higher_than_the_default_reaches_settings() {
    for value in [1, 5] {
        let settings = loaded(LOOKUP_KEY, value).expect("a positive lookup leg ceiling loads");
        assert_eq!(settings.ceilings().plan().lookup_legs().get(), value);
    }
}

#[test]
fn a_written_leg_ceiling_lower_or_higher_than_the_default_reaches_settings() {
    for value in [2, 10] {
        let settings = loaded(LEG_KEY, value).expect("a positive leg ceiling loads");
        assert_eq!(settings.ceilings().plan().legs().get(), value);
    }
}

/// The startup refusal for `value` at `key`, which must name the key.
fn refused(key: &str, value: usize) -> SettingsLoadError {
    let error = loaded(key, value).expect_err("an unusable ceiling does not load");
    assert!(
        error.reason().to_string().contains(&format!("governance.{key}")),
        "the refusal must name the key: {}",
        error.reason()
    );
    error
}

#[test]
fn zero_and_one_above_the_maximum_are_refused_naming_each_key_and_the_maximum_loads() {
    let zero = refused(LOOKUP_KEY, 0);
    assert!(
        matches!(
            zero.reason(),
            SettingsError::LookupLegCeiling {
                cause: InvalidPlanCeiling::Zero
            }
        ),
        "{zero:?}"
    );
    let above = refused(LOOKUP_KEY, LookupLegCeiling::MAX + 1);
    assert!(
        matches!(
            above.reason(),
            SettingsError::LookupLegCeiling {
                cause: InvalidPlanCeiling::AboveTheMaximum {
                    max: LookupLegCeiling::MAX
                }
            }
        ),
        "{above:?}"
    );
    loaded(LOOKUP_KEY, LookupLegCeiling::MAX).expect("the maximum is a lookup leg ceiling");
    let zero = refused(LEG_KEY, 0);
    assert!(
        matches!(
            zero.reason(),
            SettingsError::LegCeiling {
                cause: InvalidPlanCeiling::Zero
            }
        ),
        "{zero:?}"
    );
    let above = refused(LEG_KEY, LegCeiling::MAX + 1);
    assert!(
        matches!(
            above.reason(),
            SettingsError::LegCeiling {
                cause: InvalidPlanCeiling::AboveTheMaximum { max: LegCeiling::MAX }
            }
        ),
        "{above:?}"
    );
    loaded(LEG_KEY, LegCeiling::MAX).expect("the maximum is a leg ceiling");
}
