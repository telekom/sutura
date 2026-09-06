//! **The exchanged-identity cell**: hold one workload identity, exchange it, and ask the data
//! system who it thinks is running the job.
//!
//! # The claim, and the one word in it that does the work
//!
//! > Holding only the CI workload identity, sutura exchanges via WIF/STS for a principal, and
//! > `BigQuery` executes the query **as that principal**. The two principals differ.
//!
//! The word is **exchanges**. `crates/sutura-exec-bigquery/tests/two_principals.rs` already asks a
//! real dataset to enforce a row grant per principal, and it does so by holding each principal's own
//! *service-account key*: that is credential SELECTION, and this repository's own map refuses to let
//! it be cited as impersonation - *never label static-credential acceptance as proof of different
//! authorized rows*. Nothing here holds a principal's key. Each leg presents a bearer that came out
//! of an RFC 8693 exchange at Google's Security Token Service, through the composition
//! `sutura-serve` ships: `WorkloadIdentityBroker` over `StsOverHttp`, into a `BigQueryWarehouse`
//! opened `impersonation-at-source`.
//!
//! # The observable, and why the identity rather than the rows
//!
//! `SESSION_USER()`, through `BigQueryWarehouse::session_user`. It reads the identity **without
//! changing it**, it is the identity the SOURCE believes it is executing as, and it needs no row
//! access policy, no seeded table and no expected row set - so this cell is pointed at none of the
//! five values the two-principal cell waits on. Whether `BigQuery` then filters rows correctly for
//! that identity is `BigQuery`'s guarantee and is deliberately out of scope: re-testing it would be
//! rebuilding `BigQuery`.
//!
//! # What a green run here would establish, and the four things it would not
//!
//! **Would:** that a deployment holding one workload identity can obtain, per subject, a credential
//! the data system resolves to a DIFFERENT principal - the half `two_principals.rs` cannot reach,
//! because nobody asked there and nothing was exchanged.
//!
//! **Would not:** anything about Postgres, Oracle or any other source - `BigQuery`'s adapter is the
//! only one that can carry a per-subject credential at all. Anything about row or column filtering,
//! by the decision above. That a *browser-facing* caller's token reaches the exchange, which is the
//! transport's half and is `sutura_http`'s
//! `the_shipped_exchanging_broker_exchanges_the_document_leg_one_verified`. And it would say nothing
//! about a claim it never makes: the two legs here are two subjects, and this cell reads only who
//! they became.
//!
//! # ONE LEG HAS RUN AND THE TWO THAT MATTER HAVE NOT, and two things stand between them and a run
//!
//! **Measured on 2026-09-06, from a developer machine against the acceptance project:**
//! `the_deployments_own_identity_is_neither_principal` passed. So `SESSION_USER()` is a statement
//! this endpoint accepts, the answer really is one row of one text cell, and
//! `BigQueryWarehouse::session_user` reads it - which is what makes the observable this cell rests
//! on a measured thing rather than a plausible one. **It establishes nothing about impersonation:**
//! that leg runs under the credential the transport already holds and asserts only that it is
//! neither principal, which is the control's whole job.
//!
//! `each_principal_is_who_this_source_says_it_is_executing_as` has NOT run and cannot be run today.
//! Both reasons were found by writing it, neither is a defect in it, and both are recorded here
//! rather than in a commit message because the next person to reach for this file needs them first.
//!
//! **1. The environment does not carry a subject assertion per principal.** Issue #376 reasons that
//! the claim needs only variables that already exist, and that is not so. A plain RFC 8693 exchange
//! yields exactly ONE identity per subject token - the identity of whoever the token's `sub` is - so
//! two principals need two subject tokens. One CI job holds one workload identity and can mint one
//! `sub`. `SUTURA_BQ_PRINCIPAL_A_ASSERTION` and `SUTURA_BQ_PRINCIPAL_B_ASSERTION` are the two values
//! this cell is pointed at, and they are not in the `bq-test` environment. It **fails** on their
//! absence rather than skipping, for `tests/support/mod.rs`'s reason: a leg that reports green
//! against nothing has told a reader the opposite of the truth.
//!
//! **2. The shipped exchange has no service-account impersonation hop, so it cannot answer an
//! account's email at all.** `wire::StsOverHttp` posts one token-exchange request and returns what
//! comes back, which for a workload-identity pool is a FEDERATED credential - Google resolves it to
//! a pool subject, not to a service account. Becoming a service account from a federated credential
//! is a second call this adapter does not make. So against the stack as provisioned, this cell's
//! `SESSION_USER()` would come back as a pool subject and the run would be RED - which is why
//! [`WhoAnswered::AFederatedPoolSubject`] is a named verdict rather than falling into
//! [`WhoAnswered::NeitherPrincipal`]. **A red run naming that is the finding**, and it is the shape
//! this cell is built to produce rather than a shape it hides.
//!
//! `docs/where-identity-is-proven.md` carries both beside the venue's row, in the words that page
//! uses for *written and never run*.
//!
//! # Why nothing here prints an identity
//!
//! This cell's venue is a CI job whose log is public on a public repository, and the one thing
//! `SESSION_USER()` returns is a cloud account identifier. So no assertion in this file interpolates
//! what came back: every leg is judged by [`who_answered`], which reduces an answer to one of four
//! verdicts carrying no text, and a test below holds that property against the verdicts themselves.
//! `BigQueryError::NoIdentityInTheAnswer` is the same decision one layer down.
//!
//! # How to run it
//!
//! ```text
//! just bigquery-exchanged-identity
//! ```
//!
//! Its own task and its own nix app, for the reason `two_principals.rs` has its own: it needs values
//! the other legs do not, and one task demanding all of them would make the legs somebody CAN run
//! unreachable. It is deliberately **not** wired into `.github/workflows/ci.yml` yet - the two
//! assertion values do not exist, so wiring it would make `bigquery-acceptance` red on every push,
//! which is the mistake telekom/sutura#287 is held in draft to avoid.
//!
//! | Variable | What it names |
//! | --- | --- |
//! | `SUTURA_BQ_WORKLOAD_AUDIENCE` | the workload-identity provider a subject's token is exchanged against |
//! | `SUTURA_BQ_PRINCIPAL_A_EMAIL` | the account principal A's exchanged credential must resolve to |
//! | `SUTURA_BQ_PRINCIPAL_B_EMAIL` | the same for principal B |
//! | `SUTURA_BQ_PRINCIPAL_A_ASSERTION` | principal A's own subject token - **absent today, see above** |
//! | `SUTURA_BQ_PRINCIPAL_B_ASSERTION` | the same for principal B - **absent today** |
//!
//! plus whatever `tests/support/mod.rs` reads: the CI credential and the dataset, which are the
//! deployment's own identity here and the subject of the control leg.
//!
//! **Their names are here and their values are not, and will not be.**
//!
//! The scope is a constant rather than a sixth variable: it is Google's own published OAuth scope
//! string, so it is neither a resource identifier nor a thing a deployment chooses differently.

// `required-features = ["wire"]` is declared on the target in `Cargo.toml`, for the reason
// `tests/acceptance.rs` states: cargo skips the target rather than compiling an empty binary.

// `cfg(test)` around the whole file - the house pattern, because clippy honours
// `allow-expect-in-tests` only inside a `#[cfg(test)]` item.
#[cfg(test)]
mod support;

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{
        Agreed, CredentialBroker as _, PrincipalChain, RequestContext, Secret, SourceSet, Subject, SubjectId,
    };
    use sutura_domain::model::SourceName;
    use sutura_domain::source::SourcePosture;
    use sutura_exec_bigquery::wire::{StsOverHttp, WireAgent};
    use sutura_exec_bigquery::{WorkloadIdentity, WorkloadIdentityBroker};

    use crate::support::{Connection, bounds, named, opened, opened_as, presented};

    /// The scope the exchanged credential is minted for.
    ///
    /// A constant rather than a variable: it is Google's own published OAuth scope for its cloud
    /// APIs, so it names no resource of anybody's and a deployment does not choose a different one
    /// to read a dataset. A sixth environment value here would be a sixth thing to get wrong.
    const CLOUD_PLATFORM: &str = "https://www.googleapis.com/auth/cloud-platform";

    /// The prefix Google's own identifiers for a federated principal begin with.
    ///
    /// Both spellings, because a pool grants membership either way and the answer this cell would
    /// get from the stack as provisioned is one of them - see the header's second finding.
    const FEDERATED: [&str; 2] = ["principal://", "principalset://"];

    /// The source this cell opens.
    ///
    /// The same name the other two legs use, deliberately: what separates this cell from them is the
    /// credential path, not the source, and a fourth name here would suggest otherwise.
    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a source name is a source name")
    }

    /// Which of the identities this cell knows about answered, carrying none of them.
    ///
    /// **A verdict rather than the answer, and the reason is the log.** This cell's venue is a CI
    /// job on a public repository, so an assertion that failed with the observed value in its
    /// message would put a cloud account identifier into a public log - exactly the class
    /// `AGENTS.md` says not to disclose, arriving through a test that was trying to be helpful.
    /// Every variant below is a fixed word, and `a_verdict_carries_no_identity_text` is what holds
    /// that as a property rather than as care.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum WhoAnswered {
        /// The account this leg's exchange was supposed to become.
        TheExpectedPrincipal,
        /// The OTHER principal's account. Named separately because it is the failure that would
        /// otherwise read as *some stranger answered*: two legs whose expectations are crossed pass
        /// every *not empty* and *they differ* check while proving the opposite.
        TheOtherPrincipal,
        /// A workload-identity POOL SUBJECT rather than a service account - what a plain RFC 8693
        /// exchange resolves to, and therefore the answer the shipped composition would produce
        /// against the stack as it stands. Its own verdict so the run that gets it says which
        /// missing hop it is missing, rather than *somebody unknown*.
        AFederatedPoolSubject,
        /// Anything else, the deployment's own identity included - which is what an exchange that
        /// silently fell back to the transport's credential would answer.
        NeitherPrincipal,
    }

    /// Reduces one `SESSION_USER()` answer to a verdict.
    ///
    /// Case-insensitive because an account identifier is, and trimmed because a cell arrives as the
    /// endpoint spelled it. Nothing here is a substring test on the expected values: an identifier
    /// that merely CONTAINS the expected one is a different account.
    fn who_answered(observed: &str, expected: &str, the_other: &str) -> WhoAnswered {
        let observed = observed.trim();
        if observed.eq_ignore_ascii_case(expected.trim()) {
            return WhoAnswered::TheExpectedPrincipal;
        }
        if observed.eq_ignore_ascii_case(the_other.trim()) {
            return WhoAnswered::TheOtherPrincipal;
        }
        let lowered = observed.to_lowercase();
        if FEDERATED.iter().any(|prefix| lowered.starts_with(prefix)) {
            return WhoAnswered::AFederatedPoolSubject;
        }
        WhoAnswered::NeitherPrincipal
    }

    /// Did the two legs come back as one identity?
    ///
    /// **The fallback this cell exists to catch**, and it is a question of its own rather than an
    /// `assert_ne!` at the call site for two reasons. It is testable without a project, which is
    /// what makes *the fallback is caught* something shown rather than asserted; and it must
    /// compare without printing, so the call site never holds the two values in a message.
    fn both_legs_answered_one_identity(from_a: &str, from_b: &str) -> bool {
        from_a.trim().eq_ignore_ascii_case(from_b.trim())
    }

    /// Do the two expected accounts differ?
    ///
    /// A control on the CONFIGURATION rather than on the endpoint, in `two_principals.rs`'s shape
    /// and for its reason: two environment variables pointing at one account is one character in a
    /// workflow, and it would make [`who_answered`]'s first two arms unable to disagree - every
    /// answer would be `TheExpectedPrincipal` for both legs and the cell would report the strongest
    /// possible pass over a fixture that proves nothing.
    fn two_expectations_that_differ(expected_a: &str, expected_b: &str) -> bool {
        !expected_a.trim().eq_ignore_ascii_case(expected_b.trim()) && !expected_a.trim().is_empty()
    }

    // --------------------------------------------------------------- controls on this harness ---
    //
    // NOT `#[ignore]`d: they read no environment and open no socket, so they run in `just test` and
    // in `checks.nextest`, where a defect in this file's own judgement should fail on every change
    // rather than waiting for a venue that has never run.

    #[test]
    fn the_expected_principal_is_the_only_answer_this_cell_passes() {
        assert_eq!(
            who_answered(
                "principal-a@example.com",
                "principal-a@example.com",
                "principal-b@example.com"
            ),
            WhoAnswered::TheExpectedPrincipal
        );
        // The endpoint's own spelling is not this cell's to insist on.
        assert_eq!(
            who_answered(
                "  Principal-A@Example.com ",
                "principal-a@example.com",
                "principal-b@example.com"
            ),
            WhoAnswered::TheExpectedPrincipal
        );
    }

    #[test]
    fn the_two_principals_crossed_is_named_rather_than_read_as_a_stranger() {
        // **The equality that would hold for the wrong reason.** Two legs whose exchanges each
        // produced the OTHER principal satisfy *neither is empty* and *the two differ* - so a cell
        // asserting only those would report the strongest possible pass over exactly the failure it
        // exists to find. This is the verdict that makes the crossed case a distinguishable red.
        assert_eq!(
            who_answered(
                "principal-b@example.com",
                "principal-a@example.com",
                "principal-b@example.com"
            ),
            WhoAnswered::TheOtherPrincipal
        );
    }

    #[test]
    fn a_federated_pool_subject_is_not_a_service_account() {
        // The answer the SHIPPED composition would get against the stack as provisioned - the
        // header's second finding, as a verdict rather than a paragraph. A plain RFC 8693 exchange
        // resolves to a pool subject; becoming a service account is a second call this adapter does
        // not make. A run that gets this is red, and says which hop is missing.
        for observed in [
            "principal://iam.googleapis.com/projects/0/locations/global/workloadIdentityPools/p/subject/s",
            "principalSet://iam.googleapis.com/projects/0/locations/global/workloadIdentityPools/p/*",
        ] {
            assert_eq!(
                who_answered(observed, "principal-a@example.com", "principal-b@example.com"),
                WhoAnswered::AFederatedPoolSubject,
                "{observed}"
            );
        }
    }

    #[test]
    fn an_identity_this_cell_knows_nothing_about_is_never_a_pass() {
        // The deployment's own identity is the important member of this class: an exchange that
        // silently fell back to the credential the transport holds answers exactly this, and the
        // cell has to be red rather than merely unsurprised.
        assert_eq!(
            who_answered("ci@example.com", "principal-a@example.com", "principal-b@example.com"),
            WhoAnswered::NeitherPrincipal
        );
        // A near miss is a different account, not a partial match.
        assert_eq!(
            who_answered(
                "principal-a@example.com.attacker.example",
                "principal-a@example.com",
                "principal-b@example.com"
            ),
            WhoAnswered::NeitherPrincipal
        );
    }

    #[test]
    fn a_verdict_carries_no_identity_text() {
        // The property the whole enum exists for, held rather than remembered: this cell's venue is
        // a public log, so what a failing assertion prints must be a fixed word. Every verdict is
        // rendered and searched for each input, so a variant added later that carried the answer -
        // `NeitherPrincipal { observed }`, say - fails here rather than in a public log.
        let inputs = ["principal-a@example.com", "principal-b@example.com", "ci@example.com"];
        for observed in inputs {
            let rendered = format!("{:?}", who_answered(observed, inputs[0], inputs[1]));
            for identifier in inputs {
                assert!(!rendered.contains(identifier), "{rendered} carries {identifier}");
            }
        }
    }

    #[test]
    fn two_legs_that_answered_one_identity_are_caught_whatever_that_identity_is() {
        // **The fallback, shown rather than asserted.** If the exchange silently produced the same
        // credential twice - the transport's own, or one principal's for both legs - the two
        // answers are equal, and this is what the cell reads to say so. Held for an identity this
        // cell recognises and one it does not, because the fallback does not care.
        assert!(both_legs_answered_one_identity("ci@example.com", "ci@example.com"));
        assert!(both_legs_answered_one_identity(
            "principal-a@example.com",
            " Principal-A@example.com "
        ));
        assert!(!both_legs_answered_one_identity(
            "principal-a@example.com",
            "principal-b@example.com"
        ));
    }

    #[test]
    fn two_expectations_that_are_one_account_are_refused_before_a_socket_is_opened() {
        // The fixture control. One account named twice is one character in a workflow, and it would
        // make the first two arms of `who_answered` unable to disagree - both legs would answer
        // `TheExpectedPrincipal` whatever the exchange did.
        assert!(two_expectations_that_differ(
            "principal-a@example.com",
            "principal-b@example.com"
        ));
        assert!(!two_expectations_that_differ(
            "principal-a@example.com",
            " Principal-A@example.com "
        ));
        assert!(!two_expectations_that_differ("", ""));
    }

    // ------------------------------------------------------------------------------ the cell ---

    #[test]
    #[ignore = "needs the bq-test environment: a workload-identity provider, a subject assertion per principal, and the account each must resolve to"]
    fn each_principal_is_who_this_source_says_it_is_executing_as() {
        let audience = named(
            "SUTURA_BQ_WORKLOAD_AUDIENCE",
            "the workload-identity provider a subject's token is exchanged against",
        );
        let expected_a = named("SUTURA_BQ_PRINCIPAL_A_EMAIL", "the account principal A must resolve to");
        let expected_b = named("SUTURA_BQ_PRINCIPAL_B_EMAIL", "the account principal B must resolve to");
        let assertion_a = named(
            "SUTURA_BQ_PRINCIPAL_A_ASSERTION",
            "principal A's own subject token, which this leg exchanges and never holds a key for",
        );
        let assertion_b = named(
            "SUTURA_BQ_PRINCIPAL_B_ASSERTION",
            "principal B's own subject token, which this leg exchanges and never holds a key for",
        );
        // The configuration control, before anything is opened. No value is printed: the two are
        // compared and the message names the variables rather than what they hold.
        assert!(
            two_expectations_that_differ(&expected_a, &expected_b),
            "SUTURA_BQ_PRINCIPAL_A_EMAIL and SUTURA_BQ_PRINCIPAL_B_EMAIL are one account - the two \
             legs below could not disagree, and this cell would report its strongest pass over a \
             fixture that proves nothing"
        );
        // **The two assertions are deliberately NOT compared to each other here.** They are
        // credential material, and `==` on credential material is the timing oracle
        // `sutura_domain::identity::Secret` has no comparison for. It costs nothing: one token used
        // twice produces two equal answers, which the last assertion in this test already refuses.

        let bounds = bounds();
        // The running composition, exactly as `sutura-serve`'s `broker::build_broker` and
        // `build_bigquery` assemble it: one pinned agent behind the exchange and the wire, the
        // broker exchanging each subject's own token, and the adapter opened under the posture that
        // accepts the exchanged token as the job's bearer. **No principal's key is anywhere in
        // this**, which is the whole difference from `two_principals.rs`.
        let broker = WorkloadIdentityBroker::empty(StsOverHttp::new(WireAgent::pinned(bounds)))
            .with_floor(30)
            .impersonating(source(), WorkloadIdentity::of(audience, String::from(CLOUD_PLATFORM)));
        let warehouse = opened_as(source(), SourcePosture::ImpersonationAtSource, Connection::required(), bounds);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock that reads the present")
            .as_secs();

        let asked_as = |assertion: &str, subject: &str| -> String {
            let chain = |id: &str| {
                PrincipalChain::of(Subject::Verified {
                    id: SubjectId::parse(id).expect("a subject id parses"),
                })
            };
            let context = RequestContext::with_assertion(chain(subject), Secret::new(String::from(assertion)));
            let minted = broker
                .mint(&context, &SourceSet::of(source()))
                .expect("the exchange answered - an error here is the provider refusing, not a grant");
            // The agreement check the served path makes, rather than reading the grant straight out
            // of the mint: it is where an exchanged credential already inside its own deadline is
            // refused, and a cell that skipped it would be evidence for a path nobody runs.
            let agreed = minted
                .agreeing_with(chain(subject).subject(), &SourceSet::of(source()), now)
                .expect("the grant agrees with the request");
            let Agreed::Granted { credentials } = agreed else {
                panic!("an impersonating source with an assertion is granted");
            };
            let presented = credentials.presented_for(&source()).expect("a leg");
            warehouse
                .session_user(presented)
                .expect("the endpoint answered the identity read")
        };

        let from_a = asked_as(&assertion_a, "principal-a@example.com");
        let from_b = asked_as(&assertion_b, "principal-b@example.com");

        // **The claim.** Each leg became the account its own exchange was for - not the other's,
        // not a pool subject, not the deployment's. The verdict is what is printed; the answer is
        // not, because this log is public.
        assert_eq!(
            who_answered(&from_a, &expected_a, &expected_b),
            WhoAnswered::TheExpectedPrincipal,
            "principal A's leg did not execute as principal A"
        );
        assert_eq!(
            who_answered(&from_b, &expected_b, &expected_a),
            WhoAnswered::TheExpectedPrincipal,
            "principal B's leg did not execute as principal B"
        );
        // And the two are two. Equal answers are the fallback - one credential serving both legs -
        // which the two assertions above cannot catch on their own if the expectations were crossed
        // in the environment rather than in the exchange.
        assert!(
            !both_legs_answered_one_identity(&from_a, &from_b),
            "both legs executed as one identity, so nothing was exchanged per subject"
        );
    }

    #[test]
    #[ignore = "needs the bq-test environment: the CI credential and the two accounts an exchange must not answer"]
    fn the_deployments_own_identity_is_neither_principal() {
        // **The control without which the cell above is satisfied by a coincidence.** The same
        // identity read, under the credential the transport itself holds - the CI workload identity,
        // which is neither principal. If the exchanged bearers above were really the transport's,
        // this leg would answer what they answered.
        //
        // It runs even where the two assertion values are absent, because it needs neither: what it
        // is pointed at is the deployment's own credential. That makes it the one leg of this venue
        // that a CI job could run today, and its verdict is still not evidence for the claim - it
        // is evidence that the claim's control is live.
        let expected_a = named("SUTURA_BQ_PRINCIPAL_A_EMAIL", "the account principal A must resolve to");
        let expected_b = named("SUTURA_BQ_PRINCIPAL_B_EMAIL", "the account principal B must resolve to");
        let warehouse = opened(source(), Connection::required(), bounds());
        let observed = warehouse
            .session_user(&presented())
            .expect("the endpoint answered the identity read under the deployment's own credential");
        assert_eq!(
            who_answered(&observed, &expected_a, &expected_b),
            WhoAnswered::NeitherPrincipal,
            "the deployment's own credential resolves to one of the two principals, so this cell \
             has no control: an exchange that did nothing at all would pass it"
        );
    }
}
