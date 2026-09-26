//! The serde wire shapes of an ODCS v3 contract document, split out of `lib.rs` to keep both
//! files under the workspace's line bound - the same split `sutura-catalog-openmetadata` and
//! `sutura-catalog-datahub` already make between decoding and deciding. Every field here is
//! read-only wire structure; the DECISIONS - which fields become a `Model`, a `Column` or a
//! `Relationship`, and which are read and left unsurfaced - live in `lib.rs`, per
//! `docs/what-a-data-contract-can-carry.md`.

/// The serde shape of an ODCS v3 contract document, as the Open Data Contract Standard defines it.
///
/// `deny_unknown_fields` at this depth is the fidelity that a contract which carries a key the
/// adapter does not read still fails the load rather than vanishing. Only the fields the finding
/// marks as used are decoded: the top level's `apiVersion`/`kind` (to refuse an unknown version or
/// kind by name) and the `schema` array; `servers` (the physical location), the SLA surface and the
/// per-object/per-column `quality`, `classification`, `enum`, `semanticType`, `context` and `synonyms`
/// are all accepted and unsurfaced - reported-not-defined or absent-by-declaration, per
/// `docs/what-a-data-contract-can-carry.md`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Contract {
    pub(crate) api_version: String,
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) schema: Vec<SchemaObject>,
    #[expect(dead_code, reason = "the contract's id - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[expect(dead_code, reason = "the standard's own `version` - the reading version is `apiVersion`")]
    #[serde(default)]
    pub(crate) version: Option<String>,
    #[expect(dead_code, reason = "the contract's name - the model is the SchemaObject's")]
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[expect(dead_code, reason = "the tenant that owns the contract - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) tenant: Option<String>,
    #[expect(dead_code, reason = "`active`/`deprecated` - lifecycle, not structure")]
    #[serde(default)]
    pub(crate) status: Option<String>,
    #[expect(dead_code, reason = "the domain the contract belongs to - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) domain: Option<String>,
    #[expect(dead_code, reason = "deprecated since v3.1.0 - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) data_product: Option<String>,
    #[expect(dead_code, reason = "the physical location of the data - reported, not a declared kind")]
    #[serde(default)]
    pub(crate) servers: Vec<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "contract-level usage/purpose/limitations prose - not a per-model description"
    )]
    #[serde(default)]
    pub(crate) description: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "a creation timestamp - not a field this adapter reads")]
    #[serde(default, rename = "contractCreatedTs")]
    pub(crate) created_ts: Option<String>,
    #[expect(dead_code, reason = "a pricing object - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) price: Option<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "a glossary of term definitions - declared out by the finding, `docs/what-a-data-contract-can-carry.md`"
    )]
    #[serde(default)]
    pub(crate) authoritative_definitions: Vec<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "v3.2 AI-agent context - prose a Referent cannot anchor to, `docs/what-a-data-contract-can-carry.md`"
    )]
    #[serde(default)]
    pub(crate) context: Option<serde_norway::Value>,
    // From v3.1.0 `team` is `oneOf [a Team object, the deprecated array]` - an opaque `Value`
    // accepts either shape without deciding between them, which `Vec<Value>` alone would refuse
    // for a v3.1+ contract that wrote the object form.
    #[expect(dead_code, reason = "team membership - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) team: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "user roles - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) roles: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "support contact - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) support: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "the SLA surface - reported and ignored by this adapter's declaration")]
    #[serde(default)]
    pub(crate) sla_default_element: Option<String>,
    #[expect(dead_code, reason = "the SLA surface - reported and ignored by this adapter's declaration")]
    #[serde(default)]
    pub(crate) sla_properties: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "extension properties - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) custom_properties: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "contract-level tags - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) tags: Vec<serde_norway::Value>,
}

/// One `schema[]` entry: a physical table of the contract, and the columns and joins it exposes.
///
/// The `SchemaElement` base contributes `name` (required), `physicalType`, `description`,
/// `businessName`, `authoritativeDefinitions`, `tags`, `customProperties`, `id`, `deprecated` and
/// (v3.2.0) `synonyms`; the `SchemaObject` itself adds `physicalName`, `logicalType`,
/// `dataGranularityDescription`, `properties`, `quality` and (v3.1+) `relationships`. From v3.1.0
/// the standard also gives it `context` at the v3.2.0 tag.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SchemaObject {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) physical_name: Option<String>,
    #[expect(
        dead_code,
        reason = "the physical representation (`table`, `view`, …) - a hint, not structure"
    )]
    #[serde(default)]
    pub(crate) physical_type: Option<String>,
    #[expect(dead_code, reason = "a `SchemaObject` is always of logical type `object`")]
    #[serde(default)]
    pub(crate) logical_type: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) business_name: Option<String>,
    #[serde(default)]
    pub(crate) properties: Vec<SchemaProperty>,
    /// The joins this table declares to other tables, present only from v3.1.0. Gated at read time
    /// on the contract's `apiVersion`, because a v3.0 contract has no such field.
    #[serde(default)]
    pub(crate) relationships: Vec<RelationshipDecl>,
    #[expect(
        dead_code,
        reason = "a per-object SLA / free-text granularity note - reported, not a defined kind"
    )]
    #[serde(default)]
    pub(crate) data_granularity_description: Option<String>,
    #[expect(
        dead_code,
        reason = "per-table data-quality rules - reported, not defined (`docs/what-a-data-contract-can-carry.md`)"
    )]
    #[serde(default)]
    pub(crate) quality: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "v3.2 AI-agent context - prose, no Referent")]
    #[serde(default)]
    pub(crate) context: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "the object's id - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[expect(dead_code, reason = "definitions of business terms - declared out by the finding")]
    #[serde(default)]
    pub(crate) authoritative_definitions: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "tags on the object - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) tags: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "extension properties - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) custom_properties: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "a deprecation flag - lifecycle, not structure")]
    #[serde(default)]
    pub(crate) deprecated: Option<bool>,
    #[expect(dead_code, reason = "v3.2 business-vocabulary synonyms - prose, no Referent")]
    #[serde(default)]
    pub(crate) synonyms: Vec<serde_norway::Value>,
}

/// One `properties[]` entry: a column of a [`SchemaObject`].
///
/// The union of the `SchemaElement` base and the `SchemaBaseProperty` additions. `name` is required
/// (a schema-property must name itself); `physicalType`, `physicalName` and `logicalType` are the
/// type surface; `description`/`businessName` are the prose; `primaryKey`/`primaryKeyPosition`,
/// `required` and `unique` are the constraint evidence this adapter reads. The transform metadata,
/// `classification`, `enum`, `semanticType`, `partitioned`, `examples`, `criticalDataElement` and
/// `quality` are accepted and unsurfaced; a non-empty property-level `relationships` is detected
/// and refused by name rather than accepted opaque (`docs/what-a-data-contract-can-carry.md`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct SchemaProperty {
    pub(crate) name: String,
    #[expect(
        dead_code,
        reason = "the physical name of the column - `physicalType` is the type this adapter reads"
    )]
    #[serde(default)]
    pub(crate) physical_name: Option<String>,
    #[serde(default)]
    pub(crate) physical_type: Option<String>,
    #[serde(default)]
    pub(crate) logical_type: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) business_name: Option<String>,
    /// Not-null evidence: a column with `required: true` is mapped to `nullable: Some(false)`, and
    /// an absent key is "no claim" (`None`) rather than the schema's `false` default - `issue #973`.
    #[serde(default)]
    pub(crate) required: Option<bool>,
    /// `unique` is single-column target evidence for the relationship rule, the same role a
    /// `primaryKey` column fills.
    #[serde(default)]
    pub(crate) unique: Option<bool>,
    #[serde(default)]
    pub(crate) primary_key: Option<bool>,
    #[serde(default)]
    pub(crate) primary_key_position: Option<i64>,
    #[expect(
        dead_code,
        reason = "the free-text per-column classification - reported, not defined, `docs/what-a-data-contract-can-carry.md`"
    )]
    #[serde(default)]
    pub(crate) classification: Option<String>,
    #[expect(dead_code, reason = "transform metadata - how a column is derived, not a definition")]
    #[serde(default)]
    pub(crate) encrypted_name: Option<String>,
    #[expect(dead_code, reason = "transform metadata - how a column is derived, not a definition")]
    #[serde(default)]
    pub(crate) transform_source_objects: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "transform metadata - how a column is derived, not a definition")]
    #[serde(default)]
    pub(crate) transform_logic: Option<String>,
    #[expect(dead_code, reason = "transform metadata - how a column is derived, not a definition")]
    #[serde(default)]
    pub(crate) transform_description: Option<String>,
    #[expect(dead_code, reason = "sample values - data, not definition")]
    #[serde(default)]
    pub(crate) examples: Vec<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "allowed values - v3.2's `AllowedValues`, declared out: no metric owns a dimension here"
    )]
    #[serde(default)]
    r#enum: Vec<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "v3.2 `semanticType: measure` is a transform string - reported, not defined"
    )]
    #[serde(default)]
    pub(crate) semantic_type: Option<String>,
    #[expect(dead_code, reason = "v3.2 logical-type options (precision, scale, …) - a type hint")]
    #[serde(default)]
    pub(crate) logical_type_options: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "partitioning metadata - a physical layout hint")]
    #[serde(default)]
    pub(crate) partitioned: Option<bool>,
    #[expect(dead_code, reason = "partitioning metadata - a physical layout hint")]
    #[serde(default)]
    pub(crate) partition_key_position: Option<i64>,
    #[expect(
        dead_code,
        reason = "a critical-data-element flag - a classification hint, not a definition"
    )]
    #[serde(default)]
    pub(crate) critical_data_element: Option<bool>,
    // Read only to detect presence and refuse it by name (`DataContractError::PropertyLevelRelationshipUnsupported`) -
    // this adapter has no join-endpoint context for a `from` implicit at the property level.
    #[serde(default)]
    pub(crate) relationships: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "per-column data-quality rules - reported, not defined")]
    #[serde(default)]
    pub(crate) quality: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "the property's id - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[expect(dead_code, reason = "definitions of business terms - declared out by the finding")]
    #[serde(default)]
    pub(crate) authoritative_definitions: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "tags on the column - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) tags: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "extension properties - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) custom_properties: Vec<serde_norway::Value>,
    #[expect(dead_code, reason = "a deprecation flag - lifecycle, not structure")]
    #[serde(default)]
    pub(crate) deprecated: Option<bool>,
    #[expect(dead_code, reason = "v3.2 business-vocabulary synonyms - prose, no Referent")]
    #[serde(default)]
    pub(crate) synonyms: Vec<serde_norway::Value>,
}

/// A schema-level relationship declaration: the `type` (constant `foreignKey`) and the `from`/`to`
/// endpoints, each a single `table.column` reference or an array for a composite key.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RelationshipDecl {
    #[serde(default)]
    pub(crate) r#type: Option<String>,
    pub(crate) from: RelationshipEndpoint,
    pub(crate) to: RelationshipEndpoint,
    // v3.2.0 gives `RelationshipBase` an optional `id` (a `StableId`) - not read, but its absence
    // from this struct would refuse an otherwise-valid v3.2.0 relationship under
    // `deny_unknown_fields`.
    #[expect(dead_code, reason = "the relationship's id - not a field this adapter reads")]
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[expect(dead_code, reason = "optional per-relationship metadata, e.g. a human description")]
    #[serde(default)]
    pub(crate) custom_properties: Vec<serde_norway::Value>,
}

/// A relationship endpoint: a single `table.column` reference, or an array for a composite key.
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum RelationshipEndpoint {
    Single(String),
    #[expect(
        dead_code,
        reason = "matched only to detect the composite shape and refuse it; the column names themselves are unrepresentable and never read"
    )]
    Composite(Vec<String>),
}
