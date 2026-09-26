#![cfg(feature = "fixtures")]
#![forbid(unsafe_code)]

//! The compose-only Oracle venue. `just oracle-acceptance` starts it and runs this ignored cell;
//! ordinary Nix checks have no Docker socket and do not execute an Oracle statement.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned;
    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_oracle::{OracleError, OracleWarehouse, fixture::FixtureCredential};
    use sutura_sql::{Dialect, generate};

    fn drop_fixture(connection: &oracledb::Connection, table: &str) {
        let statement = format!("DROP TABLE {table} PURGE");
        if let Err(cause) = connection.execute(&statement, &[]) {
            assert!(cause.to_string().contains("ORA-00942"), "fixture table did not drop: {cause}");
        }
    }

    /// The dictionary's spelling, not the spelling of the unquoted DDL, is the table name the
    /// quoted Oracle renderer must receive. The same live cell also runs a whole plan containing
    /// `FETCH FIRST`, beyond the render-only goldens.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_quoted_identifier_names_the_object_the_dictionary_stores() {
        let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
            .endpoint()
            .cloned()
            .expect("the named acceptance task requires an Oracle endpoint");
        let user = std::env::var("SUTURA_DEV_USER").expect("the acceptance task sets SUTURA_DEV_USER");
        let password = std::env::var("SUTURA_DEV_PASSWORD").expect("the acceptance task sets SUTURA_DEV_PASSWORD");
        let address = format!("{}:{}/FREEPDB1", endpoint.host(), endpoint.port());
        let config = oracledb::Config::default()
            .set_connect_string(&address)
            .expect("the discovered endpoint is an Oracle connect string")
            .set_credentials(&user, &password);
        let setup = oracledb::connect(config).expect("the provisioned Oracle accepts the fixture credential");
        let fixture = FixtureCredential::from_env().expect("the named task sets the fixture credential");
        let warehouse = OracleWarehouse::connect_fixture(
            corpus::source(),
            corpus::posture(),
            endpoint.host(),
            endpoint.port(),
            &fixture,
        )
        .expect("the adapter opens the provisioned Oracle");
        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "total-by-region-and-day")
            .expect("the committed corpus has the region-and-day case");
        let rendered = generate(case.plan(), Dialect::Oracle).expect("the whole plan renders for Oracle");
        assert!(
            rendered.sql().contains("FETCH FIRST"),
            "the live statement must use Oracle's limit syntax"
        );

        // ponytail: this fixture user belongs to one worktree; serialize this named task if concurrent
        // acceptance runs in one worktree become necessary.
        drop_fixture(&setup, "conformance_events");
        drop_fixture(&setup, "\"conformance_events\"");
        setup
            .execute(
                "CREATE TABLE conformance_events (\"day\" DATE, \"region\" VARCHAR2(32), \"amount_cents\" NUMBER(38))",
                &[],
            )
            .expect("unquoted fixture table creates");
        let stored: String = setup
            .query_row(
                "SELECT table_name FROM user_tables WHERE table_name = 'CONFORMANCE_EVENTS'",
                &[],
            )
            .expect("the dictionary records the unquoted table")
            .get(0)
            .expect("the dictionary returns its table name");
        assert_eq!(stored, "CONFORMANCE_EVENTS");
        let refused = warehouse.execute(Executable::Query(case.plan()), &corpus::presented(), corpus::deadline());
        assert!(
            matches!(refused, Err(OracleError::Execute { ref cause }) if cause.to_string().contains("ORA-00942")),
            "a quoted lowercase name cannot reach the dictionary's uppercase table: {refused:?}"
        );

        drop_fixture(&setup, "conformance_events");
        setup
            .execute(
                "CREATE TABLE \"conformance_events\" (\"day\" DATE, \"region\" VARCHAR2(32), \"amount_cents\" NUMBER(38))",
                &[],
            )
            .expect("quoted fixture table creates");
        let csv = std::fs::read_to_string(corpus::on_disk()).expect("the committed corpus reads");
        for line in csv.lines().skip(1) {
            let mut fields = line.split(',');
            let day = fields.next().expect("a corpus row has a day");
            let region = fields.next().expect("a corpus row has a region");
            let amount = fields.next().expect("a corpus row has an amount");
            setup
            .execute(
                "INSERT INTO \"conformance_events\" (\"day\", \"region\", \"amount_cents\") VALUES (TO_DATE(:1, 'YYYY-MM-DD'), :2, TO_NUMBER(:3))",
                &[&day, &region, &amount],
            )
            .expect("a committed corpus row inserts");
        }
        setup.commit().expect("the adapter can see the inserted fixture rows");
        let warehouse = OracleWarehouse::connect_fixture(
            corpus::source(),
            corpus::posture(),
            endpoint.host(),
            endpoint.port(),
            &fixture,
        )
        .expect("the adapter opens a fresh connection after the fixture table changed");
        let answered = warehouse
            .execute(Executable::Query(case.plan()), &corpus::presented(), corpus::deadline())
            .expect("Oracle accepts the rendered whole plan")
            .to_rows()
            .expect("Oracle's answer decodes to rows");
        assert_eq!(
            answered,
            *case.expected(),
            "the live answer must reproduce the committed corpus"
        );
        drop_fixture(&setup, "\"conformance_events\"");
    }
}
