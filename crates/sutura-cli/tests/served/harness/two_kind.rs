//! The mixed-kind fixture: `files` and `postgres` in ONE deployment, over the same example data.
//!
//! **`telekom/sutura#112`'s remaining served cell.** The boot-time proof
//! (`a_catalog_reading_two_kinds_of_source_now_opens_both_and_reaches_the_second_kinds_own_boot_check`,
//! `serve/tests.rs`) says a mixed catalog opens both kinds; the in-process differential
//! (`two_sources_of_two_kinds_answer_the_same_rows_as_one_source_over_the_same_data`,
//! `sutura-app`'s own suite) says the two kinds agree on a question that SPANS them. Neither asks
//! the composed BINARY, over HTTP, whether either kind's OWN question resolves once both are open -
//! this fixture is what makes that askable, and `served/two_kind.rs` is what asks it.
//!
//! **This does not need `EXECUTES_LEGS`, and the settings below are built so it cannot drift into
//! needing it.** `daily_usage` is the one model this catalog declares with no `via` relationship at
//! all - see the model's own doc - so moving it onto a second source changes which adapter answers
//! `voice_minutes`, never how many legs a plan has. The two questions this fixture is for
//! (`recurring_revenue_june`, already in this module's parent; `voice_minutes_by_day`, `two_kind.rs`'s
//! own) each read exactly one of the two sources.

use super::postgres::{FixtureLoadGuard, load_into_tier, source_entry};
use super::{LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, derived_catalog, example_root, files_source, settings_over};

/// The second kind's source name - `postgres`, beside [`LOCAL_SOURCE`]'s `files`.
///
/// Named for what moves onto it rather than reused from [`super::LOOKUP_SOURCE`]: that name is the
/// two-`files`-sources fixture's own, and a mixed deployment declaring it under the SAME name would
/// read as the wrong topology to a reader diffing the two settings trees.
pub(crate) const PG_SOURCE: &str = "usage";

/// The one model this fixture moves off [`LOCAL_SOURCE`] and onto [`PG_SOURCE`].
const MOVED_MODEL: &str = "daily_usage.md";

/// A deployment holding one `files` source and one `postgres` source, each carrying the model the
/// other does not - and the [`FixtureLoadGuard`] the caller must hold for as long as it still asks
/// the served binary anything.
///
/// `None` is [`load_into_tier`]'s own no-tier outcome, checked here FIRST and cheaply for the same
/// reason: so a host with no tier derives no catalog at all. **The derivation runs BEFORE the
/// load, and that order is load-bearing, not incidental.** [`derived_catalog`] and
/// [`load_into_tier`]'s own password scratch directory are the SAME path - both are
/// `derived_beside(config_path(case))`, on purpose, so `Served`'s `Drop` removes one directory
/// rather than two - and [`derived_catalog`] WIPES it before copying the catalog in. Reversing the
/// order would let the copy erase the password file the load just wrote.
pub(crate) fn settings(case: &str) -> Option<(String, FixtureLoadGuard)> {
    sutura_dev::provisioned::here(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "postgres").endpoint()?;
    let example = example_root();
    let data = example.join("data");
    let catalog = derived_catalog(case, &example.join("catalog"), MOVED_MODEL, PG_SOURCE);
    let loaded = load_into_tier(case, PG_SOURCE)?;
    let sources = format!("{}{}", files_source(LOCAL_SOURCE, &data), source_entry(PG_SOURCE, &loaded));
    let settings = settings_over(
        &catalog,
        &data,
        LOOPBACK,
        &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
        &sources,
    );
    Some((settings, loaded.guard))
}
