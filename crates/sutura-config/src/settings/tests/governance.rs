//! `governance.per_replica_spend_ceiling` reaching `Settings::spend_budget()`.
//!
//! A file of its own because `settings/tests.rs` reached the thousand-line limit `cargo xtask
//! max-lines` enforces. This is the settings-layer half of the review finding that the two
//! composition roots' `.with_spend_ledger(..)` calls were held by nothing: neither root drives a
//! `BudgetExhausted` refusal today (only `BigQuery` prices a dry run, and its cells are
//! `#[ignore]`d), so `sutura-serve`'s and `sutura-cli`'s own reads of `settings.spend_budget()` are
//! held by READING rather than by a cell - see `.agents/skills/sutura/invariants/SKILL.md`'s
//! spend-counter row.

use crate::governance::SpendBudget;
use crate::settings::{Environment, Settings, Sources};

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
