//! The served two-KIND cell (`telekom/sutura#112`): `files` and `postgres`, open together, each
//! answering its OWN question over HTTP.
//!
//! **What this proves, and what it does not.** `served.rs`'s
//! `a_served_deployment_answers_a_question_spanning_two_sources` is two `files` sources federated
//! into ONE answer; `serve/tests.rs`'s `a_catalog_reading_two_kinds_of_source_now_opens_both_...`
//! is a `files`+`bigquery` mix that only reaches a credential REFUSAL, because a real `BigQuery`
//! dataset needs a project this suite has none of. Neither asks a mixed deployment for a real
//! ROW. This cell holds `files` and `postgres` open in one process and asks each kind's own
//! question - never a question that needs both legs at once, so it needs no leg promotion on
//! either adapter; see `harness/two_kind.rs`'s own header for why the catalog is shaped so that
//! stays true.
//!
//! **The identity limit, same as every other served cell in this suite.** Both sources are
//! declared `shared-service-user`; this is single-player federation twice over, not leg 2.
//!
//! **The developer-machine limit, `github.com/telekom/sutura#877`.** This cell skips - green,
//! silently, no `SKIP` in a bare `cargo nextest` summary - wherever `harness/two_kind.rs`'s
//! `settings` finds no `Postgres` tier, which is every developer machine that has not run
//! `just postgres-tier start` or set `SUTURA_DEV_REQUIRE_TIER=1`. A skip here proves nothing
//! about `#861`'s regression (`kind::open_mixed` narrowing `Mixed::attached` back to `files`-only)
//! over the real HTTP round trip. `just test`'s own `checks.nextest` sets that variable and fails
//! closed, so CI is covered either way - but the call-site regression itself needs no tier at
//! all: `crates/sutura-cli/src/serve/kind.rs`'s `trust_into_widens_attached_with_the_tables_its_
//! slice_names` unit cell holds it on every machine, every run, because `open_mixed`'s `bigquery`
//! and `postgres` branches both widen `attached` through that one function rather than each
//! inlining its own copy.

#[cfg(unix)]
#[cfg(test)]
#[cfg(feature = "postgres")]
mod tests {
    use crate::harness::{
        LOCAL_SOURCE, PG_SOURCE, TOKEN, example_root, recurring_revenue_june, start_configured, two_kind_settings, v1,
    };

    /// [`voice_minutes_by_day`]'s own quantisation, mirroring
    /// [`sutura_domain::warehouse::agreement::RealTolerance::DIFFERENTIAL`]'s twelve significant
    /// digits at the wire - see `served/corpus.rs`'s own `quantised` for the fuller argument for
    /// comparing THIS way rather than against a literal fraction. Reimplemented rather than shared:
    /// `corpus.rs`'s copy is private to its own `mod tests`, and a `pub(crate)` third copy of "is
    /// this string a float" is not worth a shared home for the one caller each file has.
    fn quantised(cell: &str) -> String {
        match cell.parse::<f64>() {
            Ok(parsed) if cell.contains('.') => format!("{parsed:.12e}"),
            _ => cell.to_owned(),
        }
    }

    /// The one question this cell asks of the `postgres` side: `voice_minutes`, day grain, native
    /// to `daily_usage` alone - no dimension, no relationship, so answering it never needs the
    /// `files` side this deployment also has open.
    fn voice_minutes_by_day() -> String {
        let path = example_root().join("questions").join("voice-minutes-by-day.yaml");
        let fixture = std::fs::read_to_string(&path)
            .unwrap_or_else(|cause| panic!("{} is the example question this body mirrors: {cause}", path.display()));
        for expected in ["voice_minutes", "day", "2026-06-01", "2026-06-05"] {
            assert!(
                fixture.contains(expected),
                "{} no longer mentions `{expected}`, so this suite asks something the example does not:\n{fixture}",
                path.display()
            );
        }
        String::from(r#"{"metrics":["voice_minutes"],"grain":"day","range":{"start":"2026-06-01","end":"2026-06-05"}}"#)
    }

    /// **The cell.** One deployment, two kinds, each kind's own question answered correctly - the
    /// coverage `served.rs`'s own header on this suite's purpose names as missing.
    #[test]
    fn a_served_deployment_holding_two_kinds_answers_each_kinds_own_question() {
        let Some((settings, _fixture_lock)) = two_kind_settings("two-kinds") else {
            return;
        };
        let served = start_configured("two-kinds", &settings);

        // The `files` side: `recurring_revenue_june`, on the `subscriptions` model, which never
        // moved off `LOCAL_SOURCE` - the certified figure `served.rs`'s own single-source cell
        // pins, re-answered here beside an open `postgres` source rather than alone.
        let files_reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_june(),
        );
        assert_eq!(files_reply.status, 200, "{}", files_reply.body);
        let files_body = files_reply.json();
        assert_eq!(files_body["outcome"], "answer", "{}", files_reply.body);
        assert_eq!(
            files_body["columns"],
            serde_json::json!(["period", "recurring_revenue"]),
            "{}",
            files_reply.body
        );
        assert_eq!(
            files_body["rows"],
            serde_json::json!([["2026-06-01", "202121"]]),
            "{}",
            files_reply.body
        );
        assert_eq!(
            files_body["executed_as"],
            serde_json::json!([{ "source": LOCAL_SOURCE, "posture": "shared-service-user" }]),
            "the `files` question must not have reached the `postgres` side: {}",
            files_reply.body
        );

        // The `postgres` side: `voice_minutes_by_day`, on `daily_usage` - the model this
        // deployment's catalog moved off `LOCAL_SOURCE` - answered by the SAME process, over the
        // SAME connection type, immediately after the `files` question above.
        let pg_reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &voice_minutes_by_day(),
        );
        assert_eq!(pg_reply.status, 200, "{}", pg_reply.body);
        let pg_body = pg_reply.json();
        assert_eq!(pg_body["outcome"], "answer", "{}", pg_reply.body);
        assert_eq!(
            pg_body["columns"],
            serde_json::json!(["period", "voice_minutes"]),
            "{}",
            pg_reply.body
        );
        let rows = pg_body["rows"].as_array().cloned().unwrap_or_default();
        let quantised_rows: Vec<[String; 2]> = rows
            .iter()
            .map(|row| {
                let day = row[0].as_str().unwrap_or_default().to_owned();
                let value = quantised(row[1].as_str().unwrap_or_default());
                [day, value]
            })
            .collect();
        assert_eq!(
            quantised_rows,
            [
                ["2026-06-01".to_owned(), "7.577000000000e2".to_owned()],
                ["2026-06-02".to_owned(), "6.287000000000e2".to_owned()],
                ["2026-06-03".to_owned(), "7.132000000000e2".to_owned()],
                ["2026-06-04".to_owned(), "7.942000000000e2".to_owned()],
            ],
            "{}",
            pg_reply.body
        );
        assert_eq!(
            pg_body["executed_as"],
            serde_json::json!([{ "source": PG_SOURCE, "posture": "shared-service-user" }]),
            "the `postgres` question must not have reached the `files` side: {}",
            pg_reply.body
        );
    }
}
