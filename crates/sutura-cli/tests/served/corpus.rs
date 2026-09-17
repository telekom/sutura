//! The golden corpus, driven through the served HTTP surface instead of only in-process.
//!
//! `github.com/telekom/sutura#124`. Every other proof of "the served binary answers" here asks ONE
//! question - `served.rs`'s `a_configured_deployment_answers_a_certified_question_over_http` and
//! `a_postgres_source_answers_a_certified_question_from_the_served_binary` - and `sutura-app`'s
//! `tests/golden/data_systems.rs` runs the WHOLE corpus, but only ever in-process, by calling
//! `sutura_app::answer` directly. Nothing bound the two: no test drove every question under
//! `examples/single-player/questions` through a real router, over a real connection, against a
//! composed binary.
//!
//! # The binding
//!
//! [`corpus`] reads that directory once, the same way `sutura-app`'s own `adapters::questions` and
//! `read_question` do - sorted, so the corpus is a function of the directory and not of the
//! filesystem's own order. [`answered_over_http`] serializes each parsed [`Query`] straight back to
//! JSON with `serde_json` and `POST`s it, keyed by the question's file stem so a divergent case
//! names itself. Nothing here names an adapter or a source kind, so a future served source reuses
//! it by calling [`answered_over_http`] against its own [`Served`] handle - the reusability issue
//! #124 asked for, though only the cell below spends it today.
//!
//! # The cell, and why comparing two served deployments is the honest shape
//!
//! `examples/single-player`'s catalog and CSV data are declared identically to two deployments that
//! differ in exactly one thing: `harness::start` opens the `files` source, which is
//! `sutura-cli/src/serve.rs`'s name for the in-process `DataFusion` engine; `harness::postgres_settings`
//! loads the same CSVs into the provisioned tier and declares the SAME source name
//! (`harness::LOCAL_SOURCE`) as `kind: "postgres"` over verified TLS instead. Both run through the
//! same catalog, the same `VERSION`, the same posture, so a byte-for-byte difference in what either
//! one answers over HTTP is a claim about the adapter or the wire, never about the fixture.
//!
//! # What this caught, measured rather than hoped for
//!
//! Running the first version of this cell over the real tier found a genuine disagreement, not a
//! bug in this file: `data-per-subscription-by-day` answered `"7.1194827586206895"` from Postgres
//! and `"7.119482758620686"` from the files engine - the same division, summed in a different
//! order by two different execution engines, disagreeing in the last few bits of an `f64`. That is
//! [`sutura_domain::warehouse::agreement::RealTolerance::DIFFERENTIAL`]'s own documented case ("it
//! has fired for real on the example corpus"), reached here for the first time from the composition
//! root rather than in-process - `sutura-app`'s in-process golden run never serializes a float to a
//! wire string at all, and the one Postgres cell `served.rs` already has asks a metric
//! (`recurring_revenue`) whose certified answer has no fractional part to disagree in.
//!
//! [`quantised`] applies that SAME tolerance - twelve significant digits, "far beyond any figure a
//! metric reports and far short of the noise" - at the wire instead of on a typed `Value::Real`,
//! because by the time an answer reaches this file every cell is already a JSON string with no tag
//! saying which one was a float. **That is a parallel policy, not a shared one, and it is the
//! limit next to this claim**: a second legitimate tolerance drifting from `RealTolerance`'s number
//! would not be caught by any mechanism that ties the two together, because there is none. Reusing
//! the domain type directly would mean re-inferring the erased tag from a rendered string, which is
//! the exact ambiguity that type exists to compare through rather than around.
//!
//! **The limit under that: the corpus this binding has.** `examples/single-player/questions` has
//! no metric whose certified answer is a negative amount or a null cell, so a stringification bug
//! specific to either shape would not be caught here regardless of the tolerance. Widening the
//! corpus widens what this catches for free; widening this file does not.

#[cfg(unix)]
#[cfg(test)]
#[cfg(feature = "postgres")]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use sutura_domain::query::Query;

    use crate::harness::{Served, TOKEN, example_root, postgres_settings, start, start_configured, v1};

    /// One question's status and body, as answered by a served deployment - keyed by file stem in
    /// every map this file builds.
    type AnsweredCorpus = BTreeMap<String, (u16, serde_json::Value)>;

    /// Every question in the shared example corpus, parsed once and keyed by file stem.
    ///
    /// Sorted before parsing for the same reason `sutura-app`'s `adapters::questions` sorts: a
    /// corpus whose iteration order depends on the filesystem produces failure output that varies
    /// between runs with nothing in the tree having changed.
    fn corpus() -> BTreeMap<String, Query> {
        let dir = example_root().join("questions");
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|cause| panic!("{} is readable: {cause}", dir.display()))
            .map(|entry| entry.expect("a directory entry is readable").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "yaml"))
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no questions under {}", dir.display());
        paths
            .into_iter()
            .map(|path| {
                let stem = path
                    .file_stem()
                    .unwrap_or_else(|| panic!("{} has no file stem", path.display()))
                    .to_string_lossy()
                    .into_owned();
                let text =
                    std::fs::read_to_string(&path).unwrap_or_else(|cause| panic!("{} is readable: {cause}", path.display()));
                let query: Query =
                    serde_norway::from_str(&text).unwrap_or_else(|cause| panic!("{} is not a question: {cause}", path.display()));
                (stem, query)
            })
            .collect()
    }

    /// Drives the whole corpus through one served deployment's `/v1/query` route - one request per
    /// question, over the real transport - and returns what it answered, keyed by file stem.
    ///
    /// **The binding.** Every parsed [`Query`] round-trips through `serde_json` rather than a
    /// hand-written string, unlike `served.rs`'s own `harness::question`: that helper mirrors ONE
    /// fixture by hand on purpose, so a rename is caught; a corpus of thirty has no such single
    /// fixture to mirror; and reserializing the type the server itself deserializes into is the
    /// wire's OWN contract; a string built by hand thirty times would drift from it first.
    fn answered_over_http(served: &Served) -> AnsweredCorpus {
        corpus()
            .into_iter()
            .map(|(stem, query)| {
                let body = serde_json::to_string(&query)
                    .unwrap_or_else(|cause| panic!("{stem}'s parsed question re-serializes to JSON: {cause}"));
                let reply = served.post(&v1(sutura_http::constants::base_paths::QUERY), Some(TOKEN), &body);
                (stem, (reply.status, reply.json()))
            })
            .collect()
    }

    /// Quantises every row cell that parses as an `f64` and carries a decimal point, so two
    /// execution engines summing the same rows in a different order - a difference in the last
    /// few bits, never in the value a metric actually reports - does not read as a disagreement.
    /// See the module documentation for why this mirrors
    /// [`sutura_domain::warehouse::agreement::RealTolerance::DIFFERENTIAL`]'s digit count rather
    /// than depending on the type itself.
    ///
    /// `contains('.')` is what keeps an exact integer cell - a row count, `recurring_revenue`'s
    /// whole-cent total - out of this at all: quantising a value that never carried a fraction
    /// would widen what this cell tolerates past floats, which is not the disagreement it exists
    /// to allow.
    fn quantised(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(text) => match text.parse::<f64>() {
                Ok(parsed) if text.contains('.') => serde_json::Value::String(format!("{parsed:.12e}")),
                _ => serde_json::Value::String(text),
            },
            serde_json::Value::Array(items) => serde_json::Value::Array(items.into_iter().map(quantised).collect()),
            serde_json::Value::Object(fields) => {
                serde_json::Value::Object(fields.into_iter().map(|(key, field)| (key, quantised(field))).collect())
            }
            other => other,
        }
    }

    /// #124's promised cell: the corpus answered over the served Postgres source and over the
    /// served files engine, compared from the composition root rather than in-process.
    #[test]
    fn the_corpus_answers_identically_over_the_served_source_and_over_the_engine() {
        // Held for the whole comparison, not just the load - see `FixtureLoadGuard`'s own
        // documentation for the reload/read race a shorter hold left open.
        let Some((settings, _fixture_lock)) = postgres_settings("corpus-over-http") else {
            return;
        };
        let engine = start("corpus-over-http-engine");
        let postgres = start_configured("corpus-over-http-postgres", &settings);

        let from_engine = answered_over_http(&engine);
        let from_postgres = answered_over_http(&postgres);

        assert_eq!(
            from_engine.keys().collect::<Vec<_>>(),
            from_postgres.keys().collect::<Vec<_>>(),
            "the two deployments answered a different set of questions"
        );
        for (stem, (engine_status, engine_body)) in from_engine {
            let (postgres_status, postgres_body) = from_postgres[&stem].clone();
            assert_eq!(
                engine_status, postgres_status,
                "`{stem}` came back with a different status over the served Postgres source than over the files engine"
            );
            assert_eq!(
                quantised(engine_body),
                quantised(postgres_body),
                "`{stem}` answered differently over the served Postgres source than over the files engine"
            );
        }
    }
}
