//! The on-disk shape of a catalog document, and its conversion into domain types.
//!
//! These structs exist so the file format is a separate thing from the model. A domain type with
//! `Deserialize` on it would make every rename in a catalog file a breaking change to the hexagon's
//! interior, and it would put the file format's defaults inside the types the business rules are
//! written in.
//!
//! `deny_unknown_fields` is on every one of them, and it is the most useful line in this module. A
//! misspelled key would otherwise be dropped in silence, and the definition that loads is not the
//! one the author wrote: `colums:` yields a model with no columns, which then refuses every question
//! about it for a reason that says nothing about a typo.
//!
//! [`sutura_domain::measure`] and [`AuthoredSql`] are the exceptions, and the computation fields of
//! [`MetricDoc`] argue for them where a reader will be standing when they wonder. In short: those
//! types already carry exactly this format's representation, and mirroring their variants here
//! would buy nothing but a place to forget the next one.

use std::collections::BTreeSet;

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::{
    Anchor, AnchorValue, Column, Description, Dimension, DimensionValue, InconsistentDefinitions, InvalidDescription,
    InvalidDimensionValue, InvalidJoinKeys, InvalidViaChain, JoinKey, JoinKeys, Metric, Model, Relationship, ViaChain,
};
use sutura_domain::expression::{AuthoredSql, Computation, InvalidComputation};
use sutura_domain::measure::{Measure, RequiredFilter};
use sutura_domain::model::{
    ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName, SourceName,
};

use crate::document::audience::{AudienceDoc, InvalidAudienceDeclaration};

// Own modules: this file is near the thousand-line cap, and each is a separate concern - who may
// see a metric, and the four knowledge documents (a definition decides what executes, a note
// decides what a reader understands).
pub mod audience;
pub mod knowledge;

/// What a document declares itself to be.
///
/// Required in every document rather than inferred from the directory it sits in. A file in the
/// wrong directory is then an error naming the mismatch, instead of a metric that was quietly never
/// loaded, and the loader can walk one tree instead of trusting a layout convention.
///
/// **Seven kinds now, and the split between them is worth reading as two groups.** The first three
/// are definitions: they decide what executes, and `sutura_domain::catalog` checks them. The last
/// four are knowledge: they decide what a reader understands, and `sutura_domain::knowledge` checks
/// them. Nothing in the loader treats the two groups differently - one walk, one tag, one dispatch -
/// which is what keeps "which directory is this in" from becoming part of the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Model,
    Relationship,
    Metric,
    /// One entry of the business glossary.
    Glossary,
    /// Something a reader has to know before trusting a number.
    Caveat,
    /// A term this catalog deliberately does not define.
    ///
    /// The word an author writes is `not_defined`, which says what they are doing; the domain type
    /// is `Absence`, which says what the thing is. Two names for two audiences, and the format's one
    /// is the one that appears in an error about a file.
    NotDefined,
    /// A worked question: how somebody asked it, and what to send.
    Example,
}

impl DocumentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Relationship => "relationship",
            Self::Metric => "metric",
            Self::Glossary => "glossary",
            Self::Caveat => "caveat",
            Self::NotDefined => "not_defined",
            Self::Example => "example",
        }
    }
}

/// Just enough of a document to know which shape to parse it as.
///
/// A separate pass over the same few lines. The alternative is an internally tagged enum, and serde
/// cannot combine one with `deny_unknown_fields`, which is the check that makes a typo an error. Two
/// parses of a frontmatter block is not a cost worth trading that for.
#[derive(Debug, serde::Deserialize)]
pub struct KindProbe {
    kind: DocumentKind,
}

impl KindProbe {
    /// What the document says it is.
    ///
    /// An accessor rather than a public field, because the boundary gate fails a public field on a
    /// public struct in a library crate: a struct literal can build a value a constructor would
    /// have rejected, and the rule does not get to make an exception for a type that currently has
    /// no invariant to protect.
    #[inline]
    pub const fn kind(&self) -> DocumentKind {
        self.kind
    }
}

/// A value an anchor may be written as.
///
/// Untagged so `value: 197122` and `value: "197122"` both work. Without it the unquoted form fails
/// with "invalid type: integer, expected a string", which is a true statement about a file that
/// looks correct to whoever wrote it. Everything becomes text either way, because that is what an
/// anchor comparison uses.
///
/// **The quoted arm stays a `String` and the parse happens in [`Self::into_value`], which is a
/// measured decision rather than the obvious one.** Making it an [`AnchorValue`] and letting its
/// `serde(try_from)` do the work - the arrangement `values:` above uses, and the first thing tried
/// here - puts the check inside deserialization, where `untagged` throws the cause away: the refusal
/// reaches an author as `data did not match any variant of untagged enum AnchorLiteral at line 10
/// column 3`, naming neither the character nor the rule. `untagged` reports that a set of attempts
/// all failed and cannot report why any one of them did. So the parse is one step later, where
/// [`InvalidMetricDocument::AnchorValue`] names the metric and carries the character fault as its
/// `source`.
///
/// **What that costs, stated rather than left to be discovered.** `values:` above refuses inside
/// serde and so reports the LINE AND COLUMN of the offending scalar; this field refuses after the
/// document is decoded and names the metric instead. And `cargo xtask check-serde-parse` cannot see
/// this route at all - it keys on an associated function returning `Result<Self, ..>`, which
/// [`Self::into_value`] is not - so for this one field, in the one adapter whose input is a file on
/// disk, parse-at-the-edge is held by review. The route that keeps both would be a local newtype
/// carrying `#[serde(try_from = "AnchorLiteral")]`, paying a third type in this module and a serde
/// error in place of a typed cause.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum AnchorLiteral {
    Integer(i64),
    Text(String),
}

impl AnchorLiteral {
    /// The value as the domain holds it, parsed.
    ///
    /// Both arms go through the one constructor, and the integer arm cannot fail there: an `i64`
    /// renders as at most twenty ASCII digits and a sign. It is written as the same call rather than
    /// a shortcut for that arm, because a second construction path is a second thing that can stop
    /// agreeing with the rule.
    fn into_value(self) -> Result<AnchorValue, InvalidDimensionValue> {
        match self {
            Self::Integer(v) => AnchorValue::parse(v.to_string()),
            Self::Text(v) => AnchorValue::parse(v),
        }
    }
}

/// One entry of a model's `columns:` list: a bare name, or a name with a type, a description and
/// whether it may hold null.
///
/// **Untagged, the same shape [`AnchorLiteral`] uses and for the same reason: every document
/// already on disk writes the short form, so it must keep loading byte for byte.** A document
/// writes the long form only for a column it has something more to say about; the two may mix
/// freely in one list. The same cost `AnchorLiteral`'s own doc states applies here too: a
/// misspelled key inside the long form is refused, but `untagged` cannot say which variant a
/// mapping was attempting or which key was wrong - the message names neither.
///
/// **`type` and `description` are read as bare text, not as [`ColumnType`](sutura_domain::catalog::ColumnType)/[`Description`]
/// directly, and that is deliberate.** `serde(try_from)` has no escape hatch: a `ColumnType` that
/// failed to parse would refuse the WHOLE document, for text nothing renders - the same defect
/// found in review over `DataHub`'s HTTP reader, and [`ColumnType`](sutura_domain::catalog::ColumnType)'s own doc argues why a type
/// this crate cannot represent should be dropped instead. Reading the raw text here and converting
/// through [`Column::from_metadata`] in [`Self::into_domain`] is what gives this format the same
/// "a type is dropped, a description still refuses" rule every other catalog adapter now has.
///
/// **What this does not check: two entries naming one column.** [`Model::new`] collects columns
/// into a map keyed by name, so a repeated name keeps whichever entry was last in the list rather
/// than refusing - the same silent collapse a `BTreeSet<ColumnName>` already gave every identical
/// bare-name repeat before this type existed. Two long-form entries that repeat a name with
/// DIFFERENT metadata are now representable and not caught: unlike [`super::MetricDoc`]'s
/// dimensions, which the domain refuses a duplicate of, a model's columns are not checked for one
/// here or in [`sutura_domain::catalog::Definitions::assemble`]. Stated as a limit rather than
/// silently accepted.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ColumnEntryDoc {
    Short(ColumnName),
    Long {
        name: ColumnName,
        #[serde(default)]
        r#type: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        nullable: Option<bool>,
    },
}

impl ColumnEntryDoc {
    /// The column's own name, either form.
    const fn name(&self) -> &ColumnName {
        match self {
            Self::Short(name) | Self::Long { name, .. } => name,
        }
    }

    /// # Errors
    ///
    /// [`InvalidDescription`], if the long form's `description:` is present and not usable. A
    /// `type:` that is not usable is dropped rather than refused - see this type's own doc.
    fn into_domain(self) -> Result<Column, InvalidDescription> {
        match self {
            Self::Short(name) => Ok(Column::from(name)),
            Self::Long {
                name,
                r#type,
                description,
                nullable,
            } => Column::from_metadata(name, r#type.as_deref(), description.as_deref(), nullable),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: ModelName,
    source: SourceName,
    /// The table, and where it lives when that is more than the default.
    ///
    /// **A dotted path, and `table: orders` is unchanged by this being one.** `QualifiedTable`
    /// deserializes from a string and parses `table`, `dataset.table` and `project.dataset.table`
    /// alike, so every document already on disk loads byte for byte - and there is no second key for
    /// an author to get out of step with this one. The parsing, and why the path is a composition of
    /// parsed names rather than a string with dots in it, is
    /// `sutura_domain::model::qualified`.
    table: QualifiedTable,
    columns: Vec<ColumnEntryDoc>,
    /// Which of `columns` the model's own primary key names - evidence only, per
    /// [`sutura_domain::catalog::Model::with_primary_key`]'s own doc.
    #[serde(default)]
    primary_key: BTreeSet<ColumnName>,
}

/// Why a model document could not become a domain [`Model`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidModelDocument {
    /// One column's own `description:` is not usable prose.
    #[error("column {column}'s description is not usable")]
    ColumnDescription {
        column: ColumnName,
        #[source]
        cause: InvalidDescription,
    },
    /// The model's own `primary_key:` names a column it does not declare.
    ///
    /// Transparent and boxed, for the reason [`InvalidMetricDocument::Inconsistent`] gives: the
    /// domain's own message already names the model and the column, and the box is what keeps
    /// `LocalCatalogError` under `clippy::result_large_err`'s threshold.
    #[error(transparent)]
    PrimaryKey(Box<InconsistentDefinitions>),
}

impl ModelDoc {
    pub fn into_domain(self, description: Description) -> Result<Model, InvalidModelDocument> {
        let mut columns = Vec::with_capacity(self.columns.len());
        for entry in self.columns {
            let name = entry.name().clone();
            columns.push(
                entry
                    .into_domain()
                    .map_err(|cause| InvalidModelDocument::ColumnDescription { column: name, cause })?,
            );
        }
        Model::new(self.name, self.source, self.table, columns, description)
            .with_primary_key(self.primary_key)
            .map_err(|cause| InvalidModelDocument::PrimaryKey(Box::new(cause)))
    }
}

/// One end of a relationship, and the grain an origin is truncated to when a join key's
/// `grain` is present, making it a truncated equality rather than a plain one.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDoc {
    model: ModelName,
    column: ColumnName,
}

/// One term of a compound join, as the document spells it.
///
/// A single column pair keeps the byte shape every existing relationship document has:
/// `origin: { model, column }` / `target: { model, column }` outside a `keys:` list stays a plain
/// equality. A compound join declares a `keys:` list, each entry `{ origin, target }` or
/// `{ origin, grain, target }` - the origin column, truncated to `grain` for a truncated key.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinKeyDoc {
    origin: ColumnName,
    /// The grain an origin is truncated to. Absent means a plain equality.
    #[serde(default)]
    grain: Option<Grain>,
    target: ColumnName,
}

impl JoinKeyDoc {
    fn into_domain(self) -> JoinKey {
        match self.grain {
            Some(grain) => JoinKey::TruncatedEqual {
                origin: self.origin,
                grain,
                target: self.target,
            },
            None => JoinKey::Equal {
                origin: self.origin,
                target: self.target,
            },
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: RelationshipName,
    origin: EndpointDoc,
    target: EndpointDoc,
    /// A compound join's ordered keys. Absent means the single `origin`/`target` pair, which is the
    /// shape every existing document has.
    #[serde(default)]
    keys: Option<Vec<JoinKeyDoc>>,
    join_type: JoinType,
}

impl RelationshipDoc {
    pub fn into_domain(self) -> Result<Relationship, InvalidJoinKeys> {
        let keys = self.keys.map_or_else(
            || {
                JoinKeys::of(vec![JoinKey::Equal {
                    origin: self.origin.column,
                    target: self.target.column,
                }])
            },
            |keys| JoinKeys::of(keys.into_iter().map(JoinKeyDoc::into_domain).collect()),
        )?;
        Ok(Relationship::new(
            self.name,
            self.origin.model,
            self.target.model,
            self.join_type,
            keys,
        ))
    }
}

/// The hops a dimension is reached through, as the document spells them: one name, or a sequence of
/// them.
///
/// An untagged enum rather than two optional fields: `via:` with a single name keeps the byte shape
/// every existing document has, and `via: [a, b]` is the chain. `deny_unknown_fields` is a struct
/// rule, so the enum carries the adapter's one place where a misspelled key is not caught - a
/// misspelled NAME inside the chain still refuses, because the name is a `RelationshipName`.
#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
pub enum ViaDoc {
    One(RelationshipName),
    Chain(Vec<RelationshipName>),
}

impl ViaDoc {
    /// Into the chain the domain type holds: a single name is a one-hop chain.
    fn into_chain(self) -> Result<ViaChain, InvalidViaChain> {
        match self {
            Self::One(name) => ViaChain::of(vec![name]),
            Self::Chain(hops) => ViaChain::of(hops),
        }
    }
}

/// A dimension, as a list entry with its own `name`.
///
/// A sequence rather than a map keyed by name, and that is not a style choice. A YAML mapping with
/// the same key twice keeps the last value and reports nothing, so a metric declaring `region`
/// twice would load with whichever definition came second. As a list the duplication survives to
/// where [`MetricDoc::into_domain`] can refuse it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionDoc {
    name: DimensionName,
    column: ColumnName,
    #[serde(default)]
    via: Option<ViaDoc>,
    /// The values a filter may use. Absent means "group by this, do not filter on it".
    #[serde(default)]
    values: Option<BTreeSet<DimensionValue>>,
    #[serde(default)]
    description: Description,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorDoc {
    range: TimeRange,
    value: AnchorLiteral,
}

impl AnchorDoc {
    /// Into the domain type a metric carries. Same shape as `SuturaAnchor::into_domain`.
    fn into_domain(self) -> Result<Anchor, InvalidDimensionValue> {
        Ok(Anchor::new(self.range, self.value.into_value()?))
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: MetricName,
    model: ModelName,
    /// The domain's [`Measure`], deserialized directly rather than restated by a local `*Doc` shape.
    ///
    /// The one departure from this module's rule that the file format is its own thing, and it is
    /// chosen rather than inherited. [`sutura_domain::measure`] already derives `Deserialize` with
    /// exactly the representation this format wants, and each part of that is load-bearing here:
    /// the shape is externally tagged, so `simple:` or `ratio:` is a word an author writes rather
    /// than something inferred from which fields are present; `deny_unknown_fields` holds at every
    /// depth, on the variant and on the struct behind a term, so a misspelled key nested inside
    /// `simple:` is still an error naming the typo; and `zero_denominator` has no default, so its
    /// absence is a missing-field error naming the field instead of a silent pick between two
    /// defensible behaviours. A mirror would be two shapes, two terms and four operators of
    /// restatement, and its failure mode is the expensive one: a shape or a term added to the domain
    /// and forgotten here is one no document can express, with nothing anywhere failing to say so.
    ///
    /// The rule still holds for everything else. `Model`, `Relationship`, `Metric` and `Dimension`
    /// derive only `Serialize`, and that asymmetry is the domain saying which of its types it also
    /// intends as a wire format. When a catalog file needs to spell a measure differently from the
    /// domain, this field grows a `MeasureDoc` and a conversion, and that diff is the discussion.
    ///
    /// `singleton_map` is the one thing this costs, and it is a YAML fact rather than a design
    /// choice. An externally tagged enum in `serde_norway` is a YAML *tag*: `measure: !simple {..}`.
    /// Nobody writing a catalog file spells a shape with a `!`, and the failure without this
    /// adapter is `invalid type: map, expected a YAML tag starting with '!'`, which tells an author
    /// nothing about the document they wrote. `singleton_map` reads the one-key mapping form that
    /// the rest of the format already looks like, and leaves every field inside it, including
    /// `deny_unknown_fields`, to the ordinary derive.
    ///
    /// Non-recursive, and that is now a statement about the shapes rather than an accident. The two
    /// enums nested inside a measure need no adapter: a term is a flat mapping read through a
    /// `try_from` struct, and `zero_denominator` is a unit variant, which `serde_norway` already
    /// spells as a plain scalar. `singleton_map_recursive` would reach into both and is not needed
    /// by either.
    /// Optional only while the sibling `authored_sql` key is considered. [`Computation::assemble`]
    /// refuses both absence and coexistence before a domain metric exists.
    #[serde(default, with = "serde_norway::with::singleton_map")]
    measure: Option<Measure>,
    /// The named escape hatch, kept as a sibling of `measure` so a review can see which path a
    /// metric chose. **This adapter stores the fragment and does not compile it**: it may not reach
    /// `sutura-sql` (`cargo xtask check-boundaries` forbids the edge, because a SQL generator in a
    /// metadata crate's tree is one in every shipped binary's), so what is checked here is what
    /// [`sutura_domain::expression::SqlFragment::parse`] checks and nothing more. A bundle carrying
    /// one is refused at startup by every adapter this workspace ships; `docs/adr/0004` is the record.
    #[serde(default)]
    authored_sql: Option<AuthoredSql>,
    /// Predicates that are part of the definition, applied to every question about the metric.
    ///
    /// Defaulted to empty, because most metrics have none and requiring the key on every document
    /// would make the common case noisy. The direction that must never be defaulted is the other
    /// one: absent means "no predicate", never "not yet decided".
    ///
    /// `singleton_map_recursive` rather than `singleton_map` because the enum is inside a sequence,
    /// and the non-recursive adapter applies to the value it is attached to. Recursion is safe here:
    /// a [`RequiredFilter`] payload holds a column name and a value, both of them newtypes over one
    /// scalar with a `try_from`, so there is no nested enum for it to reinterpret.
    #[serde(default, with = "serde_norway::with::singleton_map_recursive")]
    required_filters: Vec<RequiredFilter>,
    time_column: ColumnName,
    grains: BTreeSet<Grain>,
    #[serde(default)]
    dimensions: Vec<DimensionDoc>,
    #[serde(default)]
    anchor: Option<AnchorDoc>,
    /// Who may see this metric - `docs/adr/0028`. **No default**: a missing key is a parse error
    /// naming the metric, never a silent `open`. `singleton_map` for `measure`'s own reason:
    /// `restricted:` is a YAML mapping, and the default externally-tagged form for that needs a
    /// `!Tag` nobody authoring a catalog file spells; the unit variant `open` needs no adapter.
    #[serde(with = "serde_norway::with::singleton_map")]
    audience: AudienceDoc,
}

/// Why a metric document cannot become a metric.
///
/// Only what belongs to the DOCUMENT: the conversions this file performs that the domain's own
/// constructors can refuse. Everything about whether a metric holds together is checked in
/// [`sutura_domain::catalog`], once, for every adapter - **including the duplicated dimension this
/// enum used to carry.** That variant existed because `Metric::new` took a map, so the domain could
/// not see the pair; it takes a vector now, and the refusal is
/// [`InconsistentDefinitions::DuplicateDimension`], which [`Self::Inconsistent`] carries. A copy of
/// a check in one adapter is a check the other adapter does not have, which is exactly what
/// happened.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidMetricDocument {
    /// The document wrote neither computation key, or wrote both.
    #[error(transparent)]
    Computation(InvalidComputation),
    /// The domain refused the metric this document describes.
    ///
    /// Transparent, because the domain's own message names the metric and the fault and this layer
    /// has nothing to add - what it adds is the path, and `LocalCatalogError::Metric` is where that
    /// is attached. Reaching `LocalCatalogError::Inconsistent` instead would drop the path, which is
    /// the one thing a reader of a directory of files needs.
    ///
    /// **Boxed, and the box is what buys the path.** `clippy::result_large_err` is denied here for
    /// the reason `sutura_domain::plan::tables` states, and unboxing this reported
    /// `LocalCatalogError` at *at least 128 bytes* against a threshold of 128 in seven of its own
    /// signatures - because `InconsistentDefinitions`'s widest variants carry four name newtypes,
    /// and a `PathBuf` plus that plus two discriminants does not fit. The house remedy is to trim
    /// the variant rather than allow the lint, and there is nothing here to trim: the path is the
    /// point of this variant and the cause is a type the domain owns. So it is boxed instead - much
    /// larger than every other variant, and the alternative was losing information. The typed cause
    /// survives, which is what separates this from a `Box<dyn Error>`.
    #[error(transparent)]
    Inconsistent(Box<InconsistentDefinitions>),
    /// The anchor's `value:` is not text a number can be checked against.
    ///
    /// The metric is named here and the character fault is the `source`, which is the arrangement
    /// `sutura_domain::pinned::NotValidated::AnchorNotExecuted` already uses: this variant says
    /// which document to open, and whoever renders it walks the chain for which character to look
    /// for. Reported per metric rather than per field because a metric document declares one anchor.
    #[error("metric {metric}'s anchor value is not usable as one")]
    AnchorValue {
        metric: MetricName,
        #[source]
        cause: InvalidDimensionValue,
    },
    /// The `audience:` declaration is not a usable one - `docs/adr/0028`.
    #[error("metric {metric}'s audience declaration is not usable as one")]
    Audience {
        metric: MetricName,
        #[source]
        cause: InvalidAudienceDeclaration,
    },
    /// A dimension's `via:` chain names nothing.
    ///
    /// The domain's `ViaChain::of` refuses the empty chain; this is where the refusal learns which
    /// document to open, the same arrangement `Self::AnchorValue` uses. The refused chain is the
    /// `source`, though `ViaChain::of` has one reason to refuse and this is it - so the chain
    /// carries what the renderers walk without naming what is already spelled in the message.
    #[error("metric {metric}'s dimension {dimension} declares a via chain with no relationship in it")]
    EmptyChain {
        metric: MetricName,
        dimension: DimensionName,
        #[source]
        cause: InvalidViaChain,
    },
}

impl MetricDoc {
    pub fn into_domain(self, description: Description) -> Result<Metric, InvalidMetricDocument> {
        // A vector, handed on as a vector. This loop used to build a map and refuse a repeat in it,
        // which was a check the DataHub adapter did not have - see `InvalidMetricDocument`.
        let mut dimensions: Vec<Dimension> = Vec::with_capacity(self.dimensions.len());
        for doc in self.dimensions {
            let via = match doc.via.map(ViaDoc::into_chain) {
                Some(chain) => Some(chain.map_err(|cause| InvalidMetricDocument::EmptyChain {
                    metric: self.name.clone(),
                    dimension: doc.name.clone(),
                    cause,
                })?),
                None => None,
            };
            dimensions.push(Dimension::new(doc.name, doc.column, via, doc.values, doc.description));
        }
        let anchor = self
            .anchor
            .map(AnchorDoc::into_domain)
            .transpose()
            .map_err(|cause| InvalidMetricDocument::AnchorValue {
                metric: self.name.clone(),
                cause,
            })?;
        let audience = self.audience.into_domain().map_err(|cause| InvalidMetricDocument::Audience {
            metric: self.name.clone(),
            cause,
        })?;
        let computation = Computation::assemble(self.measure, self.authored_sql).map_err(InvalidMetricDocument::Computation)?;
        Metric::new(
            self.name,
            self.model,
            computation,
            self.required_filters,
            self.time_column,
            self.grains,
            dimensions,
            anchor,
            description,
            audience,
        )
        .map_err(|cause| InvalidMetricDocument::Inconsistent(Box::new(cause)))
    }
}

#[cfg(test)]
#[path = "document/tests.rs"]
mod tests;
