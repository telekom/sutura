//! The tools this surface advertises, and the schemas they are described by.
//!
//! # There is one source for WHICH tools exist, and it is not this file
//!
//! `sutura_app::Capability` is the tool set. This module decides how each capability is *described*
//! to a model and nothing else: the name comes from [`Capability::id`], the gate comes from
//! [`Capability::scope`], and [`every`] is a walk over `Capability::every()` with no second list to
//! keep in step with it.
//!
//! That is the shape `docs/implementation-plan.md`'s `both_transports_describe_the_same_tools` asks
//! for. It could not be a comparison between the two transports - `sutura-mcp` and `sutura-http`
//! cannot see each other - so it is one source in the crate that owns the driving port, plus a test
//! in each transport asserting it did not deviate from that source. Two tests, one source, and
//! [`tests::both_transports_describe_the_same_tools`] is this crate's half.
//!
//! **The prose is deliberately NOT shared.** A tool description is written for a model choosing
//! whether to call it; an `OpenAPI` summary is written for a person reading an interface description.
//! `AGENTS.md` already draws that line for the two refusal vocabularies - *"Nothing compares the two
//! sentences, and nothing should"* - and the same reasoning applies here.
//!
//! # The schema is GENERATED
//!
//! [`input_schema`] is `schemars::schema_for!` over a wire type and nothing else. There is no
//! hand-written JSON object anywhere in this crate, which is the property that had to be true from
//! the first line: a hand-written schema and a hand-written parser drift, and the drift is invisible
//! until a caller trusts the schema. Here the description a client reads and the deserializer that
//! enforces it come from one type - `additionalProperties: false` included, because `schemars` reads
//! the same `deny_unknown_fields` serde reads.
//!
//! # The dump, and why it is a gate rather than a convenience
//!
//! `AGENTS.md`'s *changing the query path or the tool surface* table wants a widened tool input to be
//! caught by something a reviewer cannot miss. [`tests::the_advertised_tool_schemas_are_the_committed_ones`]
//! is that byte-compare: **every** capability's generated schema is snapshotted, so a new or widened
//! field changes a snapshot and the test fails until somebody re-accepts it. That puts the new surface
//! in the diff, which is the whole mechanism - `deny_unknown_fields` stops an *undeclared* field from
//! being answered, and this stops a *declared* one from arriving unreviewed.
//!
//! Beside it, [`tests::the_question_tool_takes_exactly_the_five_fields_a_question_has`] asserts the
//! property rather than the bytes: a field named `sql`, `table`, `where`, `predicate` or `rows` is not
//! merely a snapshot change but a named failure. A snapshot can be re-accepted without thought; that
//! one cannot.

use std::sync::Arc;

use rmcp::model::{JsonObject, Tool};
use sutura_app::{Capability, Permitted};

use crate::wire::{AskArgs, DescribeCatalogArgs};

/// What each tool is for, in the words a model reads before it decides to call it.
///
/// **Advisory, and it has to be.** A gateway of the shape this product runs behind surfaces tools and
/// ignores resources and prompts, so nothing that makes an answer *correct* may live in prose: a
/// question outside the catalog is refused by the service, not discouraged by these paragraphs.
///
/// An exhaustive match rather than a constant per tool, so a capability added to
/// `sutura_app::Capability` does not compile until somebody has written what a model should be told
/// about it.
const fn description(capability: Capability) -> &'static str {
    match capability {
        Capability::DescribeCatalog => {
            "List what this deployment measures: every certified metric, the time grains it supports, \
             the dimensions it can be grouped by or filtered on, and the exact values a filter may use. \
             Read this before asking a question - it is the only way to know what a valid question is, \
             and every answer carries the same definition version and digest this listing does. It \
             returns definitions, never data rows."
        }
        Capability::AskMetric => {
            "Answer one governed question about a certified metric. \
             The metrics, time grains, dimensions and permitted filter values are fixed by this deployment: \
             a question outside them comes back as a refusal with a machine-readable reason rather than as \
             an approximate answer. Every answer carries the definition version and digest that produced it, \
             and the rows themselves. There is no way to send SQL, a table name or a filter expression, and \
             an argument that names one is rejected."
        }
    }
}

/// One capability's input schema, generated from its wire type.
///
/// Built on demand rather than cached: it is computed once per `tools/list`, which is once per session
/// for every client that exists, and a `LazyLock` would buy nothing measurable while adding a static
/// nobody can see the value of in a test.
///
/// **The match is what ties a capability to a wire type**, and it is exhaustive: a capability with no
/// arguments type does not compile.
pub fn input_schema(capability: Capability) -> JsonObject {
    let schema = match capability {
        Capability::DescribeCatalog => schemars::schema_for!(DescribeCatalogArgs),
        Capability::AskMetric => schemars::schema_for!(AskArgs),
    };
    // `schemars::Schema` is a JSON value that is an object by construction for a derived struct
    // schema, and `to_value` on it cannot fail. Neither of those is an `unwrap` this workspace
    // permits, so the fallback is an empty object - which a test would catch immediately, because the
    // committed dumps are not empty.
    serde_json::to_value(&schema)
        .ok()
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

/// One capability, as `tools/list` returns it.
pub fn tool(capability: Capability) -> Tool {
    Tool::new(capability.id(), description(capability), Arc::new(input_schema(capability)))
}

/// The tools this caller is advertised, in capability order.
///
/// **Presentation, and the module documentation says so plainly.** A caller that names a tool absent
/// from this list is refused by `crate::server::AgentSurface::call_tool` asking
/// `Permitted::includes` again, so filtering here is what a cooperative client is *shown* and not
/// what stops an uncooperative one.
pub fn every(permitted: &Permitted) -> Vec<Tool> {
    permitted.advertised().map(tool).collect()
}

/// The capability a tool name refers to, if this surface has one under that name.
///
/// **Independent of what was advertised**, which is the point: this answers *does this deployment
/// have such a tool* and the caller's permission is a separate question the server then asks. Folding
/// the two together is how a caller ends up told "no such tool" for one it holds the scope for.
#[must_use]
pub fn named(name: &str) -> Option<Capability> {
    Capability::every().find(|capability| capability.id() == name)
}

#[cfg(test)]
mod tests {
    use sutura_app::{Capability, Permitted};

    use super::{description, every, input_schema, named, tool};

    /// THE drift guard, now over every tool rather than one. Reviewed as a diff, never typed.
    ///
    /// A new or widened tool input changes these bytes and this test fails until somebody re-accepts
    /// the snapshot, which is what puts the change in front of a reviewer. The snapshot is NAMED after
    /// the capability's own identifier, so a capability added to `sutura_app::Capability` produces a
    /// new snapshot file rather than silently reusing another's.
    ///
    /// **The bytes are canonicalised rather than taken from `to_string_pretty`, and that is not
    /// tidiness - it is the same determinism hazard `sutura_http::openapi` records.** `serde_json`'s
    /// `preserve_order` feature is switched on somewhere in this workspace (by `utoipa`), and cargo
    /// unifies features, so a `serde_json::Map` keeps insertion order in a workspace build and sorts
    /// its keys in a build of this crate alone. The same schema therefore serializes two ways
    /// depending on what else is being compiled, which would make this snapshot fail for a reason that
    /// has nothing to do with the tool surface. [`canonical`] sorts every key at every depth, so the
    /// committed bytes are a property of the schema and of nothing else.
    #[test]
    fn the_advertised_tool_schemas_are_the_committed_ones() {
        for capability in Capability::every() {
            insta::assert_snapshot!(
                format!("{}_input_schema", capability.id()),
                canonical(&serde_json::Value::Object(input_schema(capability)), 0)
            );
        }
    }

    /// The same schema, twice, with the key order fixed by this function rather than by whichever
    /// feature set a build happened to unify - which is the failure the snapshot above would otherwise
    /// report as a surface change.
    #[test]
    fn the_committed_bytes_do_not_depend_on_who_else_is_being_compiled() {
        let once = canonical(&serde_json::Value::Object(input_schema(Capability::AskMetric)), 0);
        let twice = canonical(&serde_json::Value::Object(input_schema(Capability::AskMetric)), 0);
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
    fn the_question_tool_takes_exactly_the_five_fields_a_question_has() {
        let schema = input_schema(Capability::AskMetric);
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("the generated schema describes properties");
        let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["dimensions", "filters", "grain", "metric", "range"], "{names:?}");
        // Named explicitly rather than left to the equality above, so the failure says what went wrong
        // rather than only that something did.
        for forbidden in ["sql", "query", "table", "where", "predicate", "rows", "row_ids", "limit"] {
            assert!(
                !properties.contains_key(forbidden),
                "the tool surface grew a `{forbidden}` field"
            );
        }
    }

    /// The catalog tool takes NO arguments, and that is a shape rather than an absence.
    ///
    /// A tool advertising an open object is a tool whose surface a caller can guess at. Every field
    /// forbidden above is forbidden here too, and there is nothing else it could be given either -
    /// which is what makes "descriptive content only, and nothing a caller sends selects it" a
    /// statement about the type rather than about the handler.
    #[test]
    fn the_catalog_tool_takes_no_arguments_at_all() {
        let schema = input_schema(Capability::DescribeCatalog);
        assert_eq!(
            schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .map_or(0, serde_json::Map::len),
            0,
            "{schema:?}"
        );
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false)),
            "{schema:?}"
        );
    }

    /// `deny_unknown_fields` reaches the *advertised* schema and not only the deserializer, so a
    /// cooperative client can refuse the call before it is made. Every tool, not one.
    #[test]
    fn every_schema_says_no_field_it_did_not_declare_is_allowed() {
        for capability in Capability::every() {
            let schema = input_schema(capability);
            assert_eq!(
                schema.get("additionalProperties"),
                Some(&serde_json::Value::Bool(false)),
                "{}: {schema:?}",
                capability.id()
            );
        }
    }

    /// **The slice's headline property, from this side of it.**
    ///
    /// The two transports cannot compare notes - an adapter never calls another adapter - so what is
    /// asserted is that this transport advertises **exactly** `sutura_app::Capability::every()`, by
    /// identifier and in order. `sutura_http`'s test of the same name asserts the same thing of the
    /// generated interface description, against the same source. Two tests over one source is the only
    /// shape in which the two descriptions cannot disagree.
    #[test]
    fn both_transports_describe_the_same_tools() {
        let tools = every(&Permitted::every_capability());
        let advertised: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        let expected: Vec<&str> = Capability::every().map(Capability::id).collect();
        assert_eq!(advertised, expected, "{advertised:?}");
    }

    /// Advertisement is filtered by what the caller may invoke, and the name says which capability it
    /// dropped.
    #[test]
    fn the_advertised_tools_differ_by_scope() {
        let all = every(&Permitted::every_capability());
        assert_eq!(all.len(), Capability::every().count());

        let catalog_only = every(&Permitted::granted_by([Capability::DescribeCatalog.scope()]));
        assert_eq!(catalog_only.len(), 1, "{catalog_only:?}");
        assert_eq!(
            catalog_only.first().map(|tool| tool.name.as_ref()),
            Some(Capability::DescribeCatalog.id())
        );

        let nothing = every(&Permitted::granted_by(["openid"]));
        assert!(nothing.is_empty(), "{nothing:?}");
    }

    /// A tool name resolves to a capability whatever the caller may do, and the two questions stay
    /// separate.
    ///
    /// The bug this rules out: folding permission into name resolution makes an unpermitted call
    /// indistinguishable from a typo, so a caller holding the right scope and a stale tool name is
    /// told the wrong thing.
    #[test]
    fn a_tool_name_resolves_independently_of_what_the_caller_may_do() {
        for capability in Capability::every() {
            assert_eq!(named(capability.id()), Some(capability));
        }
        assert_eq!(named("run_sql"), None);
        assert_eq!(named(""), None);
        // And the scope string is not a tool name, so a caller cannot call a scope.
        for capability in Capability::every() {
            assert_eq!(named(capability.scope()), None);
        }
    }

    /// Each description a model reads names the two facts about that tool. Not the wording.
    #[test]
    fn each_tool_description_says_what_it_returns_and_what_it_will_not_take() {
        let question = tool(Capability::AskMetric);
        assert_eq!(question.name, Capability::AskMetric.id());
        let text = question.description.as_deref().unwrap_or_default();
        assert!(text.contains("refusal"), "{text}");
        assert!(text.contains("SQL"), "{text}");

        let catalog = tool(Capability::DescribeCatalog);
        assert_eq!(catalog.name, Capability::DescribeCatalog.id());
        let text = catalog.description.as_deref().unwrap_or_default();
        // The one thing a model must not conclude about this tool: that it is a way to read rows.
        assert!(text.contains("never data rows"), "{text}");
        assert!(text.contains("digest"), "{text}");
    }

    /// Every capability has a description, and no two share one.
    ///
    /// A copy-pasted description is how a model is told the wrong thing about a tool that exists, and
    /// the exhaustive match in [`description`] cannot catch it.
    #[test]
    fn no_two_capabilities_share_a_description() {
        let mut texts: Vec<&str> = Capability::every().map(description).collect();
        let count = texts.len();
        texts.sort_unstable();
        texts.dedup();
        assert_eq!(texts.len(), count);
        for capability in Capability::every() {
            assert!(description(capability).len() > 80, "{}", capability.id());
        }
    }
}
