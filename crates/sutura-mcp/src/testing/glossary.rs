//! The glossary fixture: two metrics, one restricted, and knowledge naming the restricted one.

use std::collections::BTreeSet;

use sutura_domain::catalog::{Audience, Metric, Model};
use sutura_domain::knowledge::{
    Absence, Capability, Caveat, Example, GlossaryEntry, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, NoteName,
    Phrase, Referent,
};
use sutura_domain::measure::{AggregatedColumn, Measure, Term};
use sutura_domain::model::{Aggregate, AudienceId, ColumnName, Grain, MetricName, ModelName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::query::Query;

use super::{description, source};
use sutura_domain::capabilities::MetadataCapabilities;

/// [`super::bundle_with_a_restricted_metric`], plus a glossary entry that names the `finance_only` metric
/// and an operator instructions file - the fixture issue #971's audience-scoped knowledge test needs.
///
/// One bundle carries the two metrics and one glossary entry pointing at the RESTRICTED one, so a
/// caller who may not see `finance_only` must not receive the entry either, while the deployment's
/// own view keeps both.
pub(crate) fn bundle_with_restricted_metric_and_glossary() -> PinnedDefinitions {
    let column = |raw: &str| ColumnName::parse(raw).expect("a test column is a column");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
        description("Orders, one row per order."),
    );
    let revenue = Metric::new(
        MetricName::parse("revenue").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Day, Grain::Month]),
        Vec::new(),
        None,
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let restricted_to_finance = Audience::Restricted(
        sutura_domain::catalog::AudienceGrant::parse(BTreeSet::from([
            AudienceId::parse("finance").expect("a test audience id is one")
        ]))
        .expect("one id grants"),
    );
    let finance_only = Metric::new(
        MetricName::parse("finance_only").expect("a test metric is a metric"),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        description("Only a finance-granted caller may see this."),
        restricted_to_finance,
    )
    .expect("no dimensions to duplicate");
    let definitions = sutura_domain::catalog::Definitions::assemble(vec![model], vec![], vec![revenue, finance_only])
        .expect("the test bundle is consistent");
    let finance_only_name = MetricName::parse("finance_only").expect("a restricted metric is a metric");
    let finance_only_referent = Referent::Metric {
        metric: finance_only_name.clone(),
    };
    let knowledge = Knowledge::assemble(
        &definitions,
        KnowledgeInput::new(
            KnowledgeCapabilities::of([
                Capability::Glossary,
                Capability::Caveats,
                Capability::Examples,
                Capability::Absences,
            ]),
            vec![GlossaryEntry::new(
                Phrase::parse("capital expense").expect("a test phrase is a phrase"),
                BTreeSet::new(),
                finance_only_referent.clone(),
                NoteBody::parse("the finance-only meaning of capex").expect("a test body is a body"),
            )],
            vec![Caveat::new(
                NoteName::parse("finance_only_caveat").expect("a test note name is a name"),
                // About BOTH the open `revenue` and the restricted `finance_only`, so the
                // all-referents rule is the thing under test: a caller who may see only
                // `revenue` still loses this caveat because one of its referents is invisible.
                vec![
                    finance_only_referent,
                    Referent::Metric {
                        metric: MetricName::parse("revenue").expect("a test metric is a metric"),
                    },
                ],
                NoteBody::parse("only a finance-granted caller should trust this number").expect("a test body is a body"),
            )],
            vec![Absence::new(
                Phrase::parse("customer lifetime value").expect("a test phrase is a phrase"),
                BTreeSet::new(),
                NoteBody::parse("CLV is deliberately undefined here").expect("a test body is a body"),
            )],
            vec![Example::new(
                NoteName::parse("finance_only_example").expect("a test note name is a name"),
                vec![Phrase::parse("finance example").expect("a test phrase is a phrase")],
                Query::single(
                    finance_only_name,
                    Grain::Month,
                    crate::testing::june(),
                    Vec::new(),
                    Vec::new(),
                ),
                NoteBody::parse("ask exactly this").expect("a test body is a body"),
            )],
        ),
    )
    .expect("every note names a declared metric");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        knowledge.clone(),
        ContributionManifest::single(
            source(),
            Contribution::of(MetadataCapabilities::produced(&definitions, &knowledge)),
        ),
    )
    .expect("the test definitions hash")
}
