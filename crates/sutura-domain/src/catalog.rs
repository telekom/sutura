//! What a catalog says: models, the relationships between them, and the metrics defined over them.
//!
//! These are the types every `SemanticCatalog` adapter produces, and [`Definitions::assemble`] is
//! the one place their cross-references are checked. That matters more than it looks: a directory of
//! files and a metadata service over HTTP disagree about almost everything except this, so a check
//! that lived in an adapter would be a check the other adapter did not have. Two adapters reading
//! the same content must produce the same [`Definitions`] or one of them is wrong, and the golden
//! suite asserts exactly that.
//!
//! Nothing here parses or renders SQL. A [`Computation`] may hold catalog-authored text as written;
//! the compile that would validate it lives in `sutura-sql`, and nothing published calls it - a
//! bundle carrying such a metric is refused at boot. See `docs/adr/0001-first-party-semantic-models.md`
//! and `docs/adr/0004-a-named-escape-hatch-for-authored-sql.md`.

use std::collections::{BTreeMap, BTreeSet};

// Three files rather than one, because `cargo xtask max-lines` fails at a thousand lines under
// `crates/` and cannot be exempted. `authored` holds the character-level parse of the fields in this
// module that are prose rather than structure, `consistency` holds the cross-reference checks and
// the assembled `Definitions` they gate, and this file holds the declarations. The names stay where
// they were - a caller still writes `sutura_domain::catalog::DimensionValue` - because the module is
// the unit of API and the files are not.
mod audience;
mod authored;
mod consistency;

pub use audience::{Audience, AudienceGrant, GrantedAudiences, InvalidAudienceGrant};
pub use authored::{
    AnchorValue, ColumnType, Description, DimensionValue, InvalidDescription, InvalidDimensionValue, MAX_DESCRIPTION_BYTES,
    MAX_DESCRIPTION_LINES, MAX_DIMENSION_VALUE_CHARS,
};
pub use consistency::{Definitions, InconsistentDefinitions};

use crate::calendar::TimeRange;
use crate::expression::Computation;
use crate::measure::{Measure, RequiredFilter};
use crate::model::{
    ColumnName, DimensionName, Grain, IdentifierCase, JoinType, MetricName, ModelName, QualifiedTable, RelationshipName,
    SourceName, TableName,
};

/// The label a generated projection gives the truncated time column.
///
/// It lives here rather than in the compiler because it is part of the result schema, which is a
/// contract, and because [`Definitions::assemble`] has to know it: a dimension by this name would
/// produce two columns with one label, and a caller reading a result by name would get whichever
/// the data system listed first.
pub const TIME_BUCKET_LABEL: &str = "period";

/// The most values one dimension may declare.
///
/// **Measured before it was chosen, and it is the count that was missing rather than the length.**
/// The largest allowlist in this repository's example catalog is `region`, with five values, and the
/// next largest is `product_family` with four. Nothing here declares more than five, and the
/// question this bound answers is what a REVIEWED allowlist can plausibly be: 64 is twelve times the
/// largest one written here and four times the sixteen German federal states, which is the largest
/// enumeration a person writes out by hand in one line of a document. A dimension needing two
/// hundred country codes is not an allowlist somebody read; it is a lookup table, and it wants a
/// mechanism that does not put every entry into an agent's prompt.
///
/// Argued the way [`crate::query::MAX_RANGE_DAYS`] is argued, including about what it does not bound.
/// **The number that matters is the product of this and [`MAX_DIMENSION_VALUE_CHARS`]**, because
/// `sutura_app::prompt` lists every declared value of a dimension on one line of the document an
/// agent reads: 64 values of 64 characters is 4 KiB, the same order as
/// [`crate::knowledge::MAX_NOTE_BODY_BYTES`], so one dimension's value list is bounded by about what
/// one note body is. Per-value caps alone let N conforming values do what one oversized value cannot,
/// which is the same argument [`crate::knowledge::MAX_KNOWLEDGE_BYTES`] makes for notes.
///
/// What it does not bound, said plainly. It bounds ONE dimension: nothing here caps how many
/// dimensions a metric declares or how many metrics a catalog holds. [`MAX_DEFINITIONS_BYTES`] is
/// the fix for that other half.
pub const MAX_VALUES_PER_DIMENSION: usize = 64;

/// The most bytes a whole [`Definitions`] may carry of authored content beyond its own identifiers.
///
/// Every column a model declares, every required filter and dimension value a metric declares, and
/// every model's and metric's description count toward it.
///
/// **The count [`MAX_VALUES_PER_DIMENSION`]'s own note names as missing**: that bound is one
/// dimension's, and nothing capped how many dimensions a metric declares, how many required filters
/// a metric declares, how many columns a model declares, or how many models and metrics a catalog
/// holds. Per-item caps alone let N conforming declarations do what one oversized declaration
/// cannot, the same argument [`crate::knowledge::MAX_KNOWLEDGE_BYTES`] makes for a bundle of notes,
/// applied to the catalog that bundle is checked against.
///
/// **Measured before it was chosen, and re-measured for issue #966's column type and column
/// description, which this bound did not cover before either existed.** This repository's shipped
/// `single-player` catalog - the larger of the two example catalogs - is the reference: its widest
/// model (`subscriptions`) declares 8 columns, no metric declares more than one required filter,
/// and its columns, required filters and dimension values together sum under 2 KiB - one column
/// (`subscriptions.mrr_cents`) now carries a declared type and a description, which is what moved
/// this half at all. Descriptions are the rest of it, at about 22.5 KiB across eleven metrics and
/// five models - each individually inside [`MAX_DESCRIPTION_BYTES`], and it is their COUNT that was
/// uncapped. `Definitions::authored_bytes` over the loaded corpus reads 24975 bytes, ~24.4 KiB.
///
/// [`MAX_DEFINITIONS_BYTES`] is 128 KiB: about 5.25 times that reference catalog's ~24.4 KiB, less
/// headroom than the ~6.5 times an earlier, column-blind measurement claimed - restated here rather
/// than left to say a smaller bundle than the corpus now is. Still more than
/// [`crate::knowledge::MAX_KNOWLEDGE_BYTES`]'s five times its own reference, because a definitions
/// bundle also carries the identifiers a knowledge bundle does not. Argued the way
/// [`crate::query::MAX_RANGE_DAYS`] is: what it bounds is the size of the document, not whether what
/// is in it is worth reading.
pub const MAX_DEFINITIONS_BYTES: usize = 128 * 1024;

/// One column a [`Model`] exposes: its name, and what a source's own dictionary says about it.
///
/// `data_type` and `description` are independent of each other and both optional - a database
/// dictionary types every column and comments few of them, a Table Schema descriptor may type a
/// field and describe none. `nullable` is likewise a source's own claim, read and stored, never
/// derived from anything else here.
///
/// **`data_type` is descriptive text, never a cast.** It is a quote of what the source called the
/// column - `"STRING"`, `"character varying"`, `"NUMERIC(38,9)"` - for a person reading the catalog.
/// Nothing in this crate branches on it, and `sutura_sql` has its own closed vocabulary for what a
/// statement may execute.
///
/// **Column prose is parsed and pinned, and reaches no rendering surface today.** No composition
/// root's prompt, tool result or HTTP body names a column - `sutura_app::prompt`'s own header states
/// that as a deliberate absence - so this type has nothing to gate behind `prompt.catalog_prose` yet.
/// If a future surface renders it, it goes through that same gate, the way every other quoted
/// description does.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Column {
    name: ColumnName,
    data_type: Option<ColumnType>,
    description: Description,
    nullable: Option<bool>,
}

impl Column {
    pub const fn new(name: ColumnName, data_type: Option<ColumnType>, description: Description, nullable: Option<bool>) -> Self {
        Self {
            name,
            data_type,
            description,
            nullable,
        }
    }

    #[inline]
    pub const fn name(&self) -> &ColumnName {
        &self.name
    }

    #[inline]
    pub const fn data_type(&self) -> Option<&ColumnType> {
        self.data_type.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    #[inline]
    pub const fn nullable(&self) -> Option<bool> {
        self.nullable
    }
}

/// A bare column: a name with no type, no prose and no nullability claim.
///
/// What every caller that has only ever named a column set means, and what lets [`Model::new`]
/// accept a plain `BTreeSet<ColumnName>` unchanged.
impl From<ColumnName> for Column {
    fn from(name: ColumnName) -> Self {
        Self::new(name, None, Description::default(), None)
    }
}

/// One physical table, and what the catalog knows about it.
///
/// `columns` is keyed by name because it is only ever asked "does this column exist, and what does
/// it look like" - declaring it at all is what lets a dimension naming a column that is not there be
/// a refusal from the pinned bundle instead of an error from the data system, which is the difference
/// between a governed answer and a stack trace.
///
/// `primary_key` is evidence a source's own dictionary supplied, not a cardinality rule: no join
/// type is inferred from it, and [`Relationship`]'s own `JoinType` is unaffected either way.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Model {
    name: ModelName,
    source: SourceName,
    table: QualifiedTable,
    columns: BTreeMap<ColumnName, Column>,
    primary_key: BTreeSet<ColumnName>,
    description: Description,
}

impl Model {
    /// A model over one physical table, wherever that table lives.
    ///
    /// **`impl Into<QualifiedTable>` and not `QualifiedTable`, and that is the compatibility hinge
    /// rather than a convenience.** `From<TableName>` yields an unqualified path, so every existing
    /// caller - a catalog document naming only a table, and every fixture in this workspace - passes
    /// a [`TableName`] and compiles unchanged, meaning exactly what it used to. It costs the `const`
    /// this constructor used to be, which nothing depended on.
    ///
    /// **`columns` takes anything a [`Column`] comes from, not a `Vec<Column>`.** A
    /// `BTreeSet<ColumnName>` is still what most callers here have, and [`Column`]'s `From<ColumnName>`
    /// is what lets it keep compiling unchanged. A duplicate column name silently keeps the last
    /// entry rather than refusing - unlike [`Metric::new`]'s dimensions, a model's columns come from a
    /// physical dictionary rather than an author's declaration, and a real table cannot have two
    /// columns with one name.
    pub fn new<C>(
        name: ModelName,
        source: SourceName,
        table: impl Into<QualifiedTable>,
        columns: impl IntoIterator<Item = C>,
        description: Description,
    ) -> Self
    where
        C: Into<Column>,
    {
        Self {
            name,
            source,
            table: table.into(),
            columns: columns
                .into_iter()
                .map(Into::into)
                .map(|column: Column| (column.name.clone(), column))
                .collect(),
            primary_key: BTreeSet::new(),
            description,
        }
    }

    /// Declares which of this model's columns a source's own dictionary marks as its primary key.
    ///
    /// Evidence only, per the type's own doc. [`Definitions::assemble`] refuses a key naming a
    /// column this model does not declare.
    #[must_use]
    pub fn with_primary_key(mut self, primary_key: impl IntoIterator<Item = ColumnName>) -> Self {
        self.primary_key = primary_key.into_iter().collect();
        self
    }

    #[inline]
    pub const fn name(&self) -> &ModelName {
        &self.name
    }

    #[inline]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    /// Where the table lives: the whole path, which is what a `FROM` clause names.
    ///
    /// Read [`Self::table_name`] instead wherever what is wanted is the name a column is qualified by
    /// or the name a file-registering engine registers under. Two accessors rather than one that
    /// guesses - `QualifiedTable::name` carries why both readings are real.
    #[inline]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// The table's own name, without whatever sits above it.
    #[inline]
    pub const fn table_name(&self) -> &TableName {
        self.table.name()
    }

    /// Every column this model exposes, each with whatever a source claimed about it.
    #[inline]
    pub fn columns(&self) -> impl ExactSizeIterator<Item = &Column> {
        self.columns.values()
    }

    /// One column by name, if this model declares it.
    #[inline]
    pub fn column(&self, name: &ColumnName) -> Option<&Column> {
        self.columns.get(name)
    }

    /// Which of this model's columns a source's own dictionary marked as its primary key. Evidence
    /// only - see [`Self::with_primary_key`].
    #[inline]
    pub const fn primary_key(&self) -> &BTreeSet<ColumnName> {
        &self.primary_key
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    #[inline]
    pub fn has_column(&self, column: &ColumnName) -> bool {
        self.columns.contains_key(column)
    }
}

/// A declared join between two models: two columns and a cardinality.
///
/// A pair of columns rather than a condition string. The condition form is what the reference
/// modelling languages use, and it is an escape hatch: `a.x = b.y OR 1 = 1` is a valid condition.
/// Equality on one column each is the whole of what a model needs to say here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Relationship {
    name: RelationshipName,
    origin_model: ModelName,
    origin_column: ColumnName,
    target_model: ModelName,
    target_column: ColumnName,
    join_type: JoinType,
}

impl Relationship {
    pub const fn new(
        name: RelationshipName,
        origin_model: ModelName,
        origin_column: ColumnName,
        target_model: ModelName,
        target_column: ColumnName,
        join_type: JoinType,
    ) -> Self {
        Self {
            name,
            origin_model,
            origin_column,
            target_model,
            target_column,
            join_type,
        }
    }

    #[inline]
    pub const fn name(&self) -> &RelationshipName {
        &self.name
    }

    #[inline]
    pub const fn origin_model(&self) -> &ModelName {
        &self.origin_model
    }

    #[inline]
    pub const fn origin_column(&self) -> &ColumnName {
        &self.origin_column
    }

    #[inline]
    pub const fn target_model(&self) -> &ModelName {
        &self.target_model
    }

    #[inline]
    pub const fn target_column(&self) -> &ColumnName {
        &self.target_column
    }

    #[inline]
    pub const fn join_type(&self) -> JoinType {
        self.join_type
    }
}

/// A chain of relationships a dimension is reached through, in the order the author wrote them.
///
/// **Non-empty by construction and ordered by construction.** [`ViaChain::of`] refuses an empty
/// list, and the private `Vec` keeps the order it was handed: hop 1 is the relationship from the
/// metric's model, hop N's origin must be hop N-1's target. That link-up is a cross-reference, so
/// [`Definitions::assemble`](crate::catalog::Definitions::assemble) checks it - the type holds only
/// what it can see. There is no `Deserialize` here, the same shape as
/// [`Dimension`]: adapters own how a document spells a chain, and
/// `crate::catalog::consistency` is the gate the value passes through.
///
/// No `Deref` or `Borrow` - the invariant this type exists to hold is the one an emptied or
/// reordered chain would break - and `as_slice` is the only way to lend the hops. A caller that
/// wants hop 1 or the hop pairs walks the slice with `split_first` or `windows`, which is also what
/// keeps the crate's no-indexing rule honest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ViaChain(Vec<RelationshipName>);

/// Why a relationship chain could not be built.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidViaChain {
    #[error("a via chain must name at least one relationship")]
    Empty,
}

impl ViaChain {
    /// A chain in the order given, refusing the empty chain.
    ///
    /// One constructor rather than a constructor plus an `is_valid`: an empty chain cannot be
    /// minted, so no downstream code re-checks it.
    pub fn of(hops: Vec<RelationshipName>) -> Result<Self, InvalidViaChain> {
        if hops.is_empty() {
            return Err(InvalidViaChain::Empty);
        }
        Ok(Self(hops))
    }

    /// The hops, in declared order. Hop 1 is the relationship from the metric's own model.
    pub fn as_slice(&self) -> &[RelationshipName] {
        &self.0
    }
}

/// An attribute a metric declares it can be broken down by.
///
/// `via` is `None` for a column on the metric's own model and `Some` for one reached through one
/// declared relationship - or through a chain of them, in the order the author wrote them. The
/// order is load-bearing: hop N's origin must be hop N-1's target, so a chain is a single path and
/// not a set of relationships, and the planner renders the joins in that order rather than choosing
/// one. **Every hop is refused if it could duplicate rows, and a chain crosses a data system
/// boundary at most once, only at its first hop**: hop 1 may cross - a single remote dimension is
/// the federated case the plan layer serves, by splitting the question into one link and one lookup
/// table - and a later hop is refused unless BOTH its ends sit on the metric's own source. So an
/// accepted chain is either wholly local or exactly one crossing hop, which are the two shapes the
/// plan layer can render, and no accepted hop changes what a measure sees. **The limit next to the
/// claim:** that is [`Definitions::assemble`]'s check, so it holds for a bundle that was assembled
/// here; `sutura_semantic`'s plan stage asks the same question again over the resolved chain,
/// because a load check alone is one edit away from being bypassed.
///
/// `allowed_values` is what makes a dimension filterable. `None` means it can be grouped by and not
/// filtered: a filter needs an allowlist, because the alternative is comparing against a value the
/// caller supplied, and the pinned bundle is the only thing entitled to say which values exist.
///
/// **Every entry is a [`DimensionValue`], and how many there may be is
/// [`MAX_VALUES_PER_DIMENSION`].** Both are new, and both close the same hole: this was
/// `Option<BTreeSet<String>>` read straight out of a YAML document, and `sutura_app::prompt`
/// interpolates the whole list into the line of an agent-facing document that tells an agent what it
/// may filter on. A value with an invisible code point in it made that line read as something other
/// than what it said; an unbounded count made it as long as an author liked. The character rule is
/// the type's and the count rule is [`Definitions::assemble`]'s, because a count is not a fact about
/// one value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Dimension {
    name: DimensionName,
    column: ColumnName,
    via: Option<ViaChain>,
    allowed_values: Option<BTreeSet<DimensionValue>>,
    description: Description,
}

impl Dimension {
    pub const fn new(
        name: DimensionName,
        column: ColumnName,
        via: Option<ViaChain>,
        allowed_values: Option<BTreeSet<DimensionValue>>,
        description: Description,
    ) -> Self {
        Self {
            name,
            column,
            via,
            allowed_values,
            description,
        }
    }

    #[inline]
    pub const fn name(&self) -> &DimensionName {
        &self.name
    }

    #[inline]
    pub const fn column(&self) -> &ColumnName {
        &self.column
    }

    /// The chain the dimension is reached through, in declared order, or nothing for a column on
    /// the metric's own model. Read the hops off the slice - a `&[RelationshipName]` is what every
    /// consumer of this field walks, and nothing else in the field is theirs. Not a `const fn`:
    /// `ViaChain::as_slice` returns a reference out of a `Vec`, which `const` cannot do.
    #[inline]
    pub fn via(&self) -> Option<&[RelationshipName]> {
        self.via.as_ref().map(ViaChain::as_slice)
    }

    #[inline]
    pub const fn allowed_values(&self) -> Option<&BTreeSet<DimensionValue>> {
        self.allowed_values.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    /// May this dimension be filtered on at all?
    ///
    /// Read from the presence of an allowlist rather than from a separate flag, so the two cannot
    /// disagree: a `filterable: true` beside an empty allowlist would be a dimension that permits
    /// filtering and permits no value.
    #[inline]
    pub const fn is_filterable(&self) -> bool {
        self.allowed_values.is_some()
    }

    /// Is `value` one the bundle declares?
    ///
    /// A dimension with no allowlist answers `false` for everything, which is the safe direction:
    /// the caller gets `DimensionNotFilterable` rather than a query.
    ///
    /// Takes a [`DimensionValue`] rather than a `&str`, so the two sides of the comparison are the
    /// same type: a caller's value is parsed by [`DimensionValue::parse`] at the wire boundary the way
    /// their metric name is parsed by [`MetricName::parse`](crate::model::MetricName::parse), and text
    /// that could not have been declared never reaches this comparison to be found absent from it.
    pub fn permits(&self, value: &DimensionValue) -> bool {
        self.allowed_values.as_ref().is_some_and(|values| values.contains(value))
    }
}

/// A number a metric is expected to produce, so that "it still means what it claimed" is checkable.
///
/// The value is text rather than a float on purpose. It is compared against the canonical rendering
/// of what the data system returned, and a float would make the comparison depend on how two
/// languages happen to print the same bits.
///
/// **Text, and now parsed text.** It was a `String` behind a `const` constructor written by both
/// catalog adapters, which made it the one authored scalar that entered this crate with no character
/// rule on it - see [`AnchorValue`] for the channel that closes and what it deliberately still does
/// not check. No `Deserialize`: nothing deserializes an `Anchor`, because each adapter deserializes
/// its own document shape and converts, so the derive was a public surface with no caller and one
/// more path into a private field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Anchor {
    range: TimeRange,
    value: AnchorValue,
}

impl Anchor {
    pub const fn new(range: TimeRange, value: AnchorValue) -> Self {
        Self { range, value }
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    /// The certified number as text.
    ///
    /// A `&str` rather than a `&AnchorValue`, because every caller either compares it against a
    /// rendered cell or prints it - and both want the text. **Whoever prints it uses `{:?}`**, for
    /// the reason [`RequiredFilter`]'s `Display` gives: quoting is what makes spacing visible in a
    /// line a person reads to decide whether a metric still means what it claimed.
    #[inline]
    pub fn value(&self) -> &str {
        self.value.as_str()
    }
}

/// A certified metric: one computation over one model, and the shapes of question it will answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Metric {
    name: MetricName,
    model: ModelName,
    #[serde(flatten)]
    computation: Computation,
    /// Predicates that are part of what this metric MEANS, applied to every question about it.
    ///
    /// A caller cannot see, choose or remove one. `mrr` means revenue from active subscriptions, and
    /// a statement that omits that predicate returns a different number under the same name.
    required_filters: Vec<RequiredFilter>,
    time_column: ColumnName,
    grains: BTreeSet<Grain>,
    /// Keyed, because every reader asks it "is this dimension declared, and what is it". The
    /// CONSTRUCTOR takes a vector - see [`Metric::new`] for why the two differ, and for the two
    /// refusals that are only askable before the keying.
    dimensions: BTreeMap<DimensionName, Dimension>,
    anchor: Option<Anchor>,
    description: Description,
    /// Who may see this metric - `docs/adr/0028`. Under the digest, like everything else here: a
    /// classification change moves it exactly as a rename would.
    audience: Audience,
}

impl Metric {
    /// A certified metric, or a refusal if two of its dimensions answer to one label.
    ///
    /// **Takes a `Vec<Dimension>` and returns a `Result`, and the argument for that is already
    /// written one level up.** [`Definitions::assemble`]: *"Takes vectors rather than maps so the
    /// duplicate checks are ours: a caller that built a map first has already silently dropped one
    /// of a duplicated pair."* This constructor took a map, so the check was not ours, and the two
    /// shipped adapters had answered the question differently - `sutura_catalog_local` refused a
    /// duplicate and `sutura_catalog_datahub` collected into a map and kept the last. One content,
    /// two [`Definitions`]. The module header above says two adapters reading the same content must
    /// produce the same one or one of them is wrong, and the golden suite could not see it because
    /// no fixture declares a duplicate.
    ///
    /// **A vector makes the bypass a compile error rather than a rule**, which is why the signature
    /// changed instead of a check being added beside the old one: an adapter cannot collapse the
    /// pair before this point any more, because there is nowhere earlier for it to collapse it. The
    /// field stays a [`BTreeMap`] - the digest is taken over the serialized form and every reader
    /// looks a dimension up by name - so the difference between the parameter and the field is the
    /// whole mechanism.
    ///
    /// **One scan and two refusals, because a duplicate and a folded pair are one rule.** The
    /// comparison is [`IdentifierCase::COARSEST`], which is true of two identical spellings too, so
    /// an exact repeat is the special case and is named as one:
    /// [`InconsistentDefinitions::DuplicateDimension`] says *declares dimension `region` twice*,
    /// which is what an author needs to read, and
    /// [`InconsistentDefinitions::TwoDimensionsOneLabel`] carries the pair. Asking it here rather
    /// than in [`Definitions::assemble`] is what makes this a parse: after `Ok`, no two of a
    /// metric's dimensions name one label and nothing downstream re-asks. `assemble` could not have
    /// asked - by the time a [`Metric`] reaches it the map has collapsed an exact pair - and the
    /// DECLARED order is here and nowhere later, so the refusal names the two spellings in the
    /// order the file wrote them. Same shape, and the same argument, as
    /// [`StatementTables::parse`](crate::plan::StatementTables::parse).
    ///
    /// **What a folded pair costs was measured rather than argued.** The pinned `DuckDB`
    /// (`v1.5.5 Variegata d8cdaa33fd`), whose `sutura_sql::Dialect::identifier_case` declares
    /// [`IdentifierCase::InsensitiveAscii`]:
    /// `SELECT "Region" FROM (SELECT 1 AS region, 2 AS "Region")` returns **1** - the `region`
    /// column's value - in a result column named `region`, and raises no ambiguity error.
    /// `SELECT *` over the same subquery projects `region, Region_1`, so the second label a caller
    /// was told to expect is not in the result at all. A wrong number and a missing column, from a
    /// catalog that loaded. Folded under `COARSEST` and not under the serving target's rule for the
    /// reason that constant carries: a bundle is dialect-agnostic, so the coarsest rule is the only
    /// one that cannot be wrong in the direction that returns a number.
    ///
    /// **Quadratic, and nothing caps how many dimensions a metric may declare**, so the limit is
    /// stated rather than implied: there is a cap on a dimension's VALUES
    /// ([`MAX_VALUES_PER_DIMENSION`]) and on the group-by keys one question may ask for
    /// (`crate::query::MAX_DIMENSIONS`), and neither is this. What makes it affordable anyway is
    /// position rather than size - it runs once per metric while a document that was read whole is
    /// being converted, and `Definitions`'s own `check_labels_against_table` is already the same shape
    /// over the same list. A cap on declared dimensions is worth having on its own merits and is not
    /// this constructor's to add.
    ///
    /// The refusal is an [`InconsistentDefinitions`] rather than an error of this constructor's own,
    /// so both adapters map it through the variant they already have for that type and neither
    /// grows a second one.
    pub fn new(
        name: MetricName,
        model: ModelName,
        computation: impl Into<Computation>,
        required_filters: Vec<RequiredFilter>,
        time_column: ColumnName,
        grains: BTreeSet<Grain>,
        dimensions: Vec<Dimension>,
        anchor: Option<Anchor>,
        description: Description,
        audience: Audience,
    ) -> Result<Self, InconsistentDefinitions> {
        let mut declared: BTreeMap<DimensionName, Dimension> = BTreeMap::new();
        for dimension in dimensions {
            let collision = declared
                .keys()
                .find(|earlier| IdentifierCase::COARSEST.names_one_thing(earlier.as_str(), dimension.name.as_str()))
                .cloned();
            if let Some(first) = collision {
                let second = dimension.name;
                return Err(if first == second {
                    InconsistentDefinitions::DuplicateDimension {
                        metric: name,
                        dimension: second,
                    }
                } else {
                    InconsistentDefinitions::TwoDimensionsOneLabel {
                        metric: name,
                        first,
                        second,
                    }
                });
            }
            drop(declared.insert(dimension.name.clone(), dimension));
        }
        Ok(Self {
            name,
            model,
            computation: computation.into(),
            required_filters,
            time_column,
            grains,
            dimensions: declared,
            anchor,
            description,
            audience,
        })
    }

    #[inline]
    pub const fn name(&self) -> &MetricName {
        &self.name
    }

    /// Who may see this metric - `docs/adr/0028`.
    #[inline]
    pub const fn audience(&self) -> &Audience {
        &self.audience
    }

    #[inline]
    pub const fn model(&self) -> &ModelName {
        &self.model
    }

    #[inline]
    pub const fn computation(&self) -> &Computation {
        &self.computation
    }

    /// The closed measure, if this metric does not use catalog-authored SQL.
    #[inline]
    pub const fn measure(&self) -> Option<&Measure> {
        self.computation.measure()
    }

    /// The predicates every question about this metric carries, whether the caller asked or not.
    #[inline]
    pub fn required_filters(&self) -> &[RequiredFilter] {
        &self.required_filters
    }

    #[inline]
    pub const fn time_column(&self) -> &ColumnName {
        &self.time_column
    }

    #[inline]
    pub const fn grains(&self) -> &BTreeSet<Grain> {
        &self.grains
    }

    #[inline]
    pub const fn dimensions(&self) -> &BTreeMap<DimensionName, Dimension> {
        &self.dimensions
    }

    #[inline]
    pub const fn anchor(&self) -> Option<&Anchor> {
        self.anchor.as_ref()
    }

    #[inline]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    #[inline]
    pub fn supports_grain(&self, grain: Grain) -> bool {
        self.grains.contains(&grain)
    }

    #[inline]
    pub fn dimension(&self, name: &DimensionName) -> Option<&Dimension> {
        self.dimensions.get(name)
    }
}

#[cfg(test)]
mod tests;
