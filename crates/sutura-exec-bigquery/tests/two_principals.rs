//! **The two-principal cell**: one statement, two principals, two row sets - against a real dataset
//! whose row access policies grant each principal different rows.
//!
//! # What this can establish, and the half it cannot
//!
//! Read this before citing a green run here as impersonation, because the two halves are easy to
//! elide and only one of them is here.
//!
//! **What a green run establishes.** The endpoint evaluates the statement under whoever the bearer
//! this adapter *presented* says, and not under the identity the transport itself holds. That is a
//! claim about a real data system enforcing a real row-level grant, and no fake and no mock can
//! answer it: `sutura_exec_bigquery::tests`'s
//! `two_subjects_each_run_their_statement_under_the_bearer_minted_for_them` already shows the two
//! bearers *leaving* this adapter, and `sutura_http`'s
//! `two_subjects_drive_two_different_exchanged_credentials` shows two subjects driving two
//! credentials through the transport. Neither one has a dataset behind it, so neither says a row was
//! ever filtered.
//!
//! **What it does NOT establish, and this is the sentence that matters.** The two principals here are
//! two service accounts whose *private keys this test holds*. Nothing was exchanged and nobody asked:
//! the leg mints each bearer from a key on disk, so what is shown is that a source executes as the
//! principal whose credential a leg carried - not that a deployment can *obtain* such a credential
//! for the caller who asked. That second half is a real token exchange against a real authorization
//! server, it has never run, and `docs/where-identity-is-proven.md` keeps it as its own venue with
//! its own row. **So this cell is leg 2's SOURCE half and not leg 2.** `AGENTS.md`'s position is
//! unchanged by every green run this file can produce: no source a deployment serves executes as the
//! asking subject.
//!
//! # Why it does not seed the table, when issue #123 asked it to
//!
//! **Because the one loader this crate has would delete the grant this cell asserts on.**
//! `BigQueryWarehouse::load_fixture` renders `CREATE OR REPLACE TABLE`, and replacing a table drops
//! its row access policies - so the obvious way to write the seeding step would disarm the policies,
//! and the run after it would compare two identical row sets. There is no arbitrary-SQL entry point
//! to reach for instead, deliberately: `load_fixture` takes a table name and a path, which is what
//! keeps *no arbitrary SQL* true of this crate.
//!
//! So the rows belong to whoever owns the policies, which is the stack - the predicate and the rows
//! it selects are two halves of one grant, and splitting them across pulumi and a test file is how
//! they drift. `test-infra/pulumi/google` seeds one row per principal beside the two
//! `RowAccessPolicy` resources. **What this file does instead is fail with the message that names
//! that**, rather than reporting an empty answer as a pass.
//!
//! # How to run it
//!
//! ```text
//! just bigquery-two-principals
//! ```
//!
//! Its own task rather than a third leg inside `just bigquery-acceptance`, because it needs five
//! values and two key documents that the other two legs do not - and a task that failed for a
//! developer holding one credential would make the legs they *can* run unreachable.
//!
//! **Their names are here and their values are not, and will not be.** A dataset id is one of the
//! things this repository does not write down.
//!
//! | Variable | What it names |
//! | --- | --- |
//! | `SUTURA_BQ_RLS_DATASET` | the dataset holding the table the row access policies are on |
//! | `SUTURA_BQ_RLS_TABLE` | that table: a `DATE` column `day`, an `INT64` `amount`, and the grouping column |
//! | `SUTURA_BQ_GROUP_COLUMN` | the column the two policies filter on |
//! | `SUTURA_BQ_PRINCIPAL_A_ROWS` | the grouping value principal A's policy grants |
//! | `SUTURA_BQ_PRINCIPAL_B_ROWS` | the grouping value principal B's policy grants |
//! | `SUTURA_BQ_PRINCIPAL_A_KEY` | principal A's service-account key document |
//! | `SUTURA_BQ_PRINCIPAL_B_KEY` | principal B's service-account key document |
//!
//! plus whatever `tests/support/mod.rs` reads: the CI credential, which is the *third* identity here
//! and the one the control leg is about.
//!
//! **`#[ignore]`d for the reasons `tests/acceptance.rs`'s header gives** - a fork's pull request sees
//! no secret, a gate needing a cloud project fails for an environment reason, and the nix sandbox has
//! no network - and, like that file, an unconfigured run **fails rather than skips**. Three tests
//! here are not `#[ignore]`d: they are controls on this file's own harness, they read no environment
//! and open no socket, so they run in `just test` and in `checks.nextest` where a harness defect
//! should fail.
//!
//! **The job is bounded before it is sent**, through the same `support::bounds` both other legs use.
//!
//! **This leg introduces no name of its own, which is worth saying because the corpus leg's do not
//! have room for one.** Since telekom/sutura#119 the corpus fixture tables carry a per-run suffix
//! and the widest case measured leaves three characters inside `TableName`'s ceiling - so a fourth
//! fixture name there is a parse refusal. Nothing here is affected: this cell CREATES no table. It
//! reads one the stack owns, by the name the environment gives, and every other value it uses is
//! read rather than composed.

// `required-features = ["wire"]` is declared on the target in `Cargo.toml`, for the reason
// `tests/acceptance.rs` states: cargo skips the target rather than compiling an empty binary.

// `cfg(test)` around the whole file - the house pattern, because clippy honours
// `allow-expect-in-tests` only inside a `#[cfg(test)]` item.
#[cfg(test)]
#[path = "support/support.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::identity::Presented;
    use sutura_domain::model::{
        Aggregate, ColumnName, DatasetName, DimensionName, Grain, MetricName, QualifiedTable, SourceName, TableName,
        TableQualifier,
    };
    use sutura_domain::plan::{
        Executable, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
        QueryPlan, ResultLabel, StatementTables,
    };
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::{ParamValue, RowSet, Value, Warehouse as _};
    use sutura_exec_bigquery::BigQueryError;
    use sutura_exec_bigquery::transport::DatasetId;
    use sutura_exec_bigquery::wire::credential::{AccessTokens as _, Credential, CredentialFile};
    use sutura_exec_bigquery::wire::{CallDeadline, ReasonCode, WireAgent, WireError};

    use crate::support::{Connection, Wired, bounds, named, opened, opened_as, presented};

    /// The label the grouping column is projected under.
    ///
    /// A label of this file's own rather than the column's name, so the assertions read the answer by
    /// a name they wrote instead of by one the environment supplied - the column name is
    /// configuration and the projected label is this question's own.
    const GRANT_LABEL: &str = "granted_group";

    /// The label the measure is projected under.
    const MEASURE_LABEL: &str = "total_amount";

    /// A range wide enough to hold the seeded rows and narrow enough to be a question this surface
    /// accepts.
    ///
    /// The pair `tests/acceptance.rs` uses, for the reason it gives: `MAX_RANGE_DAYS` refuses a
    /// hundred-year span before an adapter sees it, so ten years less a day is the widest a question
    /// can legitimately be. The stack seeds inside it.
    const FROM: &str = "2016-09-01";
    const UNTIL: &str = "2026-08-30";

    /// The two principals, each with the key its bearer is minted from and the rows its policy grants.
    ///
    /// **A type rather than four loose strings, because two of the ways this cell can be
    /// misconfigured make it fail for a reason that is not the claim.** Both key variables pointing at
    /// one document, or both grant values being one value, produce a run whose two row sets are equal,
    /// which reads as *the policies are not enforced* and is really *the fixture describes one
    /// principal twice*. [`Self::of`] refuses both before a socket is opened, and two tests that need
    /// no project provoke them.
    struct Principals {
        a: Principal,
        b: Principal,
    }

    /// One principal: the key document its bearer comes from, and the grouping value it is entitled
    /// to.
    struct Principal {
        /// The service-account key document. Its path, never its content, and nothing here prints it.
        key: CredentialFile,
        /// The grouping value this principal's row access policy grants.
        grants: String,
    }

    /// Why a pair of principals does not describe two principals.
    ///
    /// Its own error rather than a panic inside [`Principals::of`], so the two controls can provoke
    /// each case and assert *which* one they got - a panic message compared by `contains` is the shape
    /// this repository has been wrong about before.
    #[derive(Debug, PartialEq, Eq)]
    enum NotTwoPrincipals {
        /// Both key variables name one document, so both legs would run as one identity.
        KeyDocumentIsShared,
        /// Both policies were described as granting the same rows, so disjointness is unassertable.
        GrantValuesAreEqual,
    }

    impl Principals {
        /// The pair, or why it is not a pair.
        fn of(a_key: &str, a_grants: &str, b_key: &str, b_grants: &str) -> Result<Self, NotTwoPrincipals> {
            if a_key == b_key {
                return Err(NotTwoPrincipals::KeyDocumentIsShared);
            }
            if a_grants == b_grants {
                return Err(NotTwoPrincipals::GrantValuesAreEqual);
            }
            Ok(Self {
                a: Principal {
                    key: CredentialFile::at(a_key),
                    grants: a_grants.to_owned(),
                },
                b: Principal {
                    key: CredentialFile::at(b_key),
                    grants: b_grants.to_owned(),
                },
            })
        }

        /// The pair the environment describes, or a panic naming what is missing.
        fn required() -> Self {
            let a_key = named("SUTURA_BQ_PRINCIPAL_A_KEY", "principal A's service-account key document");
            let a_grants = named(
                "SUTURA_BQ_PRINCIPAL_A_ROWS",
                "the grouping value principal A's row access policy grants",
            );
            let b_key = named("SUTURA_BQ_PRINCIPAL_B_KEY", "principal B's service-account key document");
            let b_grants = named(
                "SUTURA_BQ_PRINCIPAL_B_ROWS",
                "the grouping value principal B's row access policy grants",
            );
            Self::of(&a_key, &a_grants, &b_key, &b_grants).expect(
                "the environment describes two principals - one key document and one grouping value each, \
                 and neither pair may be the same value twice",
            )
        }
    }

    impl Principal {
        /// The grouping values this principal's row access policy grants, as the set an answer is
        /// compared against.
        ///
        /// One value today, and a SET rather than a `String` because what the cell asserts is that
        /// the answer is made of nothing else - a comparison against one value would have to be
        /// written as a loop at the assertion to say that.
        fn entitled_to(&self) -> BTreeSet<String> {
            BTreeSet::from([self.grants.clone()])
        }

        /// The bearer this principal's own key mints, presented as this leg's subject credential.
        ///
        /// **This is issue #123's second bullet, and it reuses the crate's own credential rather than
        /// hand-rolling a token exchange:** [`sutura_exec_bigquery::wire::credential::Credential`]
        /// reads the key document, signs the assertion and trades it, which is the same code path a
        /// deployment holding a service-account key runs. What is hand-rolled here is nothing.
        ///
        /// **And it is exactly NOT the exchange leg 2 needs**, which is why the module header says so
        /// twice: a bearer minted from a key on disk authenticates whoever holds the key, and no
        /// caller asked for anything.
        fn presented(&self) -> Presented {
            let credential = Credential::read(&self.key, WireAgent::pinned(bounds()))
                .expect("a principal's key document is readable and is a service-account key");
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("this machine's clock is after 1970")
                .as_secs();
            let minted = credential
                .bearer(now, CallDeadline::opened(bounds().deadline()))
                .expect("the authorization server minted a token for this principal's key");
            // **Cloned and never exposed**, which is what `sutura_exec_bigquery::sts` does with an
            // exchanged token: `Secret` is `Clone` and a clone is still opaque, so the exposure this
            // line first took - and the `#[expect]` entry it added to the ledger `clippy.toml`
            // keeps greppable - was buying nothing.
            Presented::SubjectToken {
                material: minted.token().clone(),
            }
        }
    }

    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a source name is a source name")
    }

    /// What both live legs need from the environment, read once.
    ///
    /// The `Fixture` shape `tests/acceptance.rs` uses, for the reason it gives: the two legs open
    /// DIFFERENT warehouses over the same table and the same question, and two separately written
    /// environment readings are two things that could drift into two different questions.
    struct Fixture {
        principals: Principals,
        /// The column the two row access policies filter on, which is what the question projects.
        group_column: String,
        /// The CI credential and the billing project, pointed at the policied dataset.
        connection: Connection,
    }

    impl Fixture {
        /// The environment, or a panic naming what has to be set.
        fn required() -> Self {
            Self {
                principals: Principals::required(),
                group_column: named("SUTURA_BQ_GROUP_COLUMN", "the column the two row access policies filter on"),
                connection: connection(),
            }
        }

        /// The policied table, qualified into its own dataset.
        ///
        /// Qualified rather than bare so the path in the statement names the dataset outright: the
        /// question is which rows a principal reads, and a table resolved by a job default would
        /// leave *which table* as a second thing a failure could mean.
        fn table(&self) -> QualifiedTable {
            QualifiedTable::new(
                Some(TableQualifier::in_dataset(
                    DatasetName::parse(self.connection.dataset.as_str()).expect("a dataset id is also a dataset name"),
                )),
                TableName::parse(named("SUTURA_BQ_RLS_TABLE", "the table the two row access policies are on"))
                    .expect("a table name parses"),
            )
        }
    }

    /// The connection, pointed at the dataset the row access policies are on.
    ///
    /// The CI credential and the billing project come from [`Connection::required`] - the same
    /// environment reading both other legs do - and only the DATASET is this leg's own, because the
    /// policied table is not in the dataset the acceptance legs run against.
    ///
    /// **It therefore READS `SUTURA_BQ_DATASET` and throws the value away, which is a wart and is
    /// named rather than hidden.** `Connection::required` demands it, this cell replaces it, and the
    /// cell cannot reach past that reading: a shared test module is compiled once per target and
    /// `dead_code` is `deny`, so a `required_in(variable)` that only this leg called would be dead
    /// in the other two and a `required` that only they called would be dead here. The tidy fix is
    /// to move all three legs onto one `required_in`, which touches `tests/corpus.rs` - being
    /// rewritten by telekom/sutura#283 - so it is deferred rather than done here. **What the wart
    /// costs, concretely:** the workflow step for this cell has to set a variable it does not use,
    /// or the leg panics on a precondition before it opens a socket.
    fn connection() -> Connection {
        let mut connection = Connection::required();
        connection.dataset = DatasetId::parse(named(
            "SUTURA_BQ_RLS_DATASET",
            "the dataset holding the table the row access policies are on",
        ))
        .expect("a dataset id parses");
        connection
    }

    /// The question, projecting the column the policies filter on.
    ///
    /// **The key is the point of this plan rather than a detail.** A plan with only a bucket and a
    /// measure answers one number per month, and two principals reading different rows would show up
    /// as two different numbers - which is evidence for *something* and not for *which rows*.
    /// Projecting the grouping column makes the answer say which grant produced it, so the assertion
    /// is against the policy's own predicate rather than against an arithmetic coincidence.
    fn plan(table: &QualifiedTable, group_column: &str) -> QueryPlan {
        let column = |name: &str| PlanColumn::new(table.name().clone(), ColumnName::parse(name).expect("a column name parses"));
        let from = Date::parse(FROM).expect("an ISO date parses");
        let until = Date::parse(UNTIL).expect("an ISO date parses");
        QueryPlan::new(
            source(),
            MetricName::parse(MEASURE_LABEL).expect("a metric name parses"),
            StatementTables::only(table.clone()),
            PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("day")),
            vec![PlanKey::new(
                ResultLabel::dimension(&DimensionName::parse(GRANT_LABEL).expect("a dimension name parses")),
                column(group_column),
            )],
            PlanMeasure::Simple {
                term: PlanTerm::Aggregate {
                    aggregate: Aggregate::Sum,
                    column: column("amount"),
                },
            },
            ResultLabel::measure(&MetricName::parse(MEASURE_LABEL).expect("a metric name parses")),
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

    /// The adapter over the policied dataset, declared `impersonation-at-source`.
    ///
    /// **Through the shared module's own composition**, so this cell and the two acceptance legs are
    /// evidence for one wiring rather than for three that resemble each other - and so the money
    /// ceiling reaches all of them the same way.
    ///
    /// **The transport holds the CI credential and the subject legs never use it**, which is not a
    /// convenience: `BigQueryWire::submit` decides which bearer authorizes the job once, before
    /// anything is built, and a leg carrying a subject's own credential never reaches the credential
    /// source at all. So the identity the transport holds is a third identity, present and unread -
    /// and [`the_deployments_own_identity_reads_neither_principals_rows`] is what turns that from a
    /// claim about our code into a claim about the endpoint.
    fn impersonating(connection: Connection) -> Wired {
        opened_as(source(), SourcePosture::ImpersonationAtSource, connection, bounds())
    }

    /// The grouping values an answer carries, as a set.
    ///
    /// Read by LABEL rather than by position, because what is asserted is which grants the answer is
    /// made of and a column order is the generator's business. `named` says which leg is speaking: a
    /// failure has to name the principal, or three answers become one indistinguishable red.
    fn grants_in(who: &str, rows: &RowSet) -> BTreeSet<String> {
        let at = rows
            .column_index(GRANT_LABEL)
            .unwrap_or_else(|| panic!("{who}: the answer projects `{GRANT_LABEL}`, which is what the grant is read from"));
        (0..rows.rows().len())
            .filter_map(|row| rows.cell(row, at))
            .map(Value::render)
            .collect()
    }

    #[test]
    #[ignore = "needs the real two-principal dataset and both principals' keys, named in the environment"]
    fn each_principal_reads_exactly_the_rows_its_row_access_policy_grants_and_not_the_others() {
        // **The cell.** One statement, submitted twice, differing only in the credential presented -
        // and the endpoint answers each with the rows that principal's row access policy grants.
        //
        // Two assertions per principal, and the second is the one a reader should look for: each
        // answer is made of ITS OWN grant (so the policy's predicate decided it), and neither is
        // EMPTY (so the run is not two vacuous greens over an unseeded table - which is the shape
        // this cell would otherwise pass as). Disjointness follows and is not asserted; the comment
        // at the foot of this test says why.
        let fixture = Fixture::required();
        let table = fixture.table();
        let plan = plan(&table, &fixture.group_column);
        let principals = fixture.principals;
        let warehouse = impersonating(fixture.connection);

        // ONE plan value, borrowed twice. The statement is therefore the same statement, not two that
        // resemble each other - which is what makes a difference in the rows a difference in the
        // identity.
        let as_a = warehouse
            .execute(Executable::Query(&plan), &principals.a.presented())
            .expect("the endpoint answered under principal A's own bearer");
        let as_b = warehouse
            .execute(Executable::Query(&plan), &principals.b.presented())
            .expect("the endpoint answered under principal B's own bearer");

        let a_saw = grants_in("principal A", &as_a);
        let b_saw = grants_in("principal B", &as_b);

        assert!(
            !a_saw.is_empty(),
            "principal A read no rows at all - the policied table holds none of its grant `{}`. \
             The rows are the stack's: see test-infra/pulumi/google",
            principals.a.grants
        );
        assert!(
            !b_saw.is_empty(),
            "principal B read no rows at all - the policied table holds none of its grant `{}`. \
             The rows are the stack's: see test-infra/pulumi/google",
            principals.b.grants
        );
        assert_eq!(
            a_saw,
            principals.a.entitled_to(),
            "principal A read rows outside its own grant"
        );
        assert_eq!(
            b_saw,
            principals.b.entitled_to(),
            "principal B read rows outside its own grant"
        );
        // **No disjointness assertion, and its absence is the point rather than an omission.** It
        // cannot fail once the two above pass: each answer is one grouping value, and
        // `Principals::of` has already refused a pair whose two values are equal. An assertion that
        // cannot go red reads as a third independent check and is not one - which is why that
        // refusal is a control with a test of its own rather than a nicety.
    }

    #[test]
    #[ignore = "needs the real two-principal dataset and both principals' keys, named in the environment"]
    fn the_deployments_own_identity_reads_neither_principals_rows() {
        // **The control without which the test above is satisfied by a coincidence.** The same
        // statement over the same table, presented as the DEPLOYMENT's own identity - the CI
        // credential, which holds no row access policy on this table. If the rows a principal saw were
        // really the transport's own, this leg would see them too.
        //
        // **What BigQuery does for a principal no policy grants is not measured here yet**, and this
        // test says so rather than guessing: documented behaviour is no rows, and the observable
        // alternative is a refusal. Either is *not reading either principal's rows*, so both are
        // accepted and the leg PRINTS which one it got. The first green run is what narrows this to
        // one sentence, and `docs/where-identity-is-proven.md` carries the open question until then.
        let fixture = Fixture::required();
        let table = fixture.table();
        let plan = plan(&table, &fixture.group_column);
        let principals = fixture.principals;
        let warehouse = opened(source(), fixture.connection, bounds());

        let refused = match warehouse.execute(Executable::Query(&plan), &presented()) {
            Ok(rows) => {
                let saw = grants_in("the deployment's own identity", &rows);
                println!(
                    "bigquery-two-principals: the deployment's own identity read {} grant(s)",
                    saw.len()
                );
                assert!(
                    !saw.contains(&principals.a.grants) && !saw.contains(&principals.b.grants),
                    "the deployment's own identity read a principal's rows, so the rows the cell \
                     attributes to a presented bearer are the transport's"
                );
                return;
            }
            Err(refused) => refused,
        };

        // **The accepted set is TWO outcomes, and this match is what holds it to two.** A review
        // finding, and the sharpest case is not a permission failure: `UnmappedType`,
        // `NotAnInteger` and their siblings mean the endpoint ANSWERED and rows came back, so a
        // `let Ok(..) else { return }` here reported *this identity was refused* over a deployment
        // that had just read the table - the exact coincidence this leg exists to exclude, printed
        // as the control that excludes it. Exhaustive rather than a wildcard, in
        // `crate::wire::tables::was_refused`'s shape and for its reason: a new `BigQueryError`
        // variant is a compile error at this line instead of a new way to pass.
        //
        // Only a `403` that is not a rate or quota limit is accepted, on that function's own
        // argument - `BigQuery` documents six reasons at that status and two of them are not a
        // grant. **`401` is deliberately NOT accepted:** it means the deployment's credential could
        // not authenticate at all, which leaves this control unable to distinguish anything while
        // reading as though it had, because the subject legs never touch that credential.
        //
        // **The limit, next to the claim:** a `403` says the endpoint refused THIS identity and not
        // which grant it was missing, so *no row access policy grants it* and *it may not submit
        // jobs in this project* are indistinguishable here. Both satisfy the leg's own assertion -
        // it read neither principal's rows - and neither is evidence about the policy. The first
        // green run is what narrows exclusion 5 on the venue page to one sentence.
        match &refused {
            BigQueryError::Endpoint {
                cause: WireError::Refused { status, named, .. },
            } if *status == 403 && !matches!(named, ReasonCode::RateLimitExceeded | ReasonCode::QuotaExceeded) => {
                println!("bigquery-two-principals: the deployment's own identity was refused - {status}: {named}");
            }
            BigQueryError::UnmappedType { .. }
            | BigQueryError::NotAnInteger { .. }
            | BigQueryError::NotADouble { .. }
            | BigQueryError::NotABool { .. }
            | BigQueryError::NotFinite { .. }
            | BigQueryError::NotADate { .. }
            | BigQueryError::RowWidth { .. }
            | BigQueryError::Incomplete { .. }
            | BigQueryError::Shape { .. } => panic!(
                "the endpoint answered and rows came back, so the deployment's own identity DID read the \
                 policied table - a cell this adapter could not map is not a refusal, and accepting it \
                 would report the control as passing over rows nothing looked at: {refused}"
            ),
            BigQueryError::Endpoint { .. } => panic!(
                "the endpoint did not answer, which is not this identity being refused - one timeout here \
                 leaves the cell with no control at all while both subject legs pass: {refused}"
            ),
            BigQueryError::Render { .. }
            | BigQueryError::LegWithoutCombiner { .. }
            | BigQueryError::NoPrincipalSwitch { .. }
            | BigQueryError::PresentedDisagreesWithPosture { .. } => panic!(
                "nothing reached a socket: this is a defect in this file - the plan, the posture or the \
                 credential shape - and not an answer about the row grant: {refused}"
            ),
            // **The compile error this match exists to be**, arriving for the first time. This
            // variant belongs to `BigQueryWarehouse::session_user` - the identity read the
            // exchanged-identity cell uses - and no query leg can produce it. Its own arm rather
            // than folded into either group above, because both of their messages would be wrong:
            // it is neither *rows came back* nor *nothing reached a socket*. Reaching it means the
            // adapter changed under this cell.
            BigQueryError::NoIdentityInTheAnswer { .. } => panic!(
                "an identity read's refusal arrived from a query leg, which this adapter cannot produce - \
                 the control is judging an answer it was not written for: {refused}"
            ),
        }
    }

    #[test]
    fn one_key_document_named_twice_is_refused_before_a_socket_is_opened() {
        // **A control on this file's own fixture, and the misconfiguration it names is a live one:**
        // two environment variables pointing at one key document is one character in a workflow. Both
        // legs would then run as one principal, both answers would be equal, and the failure would
        // read as *the row access policies are not enforced* - a red run blaming the data system for a
        // typo in the job. Refused where the pair is built, so the message names the pair.
        assert_eq!(
            Principals::of("/tmp/one.json", "a", "/tmp/one.json", "b").err(),
            Some(NotTwoPrincipals::KeyDocumentIsShared)
        );
    }

    #[test]
    fn one_grant_described_twice_is_refused_because_disjointness_would_be_unassertable() {
        // The other half: two distinct keys whose grants were described as the same value. The cell's
        // disjointness assertion is then a comparison of a set with itself, which cannot fail for the
        // right reason and cannot pass for one either.
        assert_eq!(
            Principals::of("/tmp/a.json", "same", "/tmp/b.json", "same").err(),
            Some(NotTwoPrincipals::GrantValuesAreEqual)
        );
    }

    #[test]
    fn the_question_this_cell_asks_projects_the_column_the_policies_filter_on() {
        // **The harness control that keeps the cell's assertion an assertion about GRANTS.** Without
        // the key, the answer is one number per month and two principals reading different rows differ
        // by arithmetic - so a change that dropped the key would leave the cell green over a claim it
        // no longer makes. Read off the built plan rather than trusted: the label is this file's and
        // the column is the environment's, and the two have to meet.
        let table = QualifiedTable::new(
            Some(TableQualifier::in_dataset(
                DatasetName::parse("a_dataset").expect("a dataset name parses"),
            )),
            TableName::parse("policied").expect("a table name parses"),
        );
        let plan = plan(&table, "segment");
        let [key] = plan.keys() else {
            panic!("the cell asks exactly one grouping key");
        };
        assert_eq!(key.label(), GRANT_LABEL);
        assert_eq!(key.column().column().as_str(), "segment");
        assert_eq!(key.column().table().as_str(), "policied");
    }
}
