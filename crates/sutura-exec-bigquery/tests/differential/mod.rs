//! The corpus differential harness: the example corpus, the engine beside it, and the one
//! comparison both live legs are judged by.
//!
//! **Its own module because there are now two legs that make the same comparison over different
//! dataset layouts**, and the comparison is the thing that must not be written twice. `corpus.rs`
//! runs the corpus with every fixture in one dataset; `two_datasets.rs` runs it with the join
//! targets in a second one. What either leg claims rests on [`agreement_between`] deciding
//! agreement the same way - so a second copy of it would be a second notion of *correct*, and the
//! leg that got the looser one would look like coverage.
//!
//! In `tests/differential/mod.rs` rather than `tests/differential.rs` so cargo does not build it as
//! a test target of its own - the reason `tests/support/mod.rs` gives. Unlike `support`, only the
//! two corpus legs declare it, because `dead_code` is `deny` in the workspace lint table and
//! nothing here would be live in the smoke leg or in the two-principal cell.
//!
//! **Every `#[test]` over this module stays in the file that declares it.**
//! `.agents/skills/sutura/gates` states the reason as a rule: `just causality` reverts a file that
//! added no test and keeps one that did, so moving assertions out of a file turns it revertible and
//! orphans the module they moved into. What moved here is the harness.
//!
//! What is NOT here is anything about where a table lives. The bundle this reads is the committed
//! one - every model unqualified, which is what `docs/adr/0019` says the shipped corpus is
//! deliberately left as - and each leg decides for itself what it qualifies before it compiles a
//! question. That is what keeps the engine side reachable: `sutura_exec_datafusion` registers one
//! file per model with nothing above it, so it can only ever be handed unqualified names.

use std::path::{Path, PathBuf};

use sutura_domain::catalog::Model;
use sutura_domain::model::{SourceName, TableName};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::query::Query;
use sutura_domain::warehouse::{RowSet, Value};
use sutura_exec_bigquery::wire::{BytesBilledCeiling, JobBounds, QueryDeadline};

use crate::support::{Connection, Wired, opened};

/// What the fixture LOADS are bounded by, which is not what the questions are bounded by.
///
/// **Measured, not guessed.** `support::bounds` derives its deadline from
/// `server.request_timeout_seconds` - the budget for answering a caller - and a fixture load
/// answers nobody. A CI run on 2026-08-31 failed with `NotComplete` on an EIGHT-ROW
/// `CREATE OR REPLACE TABLE`: the endpoint had not finished the job inside the request-derived
/// share, which is around twelve seconds, and `jobs.query` reports an unfinished job rather than
/// waiting. Size is not the variable - the same load had succeeded twice in the two runs before -
/// so what this buys is headroom against a slow DDL rather than room for a big one.
///
/// **The MONEY ceiling is deliberately identical**, because that is the bound that protects an
/// invoice and a load has no reason to be allowed to scan more than a question. Only the clock
/// moves.
///
/// **The limit, and it is why the header names this as a known flake:** a longer deadline makes an
/// incomplete load less likely and cannot make it impossible. `apply` requires `jobComplete` on
/// purpose - answering `Ok` to an unfinished `CREATE` is a half-loaded fixture whose corpus run
/// then disagrees with the engine for a reason that looks like a dialect bug - so the honest
/// outcome of a slow endpoint is this leg going red on the load, naming the table, which is what
/// happened.
pub(crate) fn loading_bounds() -> JobBounds {
    JobBounds::of(
        QueryDeadline::parse(120).expect("two minutes is a deadline"),
        BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
    )
}

/// The adapter, opened to LOAD - the long deadline, and nothing else different.
pub(crate) fn loader() -> Wired {
    opened(source(), Connection::required(), loading_bounds())
}

/// The source name every model in the example catalog declares.
///
/// **Not a choice.** `sutura_app::answer` selects a warehouse by the name the PLAN carries, and the
/// plan takes it from the model - so the `BigQuery` adapter has to be opened under this name or no
/// question reaches it. It is the one place this leg's shape is decided by the corpus rather than
/// by the endpoint.
const SOURCE: &str = "local";

/// The version this leg pins the bundle under.
///
/// Its own string rather than the golden suite's `golden-fixture-1`, for that suite's own stated
/// reason: two suites reading the same bytes and pinning different things should not share a
/// constant. Nothing here is snapshotted, and the digest is taken over the parsed definitions
/// rather than the version, so this value reaches no assertion.
const VERSION: &str = "bigquery-acceptance-1";

/// The engine, and the side the comparison is made against.
///
/// **THE ENGINE and not a peer**, which is `crates/sutura-app/tests/differential.rs`'s reasoning
/// and the reason `docs/adr/0017` names it: a data source exists to run a subplan of what the
/// engine could otherwise compute itself, so the engine's answer is the one a pushdown has to
/// reproduce.
pub(crate) type Engine = sutura_exec_datafusion::DataFusionWarehouse;

fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

/// Every question in the corpus, in sorted order.
///
/// Sorted so the corpus is a function of the directory rather than of the filesystem.
pub(crate) fn questions() -> Vec<PathBuf> {
    let dir = example_root().join("questions");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("the questions directory is there")
        .map(|entry| entry.expect("a directory entry is readable").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    found.sort();
    assert!(!found.is_empty(), "no questions under {}", dir.display());
    found
}

pub(crate) fn read_question(path: &Path) -> Query {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    serde_norway::from_str(&text).unwrap_or_else(|e| panic!("{} is not a question: {e}", path.display()))
}

pub(crate) fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| String::from("unnamed"), |s| s.to_string_lossy().into_owned())
}

/// The bundle, read through the same catalog adapter every other suite here reads it through.
pub(crate) fn bundle() -> PinnedDefinitions {
    let version = DefinitionVersion::parse(VERSION).expect("the pinned version is a version");
    let name = SourceName::parse("local").expect("the example catalog name is a name");
    sutura_catalog_local::LocalCatalog::new(name, example_root().join("catalog"), version)
        .load()
        .expect("the example catalog loads")
}

/// Each suffixed fixture: the table this run wrote, and the committed CSV behind it.
///
/// The two bundles iterate their models in the same order - both are `BTreeMap`s keyed by the
/// unchanged `ModelName`, one for the committed bundle and one for the suffixed - so zipping them
/// pairs every suffixed table with the CSV file of the same model. That is how the loader knows
/// which committed bytes to move into a per-run table.
fn run_fixtures(committed: &PinnedDefinitions, suffixed: &PinnedDefinitions) -> Vec<(TableName, PathBuf)> {
    let csvs: Vec<PathBuf> = committed
        .definitions()
        .models()
        .values()
        .map(|model| {
            let committed = model.table_name();
            example_root().join("data").join(format!("{committed}.csv"))
        })
        .collect();
    suffixed
        .definitions()
        .models()
        .values()
        .map(|model| model.table_name().clone())
        .zip(csvs)
        .collect()
}

/// The engine, opened over the committed CSVs under this run's suffixed table names.
///
/// The plan (from the suffixed bundle) reads tables named `dim_customer_<token>_<leg>`, so the
/// engine has to register files under those SAME names - attaching under the committed names
/// would make the engine read tables the plan never asks for and answer everything `Empty`.
/// `run_fixtures` supplies that pairing: suffixed table to committed CSV.
///
/// A gibibyte for the working set, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` -
/// written as a literal rather than read from that crate, for the reason the golden suite gives:
/// this leg must not acquire a dependency on the settings tree to obtain one number. The corpus is
/// a few hundred rows, so no question in it comes near the bound.
pub(crate) fn engine(committed: &PinnedDefinitions, suffixed: &PinnedDefinitions) -> Engine {
    let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
    let engine = Engine::new(
        source(),
        posture_of_the_engine(),
        sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
    )
    .expect("an in-process engine starts");
    for (table, csv) in run_fixtures(committed, suffixed) {
        engine
            .attach_csv(&table, &csv)
            .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
    }
    engine
}

pub(crate) fn source() -> SourceName {
    SourceName::parse(SOURCE).expect("the example source name is a name")
}

/// The posture the ENGINE is opened with.
///
/// The same shape the `BigQuery` side declares and a DIFFERENT witness, deliberately: a
/// process reading CSVs and a service-account key reaching a project are two different
/// acknowledgements, and `Presented::agrees_with` compares them per adapter. Each side is opened
/// with, and presented, its own.
fn posture_of_the_engine() -> sutura_domain::source::SourcePosture {
    sutura_domain::source::SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse(
                "the example corpus is a directory of CSVs read in this process under one identity",
            )
            .expect("the fixture reason is a reason"),
        ),
    }
}

/// Never returned: this broker mints from a constant.
#[derive(Debug, thiserror::Error)]
#[error("this leg's credential broker cannot fail")]
pub(crate) struct BrokerCannotFail;

/// A broker that grants what the adapter it is asked about was opened with.
///
/// **Two postures, one per side, and that is what makes this leg's comparison honest rather than
/// convenient.** `Presented::agrees_with` compares the acknowledgement witness on the leg against
/// the one the adapter was opened with, so a single shared witness would have made the two sides
/// agree by construction about a thing they are not supposed to share.
pub(crate) struct GrantsWhatEachSideDeclares {
    /// A function rather than a value, because `Presented` is deliberately not `Clone` - it
    /// carries an operator's acknowledgement, and a type that hands out copies of one invites a
    /// call site to present a witness it was not given.
    pub(crate) presented: fn() -> sutura_domain::identity::Presented,
}

impl sutura_domain::identity::CredentialBroker for GrantsWhatEachSideDeclares {
    type Error = BrokerCannotFail;

    #[expect(
        clippy::unwrap_in_result,
        reason = "the map is built from the same source set it is checked against, so a failure \
                  there is a broken fixture rather than an input to handle"
    )]
    fn mint(
        &self,
        context: &sutura_domain::identity::RequestContext,
        sources: &sutura_domain::identity::SourceSet,
    ) -> Result<sutura_domain::identity::Minted, Self::Error> {
        let mut by_source = std::collections::BTreeMap::new();
        for name in sources.iter() {
            drop(by_source.insert(name.clone(), (self.presented)()));
        }
        let credentials = sutura_domain::identity::LegCredentials::minted(
            context.chain().subject().clone(),
            sutura_domain::identity::Expiry::NothingExpires,
            sources,
            by_source,
        )
        .expect("the map is built from the same source set, so it covers it");
        Ok(sutura_domain::identity::Minted::Granted { credentials })
    }
}

/// `Subject::TheDeploymentItself`, which is the honest value: this leg has no transport, so
/// nothing established a caller.
pub(crate) fn a_caller() -> sutura_domain::identity::RequestContext {
    sutura_domain::identity::RequestContext::of(sutura_domain::identity::PrincipalChain::of(
        sutura_domain::identity::Subject::TheDeploymentItself,
    ))
}

/// The corpus, in the dataset each model's fixture belongs to - this run's suffixed tables replaced
/// from the committed CSVs.
///
/// Asserts every model's fixture moved at least one row, because an empty table would make every
/// comparison below agree about nothing. The count comes from the CSV rather than from the
/// endpoint; what proves the endpoint STORED them is the row comparison itself.
///
/// **`into` is a function of the MODEL rather than one warehouse, and that is what makes this one
/// loader instead of two.** `tests/corpus.rs` hands over a closure ignoring its argument - every
/// fixture into one dataset - and `tests/two_datasets.rs` hands over one that picks by whether the
/// model is a join target. A second copy of this loop would be a second place the printed table
/// name, the per-row assertion and the row count could drift, and the printed line is the evidence
/// a public log carries.
///
/// The tables are named *with* this run's token (see [`crate::naming::suffixed_table`]), so two concurrent
/// runs - or a leg's own tests under nextest's default parallelism - replace only their
/// own tables. Each `CREATE` also carries a 24-hour expiration, so a cancelled run's tables
/// self-delete even though `panic = "abort"` skips the explicit DROP.
pub(crate) fn load_the_corpus<'warehouse>(
    committed: &PinnedDefinitions,
    suffixed: &PinnedDefinitions,
    into: &dyn Fn(&Model) -> &'warehouse Wired,
) -> usize {
    let mut loaded = 0_usize;
    // The two bundles iterate their models in the same order - both are `BTreeMap`s keyed by the
    // unchanged `ModelName` - which is the pairing `run_fixtures` documents, extended by one step:
    // the MODEL is what `into` reads to pick a warehouse.
    let paired = suffixed.definitions().models().values().zip(run_fixtures(committed, suffixed));
    for (model, (table, csv)) in paired {
        let rows = into(model)
            .load_fixture(&table, &csv)
            .unwrap_or_else(|e| panic!("the fixture {table} did not load: {e:?}"));
        assert!(rows > 0, "the fixture for {table} carried no rows");
        // The TABLE name, which is a committed fixture name plus this run's token, and never the
        // dataset or the project - which is what makes it safe to print in a public log.
        println!("bigquery-fixtures: loaded {rows} rows into {table}");
        loaded = loaded.saturating_add(rows);
    }
    assert!(loaded > 0, "no fixture loaded, so nothing below compares anything");
    loaded
}

/// Drops this run's suffixed tables - the tidy half of per-run cleanup.
///
/// **Why it exists beside the expiration:** the expiration guarantees a cancelled run leaves
/// nothing after a bounded interval, but a COMPLETE run should not leave its own tables behind
/// even for that interval. So a run drops them when it finishes. A drop failure is reported
/// rather than silently swallowed - the grant that created the table is the one that drops it.
///
/// It is never reached on a cancelled run - `panic = "abort"` skips it - which is exactly why
/// the expiration, not this method, is the guarantee.
pub(crate) fn drop_the_corpus(suffixed: &PinnedDefinitions, warehouse: &Wired) {
    for table in suffixed.definitions().models().values().map(|m| m.table_name().clone()) {
        warehouse
            .drop_table(&table)
            .unwrap_or_else(|e| panic!("the fixture table {table} did not drop: {e:?}"));
    }
}

/// A result as comparable text.
///
/// Lifted from `crates/sutura-app/tests/differential.rs`, whose reasoning applies unchanged and is
/// worth restating because it is what makes this a ROW comparison: rendered rather than compared as
/// `Value`, because two sides legitimately return different Rust types for the same number, and
/// `Value::render` is the one canonical form both are already required to agree on.
///
/// **Floats are cut to twelve significant digits, and that is not a loosening.** Summing the same
/// rows in a different order changes the last place of an `f64`, and neither side promises an
/// order. Twelve digits is far beyond any figure a metric reports and far short of the noise;
/// integers, dates and text are untouched, so an exact count stays exactly compared.
fn rendered(rows: &RowSet) -> Vec<Vec<String>> {
    rows.rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| match *value {
                    Value::Real(v) => format!("{v:.12e}"),
                    ref other => other.render(),
                })
                .collect()
        })
        .collect()
}

/// The rows as a set, for comparing WHAT was answered rather than in what order.
///
/// Kept even though the ORDER is now compared exactly, because it is what separates the two
/// diagnoses: *different rows* is a wrong number and *same rows, different order* is a generator
/// that stopped saying how to sort them. One assertion for both would report the first as the
/// second.
fn as_a_set(rows: &[Vec<String>]) -> Vec<&Vec<String>> {
    let mut out: Vec<&Vec<String>> = rows.iter().collect();
    out.sort();
    out
}

/// The two sides of one question, compared - and it panics rather than reporting a disagreement,
/// because a disagreement here is what this leg exists to fail on.
///
/// A function rather than an arm inside the loop, for `clippy::too_many_lines`' reason and because
/// what it decides is worth reading in one place: **the CONTENT and the ORDER are both compared
/// exactly, and there is no tolerance for either.** It returns nothing, because there is no longer
/// a second degree of agreement for a caller to count - see *What its first real run FOUND* in the
/// module header for the one that used to be here and what closed it.
pub(crate) fn agreement_between(name: &str, from_engine: &RowSet, from_bigquery: &RowSet) {
    assert_eq!(
        from_engine.columns(),
        from_bigquery.columns(),
        "{name}: the engine and BigQuery labelled the result differently"
    );
    let (here, over_there) = (rendered(from_engine), rendered(from_bigquery));

    // **The CONTENT, compared exactly.** A wrong number has to be produced twice, the same way, by
    // two things that share nothing below the plan. First, so that a wrong number is not reported
    // as a sort order.
    assert_eq!(
        as_a_set(&here),
        as_a_set(&over_there),
        "{name}: the engine and BigQuery returned different rows"
    );

    // **The ORDER, compared exactly - which this leg's first real run could not do.** A plan that
    // emits `ORDER BY` claims an order, so two data systems answering one plan in two orders is a
    // defect whatever the reason. The reason it used to have was null placement, and the generator
    // states it now.
    assert_eq!(
        here, over_there,
        "{name}: the engine and BigQuery returned the same rows in different orders, and the plan's \
         ORDER BY claims one order"
    );
    println!("bigquery-corpus: {name} agrees, {} row(s)", here.len());
}

/// An error and every cause beneath it, as one string.
///
/// `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost
/// message from the service is "the data system did not answer" - true of an outage, a rejected
/// statement and a cell that could not be carried alike.
pub(crate) fn chain(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// The one question whose two sides are required to fail for DIFFERENT reasons.
///
/// The module header carries the argument. Named by stem so the exclusion is one literal a reviewer
/// can grep for, rather than a condition spelled out at the assertion.
pub(crate) const DIVIDES_BY_ZERO: &str = "revenue-per-churned-subscription-january";

/// The engine's own presented credential, read off the posture it is opened with.
pub(crate) fn posture_of_the_engine_presented() -> sutura_domain::identity::Presented {
    match posture_of_the_engine() {
        sutura_domain::source::SourcePosture::SharedServiceUser { declared } => {
            sutura_domain::identity::Presented::SharedServiceUser { declared }
        }
        sutura_domain::source::SourcePosture::ImpersonationAtSource => {
            panic!("the engine in this leg is opened shared, one function above")
        }
    }
}
