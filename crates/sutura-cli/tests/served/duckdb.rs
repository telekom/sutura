//! The served `kind: duckdb` cell (`telekom/sutura#1292`): the example written into a local database
//! file, opened read-only by the composed binary, answering a certified question and a raw statement
//! over HTTP - the zero-infrastructure source `run_sql` has.
//!
//! **What this proves about the composition root**: a single-kind `duckdb` deployment keeps its
//! concrete adapter, so `run_sql` reads `DuckDbWarehouse::ACCEPTS_RAW_STATEMENTS` and not the
//! conservative default `crate::serve::kind::AnyWarehouse` carries. What the raw path refuses is
//! measured against the driver in `crates/sutura-exec-duckdb/tests/raw.rs`; this cell holds that a
//! served write is refused and leaves the file's table in place.
//!
//! **The identity limit, same as every other served cell in this suite**: one process holds the file
//! under its own operating-system identity, `shared-service-user`.

#[cfg(unix)]
#[cfg(test)]
#[cfg(feature = "duckdb")]
mod tests {
    use std::path::PathBuf;

    use sutura_domain::model::TableName;

    use crate::harness::{
        LOCAL_SOURCE, LOOPBACK, SINGLE_USER, TOKEN, config_path, derived_beside, example_root, recurring_revenue_june,
        settings_over, start_configured, v1,
    };

    /// The example's data directory written into one database, a table per CSV, beside the case's own
    /// configuration directory so `Served`'s `Drop` removes it.
    fn database(case: &str) -> PathBuf {
        let dir = derived_beside(&config_path(case));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the database directory is created");
        let tables: Vec<(TableName, PathBuf)> = std::fs::read_dir(example_root().join("data"))
            .expect("the example data directory is readable")
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "csv"))
            .map(|path| {
                let stem = path.file_stem().and_then(|stem| stem.to_str()).expect("a CSV has a name");
                (TableName::parse(stem).expect("a CSV is named for its table"), path.clone())
            })
            .collect();
        let file = dir.join("warehouse.duckdb");
        sutura_exec_duckdb::write_database(&file, &tables).expect("the example database is written");
        file
    }

    fn settings(case: &str) -> String {
        let source = format!(
            "  {LOCAL_SOURCE}:\n    kind: \"duckdb\"\n    database_file: \"{}\"\n    posture: \"shared-service-user\"\n",
            database(case).display()
        );
        format!(
            "{}tools:\n  run_sql:\n    enabled: true\n",
            settings_over(
                &example_root().join("catalog"),
                &example_root().join("data"),
                LOOPBACK,
                &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
                &source,
            )
        )
    }

    const SEGMENTS: &str = "select segment, count(*) as customers from dim_customer group by segment order by segment";

    #[test]
    fn a_duckdb_file_answers_a_certified_question_and_a_raw_statement_from_the_served_binary() {
        let served = start_configured("duckdb-run-sql", &settings("duckdb-run-sql"));

        // The anchors re-ran against the file at boot, so the certified figure is the example's own.
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_june(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        assert_eq!(
            reply.json()["rows"],
            serde_json::json!([["2026-06-01", "202121"]]),
            "{}",
            reply.body
        );

        let raw = |statement: &str| {
            served.post(
                &v1(sutura_http::constants::base_paths::RUN_SQL),
                Some(TOKEN),
                &serde_json::json!({ "statement": statement }).to_string(),
            )
        };
        let segments = raw(SEGMENTS);
        assert_eq!(segments.status, 200, "{}", segments.body);
        let body = segments.json();
        assert_eq!(body["outcome"], "raw_rows", "{}", segments.body);
        assert_eq!(
            body["rows"],
            serde_json::json!([["business", "12"], ["consumer", "27"], ["wholesale", "1"]]),
            "{}",
            segments.body
        );

        // A write is refused, and the table it named is still there and unchanged afterwards.
        let dropped = raw("drop table dim_customer");
        assert_eq!(dropped.status, 422, "{}", dropped.body);
        assert_eq!(dropped.json()["outcome"], "raw_refusal", "{}", dropped.body);
        assert_eq!(raw(SEGMENTS).json()["rows"], body["rows"]);
    }
}
