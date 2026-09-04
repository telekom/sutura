//! **A smoke leg, not the acceptance leg** - one real question against a real project, from a
//! developer's own machine.
//!
//! # Read this before believing anything a green run here would mean
//!
//! **This proves less than `docs/adr/0017` and issue #70 ask for, and the gap is named rather than
//! left to be discovered on the day somebody runs it.** Those records ask for *the corpus's
//! statements accepted and returning rows* and *the rows agreeing with the engine's for the same
//! plan*. What this file submits is **one hand-built `SUM` over a two-column table the developer
//! supplies**. It therefore says nothing about:
//!
//! - a join, `COUNT(DISTINCT`, `CASE WHEN`, a `NULLIF` ratio, or `CAST(... AS FLOAT64)`;
//! - `ISOWEEK`, which is the arm no golden covers and the one measured to be a wrong number under the
//!   obvious mapping;
//! - the argument order of `DATE_TRUNC`, which is the construct the parse check was **measured** to be
//!   blind about - so it is exactly what a live run is worth most for, and this leg's single bucket
//!   touches it only once.
//!
//! **What it does prove, on the day it runs:** the endpoint accepts a statement this repository
//! generated, answers it as one complete page, and the answer maps into domain values - plus the
//! composition (agent, credential source, transport, warehouse) actually fits together, which no
//! local test can show. That is worth having and it is a smaller claim.
//!
//! **And since #83, one thing more that nothing local can reach: a QUALIFIED table RESOLVES.** Every
//! golden in `crates/sutura-app/tests/golden/qualified.rs` can show that `project.dataset.table`
//! leaves the generator quoted per part and parses as `GoogleSQL`; none of them can show that the
//! service reads the table that path names.
//! [`the_same_table_read_by_its_fully_qualified_name_answers_the_same_numbers`] reads the same table
//! three ways and requires the same numbers from all three, and
//! [`a_qualified_name_naming_a_dataset_that_is_not_there_is_a_refusal_and_not_a_wrong_number`] is the
//! control without which those three greens would be satisfied by a service that ignored the
//! qualifier entirely. `docs/adr/0019` is the record, and it states what this leg still does not
//! reach: no second dataset and no second project, because the acceptance credential's IAM refuses
//! `datasets.create`.
//!
//! **The leg the records ask for is a different piece of work, and it is now BUILT - in
//! `tests/corpus.rs`, beside this one.** It is #78's importer shape pointed at a dataset: the example
//! fixtures loaded into four tables, the corpus questions run, the rows compared against the engine's.
//! So the three bullets above are covered there and not here, and the two files stay apart on purpose -
//! they prove different things over different fixtures, and a reader who runs one should not have to
//! read the other to know which claim they got.
//!
//! What this file keeps for itself is the smallest round trip there is, which is the right shape for a
//! first diagnostic: two columns, one `SUM`, and three qualification shapes with a control. When the
//! corpus leg fails, this one is what says whether the endpoint is reachable at all.
//!
//! Every sentence in this repository that promised *the corpus* over THIS file was narrowed to what it
//! does - `docs/adr/0017`'s amendment, `docs/adr/0018`, `AGENTS.md`, `docs/architecture.md` and both
//! plan pages - because a record that says "the corpus" over a test that submits one statement is the
//! overstated-claim defect this repository treats as a defect. `docs/adr/0017`'s second amendment is
//! where the corpus leg's own claim is stated, with its own limits.
//!
//! # Why it is not in any gate, and what a green `just validate` therefore does not mean
//!
//! `#[ignore]`d, so `just test` and `checks.nextest` skip it. Three reasons, and the first is the one
//! `docs/adr/0017` decided:
//!
//! 1. **This repository is public**, so a workflow secret is unavailable to a pull request from a
//!    fork - the check would be absent on exactly the contributions least likely to have been run
//!    locally, which is the shape of gate whose green means *nobody could run it*.
//! 2. A gate that needs a cloud project fails for an environment reason on somebody else's machine,
//!    and a gate that fails for a reason outside the diff gets disabled.
//! 3. **The nix sandbox has no network at all**, so this could not run there even with a credential.
//!    That is the same mechanism the compose tier's cells skip under - they log
//!    `SKIPPED - ... is not reachable`, keyed on `SUTURA_DEV_REQUIRE_DOCKER` rather than on `CI` -
//!    but this leg is not wired to that flag, because a container runtime is not what it needs and a
//!    flag that promised to make it required would be promising something no CI runner here can
//!    supply.
//!
//! **Where this leg deliberately DIFFERS from the compose tier: an unconfigured run FAILS here
//! rather than skipping.** `#[ignore]` already means nothing reaches these tests by accident, so the
//! only way in is to ask for them by name - and a developer who asked for acceptance and got green
//! ticks against no project has been told the opposite of the truth. `tests/support/mod.rs` carries
//! the full argument, including the fact that the skipping version was written first and did exactly
//! that.
//!
//! Since `docs/adr/0017`'s amendment, CI reaches this in its own `bigquery-acceptance` job, which
//! skips only on a fork's pull request - the runner there cannot see an environment's secrets, which
//! is *skip where the runner had no choice, fail where somebody typed the command*.
//!
//! **One test here is NOT `#[ignore]`d, and it is the exception the paragraph above needs stating
//! next to it.** `a_scratch_bundle_really_names_the_models_this_legs_own_source_is_asked_about` reads
//! no project and opens no socket: it is the control on the HARNESS the pre-flight seam leg is built
//! out of - the bundle written to a scratch directory and read back through the catalog adapter. So
//! it runs in `just test` and in `checks.nextest`, and `just bigquery-acceptance` skips it, because
//! `--run-ignored only` reaches the ignored set alone. That is the right way round: a harness defect
//! should fail in the gate every change runs, not in the one venue that costs a credential.
//!
//! # How to run it
//!
//! ```text
//! just bigquery-acceptance
//! ```
//!
//! Which runs BOTH legs, this one and the corpus. It needs, once: `just gcloud-login`, and up to
//! three values in the developer's own environment. **Their names are here and their values are not,
//! and will not be** - a project id is one of the things this repository does not write down, and
//! `.envrc` already sources a file under the user's own configuration directory for exactly this class
//! of value.
//!
//! | Variable | What it names | Which leg |
//! | --- | --- | --- |
//! | `SUTURA_BQ_BILLING_PROJECT` | the project the job is billed to; **only where the credential names none** | both |
//! | `SUTURA_BQ_DATASET` | the dataset an unqualified table resolves in, inside that project | both |
//! | `SUTURA_BQ_TABLE` | a table in it with a `DATE` column `day` and an `INT64` column `amount` | this one |
//! | `SUTURA_BQ_WIF_AUDIENCE` | the workload identity pool provider audience the asker's token is exchanged against | the two-subjects leg |
//! | `SUTURA_BQ_WIF_SCOPE` | the scope the exchanged credential is minted for | the two-subjects leg |
//! | `SUTURA_BQ_SUBJECT_A_TOKEN` | principal A's own OIDC `id_token`, whose grant sees one row | the two-subjects leg |
//! | `SUTURA_BQ_SUBJECT_B_TOKEN` | principal B's own OIDC `id_token`, whose grant sees the other row | the two-subjects leg |
//!
//! The table's shape is two columns because that is the smallest thing a real plan can be asked
//! about: a time bucket needs a `DATE`, and a measure needs something to sum. **A `TIMESTAMP` will
//! not do** - `transport::FieldType` maps `DATE` and refuses `TIMESTAMP`, which the crate
//! documentation states as the limit it is. The corpus leg needs no such table: it CREATES four, from
//! the committed example CSVs.
//!
//! **The pre-flight leg reads that same variable for a weaker property: that the table EXISTS.** It
//! submits no job at all - `tables.list` is a metadata read, billed for nothing - so it needs no
//! `DATE` column and no `INT64` one, and it is the one leg here that costs nothing but wall clock.
//!
//! **The job is bounded before it is sent**, which matters more here than anywhere because this is
//! the one path that spends real money: `support::bounds` sets a deadline and a `maximumBytesBilled`
//! ceiling, and a job that would scan past the ceiling fails at the service without being charged.
//! A developer pointing this at a large table gets a refusal rather than an invoice.

// **No `#![cfg(feature = "wire")]` here, and its absence is a decision.** The manifest declares this
// target with `required-features = ["wire"]` instead, so cargo skips it rather than compiling an empty
// binary. The manifest carries the argument, including a correction to a reason this comment used to
// give and should not have.

// `cfg(test)` around the whole file, which is the house pattern rather than a preference: clippy
// honours `allow-expect-in-tests` only for code inside a `#[cfg(test)]` item, and
// `tests_outside_test_module` wants the `#[test]` functions there too. An integration test target is
// compiled with `--test`, so the gate is true here and nothing below is conditional in practice -
// `crates/sutura-runtime/tests/blocking_span.rs` and `crates/sutura-sql/tests/adversarial_findings.rs`
// carry the same wrapper for the same reason.
// The environment, the bounds and the composition, shared with `tests/corpus.rs`. `bounds()` in
// particular is the one path in this repository that spends real money, and a second copy of it that
// drifted by a digit would be a leg that bills differently from the one a reviewer read.
#[cfg(test)]
mod support;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::model::{
        Aggregate, ColumnName, DatasetName, Grain, MetricName, ModelName, ProjectName, QualifiedTable, SourceName, TableName,
        TableQualifier,
    };
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
    use sutura_domain::plan::{
        Executable, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
        StatementTables,
    };
    use sutura_domain::warehouse::preflight::TablesPresent;
    use sutura_domain::warehouse::{ParamValue, PreFlight, Warehouse};

    use sutura_domain::identity::{
        Agreed, CredentialBroker as _, Presented, PrincipalChain, RequestContext, Secret, SourceSet, Subject, SubjectId,
    };
    use sutura_domain::source::SourcePosture;
    use sutura_exec_bigquery::wire::{BigQueryWire, StsOverHttp, WireAgent};
    use sutura_exec_bigquery::{BigQueryWarehouse, WorkloadIdentity, WorkloadIdentityBroker};

    use sutura_app::Warehouses;
    use sutura_app::preflight::{Verdict, ask};
    use sutura_catalog_local::LocalCatalog;
    use sutura_exec_bigquery::BigQueryError;
    use sutura_exec_bigquery::wire::WireError;

    use crate::support::{Connection, Wired, bounds, named, opened, presented};

    /// A table name no dataset holds, and the one name in this file that needs no masking.
    ///
    /// **A constant rather than four literals**, because four legs ask the same question - *what does
    /// this dataset do with a name it does not hold* - and a copy that drifted by a character would be
    /// a leg passing for the wrong reason: `preflight` reports an unknown name absent whatever it is,
    /// so nothing here would go red.
    const NO_SUCH_TABLE: &str = "sutura_acceptance_no_such_table";

    /// The model the seam leg's bundles declare over the table the dataset really holds.
    const MODEL_ON_A_HELD_TABLE: &str = "held_here";

    /// The model the seam leg's bundle declares over [`NO_SUCH_TABLE`] - the name a refusal has to
    /// print, because an operator fixes a `table:` by opening the model that wrote it.
    const MODEL_ON_AN_ABSENT_TABLE: &str = "not_here";

    /// What this leg needs from the environment: the shared [`Connection`], plus the one variable only
    /// this leg reads.
    ///
    /// **The environment reading itself moved to `tests/support/mod.rs` when the corpus leg arrived**,
    /// and the argument for FAILING rather than skipping moved with it - it applies to both legs
    /// identically and there must be one copy of it. `SUTURA_BQ_TABLE` stayed here, because the corpus
    /// leg creates its own tables and has no use for it: a shared module is compiled once per target,
    /// and `dead_code` is `deny`.
    struct Fixture {
        connection: Connection,
        table: TableName,
    }

    impl Fixture {
        /// The credential and the three names, or a panic saying exactly what is missing.
        fn required() -> Self {
            Self {
                connection: Connection::required(),
                table: TableName::parse(named(
                    "SUTURA_BQ_TABLE",
                    "a table with a DATE column `day` and an INT64 column `amount`",
                ))
                .expect("a table name parses"),
            }
        }

        /// The table as an unqualified name, resolved by the job's `defaultDataset`.
        ///
        /// The shape every leg in this file used before qualification existed, kept so the qualified
        /// legs have something to be COMPARED against: a qualified read that returns the right numbers
        /// is only evidence beside an unqualified read that returns the same ones.
        fn unqualified(&self) -> QualifiedTable {
            QualifiedTable::from(self.table.clone())
        }

        /// `dataset.table` - the same table, named without relying on the job's default.
        fn in_dataset(&self) -> QualifiedTable {
            QualifiedTable::new(
                Some(TableQualifier::in_dataset(
                    DatasetName::parse(self.connection.dataset.as_str()).expect("a dataset id is also a dataset name"),
                )),
                self.table.clone(),
            )
        }

        /// `project.dataset.table` - the same table, fully qualified.
        ///
        /// **The claim this file was extended for.** The path is built from the fixture's OWN project
        /// and dataset, so no value here is written into the repository - which is the same rule the
        /// two variables above are read under.
        fn in_project(&self) -> QualifiedTable {
            self.qualified_in_project(self.connection.dataset.as_str(), self.table.clone())
        }

        /// `project.dataset.table` in the fixture's OWN project, for any dataset and table.
        ///
        /// **One place parses the project id, which is why this is a method and not a second literal
        /// in a test body.** [`Self::in_project`] asks it about the real dataset; the soft-edge leg
        /// asks it about one the project does not hold. Two copies of the same
        /// `ProjectName::parse(billing_project)` would be two answers to *which project pays*, which
        /// is the live bug `x-goog-user-project` already cost this adapter once.
        fn qualified_in_project(&self, dataset: &str, table: TableName) -> QualifiedTable {
            QualifiedTable::new(
                Some(TableQualifier::in_project(
                    ProjectName::parse(self.connection.billing_project.as_str()).expect("a project id is also a project name"),
                    DatasetName::parse(dataset).expect("a dataset id is also a dataset name"),
                )),
                table,
            )
        }
    }

    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a source name is a source name")
    }

    /// The two-principal leg's own environment: the two subjects' tokens and the provider to exchange
    /// them against, or `None` when none of it is set.
    ///
    /// **`None` means SKIP, and that is a deliberate narrowing of these legs' fail-on-missing.** Those
    /// legs run in a job that always has their environment, so an absent value there is a
    /// misconfiguration; this leg's environment belongs to the workload-identity infra (`#106`/`#122`/
    /// `#123`) not landed with this change, so before it exists a hard failure would red a LIVE
    /// acceptance job. Hence: all four absent -> skip; all four present -> run; anything partial ->
    /// panic, because a half-configured exchange must not silently pass.
    fn subjects_env() -> Option<SubjectsEnvironment> {
        let read = |key: &str| std::env::var(key).ok().filter(|value| !value.trim().is_empty());
        let subject_a = read("SUTURA_BQ_SUBJECT_A_TOKEN");
        let subject_b = read("SUTURA_BQ_SUBJECT_B_TOKEN");
        let audience = read("SUTURA_BQ_WIF_AUDIENCE");
        let scope = read("SUTURA_BQ_WIF_SCOPE");
        match (&subject_a, &subject_b, &audience, &scope) {
            (None, None, None, None) => None,
            (Some(a), Some(b), Some(aud), Some(sc)) => Some(SubjectsEnvironment {
                subject_a: a.clone(),
                subject_b: b.clone(),
                audience: aud.clone(),
                scope: sc.clone(),
            }),
            _ => panic!(
                "the two-subjects leg is partially configured: set all four of SUTURA_BQ_SUBJECT_A_TOKEN, \
                 SUTURA_BQ_SUBJECT_B_TOKEN, SUTURA_BQ_WIF_AUDIENCE and SUTURA_BQ_WIF_SCOPE, or remove them \
                 all - a half-configured exchange must not pass"
            ),
        }
    }

    /// The two principals and the provider, as one value so the four-read guard and its users cannot
    /// disagree about which combination is complete.
    struct SubjectsEnvironment {
        subject_a: String,
        subject_b: String,
        audience: String,
        scope: String,
    }

    /// [`NO_SUCH_TABLE`], parsed - the bare name, for a caller that qualifies it itself.
    fn no_such_table() -> TableName {
        TableName::parse(NO_SUCH_TABLE).expect("a table name parses")
    }

    /// [`NO_SUCH_TABLE`] as an unqualified path, which is the shape four legs ask about.
    ///
    /// **Unqualified on purpose, in every one of them.** `preflight` partitions an unaddressable path
    /// into the absent set BEFORE anything is listed, so a name qualified into a dataset that is not
    /// there could answer *absent* without a call ever being made. Resolved by the source's own
    /// default dataset, the answer comes back from a real listing.
    fn absent_table() -> QualifiedTable {
        QualifiedTable::from(no_such_table())
    }

    /// A plan over the developer's table, built the way the compiler builds one.
    ///
    /// One bucket, one measure, and the two range bounds as predicates - because a `TimeRange` has no
    /// unbounded form, so every real plan carries them and the generator refuses one with no
    /// predicate.
    fn plan(table: &QualifiedTable) -> QueryPlan {
        // Every column is qualified by the table's BARE name, because `FROM a.b.c` gives the reference
        // an implicit alias of `c`. That is a claim about GoogleSQL that no local test can check, and
        // `the_same_table_read_by_its_fully_qualified_name_answers_the_same_numbers` is what checks it.
        let column = |name: &str| PlanColumn::new(table.name().clone(), ColumnName::parse(name).expect("a column name parses"));
        // **Under `MAX_RANGE_DAYS`, which the previous version was not.** A hundred-year span is a
        // question this surface REFUSES as `TimeRangeTooLong` before an adapter ever sees it, so
        // asking a real endpoint one was asking something no caller could ask - which made
        // "a statement this repository generated" generous. Ten years less a day is the widest a
        // question can legitimately be.
        let from = Date::parse("2016-09-01").expect("an ISO date parses");
        let until = Date::parse("2026-08-30").expect("an ISO date parses");
        QueryPlan::new(
            source(),
            MetricName::parse("total_amount").expect("a metric name parses"),
            StatementTables::only(table.clone()),
            PlanBucket::new(String::from("period"), Grain::Month, column("day")),
            Vec::new(),
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column("amount"),
                },
            },
            String::from("total_amount"),
            vec![
                PlanFilter::new(
                    PredicateOrigin::Definition,
                    PlanPredicate::AtOrAfter {
                        column: column("day"),
                        param: 0,
                    },
                ),
                PlanFilter::new(
                    PredicateOrigin::Definition,
                    PlanPredicate::Before {
                        column: column("day"),
                        param: 1,
                    },
                ),
            ],
            vec![ParamValue::Date(from), ParamValue::Date(until)],
            TimeRange::new(from, until).expect("a bounded range is a range"),
        )
    }

    /// The adapter, wired to the endpoint through the credential the environment supplied.
    ///
    /// **The composition it assembles - one [`crate::support::Wired`] out of an agent, a credential, a
    /// transport and a warehouse - moved to `tests/support/mod.rs` when the corpus leg arrived**, so
    /// both legs are evidence for the same composition rather than for two that resemble each other.
    /// What is left here is the source NAME, which is per leg.
    fn warehouse(fixture: Fixture) -> Wired {
        opened(source(), fixture.connection, bounds())
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn the_endpoint_accepts_one_statement_this_repository_generated() {
        // The claim CI cannot make: not that the statement PARSES as BigQuery - the goldens already
        // assert that - but that the service ACCEPTS it. A dry run is the cheapest way to ask: it uses
        // no slots and is not charged, which is also what makes `dry_run` answering
        // `PreFlight::Accepted` honest rather than a guess.
        //
        // **ONE statement, and the module header lists what that leaves untested.** A green here is
        // not "the corpus is accepted".
        let fixture = Fixture::required();
        let table = fixture.unqualified();
        let warehouse = warehouse(fixture);
        let plan = plan(&table);

        let accepted = warehouse
            .dry_run(Executable::Query(&plan), &presented())
            .expect("the endpoint accepted the generated statement");
        assert_eq!(accepted, PreFlight::Accepted);
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn the_endpoint_answers_with_a_complete_result_this_adapter_can_map() {
        // Three things at once, and each is a claim no local test reaches:
        //
        // 1. the statement runs and returns rows;
        // 2. the delivered count equals the endpoint's own `totalRows` - the seam's `Incomplete`
        //    refusal NOT firing is the evidence, and it is the one thing `jobs.query`'s single page
        //    makes easy to get wrong;
        // 3. the value mapping produces domain values rather than an `UnmappedType` - which is where
        //    a column typed `TIMESTAMP` rather than `DATE` will show up, loudly, as the crate
        //    documentation says it should.
        let fixture = Fixture::required();
        let table = fixture.unqualified();
        let warehouse = warehouse(fixture);
        let plan = plan(&table);

        let rows = warehouse
            .execute(Executable::Query(&plan), &presented())
            .expect("the endpoint answered with a complete result");
        assert_eq!(
            rows.columns(),
            ["period", "total_amount"],
            "the projected labels are the plan's"
        );

        assert_the_fixtures_numbers("the unqualified read", &rows);
    }

    /// The fixture's own numbers, whichever way its table was named.
    ///
    /// **The numbers, not merely the shape.** A fixture whose rows a wrong plan could also produce is
    /// not evidence, so the four rows behind this sum to 42 in June and 99 in July - two buckets that
    /// discriminate. Asserted as a set of pairs rather than by index, because the row order is the
    /// statement's `ORDER BY` and these tests are about the values.
    ///
    /// **A function rather than three copies, and that is what makes the qualified legs mean
    /// anything:** what they claim is that a fully qualified read returns *the same* numbers as the
    /// unqualified one, and three separately written expectations could drift into three different
    /// claims. `named` says which leg is speaking, because a failure has to name the path shape.
    fn assert_the_fixtures_numbers(named: &str, rows: &sutura_domain::warehouse::RowSet) {
        assert_eq!(
            rows.columns(),
            ["period", "total_amount"],
            "{named}: the projected labels are the plan's"
        );
        let mut answered: Vec<(String, String)> = Vec::new();
        for row in rows.rows() {
            assert_eq!(row.len(), 2, "{named}: a row is as wide as the schema");
            let (Some(period), Some(total)) = (row.first(), row.get(1)) else {
                panic!("{named}: a two-column row has two cells");
            };
            answered.push((period.render(), total.render()));
        }
        answered.sort();
        assert_eq!(
            answered,
            vec![
                (String::from("2026-06-01"), String::from("42")),
                (String::from("2026-07-01"), String::from("99")),
            ],
            "{named}: the endpoint's numbers are not the fixture's"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn the_same_table_read_by_its_fully_qualified_name_answers_the_same_numbers() {
        // **The claim that separates *qualification renders* from *qualification resolves*, and it is
        // the only place in this repository where the second half can be made.** Every local golden
        // can show that `project.dataset.table` comes out of the generator quoted per part and parses
        // as GoogleSQL; none of them can show that the service reads the table the path names.
        //
        // So: the SAME table, three ways - unqualified through the job's `defaultDataset`, then
        // `dataset.table`, then `project.dataset.table` - and all three have to answer 42 and 99. The
        // unqualified leg is inside this test rather than borrowed from the one above, because what is
        // being asserted is an EQUALITY between the three, and a comparison split across two test
        // functions is one a `--skip` can quietly turn into a single-sided claim.
        //
        // **It also measures the one thing `sutura_sql::generate::table_path` declares and cannot
        // prove locally:** that `FROM a.b.c` gives the reference an implicit alias of `c`, so
        // `c.column` binds to the table. If GoogleSQL bound it elsewhere this leg would fail with
        // `invalidQuery` naming the column, which is exactly how the label collision this branch also
        // fixes first showed up.
        let fixture = Fixture::required();
        let paths = [
            ("the unqualified read", fixture.unqualified()),
            ("dataset.table", fixture.in_dataset()),
            ("project.dataset.table", fixture.in_project()),
        ];
        let warehouse = warehouse(fixture);
        for (named, path) in &paths {
            // Printed so the terminal output of a run is the evidence rather than a claim about it.
            // The PATH SHAPE, never the path: a project id is not something this repository writes
            // down, and a test's own output is a place it would be written down.
            println!("bigquery-acceptance: reading the fixture table as {named}");
            let plan = plan(path);
            let rows = warehouse
                .execute(Executable::Query(&plan), &presented())
                .unwrap_or_else(|e| panic!("{named}: the endpoint did not answer: {e:?}"));
            assert_the_fixtures_numbers(named, &rows);
        }
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn a_qualified_name_naming_a_dataset_that_is_not_there_is_a_refusal_and_not_a_wrong_number() {
        // **The negative control the positive leg needs, and the one that rules out the reading that
        // would make the positive leg worthless.** If the service ignored the qualifier and resolved
        // the last part in the job's `defaultDataset`, the leg above would pass while proving nothing
        // - which is precisely the wrong-number failure this whole branch is about, one layer further
        // out.
        //
        // So the same table name is asked for in a dataset that does not exist. It has to be refused.
        // A green here plus a green above is what makes "the path is what resolved it" a measurement
        // rather than an inference.
        let fixture = Fixture::required();
        let absent = QualifiedTable::new(
            Some(TableQualifier::in_dataset(
                DatasetName::parse("sutura_no_such_dataset").expect("a dataset name parses"),
            )),
            fixture.table.clone(),
        );
        let warehouse = warehouse(fixture);
        let plan = plan(&absent);

        let refused = warehouse
            .dry_run(Executable::Query(&plan), &presented())
            .expect_err("a dataset that is not there is refused");
        // **Printing the error is safe HERE for a reason that is not local to this test**, and it is
        // worth naming because this leg provokes an endpoint refusal deliberately: the wire carries the
        // endpoint's own `message`, and that message quotes what it refused as `project:dataset.table`.
        // On a public repository a failing run's log is public, so what covers it is the
        // `::add-mask::` step in `ci.yml`'s `bigquery-acceptance` job, which redacts the project, the
        // dataset and the table from every later line in the job - a panicking test's message included.
        // `docs/adr/0017`'s amendment carries the argument and the cost. The dataset name in the path
        // above is a fictitious literal, so it is the one part of it nothing needs to mask.
        assert!(
            core::error::Error::source(&refused).is_some(),
            "the endpoint's own error did not survive #[source]: {refused:?}"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn a_table_the_dataset_does_not_hold_is_a_refusal_and_not_a_panic() {
        // The other half of *it really asked*: a `dry_run` that answers `Accepted` for everything is
        // not evidence. This is the negative control, and it is also the one live check that the
        // refusal path maps - a `404` from the endpoint has to arrive as `BigQueryError::Endpoint`
        // with the transport's own error on the chain, rather than as a panic under
        // `panic = "abort"`.
        let fixture = Fixture::required();
        let warehouse = warehouse(fixture);
        let absent = absent_table();
        let plan = plan(&absent);

        let refused = warehouse
            .dry_run(Executable::Query(&plan), &presented())
            .expect_err("a table that is not there is refused");
        assert!(
            core::error::Error::source(&refused).is_some(),
            "the endpoint's own error did not survive #[source]: {refused:?}"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn the_dataset_really_answers_a_listing_and_names_only_the_table_it_does_not_hold() {
        // **Issue #120's own *Verification* section asks for this run, and the change that built the
        // pre-flight delivered only the local half.** What is proved locally is everything this
        // repository can prove without a project: the adapter's difference against a fake transport,
        // and the composition root's refusal against a `Warehouse` fake. Neither is the claim *a real
        // dataset answered a real `tables.list` and the absent table was named*.
        //
        // Three things this leg is the only place to reach, and each is a hole a local test cannot
        // close:
        //
        // 1. `tables.list`'s real response document decodes the way `wire::tables::Listing` says it
        //    does. The documents that suite reads were written HERE, not by the service - which
        //    `docs/adr/0018` already carried as a limit for `jobs.query` and now carries for a second
        //    endpoint.
        // 2. `x-goog-user-project` carries a project the service accepts. That header was a live bug:
        //    the listing attributed quota to the DATASET's project rather than the source's billing
        //    project, and the fix is otherwise held by an assertion about a URL and a header rather
        //    than by anything having sent them.
        // 3. The paging loop runs against a real token and terminates, which no local document
        //    reaches. **What a green run does NOT say is how many pages it took** - nothing here
        //    reports a page count, so *a small dataset does not page* stays unmeasured, and the
        //    earlier version of this line claimed the run answered it. What it does establish is
        //    that the listing was COMPLETE enough to hold the table below, because a listing cut
        //    short would have reported that table absent and failed the control.
        //
        // **The control is inside this test rather than beside it, and that is the point of the
        // shape.** The pre-flight fails toward reporting a table absent - `Listing`'s fields are all
        // `#[serde(default)]`, so a document whose shape changed decodes as an empty listing and
        // therefore as *everything is missing*. A test that only asserted `AllBut` would pass exactly
        // as well in that world. So the clean set is asked FIRST, has to answer `All`, and a
        // single-sided claim is not something a `--skip` can leave behind.
        let fixture = Fixture::required();
        let present = fixture.unqualified();
        // A fictitious literal, so it is the one name in this leg nothing needs masking - and it is
        // the same spelling the `dry_run` control above uses, because both are asking *what does this
        // dataset do with a name it does not hold*.
        let absent = absent_table();
        let warehouse = warehouse(fixture);

        let clean = BTreeSet::from([present.clone()]);
        let answered = warehouse
            .preflight(&clean)
            .expect("the dataset answered the listing - a refusal here is a grant, not a missing table");
        assert_eq!(
            answered,
            TablesPresent::All,
            "the control: a set naming only a table the dataset holds has nothing absent in it"
        );
        // `All` and not `NotAsked`, which is the difference a composition root reads: this adapter
        // really listed, and a listing that decoded to nothing would have answered `AllBut` above.
        assert!(answered.was_asked(), "this adapter really looked: {answered:?}");

        let mixed = BTreeSet::from([present, absent.clone()]);
        let answered = warehouse
            .preflight(&mixed)
            .expect("the dataset answered the listing for the mixed set too");
        let named: Vec<String> = answered
            .absent()
            .map(|tables| tables.named().iter().map(ToString::to_string).collect())
            .unwrap_or_default();
        assert_eq!(
            named,
            vec![absent.to_string()],
            "the dataset's own listing names the table that is not there and nothing else"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn a_dataset_the_credential_cannot_list_is_unverified_and_never_every_table_absent() {
        // **What this leg is the only place to establish - and it is NARROWER than the version of this
        // comment review corrected.** That `404` warns rather than refuses is already pinned
        // hermetically, twice with controls: `wire::tables`'s `was_refused` suite asserts it beside
        // `401`, `403`, `500` and `503`, and `a_refused_listing_and_an_unreachable_one_are_not_the_same_outcome`
        // asserts the same one port up. A fake transport can hold a predicate over a status. What it
        // cannot hold is a claim about the SERVICE, and that is this:
        //
        // **a dataset that is not there is answered with a non-2xx, so the empty-decode path is not
        // what a missing dataset produces.** Every field of `wire::tables::Listing` is
        // `#[serde(default)]`, so a document that decodes to nothing reads as *every table is absent*
        // and would stop a boot. If the endpoint answered `200` with an empty body for a dataset that
        // does not exist, the pre-flight would refuse a deployment over a dataset name instead of
        // warning about it - and nothing in this repository could have known.
        //
        // **Both names in the path are fictitious literals, so this leg still writes no resource of
        // the developer's project into this repository.** The PROJECT is the fixture's own on
        // purpose: a dataset absent from a project the credential can see is the case being asked
        // about, and one in a project it cannot see is a different answer.
        //
        // **The oracle names the STATUS, which is review correcting an assertion that was green for
        // the wrong reasons.** `!preflight_was_refused` plus a surviving `#[source]` is satisfied by
        // `Unreachable`, by a `500` or `503`, by `DeadlineSpent`, and by the two `403`s this crate
        // deliberately puts in the warning half - `rateLimitExceeded` and `quotaExceeded`. In every
        // one of those the endpoint never answered about this dataset at all, and the leg still
        // reported *a dataset that is not there warns*. Matching `WireError::Refused { status: 404 }`
        // is what makes a different answer RED, which is the only shape in which a green run is the
        // measurement `docs/adr/0018` cites it as.
        //
        // The clean set is still asked FIRST, and it is a second control on a different axis: it says
        // this credential really can list this project, so the failure below is dataset-SPECIFIC
        // rather than an identity that reads nothing. It costs one more `tables.list`, billed for
        // nothing.
        let fixture = Fixture::required();
        let present = fixture.unqualified();
        let nowhere = fixture.qualified_in_project("sutura_acceptance_no_such_dataset", no_such_table());
        let warehouse = warehouse(fixture);

        assert_eq!(
            warehouse
                .preflight(&BTreeSet::from([present]))
                .expect("the control: this credential really can list this project's own dataset"),
            TablesPresent::All,
            "the control: a set naming only a table the dataset holds has nothing absent in it"
        );

        let unverified = warehouse
            .preflight(&BTreeSet::from([nowhere]))
            .expect_err("a dataset that is not there cannot answer a listing, and must not answer one emptily");
        assert!(
            matches!(
                unverified,
                BigQueryError::Endpoint {
                    cause: WireError::Refused { status: 404, .. }
                }
            ),
            "a dataset that is not there has to be a 404 from the service - anything else is a call that \
             never reached this dataset, and reading it as *could not verify* would be an accident: {unverified:?}"
        );
        // `preflight_was_refused` answers `true` for `401` and `403` only, so this is the half of the
        // split a live dataset can reach without a second identity. **The refusal half stays a
        // fake-transport claim**: it needs a credential holding no `bigquery.tables.list`, which is
        // not what either this leg or CI's `bq-test` environment is pointed at.
        assert!(
            !warehouse.preflight_was_refused(&unverified),
            "a dataset that is not there is a condition that passes, not a grant an operator adds: {unverified:?}"
        );
    }

    /// A bundle naming one model per `(model, table)` pair, on this leg's own source.
    ///
    /// **Through [`LocalCatalog`] rather than `PinnedDefinitions::pin`, because the pre-flight seam
    /// is about a bundle a deployment AUTHORED.** `pin` would let this file hand the decision a
    /// `Definitions` assembled in memory, which is a shape no operator can produce - and the mistake
    /// #120 exists for is a typed `table:` in a document. So the documents are written and read back
    /// through the adapter a composition root loads one through, and `columns:` is present because
    /// the format requires it rather than because the pre-flight reads it: `tables.list` reports
    /// existence, and this record's own limit is that it says nothing about the columns a model names.
    ///
    /// **`CARGO_TARGET_TMPDIR` and NOT `std::env::temp_dir()`, which review corrected and which
    /// matters more here than at the sibling call sites.** It is defined for an integration target
    /// and is inside `target/`, which is `crates/sutura-cli/tests/declared_source.rs`'s own reason -
    /// and these documents carry the fixture's real table name, deliberately left on disk after a
    /// failing run. A predictable path in a shared system temp directory is the wrong place for that.
    /// It also needs no dependency, which retires the `tempfile` argument this comment used to make:
    /// `tempfile 3.27.0` is already resolved in `Cargo.lock` transitively, so cost was never the
    /// reason.
    ///
    /// Cleared on the way IN rather than after, so a failing run leaves its documents to read.
    fn bundle_naming(what: &str, models: &[(&str, &str)]) -> PinnedDefinitions {
        let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("preflight-seam-{what}"));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).expect("a scratch directory is creatable");
        for (model, table) in models {
            let document = format!(
                "---\nkind: model\nname: {model}\nsource: {}\ntable: {table}\ncolumns: [day]\n---\n\
                 One model, so the pre-flight has a `table:` to ask this dataset about.\n",
                source()
            );
            std::fs::write(root.join(format!("{model}.md")), document).expect("a scratch document is writable");
        }
        LocalCatalog::new(
            SourceName::parse("scratch").expect("a catalog name is a name"),
            root,
            DefinitionVersion::parse("preflight-seam-1").expect("a definition version parses"),
        )
        .load()
        .expect("a bundle of model documents this file just wrote loads")
    }

    /// The control on the harness the seam leg below is built out of, and **the one test in this file
    /// that is not `#[ignore]`d** - it needs no project, so a gate can hold it.
    ///
    /// **Without it the seam leg's evidence rests on a bundle nobody checked.** `preflight::ask`
    /// SKIPS a source the bundle names no model in, so a `bundle_naming` that silently wrote nothing
    /// this source claims - a `source:` that stopped matching, a document the parse refused, a
    /// directory the walk missed - would hand the decision an empty question. The leg's
    /// `answers.len()` assertion catches that, but only in the one venue that costs a credential and
    /// a CI job; this catches it in `just test`.
    #[test]
    fn a_scratch_bundle_really_names_the_models_this_legs_own_source_is_asked_about() {
        let pinned = bundle_naming(
            "harness",
            &[
                (MODEL_ON_A_HELD_TABLE, "any_table_at_all"),
                (MODEL_ON_AN_ABSENT_TABLE, NO_SUCH_TABLE),
            ],
        );
        let declared: Vec<(String, String)> = pinned
            .definitions()
            .models()
            .values()
            .filter(|model| *model.source() == source())
            .map(|model| (model.name().to_string(), model.table().to_string()))
            .collect();
        assert_eq!(
            declared,
            vec![
                (String::from(MODEL_ON_A_HELD_TABLE), String::from("any_table_at_all")),
                (String::from(MODEL_ON_AN_ABSENT_TABLE), String::from(NO_SUCH_TABLE)),
            ],
            "the seam leg's bundle has to reach this leg's own source, or the decision it feeds is asked nothing"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn a_real_listing_reaches_the_boot_decision_and_names_the_model_behind_the_absent_table() {
        // **The last bullet of issue #216 that no run had reached: the two halves MEETING.** The
        // legs above stop at `BigQueryWarehouse::preflight`'s `TablesPresent`, and every test of the
        // decision above it - `sutura_app::preflight::ask` and each root's own sentence - runs against
        // a `Warehouse` fake. So a real listing had never produced a real verdict, and #216 named that
        // as the seam a live run covers.
        //
        // **What is reachable from here is the DECISION, and what is not is each root's WORDS.**
        // `sutura_app::preflight::ask` is the one decision sequence both composition roots call -
        // `sutura_serve::boot::refuse_absent_tables`' own documentation says so - and `sutura-app` is
        // already a dev-dependency of this crate. What stays out of reach is the rendering: those
        // functions are `pub(crate)` in crates that depend ON this one, and they are the sentence and
        // the sink rather than the decision. An earlier revision of `docs/adr/0018` called the whole
        // seam structurally unreachable from here, which was wrong by one dependency edge.
        //
        // **Two-sidedness is already in the mixed assertion, and the clean bundle is here for a
        // DIFFERENT reason - which is a correction to what this comment first said.** An empty
        // listing reports both tables absent, so comparing the absent set's keys EXACTLY against the
        // one fictitious name already fails in that world; the clean bundle is not what rescues it.
        // What the clean bundle is the only live exercise of is the `Present` arm - the decision
        // mapping a real `TablesPresent::All` to a verdict a root serves on - and #216's seam is both
        // verdicts, not just the refusing one. It costs one more `tables.list`, billed for nothing.
        //
        // **What the count assertion in `one_verdict` holds is the third case**, and it is the one a
        // reader misses: `ask` SKIPS a source the bundle names no model in, so a bundle that reached
        // this source with nothing produces no answers at all rather than a wrong verdict.
        let fixture = Fixture::required();
        let held = fixture.unqualified();
        let absent = absent_table();
        let engines = Warehouses::of(warehouse(fixture));

        let clean = bundle_naming("clean", &[(MODEL_ON_A_HELD_TABLE, held.name().as_str())]);
        match one_verdict(&clean, &engines) {
            Verdict::Present { asked } => assert_eq!(
                asked, 1,
                "the control: the decision asked this dataset about the one table the bundle names in it"
            ),
            other => panic!("the control: a bundle naming only {held} is present, and the decision said {other:?}"),
        }

        let mixed = bundle_naming(
            "mixed",
            &[
                (MODEL_ON_A_HELD_TABLE, held.name().as_str()),
                (MODEL_ON_AN_ABSENT_TABLE, NO_SUCH_TABLE),
            ],
        );
        match one_verdict(&mixed, &engines) {
            Verdict::Absent(behind) => {
                assert_eq!(
                    behind.named().keys().collect::<Vec<&QualifiedTable>>(),
                    vec![&absent],
                    "the real listing named the table that is not there, and nothing the dataset holds"
                );
                let models = behind.named().get(&absent).expect("the assertion above named this table");
                assert_eq!(
                    models.iter().map(ModelName::as_str).collect::<Vec<&str>>(),
                    vec![MODEL_ON_AN_ABSENT_TABLE],
                    "a refusal an operator can act on names the model whose `table:` is wrong: {behind}"
                );
            }
            other => panic!("a bundle naming {absent} has to be refused, and the decision said {other:?}"),
        }
    }

    /// The one verdict this leg's single-source registry can produce, or a panic saying what it got.
    ///
    /// **The count is asserted here rather than in each caller**, because it is the same guard both
    /// times and it is the one that catches a bundle that reached this source with no models: `ask`
    /// skips such a source, so the honest failure is *no answer* rather than a verdict that is wrong.
    fn one_verdict(pinned: &PinnedDefinitions, engines: &Warehouses<Wired>) -> Verdict<<Wired as Warehouse>::Error> {
        let answers = ask(pinned, engines);
        assert_eq!(
            answers.len(),
            1,
            "one source is open and the bundle names models in it, so the decision has exactly one answer"
        );
        answers
            .into_iter()
            .next()
            .expect("a vector of one has a first element")
            .into_verdict()
    }

    #[test]
    #[ignore = "needs a real BigQuery project with a Workload Identity Federation provider and two granted principals, named in the developer's own environment"]
    fn two_subjects_with_different_grants_read_two_different_row_sets() {
        // **The acceptance criterion issue 87 exists to make provable.** Two principals with
        // deliberately different row-level grants (one row visible to A and not to B) each asking the
        // same plan through the composition this repository SHIPS - the exchanging broker and the
        // adapter over the wire to a real STS and dataset. The dataset's row policy is what makes the
        // two answers differ; the exchange is what makes each answer run as its asker rather than as
        // the process. It needs the workload-identity infra not landed here (`#106`/`#122`/`#123`:
        // the pool, provider, the two principals and the row policy), so it is `#[ignore]`d and
        // guarded by [`subjects_env`]: SKIP while that infra is absent, FAIL on a partial
        // configuration. No identifier, token or provider name is written here.
        let Some(env_vars) = subjects_env() else {
            // No WIF provider configured yet - the state before the `#106`/`#122`/`#123` infra lands.
            // Skipping is honest for a job with no principal to ask with; the other legs fail on a
            // missing value only because their environment always exists.
            eprintln!(
                "SKIPPED - two-subjects leg: no SUTURA_BQ_* workload-identity variables set, so there is no provider to exchange against"
            );
            return;
        };
        // The two principals' tokens and the provider, out of the guarded value.
        let SubjectsEnvironment {
            subject_a,
            subject_b,
            audience,
            scope,
        } = env_vars;
        let fixture = Fixture::required();
        let table = fixture.unqualified();
        let plan = plan(&table);
        let connection = fixture.connection;
        let bounds = bounds();

        // The running composition, exactly as `sutura-serve`'s `broker::build_broker` and
        // `build_bigquery` compose it: one pinned agent and bounds behind both the exchange and the
        // wire, the broker exchanging each principal's own token into the leg, and the adapter opened
        // under the `impersonation-at-source` posture that accepts a subject token as the job's bearer.
        let agent = WireAgent::pinned(bounds);
        let broker = WorkloadIdentityBroker::empty(StsOverHttp::new(agent))
            .with_floor(30)
            .impersonating(source(), WorkloadIdentity::of(audience, scope));
        let warehouse = BigQueryWarehouse::new(
            source(),
            SourcePosture::ImpersonationAtSource,
            connection.billing_project,
            connection.dataset,
            BigQueryWire::new(WireAgent::pinned(bounds), connection.credentials),
        );

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock that reads the present")
            .as_secs();
        // One subject id per principal: the token is what Google's STS sees and exchanges, and the
        // subject id is what this answer is recorded as, so they have to be told apart here. The
        // subject is rebuilt for the agreement check rather than shared by reference - `request`
        // consumes the chain, exactly as the broker's own suite builds it twice.
        let rows_for = |token: &str, id: &str| -> Vec<(String, String)> {
            let chain = |id: &str| {
                PrincipalChain::of(Subject::Verified {
                    id: SubjectId::parse(id).expect("a subject id parses"),
                })
            };
            let context = RequestContext::with_assertion(chain(id), Secret::new(String::from(token)));
            let minted = broker
                .mint(&context, &SourceSet::of(source()))
                .expect("the exchange answered - an error here is a provider that refused, not a grant");
            let agreed = minted
                .agreeing_with(chain(id).subject(), &SourceSet::of(source()), now)
                .expect("the grant agrees with the request");
            let Agreed::Granted { credentials } = agreed else {
                panic!("an impersonating source with an assertion is granted");
            };
            let presented = credentials.presented_for(&source()).expect("a leg");
            // The presentation owns a `Secret`, and the grant lent it by reference; the clone hands
            // one leg its own credential without disturbing the grant's record.
            let Presented::SubjectToken { material } = presented else {
                panic!("an impersonating source gets a subject token");
            };
            let rows = warehouse
                .execute(
                    Executable::Query(&plan),
                    &Presented::SubjectToken {
                        material: material.clone(),
                    },
                )
                .expect("the endpoint answered the query");
            let mut answered: Vec<(String, String)> = Vec::new();
            for row in rows.rows() {
                let (Some(period), Some(total)) = (row.first(), row.get(1)) else {
                    panic!("a two-column row has two cells");
                };
                answered.push((period.render(), total.render()));
            }
            answered.sort();
            answered
        };

        let from_a = rows_for(&subject_a, "principal-a@example.com");
        let from_b = rows_for(&subject_b, "principal-b@example.com");
        assert!(
            !from_a.is_empty(),
            "principal A's grant must see at least one row, or the fixture is wrong"
        );
        assert!(
            !from_b.is_empty(),
            "principal B's grant must see at least one row, or the fixture is wrong"
        );
        // The claim this leg exists to make: two grants, two row sets. If a service answered both as
        // the deployment's own identity - reading every row as one identity - these would be equal.
        assert_ne!(
            from_a, from_b,
            "two principals with deliberately different grants must read different row sets"
        );
        // One row visible to A and not to B (the fixture the issue specifies), rather than an
        // ordering accident across two otherwise identical sets.
        assert!(
            from_a.iter().any(|row| !from_b.contains(row)),
            "the grant difference must be visible in the rows themselves, not an accident of the answer's shape"
        );
    }
}
