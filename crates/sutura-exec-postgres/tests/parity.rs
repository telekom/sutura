#![forbid(unsafe_code)]
//! One request, both transports, one refusal: each refusal a Postgres source makes before any SQL,
//! asserted alike through `PostgresWarehouse` and `AdbcPostgres`.
//!
//! The `tokio-postgres` side connects to `support`'s handshake-only server; the ADBC side names a
//! driver no path holds. So a guard either transport lost answers a closed connection or a load
//! failure instead of the refusal, and the last cell is that control for the ADBC side. What these
//! do not reach is an answer: `src/adbc/parity.rs` holds the two transports' values to each other.

#[cfg(test)]
#[path = "support/support.rs"]
mod support;

#[cfg(test)]
mod parity {
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::plan::Executable;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::Warehouse;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_exec_postgres::adbc::{AdbcError, AdbcPostgres, Channel, Conninfo, PostgresDriver};
    use sutura_exec_postgres::connection::ConnectionTarget;
    use sutura_exec_postgres::{PostgresError, PostgresWarehouse};

    fn tokio_under(posture: SourcePosture) -> PostgresWarehouse {
        PostgresWarehouse::connect(
            corpus::source(),
            posture,
            &super::support::config(super::support::fake_postgres()),
        )
        .expect("the fake server completes the handshake")
    }

    fn adbc_under(posture: SourcePosture) -> AdbcPostgres {
        let driver = PostgresDriver::parse("/nonexistent/libadbc_driver_postgresql.so").expect("an absolute path parses");
        let conninfo = Conninfo::new(
            &corpus::source(),
            ConnectionTarget::Host("127.0.0.1"),
            1,
            "sutura",
            "sutura",
            &Secret::new("unused"),
            Channel::Plaintext,
        )
        .expect("plaintext builds");
        AdbcPostgres::new(corpus::source(), posture, driver, conninfo).expect("the default ceiling parses")
    }

    fn subjects() -> [Presented; 2] {
        [
            Presented::SubjectToken {
                material: Secret::new("an-exchanged-token"),
                impersonate: None,
            },
            Presented::SubjectPrincipal {
                name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
            },
        ]
    }

    fn spent() -> Deadline {
        let opened = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("this host's clock is more than a minute past its epoch");
        Deadline::opened_at(
            opened,
            Budget::parse(Duration::from_secs(1)).expect("a positive budget parses"),
        )
    }

    /// The three port calls that take a request, as one refusal each - its variant, nothing else.
    fn refusals<W>(warehouse: &W, presented: &Presented, deadline: Deadline) -> [&'static str; 3]
    where
        W: Warehouse<Error = PostgresError>,
    {
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        let statement = RawStatement::parse("select 1").expect("a test statement is a statement");
        [
            kind(
                warehouse
                    .dry_run(Executable::Query(case.plan()), presented, deadline)
                    .err()
                    .as_ref(),
            ),
            kind(
                warehouse
                    .execute(Executable::Query(case.plan()), presented, deadline)
                    .err()
                    .as_ref(),
            ),
            kind(
                warehouse
                    .execute_raw(&statement, presented, deadline)
                    .and_then(Result::err)
                    .as_ref(),
            ),
        ]
    }

    fn kind(refused: Option<&PostgresError>) -> &'static str {
        match refused {
            None => "answered",
            Some(PostgresError::NoPlaceForASubject { .. }) => "no place for a subject",
            Some(PostgresError::PresentedDisagreesWithPosture { .. }) => "a disagreeing witness",
            Some(PostgresError::DeadlineSpent) => "the deadline spent",
            Some(_) => "something past the guards",
        }
    }

    #[test]
    fn a_subject_credential_is_refused_alike_by_both_transports() {
        let (tokio, adbc) = (tokio_under(corpus::posture()), adbc_under(corpus::posture()));
        for presented in subjects() {
            let expected = ["no place for a subject"; 3];
            assert_eq!(refusals(&tokio, &presented, corpus::deadline()), expected);
            assert_eq!(refusals(&adbc, &presented, corpus::deadline()), expected);
        }
    }

    #[test]
    fn a_disagreeing_witness_is_refused_alike_by_both_transports() {
        let other = || SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a different operator's acknowledgement").expect("a test reason is a reason"),
            ),
        };
        let expected = ["a disagreeing witness"; 3];
        assert_eq!(
            refusals(&tokio_under(other()), &corpus::presented(), corpus::deadline()),
            expected
        );
        assert_eq!(
            refusals(&adbc_under(other()), &corpus::presented(), corpus::deadline()),
            expected
        );
    }

    #[test]
    fn a_spent_deadline_is_refused_alike_by_both_transports_before_anything_is_sent() {
        let expected = ["the deadline spent"; 3];
        assert_eq!(
            refusals(&tokio_under(corpus::posture()), &corpus::presented(), spent()),
            expected
        );
        assert_eq!(
            refusals(&adbc_under(corpus::posture()), &corpus::presented(), spent()),
            expected
        );
    }

    #[test]
    fn a_request_every_guard_lets_through_gets_past_them_on_both_transports() {
        // The control the three cells above rest on: past the guards, the ADBC side fails at the
        // load and the `tokio-postgres` side at the fake's closed connection - so a refusal above
        // arrived INSTEAD of either.
        let past = ["something past the guards"; 3];
        assert_eq!(
            refusals(&tokio_under(corpus::posture()), &corpus::presented(), corpus::deadline()),
            past
        );
        let adbc = adbc_under(corpus::posture());
        assert_eq!(refusals(&adbc, &corpus::presented(), corpus::deadline()), past);
        let statement = RawStatement::parse("select 1").expect("a test statement is a statement");
        let reached = adbc.execute_raw(&statement, &corpus::presented(), corpus::deadline());
        assert!(
            matches!(
                reached,
                Some(Err(PostgresError::Adbc {
                    cause: AdbcError::Load(_)
                }))
            ),
            "{reached:?}"
        );
    }
}
