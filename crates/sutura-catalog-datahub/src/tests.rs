use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::capabilities::{DeclarableKind, MetadataCapabilities};
use sutura_domain::catalog::{AnchorValue, Definitions, Description, DimensionValue, InconsistentDefinitions, Metric, Model};
use sutura_domain::knowledge::{
    Capability, GlossaryEntry, InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, Phrase,
    Referent,
};
use sutura_domain::measure::{AggregatedColumn, Measure, RequiredFilter, Term, ZeroDenominator};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, MetricName, ModelName, RelationshipName, SourceName, TableName,
};
use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog};

use crate::document::{
    Cardinality, DatasetAspect, MetricAspect, RelationshipAspect, Snapshot, SuturaAnchor, SuturaContent, SuturaDimension,
};
use crate::{AspectReader, DataHubCatalog, DataHubError};

fn name() -> SourceName {
    SourceName::parse("local").expect("a test name is a name")
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test").expect("a test version is a version")
}

/// A reader that serves exactly the snapshot a test hands it.
#[derive(Debug, Clone)]
struct Stub(Snapshot);

impl AspectReader for Stub {
    fn read(&self) -> Result<Snapshot, DataHubError> {
        Ok(self.0.clone())
    }
}

fn over(snapshot: Snapshot) -> DataHubCatalog<Stub> {
    let mut sources = BTreeMap::new();
    drop(sources.insert(String::from("bigquery"), name()));
    DataHubCatalog::new(name(), version(), sources, Stub(snapshot))
}

/// The corpus with its one metric property replaced by `content`.
///
/// The aspect ENVELOPE is what this exists for: `sutura`'s scalar is a JSON document inside a JSON
/// string, so a case that wants to vary the property has to re-spell the four fields around it and
/// re-escape the payload. Written once, the cases below say what they are about.
fn corpus_carrying(content: &str) -> Snapshot {
    let scalar = serde_json::to_string(content).expect("a string serializes");
    let aspect: MetricAspect = serde_json::from_str(&format!(
        r#"{{"name":"revenue","dialect":"ANSI_SQL","expression":"SUM(amount_cents)","sutura":{{"string_value":{scalar}}}}}"#
    ))
    .expect("the aspect around the scalar is well-formed");
    let corpus = corpus();
    Snapshot::new(corpus.datasets().to_vec(), corpus.relationships().to_vec(), vec![aspect])
}

fn dataset(name: &str, table: &str, columns: &[&str], description: &str) -> DatasetAspect {
    DatasetAspect::new(
        name.to_owned(),
        table.to_owned(),
        String::from("bigquery"),
        columns.iter().map(|c| String::from(*c)).collect(),
        description.to_owned(),
    )
}

/// The two models and one relationship the recorded corpus carries, plus one certified metric.
///
/// Since issue #202 the corpus carries a metric with the deployment-defined `sutura.*` content,
/// because the adapter now DECLARES it provides metrics and a declaration measured against a
/// bundle that carried none would be aspirational. The metric is `revenue` over `orders`.
fn corpus() -> Snapshot {
    Snapshot::new(
        vec![
            dataset(
                "orders",
                "fct_order",
                &["order_id", "customer_id", "amount_cents", "order_date", "status"],
                "Net revenue orders, in minor units.",
            ),
            dataset(
                "customers",
                "dim_customer",
                &["customer_id", "segment"],
                "The customer dimension.",
            ),
        ],
        vec![RelationshipAspect::new(
            String::from("orders_to_customer"),
            String::from("orders"),
            String::from("customer_id"),
            String::from("customers"),
            String::from("customer_id"),
            Some(Cardinality::NOne),
        )],
        vec![revenue_metric()],
    )
}

/// The certified metric the corpus carries: `SUM(amount_cents)` on `orders`, monthly.
fn revenue_metric() -> MetricAspect {
    MetricAspect::with_sutura(
        String::from("revenue"),
        String::from("ANSI_SQL"),
        String::from("SUM(amount_cents)"),
        SuturaContent::new(
            String::from("orders"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(
                Aggregate::Sum,
                ColumnName::parse("amount_cents").expect("a test column is a column"),
            ))),
            String::from("order_date"),
            vec![Grain::Month],
        ),
    )
}

/// THE requirement that makes this adapter work alone at all.
///
/// A DataHub-only bundle - models, descriptions, joins and whatever metrics the deployment
/// defined - must LOAD and validate: `Definitions::assemble` has no minimum-metric refusal, so a
/// bundle of models with zero metrics also assembles, pins and validates (`docs/adr/0016`). The
/// corpus here carries one certified metric, because that is what the adapter's declaration now
/// promises.
#[test]
fn the_bundle_of_models_and_one_certified_metric_loads_and_validates() {
    let pinned = over(corpus()).load().expect("the recorded corpus loads");
    assert_eq!(pinned.definitions().models().len(), 2);
    assert_eq!(pinned.definitions().relationships().len(), 1);
    let revenue = pinned
        .definitions()
        .metric(&MetricName::parse("revenue").expect("a test name is a name"))
        .expect("the certified metric is in the bundle");
    assert_eq!(
        revenue.measure(),
        &Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            ColumnName::parse("amount_cents").expect("a test column is a column"),
        )))
    );
}

/// The issue #202 claim, held against the declaration.
///
/// `MetadataCapabilities::produced` reads what the bundle actually carries, and
/// `checked_against` compares it in BOTH directions to what the adapter declared. The declaration
/// now includes `Metrics` and `Grains`, the bundle here carries one certified metric with a grain,
/// and the pair agrees. A metric with no `sutura.*` content is absent from this bundle, which is
/// the other half of the claim: `DataHub` `provides metrics for a metric that carries the custom
/// shape`.
#[test]
fn it_declares_metrics_and_the_bundle_carries_one() {
    let catalog = over(corpus());
    let pinned = catalog.load().expect("the recorded corpus loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        <DataHubCatalog<Stub> as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(())
    );
    assert!(produced.declares(DeclarableKind::Definition(
        sutura_domain::capabilities::DefinitionKind::Metrics
    )));
}

/// A metric whose measure is a raw expression string is READ and never DEFINED.
///
/// `docs/adr/0016` decision 4: `MetricInfo.expression` is a promotion candidate, a string in a
/// dialect nothing here renders, and `aggregationFunction` beside it is authored independently
/// with nothing reconciling the two - so taking either would certify half a definition. A metric
/// entity with no `sutura.*` content stays exactly that: the adapter decodes the aspect and does
/// not convert it, and the bundle carries no metric it did not certify.
#[test]
fn a_metric_without_the_defined_shape_is_reported_and_not_defined() {
    let corpus = corpus();
    let snapshot = Snapshot::new(
        corpus.datasets().to_vec(),
        corpus.relationships().to_vec(),
        vec![MetricAspect::new(
            String::from("revenue"),
            String::from("ANSI_SQL"),
            String::from("SUM(amount_cents)"),
        )],
    );
    let pinned = over(snapshot).load().expect("a metric entity does not stop a load");
    assert!(pinned.definitions().metrics().is_empty(), "the metric is not defined");
}

/// The ratio and `CountIf` shapes convert too - both are closed-vocabulary measures.
///
/// The corpus metric is a simple aggregate; these are the other two measure shapes a certified
/// metric can carry, so the closed vocabulary is exercised past the happy path.
#[test]
fn a_ratio_and_a_count_if_are_carried_as_closed_measures() {
    let corpus = corpus();
    let ratio = MetricAspect::with_sutura(
        String::from("revenue_per_customer"),
        String::from("ANSI_SQL"),
        String::from("SUM(amount_cents) / COUNT(DISTINCT customer_id)"),
        SuturaContent::new(
            String::from("orders"),
            Measure::Ratio {
                numerator: Term::Aggregate(AggregatedColumn::new(
                    Aggregate::Sum,
                    ColumnName::parse("amount_cents").expect("a test column is a column"),
                )),
                denominator: Term::Aggregate(AggregatedColumn::new(
                    Aggregate::CountDistinct,
                    ColumnName::parse("customer_id").expect("a test column is a column"),
                )),
                zero_denominator: ZeroDenominator::Null,
            },
            String::from("order_date"),
            vec![Grain::Month],
        ),
    );
    let count_if = MetricAspect::with_sutura(
        String::from("orders_churned"),
        String::from("ANSI_SQL"),
        String::from("COUNT(CASE WHEN churned THEN 1 END)"),
        SuturaContent::new(
            String::from("orders"),
            Measure::Simple(Term::CountIf {
                column: ColumnName::parse("order_id").expect("a test column is a column"),
            }),
            String::from("order_date"),
            vec![Grain::Month],
        ),
    );
    let snapshot = Snapshot::new(
        corpus.datasets().to_vec(),
        corpus.relationships().to_vec(),
        vec![ratio, count_if],
    );
    let pinned = over(snapshot).load().expect("both closed measures load");
    assert_eq!(pinned.definitions().metrics().len(), 2);
}

/// A certified metric is still checked against the model it names, each defect by its OWN name.
///
/// The `sutura` content names a model and columns, and `Definitions::assemble` is where that has
/// to hold. Three refusals a deployment can hit are provoked by their own snapshot and the inner
/// [`InconsistentDefinitions`] variant is asserted, because the outer [`DataHubError::Inconsistent`]
/// is the same for all of them and "any inconsistency" would pass on any of them.
#[test]
fn a_metric_that_does_not_hold_together_is_refused_by_its_own_defect() {
    let corpus = corpus();
    let datasets = corpus.datasets().to_vec();
    let relationships = corpus.relationships().to_vec();

    let revenue = |model: &str, grains: Vec<Grain>| {
        MetricAspect::with_sutura(
            String::from("revenue"),
            String::from("ANSI_SQL"),
            String::from("SUM(amount_cents)"),
            SuturaContent::new(
                String::from(model),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(
                    Aggregate::Sum,
                    ColumnName::parse("amount_cents").expect("a test column is a column"),
                ))),
                String::from("order_date"),
                grains,
            ),
        )
    };

    // The model is carried, but the measure's column is not on it.
    let refused = over(Snapshot::new(
        datasets.clone(),
        relationships.clone(),
        vec![revenue("customers", vec![Grain::Month])],
    ))
    .load()
    .expect_err("a measure on a column its model lacks does not load");
    assert!(
        matches!(
            refused,
            DataHubError::Inconsistent {
                cause: InconsistentDefinitions::UnknownMeasureColumn { .. }
            }
        ),
        "{refused:?}"
    );

    // A model the bundle does not carry at all.
    let refused = over(Snapshot::new(
        datasets.clone(),
        relationships.clone(),
        vec![revenue("does_not_exist", vec![Grain::Month])],
    ))
    .load()
    .expect_err("a metric naming a missing model does not load");
    assert!(
        matches!(
            refused,
            DataHubError::Inconsistent {
                cause: InconsistentDefinitions::UnknownModel { .. }
            }
        ),
        "{refused:?}"
    );

    // No grains: `Definitions::assemble` refuses a grainless metric as `NoGrains`.
    let refused = over(Snapshot::new(datasets, relationships, vec![revenue("orders", Vec::new())]))
        .load()
        .expect_err("a grainless metric does not load");
    assert!(
        matches!(
            refused,
            DataHubError::Inconsistent {
                cause: InconsistentDefinitions::NoGrains { .. }
            }
        ),
        "{refused:?}"
    );
}

/// A property this namespace does not define, written where an operator would write it - as a
/// key at the content's top level - is refused BY NAME, through the load path's typed error.
///
/// The domain guard over the measure's own grammar is the previous test, but that one is a
/// property inside the measure; the case an operator hits the day they write a key the namespace
/// does not carry is the top level, and it must refuse naming the property rather than with a
/// bare serde string.
#[test]
fn an_unknown_key_at_the_sutura_level_is_refused_and_named() {
    let json = r#"{
            "name": "revenue",
            "dialect": "ANSI_SQL",
            "expression": "SUM(amount_cents)",
            "sutura": { "string_value": "{\"model\":\"orders\",\"measure\":{\"simple\":{\"aggregate\":\"sum\",\"column\":\"amount_cents\"}},\"time_column\":\"order_date\",\"grains\":[\"month\"],\"optimized\":true}" }
        }"#;
    let decoded: MetricAspect =
        serde_json::from_str(json).expect("the outer aspect decodes: the unknown key is inside the scalar");
    let message = decoded
        .sutura()
        .expect("the metric carries sutura content")
        .assemble()
        .expect_err("a property the namespace does not define must not read")
        .to_string();
    assert!(message.contains("optimized"), "the refusal names the property: {message}");
}

/// The measure vocabulary stays closed wherever the unknown property sits - here inside the
/// measure's own grammar, reached through the same scalar decode.
#[test]
fn an_unsupported_sutura_property_inside_the_closed_shapes_fails_to_read() {
    let json = r#"{
            "name": "revenue",
            "dialect": "ANSI_SQL",
            "expression": "SUM(amount_cents)",
            "sutura": { "string_value": "{\"model\":\"orders\",\"measure\":{\"simple\":{\"aggregate\":\"sum\",\"column\":\"amount_cents\",\"optimized\":true}},\"time_column\":\"order_date\",\"grains\":[\"month\"]}" }
        }"#;
    let decoded: MetricAspect = serde_json::from_str(json).expect("the outer aspect decodes");
    let message = decoded
        .sutura()
        .expect("the metric carries sutura content")
        .assemble()
        .expect_err("a measure carrying a property the domain does not define must not read")
        .to_string();
    assert!(message.contains("optimized"), "the refusal names the property: {message}");
}

/// The rest of a metric - a definitional filter, a dimension with its allowlist, an anchor and
/// prose - rides the `sutura` property into the certified metric, which is issue #202's scope
/// closed past the measure: `docs/adr/0011` leaves no other route by which any of these could
/// attach to a metric this adapter defines.
#[test]
fn the_rest_of_a_metric_rides_the_deployment_defined_namespace() {
    let corpus = corpus();
    let snapshot = Snapshot::new(
        corpus.datasets().to_vec(),
        corpus.relationships().to_vec(),
        vec![MetricAspect::with_sutura(
            String::from("revenue"),
            String::from("ANSI_SQL"),
            String::from("SUM(amount_cents)"),
            SuturaContent::full(
                String::from("orders"),
                Measure::Simple(Term::Aggregate(AggregatedColumn::new(
                    Aggregate::Sum,
                    ColumnName::parse("amount_cents").expect("a test column is a column"),
                ))),
                String::from("order_date"),
                vec![Grain::Month],
                String::from("Net revenue from active orders, in minor units."),
                vec![RequiredFilter::Equals {
                    column: ColumnName::parse("status").expect("a test column is a column"),
                    value: DimensionValue::parse("active").expect("a test value is a value"),
                }],
                vec![SuturaDimension::new(
                    DimensionName::parse("segment").expect("a test dimension name is a name"),
                    ColumnName::parse("segment").expect("a test column is a column"),
                    Some(RelationshipName::parse("orders_to_customer").expect("a test relationship is a name")),
                    Some(
                        [
                            DimensionValue::parse("retail").expect("a test value is a value"),
                            DimensionValue::parse("wholesale").expect("a test value is a value"),
                        ]
                        .into(),
                    ),
                    Description::parse("The customer's segment.").expect("a description is a description"),
                )],
                Some(SuturaAnchor::new(
                    TimeRange::new(
                        Date::parse("2026-06-01").expect("a test date is a date"),
                        Date::parse("2026-07-01").expect("a test date is a date"),
                    )
                    .expect("a one-month range is a range"),
                    AnchorValue::parse("412345").expect("a test anchor value is a value"),
                )),
            ),
        )],
    );
    let pinned = over(snapshot).load().expect("the full metric loads");
    let revenue = pinned
        .definitions()
        .metric(&MetricName::parse("revenue").expect("a test name is a name"))
        .expect("the certified metric is in the bundle");
    assert_eq!(revenue.description(), "Net revenue from active orders, in minor units.");
    assert_eq!(
        revenue.required_filters(),
        &[RequiredFilter::Equals {
            column: ColumnName::parse("status").expect("a test column is a column"),
            value: DimensionValue::parse("active").expect("a test value is a value"),
        }],
    );
    let segment = revenue
        .dimensions()
        .get(&DimensionName::parse("segment").expect("a test dimension name is a name"))
        .expect("the dimension is declared");
    assert_eq!(
        segment.allowed_values().map(std::collections::BTreeSet::len),
        Some(2),
        "the dimension carries its allowlist"
    );
    let anchor = revenue.anchor().expect("the anchor is declared");
    assert_eq!(anchor.value(), "412345");
    assert_eq!(
        anchor.range(),
        TimeRange::new(
            Date::parse("2026-06-01").expect("a date"),
            Date::parse("2026-07-01").expect("a date")
        )
        .expect("a range")
    );
}

/// A relationship alone licenses no dimension - cardinality is produced only by a dimension
/// declared `via` one.
///
/// The fixture's relationship is representable (`N_1`), yet the corpus's metric has no
/// `via`-dimension, so the bundle carries no `Cardinality` - a capability is observed only as *a
/// dimension reached through the relationship*, which is why the declaration marks it
/// declared-and-empty rather than unconditional. And a relationship this adapter cannot vouch
/// for - absent or many-to-many - is still refused naming it, because the `N_N` default makes
/// an unconsidered relationship indistinguishable from a considered one (`docs/adr/0016` decision
/// 5). A metric DECLARING a dimension `via` such a relationship is the part that observes
/// `Cardinality`; that is the `the_rest_of_a_metric_rides_the_deployment_defined_namespace`
/// test's subject.
fn relationship_with(cardinality: Option<Cardinality>) -> Snapshot {
    let corpus = corpus();
    Snapshot::new(
        corpus.datasets().to_vec(),
        vec![RelationshipAspect::new(
            String::from("orders_to_customer"),
            String::from("orders"),
            String::from("customer_id"),
            String::from("customers"),
            String::from("customer_id"),
            cardinality,
        )],
        Vec::new(),
    )
}

#[test]
fn a_relationship_alone_licenses_no_dimension() {
    let pinned = over(corpus()).load().expect("the recorded corpus loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert!(
        !produced.declares(DeclarableKind::Definition(
            sutura_domain::capabilities::DefinitionKind::Cardinality
        )),
        "a relationship may exist without licensing a dimension"
    );

    let refused = over(relationship_with(Some(Cardinality::NN)))
        .load()
        .expect_err("a many-to-many relationship is content this adapter does not provide");
    assert!(
        matches!(
            refused,
            DataHubError::CardinalityUnrepresentable { ref name, cardinality: "many-to-many" } if name == "orders_to_customer"
        ),
        "{refused:?}"
    );

    let refused = over(relationship_with(None))
        .load()
        .expect_err("an undeclared cardinality is refused, not defaulted");
    assert!(
        matches!(
            refused,
            DataHubError::CardinalityUnrepresentable { ref name, cardinality: "undeclared" } if name == "orders_to_customer"
        ),
        "{refused:?}"
    );
}

/// Content for a knowledge kind the adapter did not declare fails the load.
///
/// `Knowledge::assemble`'s `UndeclaredContent` guard refuses a bundle whose input carries notes
/// for a capability the declared `KnowledgeCapabilities` do not cover - the "content for a kind
/// it did not declare" shape. A standalone `DataHub` bundle never reaches it, because a note
/// needs a metric for its `Referent` to name and this adapter provides no metrics; the wiring is
/// the point here. Built and refused through the adapter's own types, so the guard is reachable
/// the day a composed bundle feeds one in, and the failure lands as `DataHubError::Knowledge`
/// rather than as prose.
#[test]
fn content_for_a_kind_it_did_not_declare_fails_the_load() {
    let model = Model::new(
        ModelName::parse("orders").expect("a model name is a name"),
        name(),
        TableName::parse("fct_order").expect("a table name is a name"),
        [
            ColumnName::parse("amount_cents").expect("a column is a name"),
            ColumnName::parse("order_date").expect("a column is a name"),
        ]
        .into_iter()
        .collect(),
        Description::parse("Net revenue orders.").expect("a description is a description"),
    );
    let metric = Metric::new(
        MetricName::parse("revenue").expect("a metric name is a name"),
        ModelName::parse("orders").expect("a model name is a name"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(
            Aggregate::Sum,
            ColumnName::parse("amount_cents").expect("a column is a name"),
        ))),
        Vec::new(),
        ColumnName::parse("order_date").expect("a column is a name"),
        std::iter::once(Grain::Month).collect(),
        Vec::new(),
        None,
        Description::parse("").expect("empty is a description"),
    )
    .expect("no dimensions to duplicate");
    let definitions = Definitions::assemble(vec![model], Vec::new(), vec![metric]).expect("a model and a metric hold together");
    let entry = GlossaryEntry::new(
        Phrase::parse("revenue").expect("a phrase is a phrase"),
        BTreeSet::new(),
        Referent::Metric {
            metric: MetricName::parse("revenue").expect("a metric name is a name"),
        },
        NoteBody::parse("Net revenue in minor units.").expect("a note body is a body"),
    );

    let refused = Knowledge::assemble(
        &definitions,
        KnowledgeInput::new(KnowledgeCapabilities::none(), vec![entry], Vec::new(), Vec::new(), Vec::new()),
    )
    .expect_err("a glossary under a declaration that provides none is undeclared content");
    assert!(
        matches!(
            refused,
            InconsistentKnowledge::UndeclaredContent {
                capability: Capability::Glossary,
                ..
            }
        ),
        "{refused:?}"
    );
    // The adapter's typed error carries it, proving the load path maps it rather than swallowing
    // it.
    let _: DataHubError = DataHubError::Knowledge { cause: refused };
}

/// A property declaring one dimension twice is refused, **with the markdown adapter's refusal**.
///
/// **This is #266's D4's runtime evidence, and it is worth more than two separate assertions.** The
/// defect was not that either adapter was wrong on its own: it was that one content produced two
/// different `Definitions` depending on which adapter read it. The markdown adapter refused a
/// repeated `name:` with an error of its own; this one collected the sequence into a map keyed by
/// name and kept the LAST entry, so the metric loaded with the second column and nothing said so.
///
/// So what this asserts is the *same value* `sutura_catalog_local`'s
/// `a_dimension_declared_twice_is_refused_rather_than_deduplicated` asserts -
/// `InconsistentDefinitions::DuplicateDimension` naming the metric and the dimension. `Metric::new`
/// takes a `Vec` now, so there is nowhere earlier for either adapter to collapse the pair, and the
/// agreement is a property of the signature rather than of two checks staying in step.
///
/// Decoded from the property rather than built by calling `SuturaDimension::new` twice, for the
/// reason the anchor test below gives: a real reader decodes a sequence it did not write, and the
/// keying that dropped the duplicate happened after the decode.
#[test]
fn a_property_declaring_one_dimension_twice_is_refused_rather_than_deduplicated() {
    let content = concat!(
        r#"{"model":"orders","measure":{"simple":{"aggregate":"sum","column":"amount_cents"}},"#,
        r#""time_column":"order_date","grains":["month"],"dimensions":["#,
        r#"{"name":"region","column":"region_code"},"#,
        r#"{"name":"region","column":"other_code"}]}"#
    );
    let refused = over(corpus_carrying(content))
        .load()
        .expect_err("one metric declaring one dimension twice is not a metric");
    assert!(
        matches!(
            refused,
            DataHubError::Inconsistent {
                cause: InconsistentDefinitions::DuplicateDimension { ref metric, ref dimension },
            } if metric.as_str() == "revenue" && dimension.as_str() == "region"
        ),
        "the domain's own refusal reaches this adapter, unchanged: {refused:?}"
    );
}

/// An anchor value a reader could not read is refused where the property is decoded.
///
/// **The other half of #266's D3, on the adapter that has no file to open.** The value
/// `4123<U+200F>45` renders as an ordinary number wherever it is printed, and it is what
/// `sutura_domain::pinned::NotValidated::AnchorMismatch` interpolates when a bundle is refused. It
/// used to load, because the domain's `Anchor::new` took a `String`.
///
/// Built by deserializing the aspect rather than by calling a constructor, and that is the point: a
/// real reader decodes a scalar it did not write, so the refusal has to come from the decode. It
/// does - `AnchorValue` deserializes through its own constructor, and a struct field (unlike the
/// local adapter's `untagged` literal) keeps the cause. Both adapters are therefore held to the one
/// character rule on the one field.
#[test]
fn an_anchor_value_a_reader_could_not_read_is_refused() {
    let content = format!(
        r#"{{"model":"orders","measure":{{"simple":{{"aggregate":"sum","column":"amount_cents"}}}},"time_column":"order_date","grains":["month"],"anchor":{{"range":{{"start":"2026-06-01","end":"2026-07-01"}},"value":"4123{}45"}}}}"#,
        '\u{200F}'
    );
    let refused = over(corpus_carrying(&content))
        .load()
        .expect_err("a direction-changing character is not an anchor value");
    let DataHubError::Sutura { ref metric, ref cause } = refused else {
        panic!("the property's own decode is what refuses it: {refused:?}");
    };
    assert_eq!(metric, "revenue");
    assert!(
        cause.to_string().contains("invisible or direction-changing"),
        "the character rule names itself through the property's decode: {cause}"
    );
}
