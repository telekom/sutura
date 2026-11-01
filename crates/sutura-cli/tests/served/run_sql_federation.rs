//! `tools.run_sql.sources` and `tools.run_sql.federation_allowed` on the composed binary: a listed
//! alias the process cannot run a statement at stops it, and one statement over a `duckdb` and a
//! `postgres` source is refused with the switch off and answered with it on - with the rows one
//! `duckdb` file holding both tables gives.
//!
//! Declared from `served/duckdb.rs`, because `served.rs` is at its line cap. Every cell names the
//! refusal and the answer by wire value only, so it compiles against a tree without the keys, where
//! the binary refuses them at startup and the cell is red by assertion.

#[cfg(test)]
mod tests {
    use sutura_config::Environment;

    use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, deployment, example_root, refused_to_start, written};

    /// The example's `files` deployment with the raw tool on over `listed`.
    fn refused_over(case: &str, listed: &str) -> Vec<String> {
        let settings = format!(
            "{}tools:\n  run_sql:\n    enabled: true\n    sources: [\"{listed}\"]\n",
            deployment(
                &example_root(),
                LOOPBACK,
                &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n")
            )
        );
        refused_to_start(Environment::Development, written(case, &settings), &[])
    }

    fn said(told: &[String], sentence: &str) {
        assert!(
            told.iter().any(|line| line.contains(sentence)),
            "the deployment did not tell the operator `{sentence}`:\n{}",
            told.join("\n")
        );
    }

    #[test]
    fn a_listed_source_whose_adapter_takes_no_raw_statement_stops_the_process() {
        said(
            &refused_over("run-sql-files", "local"),
            "tools.run_sql.sources lists `local`, whose adapter accepts no raw statement",
        );
    }

    #[test]
    fn a_listed_alias_no_open_source_carries_stops_the_process() {
        said(
            &refused_over("run-sql-ghost", "ghost"),
            "tools.run_sql.sources lists `ghost`, and no source of that alias is open",
        );
    }
}

#[cfg(test)]
#[cfg(feature = "postgres")]
mod across {
    use serde_json::json;
    use sutura_http::constants::base_paths::RUN_SQL;

    use crate::harness::{
        LOCAL_SOURCE, LOOPBACK, Reply, SINGLE_USER, Served, TOKEN, catalog_spanning_two_sources, discover_tier,
        example_database_in, example_root, load_into_tier, raw_sql_settings, settings_over, source_entry, start_configured, v1,
    };

    const PG: &str = "pg";

    /// `customers` is the model moved onto [`PG`], `regions` stays on the `duckdb` file, so the join
    /// reads both sources.
    const AREAS: &str = "select r.sales_area, count(*) as customers from dim_customer c \
                         join dim_region r on c.region = r.region group by r.sales_area order by r.sales_area";

    /// The example with `customers` on a `postgres` source and every other model on one `duckdb`
    /// file, the raw tool on over both. `None` where no tier is up; the guard holds the tier's load.
    fn spanning(case: &str, federation_allowed: bool) -> Option<(String, impl Sized)> {
        discover_tier()?;
        let catalog = catalog_spanning_two_sources(case, PG);
        let loaded = load_into_tier(case, PG)?;
        let sources = format!(
            "  {LOCAL_SOURCE}:\n    kind: \"duckdb\"\n    database_file: \"{}\"\n    posture: \"shared-service-user\"\n{}",
            example_database_in(&catalog).display(),
            source_entry(
                PG,
                loaded.port,
                &loaded.database,
                &loaded.user,
                &loaded.password_file,
                &loaded.anchor
            ),
        );
        let settings = settings_over(
            &catalog,
            &example_root().join("data"),
            LOOPBACK,
            &format!("{SINGLE_USER}  access_token: \"{TOKEN}\"\n"),
            &sources,
        );
        Some((
            format!(
                "{settings}tools:\n  run_sql:\n    enabled: true\n    sources: [\"{LOCAL_SOURCE}\", \"{PG}\"]\n    \
                 federation_allowed: {federation_allowed}\n"
            ),
            loaded.guard,
        ))
    }

    fn ran(served: &Served, statement: &str) -> Reply {
        served.post(&v1(RUN_SQL), Some(TOKEN), &json!({ "statement": statement }).to_string())
    }

    #[test]
    fn federation_allowed_false_refuses_a_statement_over_two_sources_and_answers_one_over_one() {
        let Some((settings, _tier)) = spanning("run-sql-not-federated", false) else {
            return;
        };
        let served = start_configured("run-sql-not-federated", &settings);

        let refused = ran(&served, AREAS);
        assert_eq!(refused.status, 409, "{}", refused.body);
        let body = refused.json();
        assert_eq!(body["outcome"], "raw_refusal", "{}", refused.body);
        assert_eq!(body["code"], "plan_spans_more_than_one_source", "{}", refused.body);

        let one = ran(&served, "select sales_area from dim_region order by sales_area");
        assert_eq!(one.status, 200, "{}", one.body);
        assert_eq!(one.json()["outcome"], "raw_rows", "{}", one.body);
        assert!(one.json().get("executed_as").is_none(), "{}", one.body);
    }

    #[test]
    fn federation_allowed_true_answers_over_duckdb_and_postgres_as_one_duckdb_holding_both() {
        let Some((settings, _tier)) = spanning("run-sql-federated", true) else {
            return;
        };
        let served = start_configured("run-sql-federated", &settings);

        let across = ran(&served, AREAS);
        assert_eq!(across.status, 200, "{}", across.body);
        let body = across.json();
        assert_eq!(body["outcome"], "raw_rows", "{}", across.body);
        assert_eq!(
            body["executed_as"],
            json!([
                { "source": LOCAL_SOURCE, "posture": "shared-service-user" },
                { "source": PG, "posture": "shared-service-user" },
            ]),
            "{}",
            across.body
        );
        assert!(
            body["rows"].as_array().is_some_and(|rows| !rows.is_empty()),
            "{}",
            across.body
        );

        // Refused at planning, before any leg: DDL, and a column no table has - whose name the
        // engine's error carries and the caller's body does not.
        let dropped = ran(&served, "drop table dim_region");
        assert_eq!(dropped.status, 422, "{}", dropped.body);
        assert_eq!(dropped.json()["code"], "statement_failed", "{}", dropped.body);
        let unknown = ran(&served, "select no_such_column from dim_region");
        assert_eq!(unknown.status, 422, "{}", unknown.body);
        assert!(!unknown.body.contains("no_such_column"), "{}", unknown.body);
        drop(served);

        let one = start_configured("run-sql-one-duckdb", &raw_sql_settings("run-sql-one-duckdb"));
        let whole = ran(&one, AREAS);
        assert_eq!(whole.status, 200, "{}", whole.body);
        assert_eq!(body["columns"], whole.json()["columns"], "{}\n{}", across.body, whole.body);
        assert_eq!(body["rows"], whole.json()["rows"], "{}\n{}", across.body, whole.body);
    }
}
