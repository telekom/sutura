//! A remote leg as it arrives on the wire: arbitrary bytes, parsed and then certified.
//!
//! **The boundary.** `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md` decides that a
//! typed leg crosses between nodes and the receiver certifies it against its own pinned
//! definitions - *A new parser at a trust boundary* is the ADR's own reason this target exists.
//! `serde_json::from_slice::<LegWire>` runs first, with `deny_unknown_fields`, and a leg that parses
//! is then run through [`LegWire::parse`] against a small, non-empty pinned bundle.
//!
//! **The pin, and why it is not empty.** An empty catalog refuses every leg at the first metric
//! lookup, so the certification walk past it - joins, keys, filters, a dimension chain, a range -
//! was never reached; `fuzz/fuzz_targets/catalog_document.rs` documents and fixes the identical
//! trap for the catalog-loading target ("one fuzzed document beside fixed scaffolding ... is what
//! makes the target reach anything"). The pin here is two sources, `primary` and `secondary`, one
//! relationship crossing between them (`account_region`, `accounts.region_id` to
//! `regions.region_id`), and one metric on `accounts` (`revenue`, a sum of `revenue_cents`) whose
//! one dimension (`region_name`) is reached `via` that crossing. That shape gives a fact leg a
//! root, a bucket and a crossing dimension to decline or accept, and gives a lookup leg a second
//! source for one of the metric's chains to cross into (`regions`, by `crossed_into`'s own rule) -
//! the two leg kinds [`LegWire::Fact`] and [`LegWire::Lookup`] are both reachable past their first
//! check.
//!
//! **The parameters are fixed scaffolding too.** [`LegWire::parse`] takes the leg's bound values as
//! a separate argument - they travel as Arrow batches in the real protocol, never as JSON - so
//! nothing here fuzzes them. A fact leg's own certification requires its first two filters to be
//! `range`'s bounds on the bucket column (`certify::bounded`), which in turn requires exactly two
//! parameters resolving those two filters, so the harness supplies a fixed two-`Date` list for a
//! `LegWire::Fact` and an empty one for a `LegWire::Lookup`, chosen by which variant the bytes
//! decoded to rather than by the bytes themselves.
//!
//! **What is asserted beyond "did not abort".** Nothing past that: there is no further invariant to
//! check on a `LegPlan` this target does not otherwise use. The target exists to find a panic in the
//! JSON decode, the digest comparison or the certification walk over caller-controlled names and
//! numbers (a join chain, a filter list, a key list) that an example-based test would not think to
//! generate - now with a walk that is actually reachable on both leg kinds and not just refused at
//! the door.
//!
//! **The limit.** The catalog, the metric and the two parameter lists are fixed for the life of the
//! process, so nothing here fuzzes a second metric, a second relationship, a deeper chain, a
//! shared-calendar bucket, or a requested (non-definitional) filter bound to an allow-listed value -
//! every one of those needs either a second declared shape in the pin or a parameter the fuzzed
//! bytes cannot supply. `LegWire`'s own `deny_unknown_fields` shape is exercised by the JSON parse
//! regardless of the pin.

#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use sutura_catalog_local::LocalCatalog;
use sutura_domain::calendar::Date;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::plan::LegWire;
use sutura_domain::warehouse::ParamValue;

/// `accounts`, on source `primary`: the metric's own model, and a fact leg's root.
const SCAFFOLD_ACCOUNTS: &str = "---
kind: model
name: accounts
source: primary
table: fct_accounts
columns: [account_id, region_id, month, revenue_cents]
---
One row per account per month.
";

/// `regions`, on source `secondary`: what `account_region` crosses into, and a lookup leg's root.
const SCAFFOLD_REGIONS: &str = "---
kind: model
name: regions
source: secondary
table: dim_region
columns: [region_id, region_name]
---
One row per region, on a second source from `accounts`.
";

/// The one relationship that crosses sources, so a dimension reached through it is a link for a
/// fact leg and an entry point for a lookup leg - `crossed_into` and `Surface::of_fact`'s "a chain
/// that crosses to another source" branch both need one to exist at all.
const SCAFFOLD_RELATIONSHIP: &str = "---
kind: relationship
name: account_region
origin:
  model: accounts
  column: region_id
target:
  model: regions
  column: region_id
join_type: many_to_one
---
Many accounts to one region, the one across the two scaffold sources.
";

/// `revenue`, on `accounts`: a plain sum with no required filter, so a fact leg's range bounds are
/// the whole of what its filters have to carry, and one dimension reached by the crossing
/// relationship above, so both leg kinds have something to certify past their first check.
const SCAFFOLD_METRIC: &str = "---
kind: metric
name: revenue
model: accounts
measure:
  simple: { aggregate: sum, column: revenue_cents }
time_column: month
grains: [month]
dimensions:
  - name: region_name
    column: region_name
    via: account_region
audience: open
---
Revenue by account-month, the fuzz target's own scaffold metric.
";

/// One directory for the whole run, reused per iteration - this target never writes to it, unlike
/// `catalog_document.rs`'s per-iteration generated document, because the fuzzed bytes here are the
/// leg itself rather than a catalog document.
fn catalog() -> &'static LocalCatalog {
    static ROOT: std::sync::OnceLock<LocalCatalog> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("sutura-fuzz-leg-wire-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the fuzz harness can create its own scratch directory");
        std::fs::write(root.join("accounts.md"), SCAFFOLD_ACCOUNTS).expect("the fuzz harness can write its own scaffolding");
        std::fs::write(root.join("regions.md"), SCAFFOLD_REGIONS).expect("the fuzz harness can write its own scaffolding");
        std::fs::write(root.join("account_region.md"), SCAFFOLD_RELATIONSHIP)
            .expect("the fuzz harness can write its own scaffolding");
        std::fs::write(root.join("revenue.md"), SCAFFOLD_METRIC).expect("the fuzz harness can write its own scaffolding");
        LocalCatalog::new(
            SourceName::parse("fuzz").expect("a fixture source name is one"),
            root,
            DefinitionVersion::parse("0.0.0-fuzz").expect("a fixture version is one"),
        )
    })
}

fn fixed_pin() -> PinnedDefinitions {
    catalog().load().expect("the scaffold catalog is consistent")
}

static PINNED: LazyLock<PinnedDefinitions> = LazyLock::new(fixed_pin);

/// The two parameters a fact leg's range-bound filters resolve against, fixed beside the pin: the
/// same half-open month `active_subscriptions` and friends anchor on, in `examples/single-player`.
fn fact_params() -> Vec<ParamValue> {
    vec![
        ParamValue::Date(Date::new(2026, 6, 1).expect("a fixed scaffold date is valid")),
        ParamValue::Date(Date::new(2026, 7, 1).expect("a fixed scaffold date is valid")),
    ]
}

fuzz_target!(|data: &[u8]| {
    let Ok(leg) = serde_json::from_slice::<LegWire>(data) else {
        return;
    };
    let params = match &leg {
        LegWire::Fact { .. } => fact_params(),
        LegWire::Lookup { .. } => Vec::new(),
    };
    let _ = leg.parse(&PINNED, params);
});
