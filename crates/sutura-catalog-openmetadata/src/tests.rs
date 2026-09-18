//! The decision half of the `OpenMetadata` adapter, held against its declaration.
//!
//! Every cell reads a fake reader over RECORDED documents (a JSON snapshot), never mocked HTTP —
//! the port's own rule. The corpus is the narrow/normal case the finding names: models,
//! descriptions and a declared non-duplicating join, with metrics whose measures are expression
//! strings and so stay reported-not-defined.

use std::collections::BTreeMap;

use sutura_domain::capabilities::{DeclarableKind, DefinitionKind, MetadataCapabilities};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::SemanticCatalog;

use crate::document::Snapshot;
use crate::{OpenMetadataCatalog, OpenMetadataError, SnapshotReader};

fn name() -> SourceName {
    SourceName::parse("local").expect("a test name is a name")
}

fn version() -> sutura_domain::pinned::DefinitionVersion {
    sutura_domain::pinned::DefinitionVersion::parse("test").expect("a test version is a version")
}

/// A reader that serves exactly the snapshot a test hands it.
#[derive(Debug, Clone)]
struct Stub(Snapshot);

impl SnapshotReader for Stub {
    type Error = OpenMetadataError;

    fn read(&self) -> Result<Snapshot, Self::Error> {
        Ok(self.0.clone())
    }
}

fn over(snapshot: Snapshot) -> OpenMetadataCatalog<Stub> {
    let mut sources = BTreeMap::new();
    drop(sources.insert(String::from("warehouse"), name()));
    OpenMetadataCatalog::new(name(), version(), sources, Stub(snapshot))
}

/// The recorded corpus: two tables (`orders`, `customers`), one declared one-to-many join, and one
/// metric whose measure is an expression string.
fn corpus() -> Snapshot {
    serde_json::from_str::<Snapshot>(
        r#"{
          "tables": [
            {"service":"warehouse","name":"orders","columns":["order_id","customer_id","amount_cents","order_date","status"],"description":"Net revenue orders, in minor units."},
            {"service":"warehouse","name":"customers","columns":["customer_id","segment"],"description":"The customer dimension."}
          ],
          "relationships": {
            "orders_to_customer": {"origin_model":"orders","origin_column":"customer_id","target_model":"customers","target_column":"customer_id","relationship_type":"ONE_TO_MANY"}
          },
          "metrics": [
            {"name":"revenue","metricType":"SUM","granularity":"DAY","expression":"SUM(amount_cents)"}
          ]
        }"#,
    )
    .expect("the recorded corpus is well-formed")
}

/// A snapshot whose single relationship carries exactly `relationship_type` (or none).
fn relationship_carrying(relationship_type: Option<&str>) -> Snapshot {
    let rel = relationship_type.map_or_else(
        || {
            r#"{"origin_model":"orders","origin_column":"customer_id","target_model":"customers","target_column":"customer_id"}"#
                .to_owned()
        },
        |kind| {
            format!(
                r#"{{"origin_model":"orders","origin_column":"customer_id","target_model":"customers","target_column":"customer_id","relationship_type":"{kind}"}}"#
            )
        },
    );
    serde_json::from_str::<Snapshot>(&format!(
        r#"{{"tables":[
             {{"service":"warehouse","name":"orders","columns":["order_id","customer_id"],"description":"Orders."}},
             {{"service":"warehouse","name":"customers","columns":["customer_id"],"description":"Customers."}}],
         "relationships":{{"orders_to_customer":{rel}}},
         "metrics":[]}}"#
    ))
    .expect("a relationship-only snapshot is well-formed")
}

/// THE requirement that makes this adapter work alone at all.
///
/// A bundle of models, descriptions and one declared non-duplicating join — with the metric left
/// reported-not-defined — must LOAD and validate: `Definitions::assemble` has no minimum-metric
/// refusal, so a bundle with zero metrics still assembles, pins and validates.
#[test]
fn a_bundle_of_models_and_no_metrics_loads_and_validates() {
    let pinned = over(corpus()).load().expect("the recorded corpus loads");
    assert_eq!(pinned.definitions().models().len(), 2);
    assert_eq!(pinned.definitions().relationships().len(), 1);
    // The metric is carried by the snapshot but never minted: reported, not defined.
    assert!(pinned.definitions().metrics().is_empty());
}

/// The issue #152 claim, held against the declaration.
///
/// `MetadataCapabilities::produced` reads what the bundle actually carries, and `checked_against`
/// compares it in BOTH directions to what the adapter declared. The declaration supplies
/// `Structure`/`Descriptions`/`Relationships` (the bundle carries all three) and declares
/// `Metrics`/`Grains`/`Cardinality` as declared-and-empty, so a bundle with a metric left
/// reported-not-defined still agrees.
#[test]
fn it_declares_what_it_cannot_supply_and_the_bundle_agrees() {
    let catalog = over(corpus());
    let pinned = catalog.load().expect("the recorded corpus loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        <OpenMetadataCatalog<Stub> as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(())
    );
    // The bundle provides the three kinds the adapter declares it provides ...
    assert!(produced.declares(DeclarableKind::Definition(DefinitionKind::Structure)));
    assert!(produced.declares(DeclarableKind::Definition(DefinitionKind::Descriptions)));
    // ... and correctly does NOT claim kinds it cannot supply: the metric stays reported-not-defined
    // (Metrics is declared-and-empty may-provide, which `checked_against`'s Unprovided direction
    // exempts), and RequiredFilters/Anchors are not declared at all.
    assert!(!produced.declares(DeclarableKind::Definition(DefinitionKind::Metrics)));
    assert!(!produced.declares(DeclarableKind::Definition(DefinitionKind::RequiredFilters)));
    assert!(!produced.declares(DeclarableKind::Definition(DefinitionKind::Anchors)));
}

/// A metric whose measure is an expression string is READ and never DEFINED.
///
/// `metricType` is decidable but the bound column is not resolvable from a foreign-dialect
/// expression, and taking either would certify half a definition (ADR 0016). The metric is decoded
/// and set aside; the bundle carries no metric it did not certify.
#[test]
fn a_metric_whose_measure_is_an_expression_string_is_reported_and_not_defined() {
    let pinned = over(corpus()).load().expect("a metric entity does not stop a load");
    assert!(pinned.definitions().metrics().is_empty(), "the metric is not defined");
    let corpus = corpus();
    let reported = &corpus.metrics()[0];
    assert_eq!(reported.aggregation(), "SUM", "the metric is reported, not defined");
    assert_eq!(reported.granularity(), Some("DAY"));
    assert_eq!(reported.expression(), Some("SUM(amount_cents)"));
}

/// A table without a description is refused, not defaulted.
///
/// `Descriptions` is a provided kind and a model that carried none would leave the declaration
/// unproduced for it.
#[test]
fn a_table_without_a_description_is_refused() {
    let snapshot: Snapshot = serde_json::from_str(
        r#"{"tables":[{"service":"warehouse","name":"orders","columns":["order_id"]}],"relationships":{},"metrics":[]}"#,
    )
    .expect("the snapshot is well-formed");
    let error = over(snapshot).load().expect_err("a descriptionless table is refused");
    match error {
        OpenMetadataError::MissingDescription { on } => assert_eq!(on, "orders"),
        other => panic!("expected MissingDescription, got {other:?}"),
    }
}

/// Content for a kind this adapter did not declare it can represent — a many-to-many
/// relationship — fails the load, naming it.
///
/// `JoinType` has no many-to-many shape; `OpenMetadata` declares cardinality when present, and a
/// row-duplicating one licenses no join here.
#[test]
fn content_for_a_kind_it_did_not_declare_fails_the_load() {
    let snapshot = relationship_carrying(Some("MANY_TO_MANY"));
    let error = over(snapshot).load().expect_err("a many-to-many relationship is refused");
    assert!(matches!(
        error,
        OpenMetadataError::CardinalityUnrepresentable {
            cardinality: "many-to-many",
            ..
        }
    ));
}

/// A relationship whose cardinality is undeclared licenses nothing and is refused.
#[test]
fn an_undeclared_relationship_licenses_nothing() {
    let snapshot = relationship_carrying(None);
    let error = over(snapshot).load().expect_err("an undeclared relationship is refused");
    assert!(matches!(
        error,
        OpenMetadataError::CardinalityUnrepresentable {
            cardinality: "undeclared",
            ..
        }
    ));
}

/// The three declared non-duplicating cardinalities each map to the corresponding `JoinType`.
#[test]
fn the_declared_non_duplicating_cardinalities_convert() {
    for (kind, expected) in [
        ("ONE_TO_ONE", sutura_domain::model::JoinType::OneToOne),
        ("MANY_TO_ONE", sutura_domain::model::JoinType::ManyToOne),
        ("ONE_TO_MANY", sutura_domain::model::JoinType::OneToMany),
    ] {
        let pinned = over(relationship_carrying(Some(kind)))
            .load()
            .expect("a declared cardinality converts");
        let join_type = pinned
            .definitions()
            .relationships()
            .iter()
            .next()
            .map(|(_, relationship)| relationship.join_type())
            .expect("the snapshot carries one relationship");
        assert_eq!(join_type, expected, "for cardinality {kind}");
    }
}

/// A model on a source this deployment does not read is refused, not guessed.
#[test]
fn a_table_on_an_unknown_source_is_refused() {
    let snapshot: Snapshot = serde_json::from_str(
        r#"{"tables":[{"service":"not_configured","name":"orders","columns":["order_id"],"description":"Orders."}],"relationships":{},"metrics":[]}"#,
    )
    .expect("the snapshot is well-formed");
    let error = over(snapshot).load().expect_err("an unknown source is refused");
    assert!(matches!(error, OpenMetadataError::UnknownSource { model, .. } if model == "orders"));
}
