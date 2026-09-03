//! The multi-player example, exercised through the recorded fixture.
//!
//! `examples/multi-player/README.md` is the deployment shape this test proves: a multi-player
//! deployment whose semantic catalog is `DataHub`. **The shape is not servable in this repository
//! yet** - no binary links `sutura-catalog-datahub` and `sutura-serve` refuses `catalog.kind:
//! datahub` by name - so what is runnable is the recorded fixture, and that is exactly what this
//! file is honest about. It is the same division of labour as the single player example
//! (`crates/sutura-cli/tests/example.rs`): one documented set of input, loaded and asked by the
//! build, so a quickstart that stops working fails the build instead of failing the next person who
//! tried it. The difference is that the catalog is not markdown - it is `DataHub`'s recorded entity
//! aspects served by the fixture reader, which is what a metadata service over HTTP reads once a
//! real `AspectReader` exists.
//!
//! **The documented input is a real file, not a copy in this source.** The question
//! `examples/multi-player/question.json` is read off disk here exactly as the README names it, and
//! the quote *revenue in June 2026* in that README is a sentence pointing at the file rather than a
//! second copy - so the file, the README and this test cannot hold three opinions about the
//! example question.
//!
//! **What this can and cannot prove is worth stating, because "multi-player" reads bigger than a
//! fixture can show.** It proves the metadata half of the shape: the deployment-defined `sutura`
//! structured property (the flat scalar form `DataHub` can actually store) turns a `DataHub` metric
//! entity into a certified `Metric`, and a question about it compiles all the way down to a plan.
//! It does NOT prove two callers get two different sets of rows, and it does NOT prove a served
//! deployment exists: both need things this repository does not have (a data system with grants and
//! a per-subject execution path; a composition root that links the adapter), which
//! `examples/multi-player/README.md` and the *Built and not wired* register in
//! `.agents/skills/sutura/query-surface/SKILL.md` keep naming as
//! the missing halves. Nothing here is `#[ignore]`d and nothing needs a network, which is what makes
//! it a gate in `checks.nextest` rather than prose.
//!
//! **Which of the two cells is a regression proof, and against what - because `test-causality`
//! cannot answer this one and reports it as green against base.** The gate reverts Rust files, and
//! the thing `the_documented_question_compiles_to_a_plan` needs reverted is a JSON fixture: without
//! `examples/multi-player/question.json` it fails on that file's absence, measured. The other cell,
//! `the_metric_the_namespace_defines_is_in_the_bundle`, asserts behaviour that arrives with the
//! certified-metric change one commit BELOW this one, so it is green against this branch's base by
//! construction and red against the trunk that predates that change. It is the example's
//! INTEGRATION assertion rather than a regression test for this commit, and that is stated here
//! instead of left for a reader to infer from a green gate.

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sutura_catalog_datahub::fixture::over_fixture_source;
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::model::{Grain, SourceName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_domain::query::Query;
    use sutura_semantic::{Compiled, compile};

    /// The version the example is stamped with in this suite, matching the name
    /// `examples/multi-player/README.md` gives it. The digest does not include the version, so this
    /// is provenance only.
    const VERSION: &str = "example-multi-player";

    /// The one source the recorded corpus names its models on.
    fn source() -> SourceName {
        SourceName::parse("local").expect("the example source name is a name")
    }

    /// The example question as a real documented input: `examples/multi-player/question.json`.
    ///
    /// Read off disk so the file is the single source of the question, and the README's *revenue in
    /// June 2026* is prose pointing at it. If the file stops being the documented question, this
    /// test stops compiling or reading the question the example actually documents.
    fn june_revenue() -> Query {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/multi-player/question.json");
        let document: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the example question file exists"))
                .expect("the example question file is json");
        let metric = document["metric"].as_str().expect("the question names a metric");
        let grain: Grain =
            serde_json::from_value(document["grain"].clone()).expect("the question's grain is a grain of the closed set");
        let start = Date::parse(document["from"].as_str().expect("the question has a start")).expect("the start is a date");
        let end = Date::parse(document["to"].as_str().expect("the question has an end")).expect("the end is a date");
        let range = TimeRange::new(start, end).expect("the question's range is a range");
        Query::new(
            sutura_domain::model::MetricName::parse(metric).expect("the question's metric is a name"),
            grain,
            range,
            Vec::new(),
            Vec::new(),
        )
    }

    /// The shape's first claim: the metric the namespace defines is in the bundle.
    ///
    /// The fixture's metric entity carries the `sutura` structured property the README documents, so
    /// `revenue` is a certified `Metric` in the bundle, named and anchored as the deployment defined
    /// it. "Certified" further up than this - an anchor RE-EXECUTED at boot - is not provable here,
    /// because no data system re-runs it; what this asserts is that the bundle carries the metric
    /// the namespace defines, with the content the deployment wrote.
    #[test]
    fn the_metric_the_namespace_defines_is_in_the_bundle() {
        let version = DefinitionVersion::parse(VERSION).expect("the fixed version is a version");
        let pinned = over_fixture_source(source(), version)
            .load()
            .expect("the recorded catalog loads");
        let metric = pinned
            .definitions()
            .metric(&sutura_domain::model::MetricName::parse("revenue").expect("the example metric is a name"))
            .expect("the README's metric is defined");
        assert_eq!(metric.name().as_str(), "revenue");
        assert_eq!(metric.time_column().as_str(), "order_date");
        assert!(
            metric.anchor().is_some(),
            "the deployment defined a certified number, so the bundle carries an anchor"
        );
    }

    /// The second claim, and the one that makes the example an integration rather than a bundle: a
    /// question about the certified metric compiles to a plan over the fact table. If the plan came
    /// back as a refusal the example would still "load" - and this is what catches that.
    #[test]
    fn the_documented_question_compiles_to_a_plan() {
        let version = DefinitionVersion::parse(VERSION).expect("the fixed version is a version");
        let pinned = over_fixture_source(source(), version)
            .load()
            .expect("the recorded catalog loads");
        let compiled = compile(&june_revenue(), &pinned).expect("the example question compiles");
        match compiled {
            Compiled::Planned { ref plan } => {
                assert_eq!(plan.metric().as_str(), "revenue");
                // The fact table the model maps to, read off the plan rather than off the README,
                // so the plan and the documented shape cannot drift.
                assert_eq!(plan.table_name().as_str(), "fct_order");
            }
            Compiled::Refused { reason } => {
                panic!("the README's question must not be refused by its own catalog: {reason:?}")
            }
            Compiled::Federated { .. } => {
                panic!("the example corpus is one source; nothing federates")
            }
        }
    }
}
