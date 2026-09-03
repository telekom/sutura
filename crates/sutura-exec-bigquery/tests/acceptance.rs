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
        Aggregate, ColumnName, DatasetName, Grain, MetricName, ProjectName, QualifiedTable, SourceName, TableName, TableQualifier,
    };
    use sutura_domain::plan::{
        Executable, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
        StatementTables,
    };
    use sutura_domain::warehouse::preflight::TablesPresent;
    use sutura_domain::warehouse::{ParamValue, PreFlight, Warehouse as _};

    use crate::support::{Connection, Wired, bounds, named, opened, presented};

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
            QualifiedTable::new(
                Some(TableQualifier::in_project(
                    ProjectName::parse(self.connection.billing_project.as_str()).expect("a project id is also a project name"),
                    DatasetName::parse(self.connection.dataset.as_str()).expect("a dataset id is also a dataset name"),
                )),
                self.table.clone(),
            )
        }
    }

    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a source name is a source name")
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
        let absent = QualifiedTable::from(TableName::parse("sutura_acceptance_no_such_table").expect("a table name parses"));
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
        let absent = QualifiedTable::from(TableName::parse("sutura_acceptance_no_such_table").expect("a table name parses"));
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
        // **The soft edge of the pre-flight, which the test above cannot reach and `docs/adr/0018`
        // recorded as held by a fake transport only.** A listing that FAILS has to be a different
        // answer from one that reports a table missing, and the reason it needs a live check is the
        // direction the decoder fails in: every field of `wire::tables::Listing` is
        // `#[serde(default)]`, so anything that decodes to nothing reads as *every table is absent*
        // and would stop a boot. What a composition root has to get instead is an `Err` it renders as
        // *could not verify*, on the warning side of `preflight_was_refused` - a `404` is a name
        // somebody is about to fix, not a grant to add.
        //
        // **Both names in the path are fictitious literals, so this leg still writes no resource of
        // the developer's project into this repository.** The PROJECT is the fixture's own on
        // purpose: a dataset absent from a project the credential can see is the case being asked
        // about, and one in a project it cannot see is a different answer.
        //
        // **The control is inside this test, and without it the leg passed for any failure at all.**
        // `preflight_was_refused` answers `false` for every variant that is not a `401` or a `403` -
        // a credential that would not read, an unreachable host, a document that would not decode -
        // and each of those also carries a `#[source]`. So both assertions below were satisfied by a
        // run in which NOTHING worked, and the leg would have reported *a dataset that is not there
        // warns* just as loudly. Asking the fixture's own table first and requiring `All` is what
        // makes the failure dataset-SPECIFIC: the same warehouse, in the same call sequence, listed a
        // real dataset before it failed to list this one. It costs one more `tables.list`, which is
        // billed for nothing.
        let fixture = Fixture::required();
        let present = fixture.unqualified();
        let nowhere = QualifiedTable::new(
            Some(TableQualifier::in_project(
                ProjectName::parse(fixture.connection.billing_project.as_str()).expect("a project id is also a project name"),
                DatasetName::parse("sutura_acceptance_no_such_dataset").expect("a dataset name parses"),
            )),
            TableName::parse("sutura_acceptance_no_such_table").expect("a table name parses"),
        );
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
            core::error::Error::source(&unverified).is_some(),
            "the endpoint's own error did not survive #[source]: {unverified:?}"
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
}
