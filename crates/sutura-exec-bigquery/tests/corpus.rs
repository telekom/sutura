//! **The acceptance leg `docs/adr/0017` specifies**: the example corpus loaded into a real dataset,
//! its statements accepted, its rows compared with the engine's for the same plan.
//!
//! # What this claims, and what `tests/acceptance.rs` claims instead
//!
//! Two legs, two different claims, and the difference is why they are two files. `acceptance.rs` is a
//! SMOKE leg: one hand-built `SUM` over a two-column table a developer supplies, which was the first
//! statement from this repository a real dataset ever accepted. This file is the leg the records ask
//! for, and it reaches the two bullets that one names as unreached:
//!
//! - **the corpus's statements are accepted and return rows** - every question in
//!   `examples/single-player/questions` that compiles to a plan, dry-run for acceptance and then
//!   really executed;
//! - **the rows agree with the engine's for the same plan** - `crates/sutura-app/tests/differential.rs`
//!   pointed at a second data source, comparing ROWS rather than batches for the reason that file
//!   gives.
//!
//! **So the constructs the smoke leg says nothing about are covered here, and they are the ones a live
//! run is worth most for.** The corpus renders, for `BigQuery`: 12 `LEFT JOIN`s, 6 `COUNT(DISTINCT`, 4
//! `CASE WHEN`, 4 `NULLIF` ratios, one `avg`, 90 `CAST`s and **3 `ISOWEEK` buckets** - and `ISOWEEK`
//! plus `DATE_TRUNC`'s argument order are precisely the two constructs `docs/adr/0017` MEASURED a
//! parse check to be blind about. A generator arm that got either wrong renders, parses, snapshots
//! green, and fails here.
//!
//! # What it still does not claim
//!
//! - **Identity.** The credential is a service-account key or an application-default login, both of
//!   which are one identity for everybody who asks. `BigQueryWarehouse::IMPERSONATION` reads
//!   `NoPlaceForASubject`, and a green here is *accepted, and correct for that identity*.
//! - **A registered data system.** The `data_systems:` axis of
//!   `crates/sutura-app/tests/adapters/mod.rs` still gains no entry, and that registry's own rule is
//!   why: a cell in it runs inside `just test`, and this one cannot - the nix sandbox has no network.
//! - **That a shipped binary would do any of this.** No composition root links this crate.
//!
//! # The one question that is EXCLUDED from the row comparison, and why
//!
//! `revenue-per-churned-subscription-january`, and it is excluded from the *comparison* rather than
//! from the run: it is executed, and both sides are required to FAIL. The metric declares
//! `zero_denominator: fails`, so the generator emits a bare `/` with no `NULLIF`, and January's
//! denominator is zero. **The two sides then fail for genuinely different reasons and that is a
//! dialect difference rather than a defect:** `DataFusion` and `DuckDB` return `inf`, which the port
//! refuses as a non-finite value, while `GoogleSQL` RAISES on a zero divisor and the endpoint answers
//! `400`. AGENTS.md already says the `zero_denominator: fails` credit belongs to `DuckDB` rather than
//! to this arm; [`the_corpus_rows_agree_with_the_engine`] is where that stops being a claim about
//! documentation and becomes a measurement. Nothing else in the corpus is excluded from anything.
//!
//! # What one run costs
//!
//! **Under a cent, and the number is set by the count of QUERIES rather than by the size of the
//! fixtures.** On-demand billing has a 10 MiB minimum per table referenced per query and the whole
//! fixture set is 40 KB, so the billed bytes are a function of how many jobs really read data:
//!
//! | Jobs a full run submits | Billed? |
//! | --- | --- |
//! | 12 `CREATE OR REPLACE TABLE` loads, 4 per test | No - a `CREATE TABLE AS SELECT` over a literal array scans nothing |
//! | 22 dry runs, in [`every_corpus_statement_the_compiler_produces_is_accepted_by_the_endpoint`] | No - the endpoint charges nothing for a dry run and uses no slots |
//! | ~22 executions, in [`the_corpus_rows_agree_with_the_engine`] | Yes, at the 10 MiB-per-table minimum |
//! | 12 anchor re-runs, in [`the_endpoint_reproduces_every_anchor_the_engine_does`] | Yes, same minimum |
//!
//! That is roughly 34 billed jobs referencing one to three tables each - on the order of half a
//! gibibyte of billed bytes, about a third of a cent at the current on-demand rate.
//! **The acceptance half being free is why it is a test of its own** rather than folded into the row
//! comparison: it doubles the coverage and adds nothing to the bill. `support::bounds` caps every job
//! at a gibibyte regardless, which is what protects a developer who points this at a dataset that
//! already holds something large under one of the four fixture names.
//!
//! # The grant this needs, which the smoke leg does not
//!
//! **It creates tables, so reading the dataset is no longer enough.** The credential needs
//! `bigquery.tables.create`, `bigquery.tables.updateData` and `bigquery.tables.delete` in the target
//! dataset - `roles/bigquery.dataEditor` on the dataset is the usual way to say that - on top of the
//! `roles/bigquery.jobUser` the smoke leg already needs to submit a job. `docs/adr/0019` records that
//! the acceptance credential's IAM refuses `datasets.create`; creating a TABLE in a dataset it already
//! has is a different grant, and a run whose credential lacks it fails at the first load with the
//! endpoint's own `accessDenied` on the chain rather than with anything mysterious.
//!
//! # How to run it
//!
//! ```text
//! just bigquery-acceptance
//! ```
//!
//! Which needs `just gcloud-login` once, `SUTURA_BQ_DATASET`, and - only where the credential names no
//! project of its own - `SUTURA_BQ_BILLING_PROJECT`. `tests/support/mod.rs` is where those are read
//! and where the fail-rather-than-skip argument lives. `SUTURA_BQ_TABLE` is NOT needed here: this leg
//! creates its own four tables.
//!
//! **It WRITES to the dataset**, which the smoke leg does not, and the consequence is worth stating:
//! four tables named after the example models - `dim_customer`, `dim_product`,
//! `fct_subscription_monthly` and `fct_usage_daily` - are replaced on every run. The names are fixed
//! because the generator renders them unqualified and the job's `defaultDataset` resolves them, so
//! **two runs against one dataset at the same time will race**, and the dataset this is pointed at
//! should hold nothing else under those names. The names are committed fixtures rather than resources,
//! so unlike the dataset and the project they need no masking in a public log.

// The corpus reaches OUTSIDE this crate, into `examples/single-player`. The source filter in
// `flake.nix` names `crates/*/tests` and `examples/` separately, so this leg depends on BOTH clauses -
// the same dependency `crates/sutura-app/tests/adapters/mod.rs` documents. It does not matter for a
// nix CHECK, since acceptance is a `nix run` app reading the real tree, but it would the day somebody
// tried to make it one.

#[cfg(test)]
mod support;

// `cfg(test)` around the whole file, which is the house pattern rather than a preference: clippy
// honours `allow-expect-in-tests` only for code inside a `#[cfg(test)]` item, and
// `tests_outside_test_module` wants the `#[test]` functions there too.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_domain::model::{SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
    use sutura_domain::plan::Executable;
    use sutura_domain::query::{Query, ToolOutcome};
    use sutura_domain::warehouse::{PreFlight, RowSet, Value, Warehouse as _};

    use crate::support::{Connection, Wired, opened, presented};

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
    type Engine = sutura_exec_datafusion::DataFusionWarehouse;

    fn example_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
    }

    /// Every question in the corpus, in sorted order.
    ///
    /// Sorted so the corpus is a function of the directory rather than of the filesystem.
    fn questions() -> Vec<PathBuf> {
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

    fn read_question(path: &Path) -> Query {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
        serde_norway::from_str(&text).unwrap_or_else(|e| panic!("{} is not a question: {e}", path.display()))
    }

    fn stem(path: &Path) -> String {
        path.file_stem()
            .map_or_else(|| String::from("unnamed"), |s| s.to_string_lossy().into_owned())
    }

    /// The bundle, read through the same catalog adapter every other suite here reads it through.
    fn bundle() -> PinnedDefinitions {
        let version = DefinitionVersion::parse(VERSION).expect("the pinned version is a version");
        sutura_catalog_local::LocalCatalog::new(example_root().join("catalog"), version)
            .load()
            .expect("the example catalog loads")
    }

    /// One table name and the committed CSV behind it, per model in the bundle.
    fn fixture_tables(pinned: &PinnedDefinitions) -> Vec<(TableName, PathBuf)> {
        pinned
            .definitions()
            .models()
            .values()
            .map(|model| {
                let table = model.table_name().clone();
                let csv = example_root().join("data").join(format!("{table}.csv"));
                (table, csv)
            })
            .collect()
    }

    /// The engine, opened over the committed CSVs.
    ///
    /// A gibibyte for the working set, which is `sutura_config::WorkingSetCeiling::DEFAULT_BYTES` -
    /// written as a literal rather than read from that crate, for the reason the golden suite gives:
    /// this leg must not acquire a dependency on the settings tree to obtain one number. The corpus is
    /// a few hundred rows, so no question in it comes near the bound.
    fn engine(pinned: &PinnedDefinitions) -> Engine {
        let ceiling = core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive");
        let engine = Engine::new(
            source(),
            posture_of_the_engine(),
            sutura_exec_datafusion::WorkingSet::of_bytes(ceiling),
        )
        .expect("an in-process engine starts");
        for (table, csv) in fixture_tables(pinned) {
            engine
                .attach_csv(&table, &csv)
                .unwrap_or_else(|e| panic!("the engine could not attach {}: {e}", csv.display()));
        }
        engine
    }

    fn source() -> SourceName {
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
    struct BrokerCannotFail;

    /// A broker that grants what the adapter it is asked about was opened with.
    ///
    /// **Two postures, one per side, and that is what makes this leg's comparison honest rather than
    /// convenient.** `Presented::agrees_with` compares the acknowledgement witness on the leg against
    /// the one the adapter was opened with, so a single shared witness would have made the two sides
    /// agree by construction about a thing they are not supposed to share.
    struct GrantsWhatEachSideDeclares {
        /// A function rather than a value, because `Presented` is deliberately not `Clone` - it
        /// carries an operator's acknowledgement, and a type that hands out copies of one invites a
        /// call site to present a witness it was not given.
        presented: fn() -> sutura_domain::identity::Presented,
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
    fn a_caller() -> sutura_domain::identity::RequestContext {
        sutura_domain::identity::RequestContext::of(sutura_domain::identity::PrincipalChain::of(
            sutura_domain::identity::Subject::TheDeploymentItself,
        ))
    }

    /// The corpus, in the dataset - four tables replaced from the committed CSVs.
    ///
    /// Asserts every model's fixture moved at least one row, because an empty table would make every
    /// comparison below agree about nothing. The count comes from the CSV rather than from the
    /// endpoint; what proves the endpoint STORED them is the row comparison itself.
    fn load_the_corpus(pinned: &PinnedDefinitions, warehouse: &Wired) -> usize {
        let mut loaded = 0_usize;
        for (table, csv) in fixture_tables(pinned) {
            let rows = warehouse
                .load_fixture(&table, &csv)
                .unwrap_or_else(|e| panic!("the fixture {table} did not load: {e:?}"));
            assert!(rows > 0, "the fixture for {table} carried no rows");
            // The TABLE name, which is a committed fixture name, and never the dataset or the project.
            println!("bigquery-corpus: loaded {rows} rows into {table}");
            loaded = loaded.saturating_add(rows);
        }
        assert!(loaded > 0, "no fixture loaded, so nothing below compares anything");
        loaded
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

    /// An error and every cause beneath it, as one string.
    ///
    /// `Display` on a `thiserror` enum prints the outermost message and stops, and the outermost
    /// message from the service is "the data system did not answer" - true of an outage, a rejected
    /// statement and a cell that could not be carried alike.
    fn chain(error: &dyn core::error::Error) -> String {
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
    const DIVIDES_BY_ZERO: &str = "revenue-per-churned-subscription-january";

    #[test]
    #[ignore = "needs a real BigQuery project and dataset, named in the developer's own environment"]
    fn every_corpus_statement_the_compiler_produces_is_accepted_by_the_endpoint() {
        // **`docs/adr/0017`'s first bullet, acceptance half - and it is FREE, which is why it is its
        // own test rather than folded into the row comparison.** A dry run uses no slots and is not
        // charged, so asking the endpoint about every corpus statement costs nothing; what it buys is
        // that a failure says ACCEPTED or NOT ACCEPTED without a row comparison in the way. The
        // statement is the COMPILER's, off the catalog, rather than one this file wrote.
        //
        // The tables have to exist for a dry run to accept a statement that reads them, so the load
        // comes first. That is not incidental: a dry run against a dataset with no fixture tables
        // fails with `notFound`, which is the shape a developer would otherwise read as a generator
        // defect.
        let pinned = bundle();
        let warehouse = opened(source(), Connection::required());
        let loaded = load_the_corpus(&pinned, &warehouse);
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let mut accepted = 0_usize;
        let mut refused_by_the_compiler = 0_usize;
        for path in questions() {
            let name = stem(&path);
            let question = read_question(&path);
            let compiled = sutura_semantic::compile(&question, &pinned).expect("the bundle is consistent");
            let plan = match compiled {
                sutura_semantic::Compiled::Refused { reason } => {
                    // A compile-side refusal, decided above every adapter. The corpus carries ten of
                    // them on purpose; they never reach the endpoint and are not this test's subject.
                    println!("bigquery-corpus: {name} refused before any statement existed: {reason:?}");
                    refused_by_the_compiler = refused_by_the_compiler.saturating_add(1);
                    continue;
                }
                sutura_semantic::Compiled::Planned { plan } => plan,
            };
            let verdict = warehouse
                .dry_run(Executable::Query(&plan), &presented())
                .unwrap_or_else(|e| panic!("{name}: the endpoint did not accept the corpus statement: {e:?}"));
            assert_eq!(
                verdict,
                PreFlight::Accepted,
                "{name}: the endpoint did not accept the statement"
            );
            accepted = accepted.saturating_add(1);
        }
        assert!(
            accepted > 8,
            "only {accepted} corpus statements were put to the endpoint, which is fewer than the \
             corpus holds - something excluded most of it"
        );
        assert!(
            refused_by_the_compiler > 0,
            "no question was refused by the compiler, so the corpus this ran is not the corpus"
        );
        println!("bigquery-corpus: {accepted} corpus statements accepted, {refused_by_the_compiler} refused before rendering");
    }

    #[test]
    #[ignore = "needs a real BigQuery project and dataset, named in the developer's own environment"]
    fn the_corpus_rows_agree_with_the_engine() {
        // **`docs/adr/0017`'s second bullet, which is the one the smoke leg cannot reach at all.** One
        // plan, computed by the engine over Arrow and pushed down to `BigQuery` as `GoogleSQL`, rows
        // compared. A wrong number has to be produced twice, the same way, by two things that share
        // nothing below the plan.
        //
        // This is the check that reaches `ISOWEEK` and `DATE_TRUNC`'s argument order. Both render and
        // parse cleanly when wrong - `docs/adr/0017` measured that - so a golden cannot see them and
        // this can: a Sunday bucketed into the wrong week is a different row here.
        let pinned = bundle();
        let warehouse = opened(source(), Connection::required());
        let loaded = load_the_corpus(&pinned, &warehouse);
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let engine = sutura_app::Warehouses::of(engine(&pinned));
        let there = sutura_app::Warehouses::of(warehouse);
        let validated = sutura_app::verify_and_validate(pinned, &engine).expect("the anchors hold against the engine");
        let locally = GrantsWhatEachSideDeclares {
            presented: posture_of_the_engine_presented,
        };
        let remotely = GrantsWhatEachSideDeclares { presented };

        let mut compared = 0_usize;
        let mut refused = 0_usize;
        let mut excluded = 0_usize;
        for path in questions() {
            let name = stem(&path);
            let question = read_question(&path);
            let from_engine = sutura_app::answer(&validated, &question, &a_caller(), &locally, &engine);
            let from_bigquery = sutura_app::answer(&validated, &question, &a_caller(), &remotely, &there);

            let (here, over_there) = match (from_engine, from_bigquery) {
                (Ok(one), Ok(other)) => (one.into_outcome(), other.into_outcome()),
                (Err(ref locally), Err(ref remotely)) => {
                    // **The one excluded question, and it is excluded from the COMPARISON and not from
                    // the run.** Both sides fail and the reasons legitimately differ: the engine
                    // returns `inf` and the port refuses a non-finite value, while `GoogleSQL` raises
                    // on a zero divisor and the endpoint answers `400`. Each side is checked against
                    // its own expected reason rather than against the other's, which is what makes
                    // this an assertion instead of a shrug.
                    assert_eq!(name, DIVIDES_BY_ZERO, "{name}: both sides failed and only one question may");
                    let here = chain(locally);
                    assert!(
                        here.contains("is not a finite number"),
                        "{name}: the engine failed otherwise:\n{here}"
                    );
                    let over_there = chain(remotely);
                    // The endpoint's own words are not asserted on - Hyrum's Law applies to a service's
                    // message as much as to ours, and this repository does not pin one. What is
                    // asserted is that the failure came from the DATA SYSTEM rather than from the
                    // compiler or the credential path.
                    assert!(
                        over_there.contains("the data system did not answer"),
                        "{name}: BigQuery failed somewhere other than at the data system:\n{over_there}"
                    );
                    println!("bigquery-corpus: {name} failed on both sides, for their own reasons");
                    excluded = excluded.saturating_add(1);
                    continue;
                }
                (one, other) => {
                    panic!("{name}: one side answered and the other did not\n  engine: {one:?}\n  bigquery: {other:?}");
                }
            };

            match (here, over_there) {
                (ToolOutcome::Answer { rows: ref a, .. }, ToolOutcome::Answer { rows: ref b, .. }) => {
                    assert_eq!(
                        a.columns(),
                        b.columns(),
                        "{name}: the engine and BigQuery labelled the result differently"
                    );
                    assert_eq!(
                        rendered(a),
                        rendered(b),
                        "{name}: the engine and BigQuery returned different rows"
                    );
                    println!("bigquery-corpus: {name} agrees, {} row(s)", a.rows().len());
                    compared = compared.saturating_add(1);
                }
                (ToolOutcome::Refusal { reason: ref a }, ToolOutcome::Refusal { reason: ref b }) => {
                    // A refusal is decided by the compiler, above both adapters, so the two must always
                    // agree. If they ever do not, something below the plan is deciding governance.
                    assert_eq!(
                        format!("{a:?}"),
                        format!("{b:?}"),
                        "{name}: the engine and BigQuery refused for different reasons"
                    );
                    refused = refused.saturating_add(1);
                }
                (one, other) => {
                    panic!("{name}: one side answered and the other refused\n  engine: {one:?}\n  bigquery: {other:?}");
                }
            }
        }
        // **The three counts have to add up to the corpus, and that identity is what stops this test
        // silently shrinking.** A floor alone ("more than eight agreed") would pass a run that
        // `continue`d past half the corpus; a fixed expected total would be a test edit every time a
        // question is added. The sum is neither: it is a function of the directory.
        let total = questions().len();
        assert_eq!(
            compared.saturating_add(refused).saturating_add(excluded),
            total,
            "{compared} agreed + {refused} refused + {excluded} excluded is not the {total} questions in the corpus"
        );
        assert!(compared > 8, "only {compared} questions produced rows from both sides");
        assert!(
            refused > 0,
            "no question was refused by both sides, so the compile-side corpus proved nothing here"
        );
        assert_eq!(
            excluded, 1,
            "the divide-by-zero question is the only exclusion and it has to be reached"
        );
        println!(
            "bigquery-corpus: {compared} answers agreed, {refused} refusals agreed, {excluded} excluded, {total} in the corpus"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project and dataset, named in the developer's own environment"]
    fn the_endpoint_reproduces_every_anchor_the_engine_does() {
        // **Not one of `docs/adr/0017`'s four bullets, and it is the strongest single claim available
        // here, which is why it is in.** An anchor is a number somebody CERTIFIED, written in the
        // catalog and compared as rendered text at the metric's coarsest grain. Checking it against
        // the service asks whether the definition still means what its author said when a different
        // data system computes it - and `verify_anchors` reaching the same verdict on both sides is
        // what would catch an arithmetic difference that a row comparison over this corpus happened
        // not to touch.
        let pinned = bundle();
        let warehouse = opened(source(), Connection::required());
        let loaded = load_the_corpus(&pinned, &warehouse);
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let engine = sutura_app::Warehouses::of(engine(&pinned));
        let there = sutura_app::Warehouses::of(warehouse);
        let locally = sutura_app::verify_anchors(&pinned, &engine);
        let remotely = sutura_app::verify_anchors(&pinned, &there);
        assert!(
            !locally.checks().is_empty(),
            "the example catalog declares no anchor, so this proved nothing"
        );
        assert_eq!(
            locally.checks(),
            remotely.checks(),
            "the engine and BigQuery disagree about whether the anchors hold"
        );
        // And once more through the operation that mints the proof, against the ENDPOINT. It re-runs
        // the anchors rather than being handed the report above, which is the point: a report is
        // evidence a caller could have written by hand, and a `Validated` bundle is not.
        drop(
            sutura_app::verify_and_validate(pinned, &there).expect("a data system that reproduced every anchor is fit to serve"),
        );
        println!(
            "bigquery-corpus: {} anchor(s) reproduced by the endpoint",
            locally.checks().len()
        );
    }

    /// The engine's own presented credential, read off the posture it is opened with.
    fn posture_of_the_engine_presented() -> sutura_domain::identity::Presented {
        match posture_of_the_engine() {
            sutura_domain::source::SourcePosture::SharedServiceUser { declared } => {
                sutura_domain::identity::Presented::SharedServiceUser { declared }
            }
            sutura_domain::source::SourcePosture::ImpersonationAtSource => {
                panic!("the engine in this leg is opened shared, one function above")
            }
        }
    }
}
