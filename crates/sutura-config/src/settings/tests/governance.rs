//! `governance.per_replica_spend_ceiling` reaching `Settings::spend_budget()`.
//!
//! A file of its own because `settings/tests.rs` reached the thousand-line limit `cargo xtask
//! max-lines` enforces. This is the settings-layer half of the review finding that the two
//! composition roots' `.with_spend_ledger(..)` calls were held by nothing: neither root drives a
//! `BudgetExhausted` refusal today (only `BigQuery` prices a dry run, and its cells are
//! `#[ignore]`d), so both composition roots' own reads of `settings.spend_budget()` are
//! held by READING rather than by a cell - see `.agents/skills/sutura/invariants/SKILL.md`'s
//! spend-counter row.

use crate::governance::SpendBudget;
use sutura_domain::model::SourceName;

use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

#[test]
fn a_written_ceiling_reaches_settings_as_a_parsed_spend_budget() {
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("governance:\n  per_replica_spend_ceiling:\n    bytes: 1000\n    window_seconds: 60\n");
    let settings = Settings::load(&sources).expect("a valid ceiling and window load");
    assert_eq!(
        settings.spend_budget(),
        Some(SpendBudget::parse(1000, 60).expect("1000 bytes over 60s is a valid budget"))
    );
}

#[test]
fn no_written_ceiling_is_no_budget_at_all() {
    // The embedded defaults name no `governance` key, and that is `docs/adr/0030`'s "absent means
    // no ceiling" read back through the whole settings stack rather than asserted at `parse_spend_budget`
    // alone.
    let settings =
        Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load with no ceiling written");
    assert_eq!(settings.spend_budget(), None);
}

/// One `bigquery` source, single-user so the shared posture needs no acknowledgement of its own.
const BIGQUERY_SOURCE: &str = "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\nsources:\n  \
    warehouse:\n    kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    dataset: \"warehouse\"\n    \
    credential_file: \"/nonexistent/sutura-test-bigquery.json\"\n    max_bytes_billed: 1073741824\n    \
    posture: \"shared-service-user\"\n";

const A_CEILING: &str = "governance:\n  per_replica_spend_ceiling:\n    bytes: 1000\n    window_seconds: 60\n";

#[test]
fn a_spend_ceiling_over_a_bigquery_source_that_prices_nothing_is_not_fit_to_serve() {
    // `telekom/sutura#929`'s spend finding: the ADBC transport declines the dry run, so the ledger
    // charged every `BigQuery` question zero and a declared ceiling bounded nothing on the one
    // source it exists for - served as a normal source, in silence.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!("{BIGQUERY_SOURCE}{A_CEILING}"));
    let error = Settings::load(&sources).expect_err("a ceiling over an unpriced source does not serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    let alias = SourceName::parse("warehouse").expect("a valid alias");
    assert_eq!(*refusals, vec![NotFitToServe::UnpricedSourceUnderSpendCeiling { alias }]);
    // THE CONTROL: the same source with no ceiling declared refuses nothing, so the cell above is
    // the ceiling's refusal and not something else about the fixture.
    let unbounded = Settings::load(&Sources::defaults(Environment::Development).with_overlay(BIGQUERY_SOURCE))
        .expect("the source alone loads");
    assert!(unbounded.refusals().is_empty(), "{:?}", unbounded.refusals());
}
