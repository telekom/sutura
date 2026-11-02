#![forbid(unsafe_code)]
//! A stream refused at the transport's row ceiling or byte budget is a result that did not fit, so
//! a federation leg past either is refused as too large rather than left as the retryable `503` a
//! transport failure becomes.
//!
//! Each error is built through the transport's own conversion from the `AdbcError` its stream reader
//! returns; `src/adbc/tests.rs` holds that a stream past the ceiling IS that error. No database tier:
//! the predicate reads the error alone. What this does not reach is the service turning `true` into
//! the caller's refusal - `sutura-app`'s
//! `a_federated_leg_that_hits_the_volume_bound_is_refused_not_a_503` holds that half.

#[cfg(test)]
mod result_bound {
    use sutura_conformance::corpus;
    use sutura_domain::identity::Secret;
    use sutura_domain::warehouse::{UnannouncedBatch, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcError, AdbcPostgres, Channel, Conninfo, PostgresDriver};
    use sutura_exec_postgres::connection::ConnectionTarget;

    fn warehouse() -> AdbcPostgres {
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
        AdbcPostgres::new(corpus::source(), corpus::posture(), driver, conninfo).expect("the default ceiling parses")
    }

    fn refused(cause: UnannouncedBatch) -> PostgresError {
        PostgresError::from(AdbcError::Unannounced(cause))
    }

    #[test]
    fn the_row_ceiling_is_a_result_too_large_and_a_mis_width_stream_is_not() {
        let warehouse = warehouse();
        let over = refused(UnannouncedBatch::OverBound { most: 1_000_000 });
        assert!(warehouse.result_did_not_fit(&over), "{over:?}");
        let width = refused(UnannouncedBatch::Width {
            announced: 2,
            delivered: 3,
        });
        assert!(!warehouse.result_did_not_fit(&width), "{width:?}");
    }

    #[test]
    fn the_byte_budget_is_a_result_too_large_and_a_mislabelled_stream_is_not() {
        let warehouse = warehouse();
        let over = refused(UnannouncedBatch::OverBudget {
            most_bytes: 256 * 1024 * 1024,
        });
        assert!(warehouse.result_did_not_fit(&over), "{over:?}");
        let mislabelled = refused(UnannouncedBatch::Mislabelled {
            at: 0,
            announced: "n".to_owned(),
            delivered: "m".to_owned(),
        });
        assert!(!warehouse.result_did_not_fit(&mislabelled), "{mislabelled:?}");
    }
}
