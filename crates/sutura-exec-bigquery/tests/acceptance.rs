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
//! **The leg the records ask for is a different piece of work**, and #78's Postgres importer already
//! has its shape: load the example fixtures into the dataset, run the 21 corpus questions, compare
//! rows against the engine. Every sentence in this repository that promised *the corpus* has been
//! narrowed to what this file does - `docs/adr/0017`'s amendment, `docs/adr/0018`, `AGENTS.md`,
//! `docs/architecture.md` and both plan pages - because a record that says "the corpus" over a test
//! that submits one statement is the overstated-claim defect this repository treats as a defect.
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
//! only way in is to ask for them by name - and a developer who asked for acceptance and got three
//! green ticks against no project has been told the opposite of the truth. [`Fixture`] carries the
//! full argument, including the fact that the skipping version was written first and did exactly
//! that.
//!
//! So the evidence a run of this produces lives in a developer's terminal and nowhere else, and until
//! somebody pastes one, **nothing in this repository has sent a statement to a real dataset.**
//!
//! # How to run it
//!
//! ```text
//! just bigquery-acceptance
//! ```
//!
//! Which needs, once: `just gcloud-login`, and three values in the developer's own environment.
//! **Their names are here and their values are not, and will not be** - a project id is one of the
//! things this repository does not write down, and `.envrc` already sources a file under the user's
//! own configuration directory for exactly this class of value.
//!
//! | Variable | What it names |
//! | --- | --- |
//! | `SUTURA_BQ_BILLING_PROJECT` | the project the job is billed to, and its quota project |
//! | `SUTURA_BQ_DATASET` | the dataset an unqualified table resolves in, inside that project |
//! | `SUTURA_BQ_TABLE` | a table in it with a `DATE` column `day` and an `INT64` column `amount` |
//!
//! The table's shape is two columns because that is the smallest thing a real plan can be asked
//! about: a time bucket needs a `DATE`, and a measure needs something to sum. **A `TIMESTAMP` will
//! not do** - `transport::FieldType` maps `DATE` and refuses `TIMESTAMP`, which the crate
//! documentation states as the limit it is.
//!
//! **The job is bounded before it is sent**, which matters more here than anywhere because this is
//! the one path that spends real money: [`bounds`] sets a deadline and a `maximumBytesBilled`
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
#[cfg(test)]
mod tests {
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::identity::Presented;
    use sutura_domain::model::{
        Aggregate, ColumnName, DatasetName, Grain, MetricName, ProjectName, QualifiedTable, SourceName, TableName, TableQualifier,
    };
    use sutura_domain::plan::{
        Executable, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    };
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::{ParamValue, PreFlight, Warehouse as _};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::transport::{DatasetId, ProjectId};
    use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
    use sutura_exec_bigquery::wire::{BigQueryWire, BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

    /// What every job this leg submits is bounded by.
    ///
    /// **30 seconds because that is `server.request_timeout_seconds`' shipped default**, which is the
    /// setting a composition root would fill this from - so the number here is a copy of a real one
    /// rather than a round guess, and this comment says which.
    ///
    /// **1 GiB because this leg is the one path that spends real money.** A developer or a CI job
    /// pointed at a partitioned table with years of history gets `bytesBilledLimitExceeded` from the
    /// service, unbilled, instead of discovering the scan on an invoice. Nothing else in this
    /// repository bounds bytes scanned - the row cap bounds rows returned, not bytes read.
    fn bounds() -> JobBounds {
        JobBounds::of(
            QueryDeadline::parse(30).expect("30 seconds is a deadline"),
            BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
        )
    }

    /// What this leg needs from the environment, and what it takes from the credential instead.
    ///
    /// **It FAILS when a value is absent, and it used to SKIP - the reversal is the point of this
    /// paragraph.** The skipping version printed `SKIPPED - ... is not set` and returned, and every
    /// one of these three tests then reported **PASS** with no project anywhere. That is a green
    /// nobody asked for, over exactly the claim this file exists to make.
    ///
    /// **Why the compose tier's direction is right there and wrong here**, since it does skip: its
    /// cells run inside `just test`, so failing would break the suite on every machine with no
    /// docker. These tests are `#[ignore]`d, so the only way to reach one is to type
    /// `just bigquery-acceptance` - which is already a statement of intent. A developer who asked for
    /// acceptance and got three green ticks against nothing has been told the opposite of the truth.
    /// It earned its keep on its first real use: pointed at a service-account key the wire could not
    /// then read, it reported `0 passed, 3 failed` rather than three ticks.
    ///
    /// **The billing project comes from the CREDENTIAL where the credential names one**, which a
    /// service-account key does and an application-default login does not. One fewer variable to set
    /// wrongly, and it removes a second answer to *who pays* that could disagree with the first - the
    /// same argument the wire makes for not reading `quota_project_id`.
    struct Fixture {
        billing_project: ProjectId,
        dataset: DatasetId,
        table: TableName,
        credentials: Credential,
    }

    impl Fixture {
        /// The credential and the two names, or a panic saying exactly what is missing.
        ///
        /// A panic rather than a `Result`, because a test's own precondition is not an outcome a test
        /// reports on - and because the message is the whole product here: it has to name what a
        /// developer has to set.
        fn required() -> Self {
            let agent = WireAgent::pinned(bounds());
            let file = CredentialFile::well_known().expect("this machine names a well-known credential location");
            let credentials = Credential::read(&file, agent).expect(
                "a credential file is readable - run `just gcloud-login`, or point \
                 GOOGLE_APPLICATION_CREDENTIALS at a service-account key",
            );
            // Printed so a green run says WHICH identity produced it: the two kinds are two deployment
            // shapes, and an operator reading a log needs to know which one answered. A fixed word from
            // a closed match, never the file's own text.
            println!("bigquery-acceptance: credential kind {}", credentials.kind());

            // The key's own `project_id` first, then the variable, then a panic. A service-account key
            // carries it; an application-default login does not, so a developer on a laptop still sets
            // the variable and CI sets nothing.
            let billing = credentials.project().cloned().unwrap_or_else(|| {
                Self::named(
                    "SUTURA_BQ_BILLING_PROJECT",
                    "this credential names no project of its own, so the billing project has to be set",
                )
            });
            Self {
                billing_project: ProjectId::parse(billing).expect("a project id parses"),
                dataset: DatasetId::parse(Self::named(
                    "SUTURA_BQ_DATASET",
                    "the dataset an unqualified table resolves in",
                ))
                .expect("a dataset id parses"),
                table: TableName::parse(Self::named(
                    "SUTURA_BQ_TABLE",
                    "a table with a DATE column `day` and an INT64 column `amount`",
                ))
                .expect("a table name parses"),
                credentials,
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
                    DatasetName::parse(self.dataset.as_str()).expect("a dataset id is also a dataset name"),
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
                    ProjectName::parse(self.billing_project.as_str()).expect("a project id is also a project name"),
                    DatasetName::parse(self.dataset.as_str()).expect("a dataset id is also a dataset name"),
                )),
                self.table.clone(),
            )
        }

        /// One variable, or a panic naming it and saying what it is for.
        fn named(key: &str, what: &str) -> String {
            match std::env::var(key) {
                Ok(value) if !value.trim().is_empty() => value,
                Ok(_) | Err(_) => panic!(
                    "{key} is not set - it names {what}. This is the acceptance leg: it needs a real \
                     project, and NOTHING in this repository had ever run against one before it was \
                     first run by hand. See the header of this file"
                ),
            }
        }
    }

    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a source name is a source name")
    }

    /// The one acknowledgement both the posture and the presented leg are built from.
    ///
    /// **One function rather than two literals**, because `Presented::agrees_with` compares the two
    /// witnesses for equality - so two copies of this sentence that drifted by a character would be
    /// a leg refused for a reason that has nothing to do with `BigQuery`.
    fn declared() -> SharedIdentityDeclared {
        SharedIdentityDeclared::of(
            AcknowledgementReason::parse("a developer's own application-default credential, reaching their own project")
                .expect("an acknowledgement is an acknowledgement"),
        )
    }

    /// The posture this leg runs under, which is the only one this adapter can deliver.
    ///
    /// A developer's own application-default credential IS one identity for everybody who asks, so
    /// `SharedServiceUser` is the honest declaration and not a placeholder. `just gcloud-login`'s own
    /// documentation says the same thing.
    fn posture() -> SourcePosture {
        SourcePosture::SharedServiceUser { declared: declared() }
    }

    fn presented() -> Presented {
        Presented::SharedServiceUser { declared: declared() }
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
            table.clone(),
            Vec::new(),
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
    /// **The composition, and it is the thing this test is really evidence for beside the round
    /// trip:** one [`WireAgent`] carrying the bounds, one [`Credential`] behind the token port, one
    /// transport behind the seam, one warehouse behind the domain's port. The agent is the pinned kind
    /// because it is the only kind either half accepts, which is what makes the wire's claims
    /// properties of the types rather than of this file.
    fn opened(fixture: Fixture) -> BigQueryWarehouse<BigQueryWire<Credential>> {
        BigQueryWarehouse::new(
            source(),
            posture(),
            fixture.billing_project,
            fixture.dataset,
            BigQueryWire::new(WireAgent::pinned(bounds()), fixture.credentials),
        )
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
        let warehouse = opened(fixture);
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
        let warehouse = opened(fixture);
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
        let warehouse = opened(fixture);
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
        let warehouse = opened(fixture);
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
        let warehouse = opened(fixture);
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
}
