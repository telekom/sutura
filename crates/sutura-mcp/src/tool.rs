//! The one tool this slice advertises, and the schema it is described by.
//!
//! # The schema is GENERATED
//!
//! [`input_schema`] is `schemars::schema_for!(AskArgs)` and nothing else. There is no hand-written
//! JSON object anywhere in this crate, which is the property that had to be true from the first line:
//! a hand-written schema and a hand-written parser drift, and the drift is invisible until a caller
//! trusts the schema. Here the description a client reads and the deserializer that enforces it come
//! from one type - `additionalProperties: false` included, because `schemars` reads the same
//! `deny_unknown_fields` serde reads.
//!
//! # The dump, and why it is a gate rather than a convenience
//!
//! `AGENTS.md`'s *changing the query path or the tool surface* table wants a widened tool input to be
//! caught by something a reviewer cannot miss, and it says plainly that the byte-compare it used to
//! name **does not exist**. [`tests::the_advertised_tool_schema_is_the_committed_one`] is that
//! byte-compare: the generated schema is snapshotted, so a new or widened field changes the snapshot
//! and the test fails until somebody re-accepts it. That puts the new surface in the diff, which is
//! the whole mechanism - `deny_unknown_fields` stops an *undeclared* field from being answered, and
//! this stops a *declared* one from arriving unreviewed.
//!
//! Beside it, [`tests::the_tool_takes_exactly_the_five_fields_a_question_has`] asserts the property
//! rather than the bytes: a field named `sql`, `table`, `where`, `predicate` or `rows` is not merely
//! a snapshot change but a named failure. A snapshot can be re-accepted without thought; that one
//! cannot.

use std::sync::Arc;

use rmcp::model::{JsonObject, Tool};

use crate::wire::AskArgs;

/// The tool's name, as a client sees it.
///
/// One tool in this slice. The rest of the set, and advertisement filtered by what a caller may
/// invoke, is the next slice - and filtering needs a verified claim to filter on, which is a
/// decision recorded in `docs/adr/0014` and not yet built.
pub const ASK_METRIC: &str = "ask_metric";

/// What the tool is for, in the words a model reads before it decides to call it.
///
/// **Advisory, and it has to be.** A gateway of the shape this product runs behind surfaces tools
/// and ignores resources and prompts, so nothing that makes an answer *correct* may live in prose: a
/// question outside the catalog is refused by the service, not discouraged by this paragraph.
const DESCRIPTION: &str = "Answer one governed question about a certified metric. \
     The metrics, time grains, dimensions and permitted filter values are fixed by this deployment: \
     a question outside them comes back as a refusal with a machine-readable reason rather than as \
     an approximate answer. Every answer carries the definition version and digest that produced it, \
     and the rows themselves. There is no way to send SQL, a table name or a filter expression, and \
     an argument that names one is rejected.";

/// The tool's input schema, generated from [`AskArgs`].
///
/// Built on demand rather than cached: it is computed once per `tools/list`, which is once per
/// session for every client that exists, and a `LazyLock` would buy nothing measurable while adding
/// a static nobody can see the value of in a test.
pub fn input_schema() -> JsonObject {
    let schema = schemars::schema_for!(AskArgs);
    // `schemars::Schema` is a JSON value that is an object by construction for a derived struct
    // schema, and `to_value` on it cannot fail. Neither of those is an `unwrap` this workspace
    // permits, so the fallback is an empty object - which a test would catch immediately, because
    // the committed dump is not empty.
    serde_json::to_value(&schema)
        .ok()
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// The tool, as `tools/list` returns it.
pub fn ask_metric() -> Tool {
    Tool::new(ASK_METRIC, DESCRIPTION, Arc::new(input_schema()))
}

#[cfg(test)]
mod tests {
    use super::{ASK_METRIC, ask_metric, input_schema};

    /// THE drift guard. Reviewed as a diff, never typed.
    ///
    /// A new or widened tool input changes these bytes and this test fails until somebody re-accepts
    /// the snapshot, which is what puts the change in front of a reviewer. `AGENTS.md` describes this
    /// mechanism as owed rather than standing; this is the crate that owes it.
    ///
    /// **The bytes are canonicalised rather than taken from `to_string_pretty`, and that is not
    /// tidiness - it is the same determinism hazard `sutura_http::openapi` records.** `serde_json`'s
    /// `preserve_order` feature is switched on somewhere in this workspace (by `utoipa`), and cargo
    /// unifies features, so a `serde_json::Map` keeps insertion order in a workspace build and sorts
    /// its keys in a build of this crate alone. The same schema therefore serializes two ways
    /// depending on what else is being compiled, which would make this snapshot fail for a reason
    /// that has nothing to do with the tool surface. [`canonical`] sorts every key at every depth, so
    /// the committed bytes are a property of the schema and of nothing else.
    #[test]
    fn the_advertised_tool_schema_is_the_committed_one() {
        insta::assert_snapshot!(
            "ask_metric_input_schema",
            canonical(&serde_json::Value::Object(input_schema()), 0)
        );
    }

    /// The same schema, twice, with the key order fixed by this function rather than by whichever
    /// feature set a build happened to unify - which is the failure the snapshot above would
    /// otherwise report as a surface change.
    #[test]
    fn the_committed_bytes_do_not_depend_on_who_else_is_being_compiled() {
        let once = canonical(&serde_json::Value::Object(input_schema()), 0);
        let twice = canonical(&serde_json::Value::Object(input_schema()), 0);
        assert_eq!(once, twice);
        // Sorted at the top level, which is what a reader of the snapshot is entitled to assume.
        let keys: Vec<&str> = once
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .filter_map(|rest| rest.split('"').next())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "{keys:?}");
    }

    /// JSON with every object's keys sorted, two spaces per level.
    ///
    /// Hand-written because there is no way to ask `serde_json` for it: with `preserve_order` unified
    /// on, its `Map` *is* insertion-ordered and a `BTreeMap` round trip cannot get the ordering back.
    fn canonical(value: &serde_json::Value, depth: usize) -> String {
        let pad = "  ".repeat(depth.saturating_add(1));
        let close = "  ".repeat(depth);
        match *value {
            serde_json::Value::Object(ref map) => {
                if map.is_empty() {
                    return String::from("{}");
                }
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let body: Vec<String> = keys
                    .into_iter()
                    .map(|key| {
                        let nested = map.get(key).unwrap_or(&serde_json::Value::Null);
                        format!(
                            "{pad}{}: {}",
                            serde_json::Value::String(key.clone()),
                            canonical(nested, depth.saturating_add(1))
                        )
                    })
                    .collect();
                format!("{{\n{}\n{close}}}", body.join(",\n"))
            }
            serde_json::Value::Array(ref items) => {
                if items.is_empty() {
                    return String::from("[]");
                }
                let body: Vec<String> = items
                    .iter()
                    .map(|item| format!("{pad}{}", canonical(item, depth.saturating_add(1))))
                    .collect();
                format!("[\n{}\n{close}]", body.join(",\n"))
            }
            ref scalar => scalar.to_string(),
        }
    }

    /// The property behind the snapshot, so a re-accepted snapshot is not the only thing standing
    /// between the tool surface and a field that carries SQL.
    #[test]
    fn the_tool_takes_exactly_the_five_fields_a_question_has() {
        let schema = input_schema();
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("the generated schema describes properties");
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["dimensions", "filters", "grain", "metric", "range"], "{names:?}");
        // Named explicitly rather than left to the equality above, so the failure says what went
        // wrong rather than only that something did.
        for forbidden in ["sql", "query", "table", "where", "predicate", "rows", "row_ids", "limit"] {
            assert!(
                !properties.contains_key(forbidden),
                "the tool surface grew a `{forbidden}` field"
            );
        }
    }

    /// `deny_unknown_fields` reaches the *advertised* schema and not only the deserializer, so a
    /// cooperative client can refuse the call before it is made.
    #[test]
    fn the_schema_says_no_field_it_did_not_declare_is_allowed() {
        let schema = input_schema();
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false)),
            "{schema:?}"
        );
    }

    /// The description a model reads names the refusal and names what it cannot send. Not the
    /// wording - the two facts.
    #[test]
    fn the_tool_description_says_a_question_can_be_refused_and_that_sql_is_not_a_field() {
        let tool = ask_metric();
        assert_eq!(tool.name, ASK_METRIC);
        let description = tool.description.as_deref().unwrap_or_default();
        assert!(description.contains("refusal"), "{description}");
        assert!(description.contains("SQL"), "{description}");
    }
}
