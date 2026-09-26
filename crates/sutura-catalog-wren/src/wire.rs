//! The on-disk shape of a `WrenAI` project's `manifest.json`.
//!
//! Mirrors `wren-core-base::mdl::manifest` (`Canner/wren-engine`, layout version 2, macro-expanded
//! `Manifest`/`Model`/`Column`/`Relationship`/`View`/`Cube`/`Measure`/`CubeDimension`/
//! `TimeDimension`/`RowLevelAccessControl` structs) - read from the JSON itself rather than taken as
//! a dependency, because sutura links no wren crate and this converter runs once, offline, on a
//! human's own machine. `deny_unknown_fields` throughout for the same reason
//! `sutura-catalog-local`'s own document format uses it: a field this module has not modelled must
//! fail loudly rather than be silently dropped from the report.
//!
//! **A column's or relationship's access-control payload is read as opaque JSON.** `[super::convert]`
//! only needs to know one is *present*, to refuse it by name - modelling `NormalizedExpr`'s own wire
//! shape (a custom `FromStr`/`Display` pair upstream, undocumented here) would buy nothing this
//! converter reads.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Manifest {
    /// Read for fidelity with the upstream shape and named in no refusal - a wren project's
    /// `layoutVersion`, `catalog`, `schema` and `dataSource` are cluster-wide configuration, not a
    /// definition kind, so this converter has nothing to map them onto or refuse them as.
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "cluster-wide configuration, not a definition kind this converter maps or refuses"
    )]
    pub(crate) layout_version: Option<u32>,
    #[expect(
        dead_code,
        reason = "cluster-wide configuration, not a definition kind this converter maps or refuses"
    )]
    pub(crate) catalog: String,
    #[expect(
        dead_code,
        reason = "cluster-wide configuration, not a definition kind this converter maps or refuses"
    )]
    pub(crate) schema: String,
    #[serde(default)]
    pub(crate) models: Vec<Model>,
    #[serde(default)]
    pub(crate) relationships: Vec<Relationship>,
    #[serde(default)]
    pub(crate) views: Vec<View>,
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "cluster-wide configuration, not a definition kind this converter maps or refuses"
    )]
    pub(crate) data_source: Option<String>,
    #[serde(default)]
    pub(crate) cubes: Vec<Cube>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Model {
    pub(crate) name: String,
    /// A model whose rows come from an authored statement rather than a table - wren's `ref_sql`.
    /// Present means this model is a named refusal (`SQL view / ref_sql`) rather than a physical
    /// table; its own `table_reference`, if any, is not read.
    #[serde(default)]
    pub(crate) ref_sql: Option<String>,
    #[serde(default)]
    pub(crate) table_reference: Option<TableReference>,
    pub(crate) columns: Vec<Column>,
    #[serde(default)]
    pub(crate) primary_key: Option<String>,
    #[serde(default)]
    pub(crate) row_level_access_controls: Vec<RowLevelAccessControl>,
}

/// The parts of a table path - `wren-core`'s own wire shape is a joined, pre-quoted string; this
/// converter reads the object shape a `WrenAI` project's own JSON writes on disk instead, and
/// composes [`sutura_domain::model::QualifiedTable`]'s parts from it directly.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct TableReference {
    #[serde(default)]
    pub(crate) catalog: Option<String>,
    #[serde(default)]
    pub(crate) schema: Option<String>,
    #[serde(default)]
    pub(crate) table: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) r#type: String,
    /// Names the relationship this column navigates through, for a wren column that is not a
    /// physical field at all. Folded into the model's `relationships:` entry rather than kept as a
    /// column - see [`super::convert`]'s header.
    #[serde(default)]
    pub(crate) relationship: Option<String>,
    #[serde(default)]
    pub(crate) is_calculated: bool,
    #[serde(default)]
    pub(crate) not_null: bool,
    #[serde(default)]
    pub(crate) expression: Option<String>,
    /// Wren's own visibility flag - not consulted here. A hidden non-relationship column is still
    /// mapped; hiding a column from wren's own reader says nothing about whether sutura's column
    /// metadata (`#995`) should carry it too, and this converter leaves that decision to the person
    /// reviewing the output rather than guessing it.
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "read for fidelity with the upstream wire shape; not a mapping decision here"
    )]
    pub(crate) is_hidden: bool,
    /// Wren's `columnLevelAccessControl` - renamed off the struct-prefixed wire name so the field
    /// itself does not repeat `Column`.
    #[serde(default, rename = "columnLevelAccessControl")]
    pub(crate) access_control: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Relationship {
    pub(crate) name: String,
    /// Exactly two model names, origin then target - `wren-core`'s own type is an unchecked
    /// `Vec<String>`; [`super::convert`] refuses anything else by name rather than panicking on it.
    pub(crate) models: Vec<String>,
    pub(crate) join_type: JoinType,
    pub(crate) condition: String,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum JoinType {
    OneToOne,
    OneToMany,
    ManyToOne,
    ManyToMany,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct View {
    pub(crate) name: String,
    #[expect(
        dead_code,
        reason = "named in the refusal report; the statement itself is never read or rendered"
    )]
    pub(crate) statement: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Cube {
    pub(crate) name: String,
    pub(crate) base_object: String,
    #[serde(default)]
    pub(crate) measures: Vec<Measure>,
    #[serde(default)]
    pub(crate) dimensions: Vec<CubeDimension>,
    #[serde(default)]
    pub(crate) time_dimensions: Vec<TimeDimension>,
    /// Ordered drill-down paths. No sutura equivalent - a non-empty map is a named refusal.
    #[serde(default)]
    pub(crate) hierarchies: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Measure {
    pub(crate) name: String,
    pub(crate) expression: String,
    #[expect(
        dead_code,
        reason = "wren's own value type; sutura's Aggregate decides the shape, not this string"
    )]
    pub(crate) r#type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CubeDimension {
    pub(crate) name: String,
    pub(crate) expression: String,
    #[expect(
        dead_code,
        reason = "wren's own value type; the recognised expression shape decides the sutura side, not this string"
    )]
    pub(crate) r#type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct TimeDimension {
    pub(crate) name: String,
    pub(crate) expression: String,
    #[expect(
        dead_code,
        reason = "wren's own value type; the recognised expression shape decides the sutura side, not this string"
    )]
    pub(crate) r#type: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RowLevelAccessControl {
    pub(crate) name: String,
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "presence of the control is the whole refusal; which session properties it needs is not read"
    )]
    pub(crate) required_properties: Vec<serde_json::Value>,
    #[expect(
        dead_code,
        reason = "named in the refusal report; the condition itself is never read or rendered"
    )]
    pub(crate) condition: String,
}
