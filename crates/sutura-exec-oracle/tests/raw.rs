#![cfg(feature = "fixtures")]
#![forbid(unsafe_code)]

//! Shared-user raw queries at the real Oracle venue. No offline cell executes these statements.

#[cfg(test)]
mod acceptance {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use sutura_conformance::corpus;
    use sutura_dev::provisioned;
    use sutura_domain::identity::Presented;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_domain::warehouse::{RawRows, ResultBudget, Value, Warehouse as _};
    use sutura_exec_oracle::{Channel, Dial, OracleError, OracleWarehouse};

    struct Tier {
        host: String,
        port: u16,
        service: String,
        user: String,
        password: String,
        anchors: Option<String>,
    }

    impl Tier {
        fn here() -> Self {
            if let Ok(mode) = std::env::var("SUTURA_ORACLE_ACCEPTANCE_MODE") {
                let required = |name| std::env::var(name).expect("the native acceptance task sets its fixture");
                Self {
                    host: String::from("127.0.0.1"),
                    port: required("SUTURA_ORACLE_ACCEPTANCE_PORT").parse().expect("a fixture port"),
                    service: required("SUTURA_ORACLE_ACCEPTANCE_SERVICE"),
                    user: required("SUTURA_ORACLE_ACCEPTANCE_USER"),
                    password: required("SUTURA_ORACLE_ACCEPTANCE_PASSWORD"),
                    anchors: (mode == "verified").then(|| {
                        std::fs::read_to_string(required("SUTURA_ORACLE_ACCEPTANCE_CA"))
                            .expect("the verified fixture CA reads")
                    }),
                }
            } else {
                let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
                    .endpoint()
                    .cloned()
                    .expect("the Oracle acceptance task requires its tier");
                Self {
                    host: String::from(endpoint.host()),
                    port: endpoint.port(),
                    service: String::from("FREEPDB1"),
                    user: std::env::var("SUTURA_DEV_USER").expect("the fixture user is set"),
                    password: std::env::var("SUTURA_DEV_PASSWORD").expect("the fixture password is set"),
                    anchors: Some(
                        std::fs::read_to_string(std::env::var("SUTURA_ORACLE_TIER_CA").expect("the fixture CA is set"))
                            .expect("the fixture CA reads"),
                    ),
                }
            }
        }

        fn adapter(&self, bytes: usize) -> OracleWarehouse {
            let channel = self
                .anchors
                .as_ref()
                .map_or(Channel::NativeEncryption, |anchors| Channel::Verified {
                    anchors_pem: anchors,
                });
            OracleWarehouse::connect(
                corpus::source(),
                corpus::posture(),
                Dial::new(&self.host, self.port, &self.service, channel),
                &self.user,
                &self.password,
                ResultBudget::of_bytes(core::num::NonZeroUsize::new(bytes).expect("a positive fixture budget")),
            )
            .expect("the adapter opens the fixture")
        }
    }

    #[expect(clippy::unwrap_in_result, reason = "the fixture helper returns execution errors only")]
    fn run(warehouse: &OracleWarehouse, sql: &str) -> Result<RawRows, OracleError> {
        warehouse
            .execute_raw(
                &RawStatement::parse(sql).expect("a bounded fixture statement"),
                &corpus::presented(),
                corpus::deadline(),
            )
            .expect("a shared Oracle source accepts raw statements")
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn oracle_raw_select_and_with_keep_exact_numbers_and_dates() {
        let warehouse = Tier::here().adapter(1 << 20);
        assert!(warehouse.accepts_raw_statements());
        let rows = run(
            &warehouse,
            "WITH reading AS (SELECT CAST(9007199254740993 AS NUMBER) N, \
             TO_DATE('2026-10-11 13:45:30', 'YYYY-MM-DD HH24:MI:SS') D, \
             TO_TIMESTAMP_TZ('2026-10-11 13:45:30 +02:00', 'YYYY-MM-DD HH24:MI:SS TZH:TZM') T FROM dual) \
             SELECT N, D, T FROM reading",
        )
        .expect("one query answers");
        assert_eq!(rows.columns(), ["N", "D", "T"]);
        assert_eq!(
            rows.rows(),
            [vec![
                Value::Integer(0x0020_0000_0000_0001),
                Value::Text(String::from("2026-10-11T13:45:30")),
                Value::Text(String::from("2026-10-11T11:45:30")),
            ]]
        );
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn oracle_raw_nonqueries_and_delimiters_are_refused_before_execution() {
        let warehouse = Tier::here().adapter(1 << 20);
        for sql in [
            "CREATE TABLE raw_forbidden (n NUMBER)",
            "INSERT INTO raw_forbidden VALUES (1)",
            "UPDATE raw_forbidden SET n = 2",
            "DELETE FROM raw_forbidden",
            "BEGIN NULL; END;",
            "DECLARE n NUMBER; BEGIN NULL; END;",
            "CALL raw_forbidden()",
            "ALTER SESSION SET NLS_DATE_FORMAT = 'YYYY'",
            "SET TRANSACTION READ WRITE",
            "COMMIT",
            "ROLLBACK",
            "SELECT 1 FROM dual;",
            "SELECT 1 FROM dual /",
            "SELECT 1 FROM dual; SELECT 2 FROM dual",
            "WITH FUNCTION attempt RETURN NUMBER IS BEGIN EXECUTE IMMEDIATE 'CREATE TABLE raw_forbidden (n NUMBER)'; RETURN 1; END; SELECT attempt FROM dual",
        ] {
            let error = run(&warehouse, sql).expect_err("the raw statement is refused");
            assert!(matches!(error, OracleError::RawNotQuery), "{error:?}");
        }
        let rows = run(
            &warehouse,
            "SELECT COUNT(*) N FROM user_tables WHERE table_name = 'RAW_FORBIDDEN'",
        )
        .expect("the DDL never created a table");
        assert_eq!(rows.rows(), [vec![Value::Integer(0)]]);
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn oracle_raw_lob_long_and_raw_values_are_refused_without_materialising_them() {
        let warehouse = Tier::here().adapter(1 << 20);
        for sql in [
            "SELECT TO_CLOB(RPAD('x', 4000, 'x')) PAYLOAD FROM dual",
            "SELECT TO_NCLOB(RPAD('x', 4000, 'x')) PAYLOAD FROM dual",
            "SELECT HEXTORAW('CAFE') PAYLOAD FROM dual",
            "SELECT text FROM user_views WHERE ROWNUM <= 1",
        ] {
            let error = run(&warehouse, sql).expect_err("the column has no bounded raw mapping");
            assert!(matches!(error, OracleError::UnsupportedType { .. }), "{error:?}");
        }
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn oracle_raw_retains_the_cap_plus_one_refusal_witness() {
        let warehouse = Tier::here().adapter(16 << 20);
        let cap = sutura_domain::plan::MAX_ROWS;
        let sql = format!("SELECT LEVEL N FROM dual CONNECT BY LEVEL <= {}", cap + 2);
        let rows = run(&warehouse, &sql).expect("the application receives its overflow witness");
        assert_eq!(rows.rows().len(), cap as usize + 1);
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn oracle_raw_wide_metadata_and_collected_bytes_are_refused() {
        let warehouse = Tier::here().adapter(4096);
        let error = run(&warehouse, "SELECT CAST('x' AS VARCHAR2(4000)) PAYLOAD FROM dual")
            .expect_err("declared width is refused before fetching");
        assert!(warehouse.result_did_not_fit(&error), "{error:?}");
        let error = run(
            &warehouse,
            "SELECT CAST(RPAD('x', 128, 'x') AS VARCHAR2(128)) PAYLOAD FROM dual CONNECT BY LEVEL <= 100",
        )
        .expect_err("collected rows exceed the materialisation budget");
        assert!(warehouse.result_did_not_fit(&error), "{error:?}");
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn oracle_raw_spent_deadlines_and_other_credentials_never_run() {
        let warehouse = Tier::here().adapter(1 << 20);
        let statement = RawStatement::parse("SELECT 1 FROM dual").expect("a fixture statement");
        let spent = Deadline::opened_at(
            Instant::now()
                .checked_sub(Duration::from_secs(60))
                .expect("a minute before now"),
            Budget::parse(Duration::from_millis(1)).expect("a positive budget"),
        );
        let error = warehouse
            .execute_raw(&statement, &corpus::presented(), spent)
            .expect("the source accepts raw text")
            .expect_err("the spent deadline is refused");
        assert!(warehouse.deadline_exceeded(&error));
        let token = Presented::SubjectToken {
            material: sutura_domain::identity::Secret::new("fixture-token"),
        };
        let error = warehouse
            .execute_raw(&statement, &token, corpus::deadline())
            .expect("the source accepts raw text")
            .expect_err("a shared source refuses a token");
        assert!(matches!(error, OracleError::Undeliverable { .. }), "{error:?}");
    }
}
