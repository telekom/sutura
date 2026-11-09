#![cfg(feature = "fixtures")]
#![forbid(unsafe_code)]
#![expect(
    clippy::disallowed_methods,
    reason = "a test dials the driver directly to observe the driver itself; test code does not ship"
)]

//! The importer against the compose-only Oracle venue: what a loaded fixture IS in the dictionary,
//! and what the adapter and the driver read back out of it. `just oracle-acceptance` and
//! `nix run .#oracle-acceptance` (the `oracle-tier` CI job) start the tier and run this ignored
//! cell; the nix checks `just validate` runs execute no Oracle statement.
//!
//! The expectations below are literals written from the fixture text, not read off the importer's
//! own parse, so a column typed or a cell bound the same wrong way by both halves still fails.

#[cfg(test)]
mod acceptance {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned;
    use sutura_domain::model::TableName;
    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::{ResultBudget, Warehouse as _};
    use sutura_exec_oracle::OracleWarehouse;
    use sutura_exec_oracle::fixture::{CellName, FixtureCredential};

    const TABLE: &str = "importer_types";

    /// One column of every classification, one row of values, one of the other spellings, one empty.
    const EVERY_TYPE: &str = "id,flag,small,wide,ratio,real,day,label\n\
        1,true,10,18446744073709551615,2.47,1e2,2026-01-01,alpha\n\
        2,FALSE,-20,0,0.5,-0.25,2026-02-28,beta\n\
        3,,,,,,,\n";

    const COLUMNS: [&str; 8] = ["id", "flag", "small", "wide", "ratio", "real", "day", "label"];

    /// What the dictionary says each column is: its name, its type, and a `NUMBER`'s precision and scale.
    const DECLARED: [&str; 8] = [
        "id NUMBER(19,0)",
        "flag BOOLEAN",
        "small NUMBER(19,0)",
        "wide NUMBER(38,0)",
        "ratio BINARY_DOUBLE",
        "real BINARY_DOUBLE",
        "day DATE",
        "label VARCHAR2",
    ];

    /// How many cells of each column are not `NULL`, in column order.
    const PRESENT: [i64; 8] = [3, 2, 2, 2, 2, 2, 2, 2];

    #[derive(Debug, PartialEq)]
    struct Loaded {
        id: Option<i64>,
        flag: Option<bool>,
        small: Option<i64>,
        wide: Option<u64>,
        ratio: Option<f64>,
        real: Option<f64>,
        day: Option<String>,
        label: Option<String>,
    }

    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn a_loaded_fixture_has_the_declared_types_its_null_cells_and_every_row_back() {
        let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
            .endpoint()
            .cloned()
            .expect("the named acceptance task requires an Oracle endpoint");
        let tier = FixtureCredential::from_env().expect("the named task sets the fixture credential");
        let cell = CellName::of(std::process::id(), 0);
        let own = tier
            .provision_cell(endpoint.host(), endpoint.port(), &cell)
            .expect("the administrator creates the cell's user");
        let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("a mebibyte is positive"));
        let warehouse = OracleWarehouse::connect_fixture(
            corpus::source(),
            corpus::posture(),
            endpoint.host(),
            endpoint.port(),
            &own,
            budget,
        )
        .expect("the adapter opens the cell's own user");

        let directory = std::env::temp_dir().join(format!("sutura-oracle-importer-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let fixture = directory.join("importer_types.csv");
        std::fs::write(&fixture, EVERY_TYPE).expect("the fixture writes");
        let table = TableName::parse(TABLE).expect("a table name");
        warehouse.load_csv(&table, &fixture).expect("the fixture loads");
        warehouse
            .load_csv(&table, &fixture)
            .expect("a second load replaces the table rather than refusing it");

        let anchors = std::fs::read_to_string(
            std::env::var("SUTURA_ORACLE_TIER_CA").expect("the acceptance task sets SUTURA_ORACLE_TIER_CA"),
        )
        .expect("the tier's certificate authority is readable");
        let password = std::env::var("SUTURA_DEV_PASSWORD").expect("the acceptance task sets SUTURA_DEV_PASSWORD");
        let config = oracledb::Config::default()
            .set_connect_string(&format!("tcps://{}:{}/FREEPDB1", endpoint.host(), endpoint.port()))
            .expect("the discovered endpoint is an Oracle connect string")
            .set_trust_anchors_pem(&anchors)
            .expect("the tier's certificate authority is an anchor")
            .set_credentials(cell.as_str(), &password);
        let reader = oracledb::connect(config).expect("the cell's own user logs in");

        let dictionary: Vec<String> = reader
            .query(
                "SELECT column_name || ' ' || data_type || \
                        CASE WHEN data_type = 'NUMBER' THEN '(' || data_precision || ',' || data_scale || ')' END \
                 FROM user_tab_columns WHERE table_name = :1 ORDER BY column_id",
                &[&TABLE],
            )
            .expect("the dictionary answers")
            .map(|row| row.expect("a dictionary row").get(0).expect("a column description"))
            .collect();
        assert_eq!(
            dictionary, DECLARED,
            "the dictionary must hold the types the importer declares"
        );

        let counts = COLUMNS
            .iter()
            .map(|name| format!("COUNT(\"{name}\")"))
            .collect::<Vec<_>>()
            .join(", ");
        let counted = reader
            .query_row(&format!("SELECT COUNT(*), {counts} FROM \"{TABLE}\""), &[])
            .expect("the counts answer");
        assert_eq!(
            counted.get::<i64>(0).expect("the row count"),
            3,
            "three data rows were loaded, once"
        );
        let present: Vec<i64> = (1..=COLUMNS.len())
            .map(|index| counted.get(index).expect("a column's non-null count"))
            .collect();
        assert_eq!(present, PRESENT, "the empty cells are NULL, and every other cell is present");

        let read: Vec<Loaded> = reader
            .query(
                &format!(
                    "SELECT \"id\", \"flag\", \"small\", \"wide\", \"ratio\", \"real\", TO_CHAR(\"day\", 'YYYY-MM-DD'), \
                     \"label\" FROM \"{TABLE}\" ORDER BY \"id\""
                ),
                &[],
            )
            .expect("the table reads")
            .map(|row| {
                let row = row.expect("a data row");
                Loaded {
                    id: row.get(0).expect("id"),
                    flag: row.get(1).expect("flag"),
                    small: row.get(2).expect("small"),
                    wide: row.get(3).expect("wide"),
                    ratio: row.get(4).expect("ratio"),
                    real: row.get(5).expect("real"),
                    day: row.get(6).expect("day"),
                    label: row.get(7).expect("label"),
                }
            })
            .collect();
        assert_eq!(
            read,
            vec![
                Loaded {
                    id: Some(1),
                    flag: Some(true),
                    small: Some(10),
                    wide: Some(u64::MAX),
                    ratio: Some(2.47),
                    real: Some(100.0),
                    day: Some(String::from("2026-01-01")),
                    label: Some(String::from("alpha")),
                },
                Loaded {
                    id: Some(2),
                    flag: Some(false),
                    small: Some(-20),
                    wide: Some(0),
                    ratio: Some(0.5),
                    real: Some(-0.25),
                    day: Some(String::from("2026-02-28")),
                    label: Some(String::from("beta")),
                },
                Loaded {
                    id: Some(3),
                    flag: None,
                    small: None,
                    wide: None,
                    ratio: None,
                    real: None,
                    day: None,
                    label: None,
                },
            ],
            "every loaded cell must read back as the fixture wrote it"
        );

        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "total-by-region-and-day")
            .expect("the committed corpus has the region-and-day case");
        warehouse
            .load_csv(&corpus::table(), &corpus::on_disk())
            .expect("the committed corpus loads");
        let answered = warehouse
            .execute(Executable::Query(case.plan()), &corpus::presented(), corpus::deadline())
            .expect("Oracle accepts the rendered whole plan over the loaded corpus")
            .to_rows()
            .expect("Oracle's answer decodes to rows");
        assert_eq!(
            answered,
            *case.expected(),
            "the corpus the importer loaded must answer what the committed corpus certifies"
        );
        drop(std::fs::remove_dir_all(&directory));
    }
}
