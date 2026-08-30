//! The acceptance leg: one real question against a real project, from a developer's own machine.
//!
//! **This has never been run.** The change that added the wire is the change
//! [`docs/adr/0017`](https://github.com/telekom/sutura/blob/main/docs/adr/0017-what-a-bigquery-test-runs-against.md)
//! said could first verify it, and it could not: the machine it was written on has no `gcloud`, no
//! application-default credential and no project. So this file is the fixture that record asked for,
//! written and unexecuted, and `docs/adr/0018` records that plainly rather than leaving a reader to
//! infer a green run from the file's existence.
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
//! So **the acceptance evidence for this dialect lives in a developer's terminal and nowhere else**,
//! and until somebody pastes one, the honest summary of `BigQuery` support in this repository is
//! `docs/adr/0017`'s sentence with one word changed: *the statement is right as far as five
//! mechanisms can tell, and nobody has run one.*
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
//! | `SUTURA_BQ_BILLING_PROJECT` | the project the job is billed to |
//! | `SUTURA_BQ_DATASET` | the dataset an unqualified table resolves in, inside that project |
//! | `SUTURA_BQ_TABLE` | a table in it with a `DATE` column `day` and an `INT64` column `amount` |
//!
//! The table's shape is two columns because that is the smallest thing a real plan can be asked
//! about: a time bucket needs a `DATE`, and a measure needs something to sum. **A `TIMESTAMP` will
//! not do** - `transport::FieldType` maps `DATE` and refuses `TIMESTAMP`, which the crate
//! documentation states as the limit it is.

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
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
    use sutura_domain::plan::{
        Executable, PlanBucket, PlanColumn, PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    };
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::{ParamValue, PreFlight, Warehouse as _};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::transport::{DatasetId, ProjectId};
    use sutura_exec_bigquery::wire::credential::{ApplicationDefault, CredentialFile};
    use sutura_exec_bigquery::wire::{BigQueryWire, agent};

    /// What this leg needs from the developer's environment.
    ///
    /// **It FAILS when a value is absent, and it used to SKIP - the reversal is the point of this
    /// paragraph.** The skipping version printed `SKIPPED - ... is not set` and returned, and every
    /// one of these three tests then reported **PASS** with no project anywhere. That is a green
    /// nobody asked for, over exactly the claim this file exists to make, and it is the same defect
    /// `docs/adr/0017` refuses an emulator for one size smaller.
    ///
    /// **Why the compose tier's direction is right there and wrong here**, since it does skip: its
    /// cells run inside `just test`, so failing would break the suite on every machine with no
    /// docker. These tests are `#[ignore]`d, so the only way to reach one is to type
    /// `just bigquery-acceptance` - which is already a statement of intent. A developer who asked for
    /// acceptance and got three green ticks against nothing has been told the opposite of the truth.
    ///
    /// Once a project IS named, everything after this point fails too, because from there a refusal
    /// is the finding.
    struct Fixture {
        billing_project: ProjectId,
        dataset: DatasetId,
        table: TableName,
    }

    impl Fixture {
        /// The three values, or a panic naming the first one that is missing.
        ///
        /// A panic rather than a `Result`, because a test's own precondition is not an outcome a test
        /// reports on - and because the message is the whole product here: it has to name the variable
        /// a developer has to set.
        fn required() -> Self {
            let read = |key: &str| match std::env::var(key) {
                Ok(value) if !value.trim().is_empty() => value,
                Ok(_) | Err(_) => panic!(
                    "{key} is not set, so this leg has no project to ask. It is the acceptance leg: set the three \
                     SUTURA_BQ_* variables in your own environment and run `just gcloud-login` once. See the header \
                     of this file - and note that NOTHING here has ever run against a real project"
                ),
            };
            Self {
                billing_project: ProjectId::parse(read("SUTURA_BQ_BILLING_PROJECT")).expect("a project id parses"),
                dataset: DatasetId::parse(read("SUTURA_BQ_DATASET")).expect("a dataset id parses"),
                table: TableName::parse(read("SUTURA_BQ_TABLE")).expect("a table name parses"),
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
    fn plan(table: &TableName) -> QueryPlan {
        let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a column name parses"));
        let from = Date::parse("2000-01-01").expect("an ISO date parses");
        let until = Date::parse("2100-01-01").expect("an ISO date parses");
        QueryPlan::new(
            source(),
            MetricName::parse("acceptance").expect("a metric name parses"),
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
            String::from("acceptance"),
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

    /// The adapter, wired to the endpoint through the developer's own credential.
    ///
    /// **The composition, and it is four lines** - which is the thing this test is really evidence
    /// for beside the round trip: one agent shared by the credential source and the transport, one
    /// transport behind the seam, one warehouse behind the domain's port.
    fn opened(fixture: Fixture) -> BigQueryWarehouse<BigQueryWire<ApplicationDefault>> {
        let agent = agent();
        let file = CredentialFile::well_known().expect("this machine names a well-known credential location");
        let credentials = ApplicationDefault::read(&file, agent.clone()).expect("`just gcloud-login` has been run");
        BigQueryWarehouse::new(
            source(),
            posture(),
            fixture.billing_project,
            fixture.dataset,
            BigQueryWire::new(agent, credentials),
        )
    }

    #[test]
    #[ignore = "needs a real BigQuery project, named in the developer's own environment"]
    fn the_endpoint_accepts_a_statement_this_repository_generated() {
        // The claim CI cannot make, and the one `docs/adr/0017` says the corpus does not: not that
        // the statement PARSES as BigQuery - the goldens already assert that - but that the service
        // ACCEPTS it. A dry run is the cheapest way to ask: it uses no slots and is not charged,
        // which is also what makes `dry_run` answering `PreFlight::Accepted` honest rather than a
        // guess.
        let fixture = Fixture::required();
        let table = fixture.table.clone();
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
        let table = fixture.table.clone();
        let warehouse = opened(fixture);
        let plan = plan(&table);

        let rows = warehouse
            .execute(Executable::Query(&plan), &presented())
            .expect("the endpoint answered with a complete result");
        assert_eq!(
            rows.columns(),
            ["period", "acceptance"],
            "the projected labels are the plan's"
        );
        for row in rows.rows() {
            assert_eq!(row.len(), 2, "a row is as wide as the schema");
        }
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
        let absent = TableName::parse("sutura_acceptance_no_such_table").expect("a table name parses");
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
