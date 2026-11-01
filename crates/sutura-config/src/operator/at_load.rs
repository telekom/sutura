//! The `operator` section through `OperatorSettings::load`.

use super::{ApiGroup, InvalidApiGroup, OperatorSettings, OperatorSettingsError};
use crate::environment::Environment;
use crate::settings::Sources;

#[test]
fn the_default_api_group_loads_from_a_tree_unfit_to_serve() {
    let settings = OperatorSettings::load(&Sources::defaults(Environment::Production)).expect("the defaults load");
    assert_eq!(settings.api_group().as_str(), "sutura.telekom.github.io");
}

#[test]
fn an_api_group_that_is_not_a_lowercase_subdomain_is_refused_at_load() {
    let refused = OperatorSettings::load(
        &Sources::defaults(Environment::Production).with_overlay("operator:\n  api_group: Sutura.example.com\n"),
    );
    assert!(
        matches!(
            refused,
            Err(OperatorSettingsError::ApiGroup {
                cause: InvalidApiGroup::NotSubdomain { .. }
            })
        ),
        "{refused:?}"
    );
    let too_long = format!("{}.example.com", "a".repeat(242));
    for raw in [
        "a..example.com",
        "-a.example.com",
        "a-.example.com",
        "a_b.example.com",
        too_long.as_str(),
    ] {
        assert!(
            matches!(ApiGroup::parse(raw), Err(InvalidApiGroup::NotSubdomain { .. })),
            "{raw}"
        );
    }
}

#[test]
fn a_metrics_token_serve_would_refuse_is_refused_at_load() {
    let refused =
        OperatorSettings::load(&Sources::defaults(Environment::Production).with_overlay("security:\n  metrics_token: short\n"));
    assert!(
        refused.is_err_and(|why| why.to_string().contains("metrics_token")),
        "the operator presents the token it loads"
    );
}
