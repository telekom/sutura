#![forbid(unsafe_code)]
//! The federated answer path end to end: compile, mint, split, execute two `DataFusion` legs,
//! combine, materialise - over a derived two-source corpus, so #140's request deadline has a
//! measured number for a federated answer to be set against rather than "a provisional number
//! nobody has measured".
//!
//! `github.com/telekom/sutura#915`. Deterministic and local: two in-process `DataFusion` engines
//! over a derived copy of `examples/single-player`'s catalog and CSVs - no network, no server, no
//! second host, so this runs on a developer machine and inside the nix sandbox alike. The real
//! `DataFusionCombiner`, not `RefusingCombiner`: the bench measures the production federated path,
//! and a stub that never assembles would make the two-source topology describe a path no deployment
//! runs.
//!
//! **The limit, stated where the claim is.** In-process, single host, no network, a few hundred
//! rows, so this measures the shape of the federated path - compile, split, two legs, combine,
//! materialise - and not its asymptote at a working set near the ceiling or a network-dominated
//! deadline. A number here is a floor on a federated answer's cost, not a ceiling on it. Two sources
//! are not two identities: both legs run as `TheDeploymentItself`, so this measures the path, not
//! leg 2's impersonation. The two-fact ratio (three statements) is not covered. The orphan rows are
//! the corpus's July rows.
//!
//! Run with `just bench`. Prints the host's own load average first - a number taken under
//! contention is not comparable with one taken idle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Instant;

use sutura_app::{SpendLedger, Validated, Warehouses, answer, verify_and_validate};
use sutura_catalog_local::LocalCatalog;
use sutura_config::{WorkingSetCeiling, available_memory_bytes};
use sutura_domain::identity::{
    CredentialBroker, CredentialsDoNotCoverThePlan, Expiry, LegCredentials, Minted, Presented, PrincipalChain, RequestContext,
    SourceSet, Subject,
};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::plan::RowCeilings;
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_exec_datafusion::{DataFusionCombiner, DataFusionWarehouse, WorkingSet};

/// Everything one call to [`answer`] needs, built once - deriving the two-source corpus, opening
/// both engines and validating every anchor is real work this harness does not want inside the
/// timed section.
struct Fixture {
    validated: Validated<PinnedDefinitions>,
    warehouses: Warehouses<DataFusionWarehouse>,
    broker: SharedBroker,
    budget: Budget,
}

/// Over `clippy::type_complexity`'s threshold otherwise - this crate lowered it from the default
/// 250 to 100, so a `Box<dyn Error>` return type needs the same factoring the lint asks for.
type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

const FACT_SOURCE: &str = "local";
const LOOKUP_SOURCE: &str = "geo";

fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

fn shared_declared() -> Fallible<SharedIdentityDeclared> {
    let reason =
        AcknowledgementReason::parse("the benchmark reads the derived catalog as the process's own operating-system identity")?;
    Ok(SharedIdentityDeclared::of(reason))
}

fn shared_posture() -> Fallible<SourcePosture> {
    Ok(SourcePosture::SharedServiceUser {
        declared: shared_declared()?,
    })
}

/// A bench fixture that grants the declared shared identity to every source asked about; two
/// sources are not two identities.
struct SharedBroker {
    declared: SharedIdentityDeclared,
}

impl CredentialBroker for SharedBroker {
    type Error = CredentialsDoNotCoverThePlan;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let mut presented = BTreeMap::new();
        for source in sources.iter() {
            drop(presented.insert(
                source.clone(),
                Presented::SharedServiceUser {
                    declared: self.declared.clone(),
                },
            ));
        }
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
    }
}

/// Copies a directory of documents, recursively.
fn copy_tree(from: &Path, to: &Path) -> Fallible<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Rewrites one document: read it, require exactly one `find`, replace it, write it back - refusing
/// to derive nothing rather than matching zero or many.
fn rewrite(path: &Path, find: &str, with: &str) -> Fallible<()> {
    let text = std::fs::read_to_string(path)?;
    if text.matches(find).count() != 1 {
        return Err(format!(
            "{} no longer holds exactly one {find:?}, so this edit would derive nothing",
            path.display()
        )
        .into());
    }
    std::fs::write(path, text.replace(find, with))?;
    Ok(())
}

/// Removes one file.
fn remove(path: &Path) -> Fallible<()> {
    std::fs::remove_file(path)?;
    Ok(())
}

/// Appends rows to one CSV, refusing to join two.
fn append(path: &Path, rows: &str) -> Fallible<()> {
    let mut text = std::fs::read_to_string(path)?;
    if !text.ends_with('\n') {
        return Err(format!("{} does not end a row, so appending would join two", path.display()).into());
    }
    text.push_str(rows);
    std::fs::write(path, &text)?;
    Ok(())
}

/// The derived two-source corpus: one data directory, one catalog.
struct Derived {
    data: PathBuf,
    catalog: PathBuf,
}

/// Derive the two-source corpus from `examples/single-player` into cargo's target temp directory.
///
/// A minimal subset of `tests/differential/federated/corpus.rs`'s derivation: only the catalog edits
/// and data rows the three questions below exercise. The corpus's own copy carries eight catalog
/// cases and ten derived questions.
fn derive() -> Fallible<Derived> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("federated-bench-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    let derived = Derived {
        data: root.join("data"),
        catalog: root.join("catalog-two-source"),
    };
    copy_tree(&example_root().join("catalog"), &derived.catalog)?;
    copy_tree(&example_root().join("data"), &derived.data)?;

    // Which data system holds the dimension model - the one line the derivation varies.
    rewrite(&derived.catalog.join("models/customers.md"), "source: local", "source: geo")?;

    // A federated plan carries exactly one link, so the chained `sales_area` dimension is removed.
    rewrite(
        &derived.catalog.join("metrics/recurring_revenue.md"),
        "  - name: sales_area\n    column: sales_area\n    via: [subscription_customer, customer_region]\n    values: [central, north_east, south_west]\n    description: >\n      Which sales area the customer's region rolls up into. The one dimension here\n      reached through a chain of two relationships rather than one.\n",
        "",
    )?;
    // The caveat written about the removed dimension's chain: the load refuses it otherwise.
    remove(&derived.catalog.join("knowledge/caveats/sales-area-as-of-today.md"))?;

    // July 2026 fact rows: a null join key, a remote orphan, and a same-source orphan, outside every
    // anchor range and every shared question.
    append(
        &derived.data.join("fct_subscription_monthly.csv"),
        "2026-07-01,1901,,3,active,1000,false,monthly\n\
         2026-07-01,1902,41,3,active,2000,false,monthly\n\
         2026-07-01,1903,2,3,active,3000,true,monthly\n\
         2026-07-01,1904,1,4,active,4000,false,annual\n\
         2026-07-01,1905,1,99,active,5000,false,monthly\n",
    )?;

    Ok(derived)
}

/// One engine over its own source's tables only.
fn engine_on(
    data: &Path,
    name: &SourceName,
    pinned: &PinnedDefinitions,
    posture: &SourcePosture,
) -> Fallible<DataFusionWarehouse> {
    let ceiling = WorkingSetCeiling::parse(WorkingSetCeiling::DEFAULT_BYTES, available_memory_bytes())?;
    let engine = DataFusionWarehouse::new(name.clone(), posture.clone(), WorkingSet::of_bytes(ceiling.bytes()))?;
    for model in pinned.definitions().models().values() {
        if model.source() == name {
            let table = model.table_name().clone();
            let csv = data.join(format!("{table}.csv"));
            engine.attach_csv(&table, &csv)?;
        }
    }
    Ok(engine)
}

fn build() -> Fallible<Fixture> {
    let derived = derive()?;
    let version = DefinitionVersion::parse("bench-federated")?;
    let fact = SourceName::parse(FACT_SOURCE)?;
    let lookup = SourceName::parse(LOOKUP_SOURCE)?;
    let posture = shared_posture()?;

    let pinned = LocalCatalog::new(fact.clone(), derived.catalog.clone(), version).load()?;
    let fact_engine = engine_on(&derived.data, &fact, &pinned, &posture)?;
    let lookup_engine = engine_on(&derived.data, &lookup, &pinned, &posture)?;
    let warehouses = Warehouses::of(fact_engine)
        .and(lookup_engine)
        .map_err(|cause| format!("two sources have the same name: {cause}"))?;
    let validated = verify_and_validate(pinned, &warehouses)
        .map_err(|cause| format!("the derived two-source anchors do not hold: {cause}"))?;

    let broker = SharedBroker {
        declared: shared_declared()?,
    };
    let budget = Budget::parse(std::time::Duration::from_secs(30))?;
    Ok(Fixture {
        validated,
        warehouses,
        broker,
        budget,
    })
}

static FIXTURE: LazyLock<Option<Fixture>> = LazyLock::new(|| {
    build()
        .map_err(|cause| eprintln!("federated_answer: fixture setup failed: {cause}"))
        .ok()
});

fn caller() -> RequestContext {
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself))
}

fn parse_question(text: &str) -> Option<Query> {
    serde_norway::from_str(text)
        .map_err(|cause| eprintln!("federated_answer: a derived question is not a question: {cause}"))
        .ok()
}

/// A bench of a refusal or an answer with no rows measures nothing, so the question is answered
/// once before the timing and only a question that answered with rows is timed.
fn run_question(bencher: divan::Bencher, name: &str, question: &str) {
    let Some(fixture) = FIXTURE.as_ref() else { return };
    let Some(query) = parse_question(question) else { return };
    let Ok(combiner) = DataFusionCombiner::new() else {
        eprintln!("federated_answer: {name}: a combiner did not build");
        return;
    };
    let ask = || {
        answer(
            &fixture.validated,
            &query,
            &caller(),
            &fixture.broker,
            &fixture.warehouses,
            &combiner,
            WorkingSetCeiling::DEFAULT_BYTES,
            Deadline::opened_at(Instant::now(), fixture.budget),
            &SpendLedger::no_budget(),
            RowCeilings::DEFAULT,
        )
    };
    match ask() {
        Ok(answered) => match answered.outcome() {
            ToolOutcome::Answer { rows, .. } if !rows.rows().is_empty() => bencher.bench_local(ask),
            ToolOutcome::Answer { .. } => eprintln!("federated_answer: {name} answered no rows"),
            outcome @ ToolOutcome::Refusal { .. } => {
                eprintln!("federated_answer: {name} was refused: {outcome:?}");
            }
        },
        Err(cause) => eprintln!("federated_answer: {name} failed: {cause}"),
    }
}

/// The widest two-source cut: two dimensions across two sources (`region` on the lookup source,
/// `product_family` on the fact source), a same-source orphan (`product_key` 99), a null join key,
/// and a remote orphan. The fact leg carries a same-source hop and the lookup leg the remote one.
#[divan::bench]
fn widest_two_source_answer(bencher: divan::Bencher) {
    run_question(
        bencher,
        "widest_two_source_answer",
        "metrics: [recurring_revenue]\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region, product_family]\n",
    );
}

/// One remote dimension: a null join key and a remote orphan landing in one answer group, under LEFT.
#[divan::bench]
fn one_remote_dimension(bencher: divan::Bencher) {
    run_question(
        bencher,
        "one_remote_dimension",
        "metrics: [recurring_revenue]\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region]\n",
    );
}

/// `#777`'s case 2 - the one plan shape the splitter can produce: `region` is on the second data
/// system, so the rank waits for the combine rather than happening inside either leg.
#[divan::bench]
fn combine_then_rank_top(bencher: divan::Bencher) {
    run_question(
        bencher,
        "combine_then_rank_top",
        "metrics: [recurring_revenue]\ngrain: month\nrange:\n  start: 2026-07-01\n  end: 2026-08-01\ndimensions: [region]\ntop: { n: 3, by: metric, direction: desc }\n",
    );
}

fn main() {
    sutura_dev::bench_venue::print();
    divan::main();
}
