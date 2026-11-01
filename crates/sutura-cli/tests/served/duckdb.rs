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
    use crate::harness::{TOKEN, raw_sql_settings, recurring_revenue_june, start_configured, v1};

    const SEGMENTS: &str = "select segment, count(*) as customers from dim_customer group by segment order by segment";

    #[test]
    fn a_duckdb_file_answers_a_certified_question_and_a_raw_statement_from_the_served_binary() {
        let served = start_configured("duckdb-run-sql", &raw_sql_settings("duckdb-run-sql"));

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

// `tools.run_sql.sources` and `federation_allowed` on the served binary, declared here because
// `served.rs` is at its line cap. `#[path]` resolves beside this file.
#[path = "run_sql_federation.rs"]
mod run_sql_federation;
