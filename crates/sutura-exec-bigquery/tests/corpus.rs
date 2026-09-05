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
//!   gives. **The CONTENT and the ORDER are both compared, cell type included, and the one
//!   approximation is named**: `RealTolerance::DIFFERENTIAL` cuts a `Value::Real` and reaches no
//!   other variant. This bullet read *with no tolerance for either* until the float cut inside the
//!   comparator was noticed, which is an overstated control and therefore its own defect. See *What
//!   its first real run FOUND* below for the one divergence this leg measured, and the generator
//!   change that closed it.
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
//! - **That a SHIPPED binary would do any of this.** The branch below this one gives the adapter a
//!   composition root, so *nothing links the crate* has stopped being true - but it is behind a
//!   default-off feature and no published artifact turns it on, so what this leg exercises is still
//!   the adapter and not a deployment. `.agents/skills/sutura/crate-map` is the authority on that distinction.
//!
//! # What its first real run FOUND, which is the point of having it - and what closed it
//!
//! **`ORDER BY x` did not say where a null goes, and the two sides disagreed.** `DataFusion` orders
//! nulls LAST and `GoogleSQL` orders them FIRST, so every corpus question grouping by a dimension
//! behind a `LEFT JOIN` - the example fact table holds a `customer_key` with no `dim_customer` row -
//! returned the same rows in a different order.
//!
//! **Five of the 31 questions, measured in CI on 2026-08-31**, one of them at 61 rows:
//!
//! ```text
//! bigquery-corpus: 16 answers agreed exactly, 5 agreed on content and differed on NULL
//!                  placement, 9 refusals agreed, 1 excluded, 31 in the corpus
//! ```
//!
//! It is worth reading as evidence about the instrument rather than about `BigQuery`: no golden could
//! see it, because a golden pins the statement TEXT and the text is the same on both sides. It is the
//! class `crates/sutura-app/tests/differential.rs` was written for, found the first time this leg ran.
//!
//! **It is FIXED, in the generator rather than here, and this leg no longer tolerates it.** The plan's
//! `ORDER BY` now states the placement - `sutura_sql::generate`'s `ordered_nulls_last`, emitted by
//! both `generate` and `generate_leg` - so all four dialects converge on the engine's own order and
//! `agreement_between` compares CONTENT and ORDER through the shared policy. The measurement that
//! used to be asserted here (*at least one question diverges on null placement*) is gone rather than
//! relaxed: a divergence in either now fails.
//!
//! **Measured in CI on 2026-08-31**, and the five questions above are the five that moved into the
//! first number:
//!
//! ```text
//! bigquery-corpus: 21 answers agreed exactly on content AND order, 9 refusals agreed, 1 excluded,
//!                  31 in the corpus
//! ```
//!
//! **What `exactly` meant on that run is NOT what it means above, and the difference is this
//! branch.** That tally was produced by the render-based comparison, which could not tell
//! `Value::Null` from `Value::Text("null")` or `Value::Integer(1)` from `Value::Text("1")` - so a
//! cell-type divergence on any of the 21 was counted as agreement. Read it as evidence about
//! acceptance, about rendered row content and about ORDER, and about nothing else. **No live number
//! is claimed for the typed policy**: nothing has run this leg against a real dataset since, so what
//! is measured for it is `sutura_domain::warehouse::agreement`'s own suite plus the two cells at the
//! bottom of this file, and the next live run is what would restate the tally.
//!
//! `docs/adr/0017`'s THIRD amendment records the finding and its FOURTH records this closure. The
//! numbering is worth getting right rather than approximating: the constant deleted from this file
//! cited *the second amendment*, which is the registration record and says nothing about a null.
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
//! # The one way this leg is known to be FLAKY, and what bounds it
//!
//! **A load can come back `NotComplete`.** `jobs.query` reports an unfinished job rather than waiting,
//! and a CI run on 2026-08-31 hit it on an eight-row `CREATE OR REPLACE TABLE` that had succeeded
//! twice before - so the variable is the endpoint's speed, not the fixture's size. `loading_bounds`
//! answers it by giving the LOADS a deadline of their own, two minutes, because a fixture load is
//! setup and answers no caller; the money ceiling is unchanged.
//!
//! **It is headroom rather than a guarantee, deliberately.** `transport::JobTransport::apply` requires
//! `jobComplete`, because answering `Ok` to an unfinished `CREATE` is a half-loaded fixture whose
//! corpus run then disagrees with the engine for a reason that looks like a dialect bug. So a slow
//! endpoint makes this leg go red on the LOAD, naming the table - which is what happened, and is the
//! right failure to have.
//!
//! **It WRITES to the dataset**, which the smoke leg does not, and the consequence is worth stating:
//! four tables named after the example models - `dim_customer`, `dim_product`,
//! `fct_subscription_monthly` and `fct_usage_daily` - are replaced on every run. **That has been
//! true, and it stopped being a race in the diff that closes #119:** every table is now named with
//! the run's own token - `dim_customer_<token>_<leg>` and friends - so two runs against one dataset
//! at the same time create, read and drop only their own tables and never touch each other's. The
//! committed names above are the stem the suffix is appended to; the generator still renders them
//! unqualified and the job's `defaultDataset` resolves them, but the run's suffixed bundle drives
//! the plan, so the tables a run reads are its own. Each `CREATE` also carries a 24-hour expiration,
//! so a run that is CANCELLED - which `panic = "abort"` makes the explicit DROP unable to reach -
//! still leaves nothing behind after a bounded interval.
//!
//! The per-run table names remain committed-fixture-names-plus-tokens rather than resources: only
//! the dataset and the project are resources, and only those need masking in a public log. The
//! dataset a developer points this at is still shared with CI by configuration, so the table names
//! a run prints include its token - a log says WHICH run wrote them.

// The corpus reaches OUTSIDE this crate, into `examples/single-player`. The source filter in
// `flake.nix` names `crates/*/tests` and `examples/` separately, so this leg depends on BOTH clauses -
// the same dependency `crates/sutura-app/tests/adapters/mod.rs` documents. It does not matter for a
// nix CHECK, since acceptance is a `nix run` app reading the real tree, but it would the day somebody
// tried to make it one.

#[cfg(test)]
mod support;

// The per-run table naming harness, which is the mechanism this leg's re-entrancy rests on. Its own
// module because it is a pure function of a bundle and a token - no endpoint - and its own file
// rather than more of this one because this file is at the 1000-line cap. Every `#[test]` over it
// stays here: `.agents/skills/sutura/gates` records why moving assertions instead orphans them.
#[cfg(test)]
mod naming;

// `cfg(test)` around the whole file, which is the house pattern rather than a preference: clippy
// honours `allow-expect-in-tests` only for code inside a `#[cfg(test)]` item, and
// `tests_outside_test_module` wants the `#[test]` functions there too.
#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_domain::model::{InvalidIdentifier, SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
    use sutura_domain::plan::Executable;
    use sutura_domain::query::{Query, ToolOutcome};
    use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
    use sutura_domain::warehouse::{PreFlight, RowSet, Value, Warehouse as _};

    use sutura_exec_bigquery::wire::{BytesBilledCeiling, JobBounds, QueryDeadline};

    use crate::naming::{build_token, ci_run_id, run_token, suffixed_bundle, suffixed_table};
    use crate::support::{Connection, Wired, bounds, opened, presented};

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
    fn loading_bounds() -> JobBounds {
        JobBounds::of(
            QueryDeadline::parse(120).expect("two minutes is a deadline"),
            BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
        )
    }

    /// The adapter, opened to LOAD - the long deadline, and nothing else different.
    fn loader() -> Wired {
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
    fn engine(committed: &PinnedDefinitions, suffixed: &PinnedDefinitions) -> Engine {
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

    /// The corpus, in the dataset - this run's suffixed tables replaced from the committed CSVs.
    ///
    /// Asserts every model's fixture moved at least one row, because an empty table would make every
    /// comparison below agree about nothing. The count comes from the CSV rather than from the
    /// endpoint; what proves the endpoint STORED them is the row comparison itself.
    ///
    /// The tables are named *with* this run's token (see [`crate::naming::suffixed_table`]), so two concurrent
    /// runs - or this leg's own three tests under nextest's default parallelism - replace only their
    /// own tables. Each `CREATE` also carries a 24-hour expiration, so a cancelled run's tables
    /// self-delete even though `panic = "abort"` skips the explicit DROP.
    fn load_the_corpus(committed: &PinnedDefinitions, suffixed: &PinnedDefinitions, warehouse: &Wired) -> usize {
        let mut loaded = 0_usize;
        for (table, csv) in run_fixtures(committed, suffixed) {
            let rows = warehouse
                .load_fixture(&table, &csv)
                .unwrap_or_else(|e| panic!("the fixture {table} did not load: {e:?}"));
            assert!(rows > 0, "the fixture for {table} carried no rows");
            // The TABLE name, which is a committed fixture name plus this run's token, and never the
            // dataset or the project - which is what makes it safe to print in a public log.
            println!("bigquery-corpus: loaded {rows} rows into {table}");
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
    fn drop_the_corpus(suffixed: &PinnedDefinitions, warehouse: &Wired) {
        for table in suffixed.definitions().models().values().map(|m| m.table_name().clone()) {
            warehouse
                .drop_table(&table)
                .unwrap_or_else(|e| panic!("the fixture table {table} did not drop: {e:?}"));
        }
    }

    /// The two sides of one question, compared - and it panics rather than reporting a disagreement,
    /// because a disagreement here is what this leg exists to fail on.
    ///
    /// **What makes two answers the same answer is `sutura_domain::warehouse::agreement`, and this
    /// leg no longer has an opinion of its own.** It used to: a `rendered()` copied from
    /// `crates/sutura-app/tests/differential.rs`, comparing every cell through `Value::render`. That
    /// is a DISPLAY form, so the variant was erased on both sides of the copy - a `Null` and the text
    /// `"null"` compared equal, and so did `Integer(1)` and the text `"1"`. **The comparisons this
    /// file makes against a live dataset inherited that**, which is why the policy now lives in one
    /// module with its own tests and both legs call it.
    ///
    /// **The CONTENT and the ORDER are both compared, and the ONE approximation is
    /// `RealTolerance::DIFFERENTIAL`** - twelve digits after the point on a `Value::Real` and nothing
    /// else, because two sides summing the same rows in different orders differ in the last place of
    /// an `f64`. The module header used to say *no tolerance for either*, which was an overstatement
    /// of the control: the float cut was already there, inside `rendered`.
    ///
    /// Content first, so a wrong number is never reported as a sort order - *different rows* is a
    /// wrong number and *same rows, different order* is a generator that stopped saying how to sort
    /// them. It returns nothing, because there is no longer a second degree of agreement for a caller
    /// to count - see *What its first real run FOUND* in the module header for the one that used to be
    /// here and what closed it.
    fn agreement_between(name: &str, from_engine: &RowSet, from_bigquery: &RowSet) {
        if let Err(disagreement) = agree_on_content(from_engine, from_bigquery, RealTolerance::DIFFERENTIAL) {
            panic!("{name}: the engine and BigQuery returned different rows - {disagreement}");
        }
        if let Err(disagreement) = agree_on_order(from_engine, from_bigquery, RealTolerance::DIFFERENTIAL) {
            panic!(
                "{name}: the engine and BigQuery returned the same rows in different orders, and the \
                 plan's ORDER BY claims one order - {disagreement}"
            );
        }
        println!("bigquery-corpus: {name} agrees, {} row(s)", from_engine.rows().len());
    }

    /// One cell, as a whole result, for the two comparisons below.
    fn one_cell(label: &str, cell: Value) -> RowSet {
        RowSet::new(vec![String::from(label)], vec![vec![cell]]).expect("a one-cell result is rectangular")
    }

    /// **The comparison this leg makes against a live dataset is type-aware, and this is where that
    /// stops being a claim.**
    ///
    /// NOT `#[ignore]`d, unlike the three legs below, and that is the point: [`agreement_between`] is
    /// a pure function of two results, so the property is checked on every `just test` with no
    /// credential, no dataset and no network. Against the comparator this replaced both of these
    /// PASSED - which is what made the finding worth a fix rather than a note.
    ///
    /// **The `expected` string names the CONTENT diagnosis, and it has to.** It stopped at *the
    /// engine and `BigQuery`* first, which is a prefix of both panics [`agreement_between`] can
    /// raise - so with `agree_on_content` made vacuous these two cells stayed green on the order
    /// panic while five of `sutura_domain::warehouse::agreement`'s own tests reddened. That is the
    /// same shape one file over: a cell passing for the wrong reason reads as coverage.
    #[test]
    #[should_panic(
        expected = "a-null-is-not-the-word-null: the engine and BigQuery returned different rows - one \
                    side answered a row 1 time(s) and the other 0 time(s)"
    )]
    fn a_null_and_the_word_null_do_not_agree_in_this_leg_s_comparison() {
        agreement_between(
            "a-null-is-not-the-word-null",
            &one_cell("region", Value::Null),
            &one_cell("region", Value::Text(String::from("null"))),
        );
    }

    /// The other half of the same hole: a count and the text of that count.
    #[test]
    #[should_panic(
        expected = "an-integer-is-not-its-text: the engine and BigQuery returned different rows - one \
                    side answered a row 1 time(s) and the other 0 time(s)"
    )]
    fn an_integer_and_its_own_text_do_not_agree_in_this_leg_s_comparison() {
        agreement_between(
            "an-integer-is-not-its-text",
            &one_cell("subscriptions", Value::Integer(1)),
            &one_cell("subscriptions", Value::Text(String::from("1"))),
        );
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

    /// The re-entrancy fix, tested locally without ever touching a dataset.
    ///
    /// **This is the deterministic half of what #119 makes provable.** `just bigquery-acceptance`
    /// twice concurrently is the measurement, but the mechanism that makes the two runs not see each
    /// other's tables is derived from the committed bundle, which needs no network. Two runs - or
    /// this leg's two shells - differ in their token; two tests within one run differ in their leg.
    /// Both together have to produce DISTINCT legal table names every model resolves to, and the
    /// suffixed bundle they compile against has to still assemble (relationships and metrics are
    /// cloned, the digest is recomputed), or the acceptance legs below would be exercising tables the
    /// plan never names.
    #[test]
    fn two_runs_of_the_acceptance_leg_at_once_do_not_see_each_other_s_tables() {
        let committed = bundle();
        // **Two runs and two tests, all four distinct.** Different tokens are two CI runs or a CI
        // run and a local one; different legs are this file's own three tests under nextest's
        // parallelism. If any pair collides, two processes could replace the table the other reads.
        let pairs = [
            ("runonetoken", "rows"),
            ("runonetoken", "accept"),
            ("runtwotoken", "rows"),
            ("runtwotoken", "anchors"),
        ];
        let mut names: Vec<TableName> = Vec::new();
        for &(token, leg) in &pairs {
            let suffixed = suffixed_bundle(&committed, token, leg);
            for model in suffixed.definitions().models().values() {
                names.push(model.table_name().clone());
            }
        }
        assert!(
            names.len() >= 8,
            "two runs of four models should produce at least eight table names, got {}",
            names.len()
        );
        // **Distinct in the whole space at once.** A collision anywhere - same table name for two
        // different (token, leg) pairs - is exactly the race. Turned into a set, NO name may be lost:
        // the set's size has to equal the list's length, so any duplicate would shrink it.
        let set: std::collections::BTreeSet<TableName> = names.iter().cloned().collect();
        assert_eq!(
            set.len(),
            names.len(),
            "two runs shared a table name: {} distinct among {}",
            set.len(),
            names.len()
        );
    }

    /// The token source itself is pinned, because distinctness is what the whole fix rests on.
    ///
    /// The suffixing logic is tested above with fixed strings; this one pins the value that makes
    /// "two runs have distinct table names" true in production - the generated token. Two runs at
    /// different instants (differing clocks) must differ, and a fabricated CI run id must differ
    /// from the local fallback, or the leg would race for a reason no fixed-string test can see.
    #[test]
    fn the_run_token_is_distinct_across_runs_and_sources() {
        // Two local runs: same process, different instants. The clock differs, so the tokens differ
        // - unless the separator-less join below collided, which `_` prevents by construction.
        let first = build_token(None, 1_700_000_000_000_000_000, 42);
        let second = build_token(None, 1_700_000_000_000_000_001, 42);
        assert_ne!(first, second, "two local runs at different instants shared a token: {first}");

        // A fabrication of distinctness: CI and local produce different tokens for the same clock
        // and pid, so a machine that believes it is CI cannot collide with a local run.
        let ci = build_token(Some("1234567890"), 1_700_000_000_000_000_000, 42);
        assert_ne!(ci, first, "a CI run and a local run at the same instant shared a token: {ci}");

        // And what the CI branch advertises: the run id is the token, so a log says which run.
        assert_eq!(ci, "1234567890");
    }

    /// The CI branch is taken only when GitHub says it is CI, and that is a test, not a paragraph.
    ///
    /// **A run id alone is not proof of CI.** A developer with a stale `GITHUB_RUN_ID` exported would
    /// otherwise have both of their runs collapse onto one token and race for one set of tables -
    /// the defect the per-run suffix exists to remove. `GITHUB_ACTIONS` is `"true"` in every GitHub
    /// Actions job and set by nothing else, so requiring both means the run id is used exactly when
    /// GitHub supplies it. Asserted on the pure decision rather than through `std::env`, because a
    /// test that mutates the process environment makes its own result depend on the other tests'.
    #[test]
    fn a_run_id_without_the_ci_flag_is_a_stale_export_rather_than_a_run() {
        assert_eq!(
            ci_run_id(Some("true"), Some("1234567890")).as_deref(),
            Some("1234567890"),
            "a real CI pair did not yield the run id"
        );
        assert_eq!(
            ci_run_id(None, Some("1234567890")),
            None,
            "a run id with no CI flag was trusted - two local runs would collapse onto one token"
        );
        assert_eq!(
            ci_run_id(Some("false"), Some("1234567890")),
            None,
            "GITHUB_ACTIONS=false was read as CI"
        );
        assert_eq!(
            ci_run_id(Some("true"), None),
            None,
            "CI with no run id has to fall back to the clock, not to an empty suffix"
        );
        assert_eq!(
            ci_run_id(Some("true"), Some("   ")),
            None,
            "a blank run id became a token, and a blank suffix is the collision"
        );
        // Trimmed, because the token becomes part of a table name and whitespace is not legal there.
        assert_eq!(ci_run_id(Some("true"), Some(" 42 ")).as_deref(), Some("42"));
    }

    /// The ceiling on a per-run table name is owned by [`TableName::parse`], and this pins that.
    ///
    /// **It asserts nothing about a length itself, deliberately.** A re-check here would be dead
    /// code: a name over the ceiling never gets past [`crate::naming::suffixed_table`] to be
    /// measured, so the boundary belongs to the parse and the honest thing to pin is that BOTH
    /// token shapes this leg can generate get through it, and that an over-long one comes back
    /// as `TooLong` **carrying the limit** - named, so a panic from elsewhere in the assembly
    /// cannot pass for it. A data system silently truncates a longer name, and a plan naming a
    /// table the load did not write is a wrong number rather than a failure.
    #[test]
    fn a_per_run_table_name_that_would_exceed_the_ceiling_is_refused_not_truncated() {
        // The widest LOCAL token that can exist: a 2026 clock is 16 hex digits and `u32::MAX` is
        // the widest pid any platform can hand us. The local branch is the longer of the two - and
        // the one nothing else here puts through the parse, so without this the local leg would
        // panic on its first table while every deterministic test in this file stayed green. That
        // is the same defect review found one level up, in the token's own source.
        let widest = build_token(None, 1_777_000_000_000_000_000, u32::MAX);
        // Every committed fixture name rather than one, since the ceiling is reached by the LONGEST.
        let committed = bundle();
        for model in committed.definitions().models().values() {
            let name = model.table_name();
            // A realistic CI run id, which is the shape `just bigquery-acceptance` ships under CI.
            assert!(
                suffixed_table(name, "1234567890", "rows").is_ok(),
                "a realistic CI token was refused for {name}"
            );
            assert!(
                suffixed_table(name, &widest, "anchors").is_ok(),
                "the local token at its widest was refused for {name}: {widest}"
            );
        }

        // The other direction, and the refusal is NAMED rather than merely counted: an absurd token
        // pushes the name past the ceiling, and what comes back has to be the parse's own `TooLong`
        // with the limit on it. `is_err()` alone would pass on any refusal at all.
        let refused = suffixed_table(
            &TableName::parse("fct_usage_daily").expect("a fixture name parses"),
            &"x".repeat(50),
            "rows",
        )
        .err();
        assert!(
            matches!(refused, Some(InvalidIdentifier::TooLong { limit: 63, .. })),
            "an over-long per-run name was not refused as TooLong at 63: {refused:?}"
        );
    }

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
        //
        // Every table is named with THIS run's token (so two concurrent runs never collide) and the
        // whole leg compiles against a bundle rebuilt with those suffixed names, so the plan and the
        // dataset agree. The tables are dropped when the test finishes.
        let token = run_token();
        let committed = bundle();
        let pinned = suffixed_bundle(&committed, &token, "accept");
        let warehouse = opened(source(), Connection::required(), bounds());
        let loaded = load_the_corpus(&committed, &pinned, &loader());
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
                sutura_semantic::Compiled::Federated { .. } => {
                    // This acceptance leg proves the single-statement BigQuery renderer against the
                    // endpoint. Federated questions render as two legs and are covered elsewhere.
                    println!("bigquery-corpus: {name} skipped because it compiled to a federated plan");
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
        drop_the_corpus(&pinned, &loader());
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
        // Every table is named with THIS run's token and a per-test leg suffix, so this test and
        // its two siblings run under nextest's default parallelism without touching each other's
        // tables. The plan is compiled against the suffixed bundle, the engine reads the same
        // suffixed names, and the tables are dropped when the test finishes.
        let token = run_token();
        let committed = bundle();
        let pinned = suffixed_bundle(&committed, &token, "rows");
        let warehouse = opened(source(), Connection::required(), bounds());
        let loaded = load_the_corpus(&committed, &pinned, &loader());
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let engine = sutura_app::Warehouses::of(engine(&committed, &pinned));
        let there = sutura_app::Warehouses::of(warehouse);
        let validated = sutura_app::verify_and_validate(pinned.clone(), &engine).expect("the anchors hold against the engine");
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
            let from_engine = sutura_app::answer(&validated, &question, &a_caller(), &locally, &engine, 1 << 30);
            let from_bigquery = sutura_app::answer(&validated, &question, &a_caller(), &remotely, &there, 1 << 30);

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
                    agreement_between(&name, a, b);
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
        // **The counts have to add up to the corpus, and that identity is what stops this test
        // silently shrinking.** A floor alone ("more than eight agreed") would pass a run that
        // `continue`d past half the corpus; a fixed expected total would be a test edit every time a
        // question is added. The sum is neither: it is a function of the directory.
        let total = questions().len();
        assert_eq!(
            compared.saturating_add(refused).saturating_add(excluded),
            total,
            "{compared} agreed + {refused} refused + {excluded} excluded is not the {total} questions \
             in the corpus"
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
        drop_the_corpus(&pinned, &loader());
        println!(
            "bigquery-corpus: {compared} answers agreed exactly on content AND order, {refused} refusals \
             agreed, {excluded} excluded, {total} in the corpus"
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
        // Every table is named with THIS run's token and this test's own leg suffix, so the three
        // corpus tests run concurrently without colliding. The tables are dropped when the test
        // finishes.
        let token = run_token();
        let committed = bundle();
        let pinned = suffixed_bundle(&committed, &token, "anchors");
        let warehouse = opened(source(), Connection::required(), bounds());
        let loaded = load_the_corpus(&committed, &pinned, &loader());
        assert!(
            loaded > 1000,
            "the example corpus is over a thousand rows and {loaded} loaded"
        );

        let engine = sutura_app::Warehouses::of(engine(&committed, &pinned));
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
            sutura_app::verify_and_validate(pinned.clone(), &there)
                .expect("a data system that reproduced every anchor is fit to serve"),
        );
        drop_the_corpus(&pinned, &loader());
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
