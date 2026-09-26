//! Tests for [`super`].
//!
//! A file rather than an inline `mod tests`, which is this repository's usual shape over the
//! thousand-line gate. The scratch-directory helpers follow `sutura-catalog-okf`'s: each test
//! writes real ODCS v3 contracts to a per-process temp directory and loads through
//! [`super::DataContractCatalog::load`], so a cell asserts on what the adapter reads, not on source
//! text.

use std::path::PathBuf;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::model::SourceName;
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog};

use crate::{DataContractCatalog, DataContractError};

fn test_name() -> SourceName {
    SourceName::parse("test").expect("a test name is a name")
}

fn catalog(root: PathBuf) -> DataContractCatalog {
    DataContractCatalog::new(
        test_name(),
        root,
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
    )
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-catalog-datacontract-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
    dir
}

/// Writes the given `(file, document)` pairs into a fresh scratch directory and opens a catalog over it.
fn catalog_of(name: &str, documents: &[(&str, &str)]) -> (PathBuf, DataContractCatalog) {
    let root = scratch(name);
    for &(file, document) in documents {
        std::fs::write(root.join(file), document).expect("a contract is writable");
    }
    (root.clone(), catalog(root))
}

/// Loads a catalog built over the given contracts, cleaning up the scratch directory on the way out.
fn outcome_of(name: &str, documents: &[(&str, &str)]) -> Result<PinnedDefinitions, DataContractError> {
    let (root, catalog) = catalog_of(name, documents);
    let outcome = catalog.load();
    drop(std::fs::remove_dir_all(&root));
    outcome
}

fn error_for(name: &str, document: &str) -> DataContractError {
    outcome_of(name, &[("contract.yaml", document)]).expect_err("the contract must be refused")
}

/// A complete, realistic v3.1.0 contract over one table with a primary key - the smallest contract
/// that exercises the model, column, type, prose and key decoding this adapter provides.
const ORDERS: &str = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.1.0
schema:
  - name: orders
    physicalName: fct_order
    description: Orders, one row per placed order.
    properties:
      - name: id
        physicalType: bigint
        logicalType: integer
        description: The order's unique identifier.
        primaryKey: true
        primaryKeyPosition: 1
        required: true
      - name: total
        physicalType: double
        logicalType: number
        description: The order's total value.
";

/// A complete v3.1.0 contract whose target column is unique, so the relationship licences a join.
const CUSTOMERS: &str = "
version: 1.0.0
kind: DataContract
id: 22222222-bbbb-4b2a-a65f-222222222222
status: active
name: customers
apiVersion: v3.1.0
schema:
  - name: customers
    physicalName: dim_customer
    description: The people who place orders, one row per person.
    properties:
      - name: id
        physicalType: bigint
        logicalType: integer
        description: The customer's unique identifier.
        primaryKey: true
        primaryKeyPosition: 1
        required: true
";

/// `orders` with a foreign key to `customers.id` (a primary-key target).
const ORDERS_WITH_FK: &str = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.1.0
schema:
  - name: orders
    physicalName: fct_order
    description: Orders, one row per placed order.
    properties:
      - name: id
        physicalType: bigint
        logicalType: integer
        primaryKey: true
        primaryKeyPosition: 1
        required: true
      - name: customer_id
        physicalType: bigint
        logicalType: integer
        description: The customer who placed the order.
    relationships:
      - type: foreignKey
        from: orders.customer_id
        to: customers.id
";

#[test]
fn a_bundle_from_this_vocabulary_loads_pins_and_validates() {
    let pinned = outcome_of("loads", &[("orders.yaml", ORDERS), ("customers.yaml", CUSTOMERS)])
        .expect("a directory of data contracts loads");
    let models = pinned.definitions().models();
    assert_eq!(models.len(), 2, "two contracts load as two models");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = models.get(&orders_name).expect("orders is a model");
    assert_eq!(orders.columns().len(), 2, "orders carries its two columns");
    assert!(!orders.description().is_empty(), "orders carries its description");
    assert_eq!(
        orders.name(),
        &sutura_domain::model::ModelName::parse("orders").expect("a name"),
        "orders is named after the SchemaObject"
    );
    assert_eq!(
        orders.table_name(),
        &sutura_domain::model::TableName::parse("fct_order").expect("a physical table name is a name"),
        "the SchemaObject's physicalName is the physical table"
    );
}

/// `it_provides_exactly_what_it_declares`, lived in the adapter's own crate too: the bundle the
/// adapter produces and the declaration it publishes agree in both directions.
#[test]
fn it_declares_what_it_cannot_supply_and_the_bundle_agrees() {
    let pinned = outcome_of("fidelity", &[("orders.yaml", ORDERS)]).expect("the contract loads");
    let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
    assert_eq!(
        <DataContractCatalog as SemanticCatalog>::capabilities().checked_against(&produced),
        Ok(()),
        "the data-contract catalog's declaration and its bundle disagree"
    );
    // And the honest half of "declares what it cannot supply": a `logicalType: number` column is a
    // data type, NOT a measure, so a bundle with one still carries no metric and says so.
    assert!(
        pinned.definitions().metrics().is_empty(),
        "a data-contract column type is not a measure"
    );
}

/// The relationship the finding licenses: a v3.1 foreign key whose single-column target is a
/// primary key maps to `ManyToOne`, under the `sutura-catalog-rdbms` target-uniqueness rule.
#[test]
fn a_v3_1_foreign_key_with_unique_target_licences_a_many_to_one_join() {
    let pinned = outcome_of(
        "relationship",
        &[("orders.yaml", ORDERS_WITH_FK), ("customers.yaml", CUSTOMERS)],
    )
    .expect("an evidenced relationship loads");
    let relationships = pinned.definitions().relationships();
    assert_eq!(relationships.len(), 1, "the foreign key loads as one relationship");
    let relationship = relationships.values().next().expect("one relationship");
    assert_eq!(relationship.join_type(), sutura_domain::model::JoinType::ManyToOne);
    assert_eq!(
        relationship.origin_model(),
        &sutura_domain::model::ModelName::parse("orders").expect("a name")
    );
    assert_eq!(
        relationship.target_model(),
        &sutura_domain::model::ModelName::parse("customers").expect("a name")
    );
}

/// A foreign key whose target carries no `primaryKey`/`unique` evidence is refused, not guessed at.
#[test]
fn a_relationship_without_single_column_target_uniqueness_evidence_is_refused() {
    let no_key = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.1.0
schema:
  - name: orders
    properties:
      - name: id
        primaryKey: true
      - name: customer_id
    relationships:
      - type: foreignKey
        from: orders.customer_id
        to: customers.id
";
    let plain_customers = "
version: 1.0.0
kind: DataContract
id: 22222222-bbbb-4b2a-a65f-222222222222
status: active
name: customers
apiVersion: v3.1.0
schema:
  - name: customers
    properties:
      - name: id
";
    let err = outcome_of("no-evidence", &[("orders.yaml", no_key), ("customers.yaml", plain_customers)])
        .expect_err("a relationship without target evidence is refused");
    assert!(
        matches!(&err, DataContractError::TargetUniquenessUnknown { table, column } if table == "customers" && column == "id"),
        "the refusal names the target: {err}"
    );
}

/// A v3.0 contract that DOES carry a `relationships` array (invalid against the real v3.0.0
/// schema, but structurally decodable by this adapter's own type) is refused by name rather than
/// silently dropped: an absent field and a dropped one would both load with an empty relationship set.
#[test]
fn a_v3_0_contract_with_a_relationships_array_is_refused_by_name() {
    let v3_0_with_relationships = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.0.0
schema:
  - name: orders
    properties:
      - name: id
        primaryKey: true
      - name: customer_id
    relationships:
      - type: foreignKey
        from: orders.customer_id
        to: customers.id
";
    let err = error_for("v3-0-with-rel", v3_0_with_relationships);
    assert!(
        matches!(err, DataContractError::RelationshipsBeforeV3_1 { .. }),
        "a relationships array before v3.1.0 is refused by name: {err}"
    );
    assert!(err.to_string().contains("v3.0.0"), "{err}");
}

/// A property-level relationship (v3.1+, `from` implicit) is refused by name rather than silently
/// dropped as opaque - this adapter has no join-endpoint context to convert one, and converting it
/// wrongly would mint a join nobody reviewed.
#[test]
fn a_property_level_relationship_is_refused_by_name() {
    let with_property_relationship = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.1.0
schema:
  - name: orders
    properties:
      - name: id
        primaryKey: true
      - name: customer_id
        relationships:
          - type: foreignKey
            to: customers.id
";
    let err = error_for("prop-level-rel", with_property_relationship);
    assert!(
        matches!(err, DataContractError::PropertyLevelRelationshipUnsupported { .. }),
        "a property-level relationship is refused by name: {err}"
    );
    assert!(err.to_string().contains("customer_id"), "{err}");
}

/// From v3.1.0 `team` is `oneOf [a Team object, the deprecated array]` - the object form is
/// schema-valid and must not be refused as malformed.
#[test]
fn a_v3_1_contract_with_an_object_shaped_team_loads() {
    let with_team_object = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.1.0
team:
  name: data-platform
  members:
    - username: user@example.com
schema:
  - name: orders
    properties:
      - name: id
        primaryKey: true
";
    outcome_of("team-object", &[("orders.yaml", with_team_object)]).expect("an object-shaped team is schema-valid and loads");
}

/// v3.2.0 gives a schema-level relationship an optional `id` - a schema-valid field this adapter
/// does not read, but whose absence from the decoded shape would refuse an otherwise-valid
/// v3.2.0 relationship under `deny_unknown_fields`.
#[test]
fn a_v3_2_relationship_with_an_id_loads() {
    let orders_with_rel_id = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.2.0
schema:
  - name: orders
    properties:
      - name: id
        primaryKey: true
      - name: customer_id
    relationships:
      - id: rel-1
        type: foreignKey
        from: orders.customer_id
        to: customers.id
";
    let customers = "
version: 1.0.0
kind: DataContract
id: 22222222-bbbb-4b2a-a65f-222222222222
status: active
name: customers
apiVersion: v3.2.0
schema:
  - name: customers
    properties:
      - name: id
        primaryKey: true
";
    let pinned = outcome_of(
        "rel-id",
        &[("orders.yaml", orders_with_rel_id), ("customers.yaml", customers)],
    )
    .expect("a v3.2.0 relationship carrying an id is schema-valid and loads");
    assert_eq!(
        pinned.definitions().relationships().len(),
        1,
        "the relationship still loads with its id present"
    );
}

/// A relationship endpoint that is neither the `table.column` shorthand nor a composite array -
/// a `FullyQualifiedReference` - is refused by its own name rather than folded into the
/// composite-key refusal it is not.
#[test]
fn a_fully_qualified_reference_endpoint_is_refused_by_its_own_name() {
    let with_fq_reference = "
version: 1.0.0
kind: DataContract
id: 11111111-aaaa-4b2a-a65f-111111111111
status: active
name: orders
apiVersion: v3.1.0
schema:
  - name: orders
    properties:
      - name: customer_id
    relationships:
      - type: foreignKey
        from: orders.customer_id
        to: schema/customers/properties/id
";
    let err = error_for("fq-reference", with_fq_reference);
    assert!(
        matches!(err, DataContractError::UnsupportedReference { .. }),
        "a fully-qualified reference is refused by its own name, not as a composite key: {err}"
    );
}

/// An unknown `apiVersion` is refused by name - the adapter cannot know which fields such a
/// contract may carry (notably whether `relationships` exist).
#[test]
fn an_unknown_api_version_is_refused_by_name() {
    let err = error_for(
        "badversion",
        "version: 1.0.0\nkind: DataContract\nid: x\nstatus: active\napiVersion: v2.2.2\nschema: []\n",
    );
    assert!(
        matches!(err, DataContractError::UnsupportedVersion { .. }),
        "an unknown apiVersion is refused naming it: {err}"
    );
    assert!(err.to_string().contains("v2.2.2"), "{}", err);
}

/// An unknown `kind` is refused by name.
#[test]
fn an_unknown_kind_is_refused_by_name() {
    let err = error_for(
        "badkind",
        "version: 1.0.0\nkind: NotADataContract\nid: x\nstatus: active\napiVersion: v3.1.0\nschema: []\n",
    );
    assert!(
        matches!(err, DataContractError::UnknownKind { .. }),
        "an unknown kind is refused naming it: {err}"
    );
    assert!(err.to_string().contains("NotADataContract"), "{}", err);
}

/// A document field the adapter does not read is refused, not silently dropped.
#[test]
fn a_document_field_it_does_not_declare_fails_the_load() {
    let err = error_for(
        "unknown",
        "version: 1.0.0\nkind: DataContract\nid: x\nstatus: active\napiVersion: v3.1.0\nschema: []\ncompletely_unknown: 1\n",
    );
    assert!(
        matches!(err, DataContractError::Malformed { .. }),
        "an unknown top-level key is refused: {err}"
    );
}

#[test]
fn an_empty_directory_is_an_error() {
    let root = scratch("empty");
    let err = catalog(root.clone()).load();
    drop(std::fs::remove_dir_all(&root));
    assert!(
        matches!(err, Err(DataContractError::Empty { .. })),
        "an empty root loads no catalog: {err:?}"
    );
}

#[test]
fn duplicate_column_names_are_refused() {
    let yaml = "
version: 1.0.0
kind: DataContract
id: x
status: active
apiVersion: v3.1.0
schema:
  - name: orders
    properties:
      - name: id
      - name: id
";
    let err = error_for("duplicate", yaml);
    assert!(
        matches!(err, DataContractError::DuplicateColumn { .. }),
        "a doubled column is refused rather than silently deduplicated: {err}"
    );
}

/// `required: true` maps to `Column.nullable: Some(false)`; an absent `required` stays "no claim"
/// (`None`) rather than inheriting the schema's `false` default.
#[test]
fn a_required_column_is_reported_not_null_and_an_absent_key_is_no_claim() {
    let pinned = outcome_of("required", &[("orders.yaml", ORDERS)]).expect("the contract loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    assert_eq!(
        orders.column(&id).expect("id is declared").nullable(),
        Some(false),
        "a required column is reported not-null"
    );
    let total = sutura_domain::model::ColumnName::parse("total").expect("a fixture column is a column");
    assert_eq!(
        orders.column(&total).expect("total is declared").nullable(),
        None,
        "an absent required key is no claim, not a default"
    );
}

/// A column's `physicalType` (falling back to `logicalType`) and `description` (falling back to
/// `businessName`) arrive on the model's own column, and the `primaryKey` arrives as evidence.
#[test]
fn a_column_s_type_and_description_arrive_on_the_model_s_column() {
    let pinned = outcome_of("column-metadata", &[("orders.yaml", ORDERS)]).expect("the contract loads");
    let orders_name = sutura_domain::model::ModelName::parse("orders").expect("a fixture name is a name");
    let orders = pinned.definitions().models().get(&orders_name).expect("orders is a model");
    let id = sutura_domain::model::ColumnName::parse("id").expect("a fixture column is a column");
    let column = orders.column(&id).expect("id is a declared column");
    assert_eq!(
        column.data_type().map(sutura_domain::catalog::ColumnType::as_str),
        Some("bigint"),
        "physicalType is the column type"
    );
    assert_eq!(column.description(), "The order's unique identifier.");
    // `id` is a primary key, so it arrives as key evidence on the model.
    assert_eq!(
        orders.primary_key(),
        &std::collections::BTreeSet::from([id]),
        "primaryKey is evidence on the model"
    );
}
