#![forbid(unsafe_code)]
//! The answer path end to end: compile, mint, execute, materialise - on the example's own
//! `DataFusion` engine and CSVs, so #140's two ceilings (the working-set maximum, the request
//! deadline) have a measured number to be set against rather than "a provisional number nobody
//! has measured".
//!
//! `github.com/telekom/sutura#915`. Deterministic and local: `examples/single-player` is a
//! committed catalog over committed CSVs, read the same way `sutura-cli/tests/example.rs` reads
//! it for the quickstart - no network, no server, so this runs on a developer machine and inside
//! the nix sandbox alike. `DataFusion` rather than `DuckDB`: it is THE engine, in-process, and the
//! one a served deployment actually answers a `files` source with - `crates/sutura-cli/src/
//! sources/files.rs`.
//!
//! **The limit, stated where the claim is.** The corpus is a small synthetic telco warehouse
//! (hundreds of rows, not millions), so this measures the shape of the path - compile, mint,
//! execute, materialise, in that order, on one connection - and not its asymptote at a working
//! set near the ceiling. A number here is a floor on a federated or wide answer's cost, not a
//! ceiling on it.
//!
//! Run with `just bench`. Prints the host's own load average first - a number taken under
//! contention is not comparable with one taken idle.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use sutura_app::{SpendLedger, Validated, Warehouses, verify_and_validate};
use sutura_catalog_local::LocalCatalog;
use sutura_config::{StaticCredentialBroker, WorkingSetCeiling, available_memory_bytes};
use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::plan::RowCeiling;
use sutura_domain::query::Query;
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_datafusion::{DataFusionWarehouse, WorkingSet};

fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

/// Everything one call to [`sutura_app::answer`] needs, built once - loading the catalog and
/// verifying every anchor is real work this harness does not want inside the timed section.
struct Fixture {
    validated: Validated<PinnedDefinitions>,
    warehouses: Warehouses<DataFusionWarehouse>,
    broker: StaticCredentialBroker,
    budget: Budget,
}

/// Over `clippy::type_complexity`'s threshold otherwise - this crate lowered it from the default
/// 250 to 100, so a `Box<dyn Error>` return type needs the same factoring the lint asks for.
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

fn source_name() -> Option<SourceName> {
    SourceName::parse("local").ok()
}

fn shared_declared() -> Option<SharedIdentityDeclared> {
    let reason =
        AcknowledgementReason::parse("the benchmark reads the example catalog as the process's own operating-system identity")
            .ok()?;
    Some(SharedIdentityDeclared::of(reason))
}

fn shared_posture() -> Option<SourcePosture> {
    Some(SourcePosture::SharedServiceUser {
        declared: shared_declared()?,
    })
}

fn build() -> Fallible<Fixture> {
    let source = source_name().ok_or("the fixture source name is not a source")?;
    let version = DefinitionVersion::parse("bench-single-player")?;
    let pinned = LocalCatalog::new(source.clone(), example_root().join("catalog"), version).load()?;

    let ceiling = WorkingSetCeiling::parse(WorkingSetCeiling::DEFAULT_BYTES, available_memory_bytes())?;
    let posture = shared_posture().ok_or("the fixture posture is not a posture")?;
    let warehouse = DataFusionWarehouse::new(source.clone(), posture, WorkingSet::of_bytes(ceiling.bytes()))?;
    let data = example_root().join("data");
    for model in pinned.definitions().models().values() {
        let csv = data.join(format!("{}.csv", model.table()));
        warehouse.attach_csv(model.table_name(), &csv)?;
    }

    let warehouses = Warehouses::of(warehouse);
    let validated =
        verify_and_validate(pinned, &warehouses).map_err(|cause| format!("the example's anchors do not hold: {cause}"))?;
    let broker =
        StaticCredentialBroker::for_one_shared_source(source, shared_declared().ok_or("the fixture posture is not a posture")?);
    let budget = Budget::parse(std::time::Duration::from_secs(30))?;
    Ok(Fixture {
        validated,
        warehouses,
        broker,
        budget,
    })
}

fn fixture() -> Option<&'static Fixture> {
    static FIXTURE: OnceLock<Option<Fixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            build()
                .map_err(|cause| eprintln!("answer_path: fixture setup failed: {cause}"))
                .ok()
        })
        .as_ref()
}

fn caller() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

fn load_question(name: &str) -> Option<Query> {
    let path = example_root().join("questions").join(name);
    let text = std::fs::read_to_string(&path)
        .map_err(|cause| eprintln!("answer_path: could not read {}: {cause}", path.display()))
        .ok()?;
    serde_norway::from_str(&text)
        .map_err(|cause| eprintln!("answer_path: {} is not a question: {cause}", path.display()))
        .ok()
}

fn run_question(bencher: divan::Bencher, name: &str) {
    let Some(fixture) = fixture() else { return };
    let Some(question) = load_question(name) else { return };
    bencher.bench_local(|| {
        sutura_app::answer(
            &fixture.validated,
            &question,
            &caller(),
            &fixture.broker,
            &fixture.warehouses,
            &sutura_domain::plan::RefusingCombiner,
            WorkingSetCeiling::DEFAULT_BYTES,
            Deadline::opened_at(Instant::now(), fixture.budget),
            &SpendLedger::no_budget(),
            RowCeiling::DEFAULT,
        )
    });
}

/// A single aggregate over one metric, one grain, no dimension - the cheapest shape a real
/// question takes.
#[divan::bench]
fn recurring_revenue_by_month(bencher: divan::Bencher) {
    run_question(bencher, "recurring-revenue-by-month.yaml");
}

/// Two dimensions over the same range - the widest cut the example corpus asks for.
#[divan::bench]
fn subscription_months_by_region_and_term(bencher: divan::Bencher) {
    run_question(bencher, "subscription-months-by-region-and-term.yaml");
}

fn main() {
    sutura_dev::bench_venue::print();
    divan::main();
}
