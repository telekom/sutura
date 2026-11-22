#![forbid(unsafe_code)]
//! A [`SemanticCatalog`] over an RDBMS dictionary - the narrowest declaration, and the one that
//! needs no service.
//!
//! `docs/adr/0011-pluggable-by-declaration.md` defines the role this conversion implements. A
//! database dictionary is mainly DDL and comments: the tables, the columns, their constraints, and
//! table prose. It declares these kinds without inventing a semantic layer, and is measured
//! against that declaration rather than against the golden adapters' oracle.
//!
//! This crate implements the dictionary conversion from `github.com/telekom/sutura#151`. The
//! runtime derives the zero-metric physical-schema guidance from the pinned bundle rather than
//! coupling the application to this adapter.
//!
//! # What a real dictionary yields
//!
//! ADR 0016's method, applied here: read a real dictionary before writing the adapter. A throwaway
//! reader, measured once against a two-table Postgres 18 schema (two tables, a primary key each, one
//! foreign key, and table comments), reported the following:
//!
//! | What | Count |
//! | --- | --- |
//! | tables | 2 |
//! | columns | 6 |
//! | tables carrying a comment | 2 |
//! | foreign keys | 1 |
//! | primary/unique constraints | 2 |
//!
//! Three findings, and two of them are the declaration's content:
//!
//! 1. **A dictionary yields structure and prose, and nothing else.** Tables and columns are the
//!    [`Structure`](sutura_domain::capabilities::DefinitionKind::Structure) half; table comments are
//!    the [`Descriptions`](sutura_domain::capabilities::DefinitionKind::Descriptions)
//!    half. A dictionary carries **no measure, no grain, no definitional filter, no value allowlist
//!    and no anchor** - those are declared by a human in a semantic layer, which is what ADR 0011's
//!    table says ("certified metrics, measures, grains, allowed values: **no** - a human declares
//!    those elsewhere").
//! 2. **A foreign key carries no metric cardinality.** It names the source and target columns, so
//!    this adapter declares the `Cardinality` *capability* absent: a dictionary carries no metric to
//!    reach a dimension `via` a relationship.
//! 3. **Only the complete referenced primary or unique key licenses many-to-one.** A native
//!    reader checks enforcement and validation in the same snapshot, then supplies every ordered
//!    pair and the whole target key. A partial composite is refused. Operator assertions instead
//!    require visible endpoints and compatible known types; a configured boot probe can also
//!    check current target uniqueness, without promising future data stays unique.
//!
//! Dictionaries may also carry per-column types/comments and primary keys. Those declarations
//! are descriptive evidence, not metric cardinality or measures. [`Table::with_column_metadata`]
//! and [`Table::with_primary_key`] attach them without changing a reader's required structure.
//!
//! The fixture carries a type, a comment and a key for two columns, so the converter's mapping and
//! byte accounting are exercised without a socket. The feature-gated live reader also reads these
//! fields from a declared Postgres documentation schema; its provisioned test is the socket venue.
//!
//! # The declaration, and what it means for the bundle
//!
//! [`SemanticCatalog::capabilities`] provides `Structure` and may provide `Descriptions`,
//! `Relationships`, `ColumnTypes` and `ColumnDescriptions`. A sparse dictionary - structure with no
//! comments, foreign keys, column types or column comments - is therefore faithful without making
//! structure optional. What is declared is nothing more: no `Cardinality`
//! (a foreign key vouches for no metric fan-out), no `Metrics`, no
//! `Grains`, no `RequiredFilters`, no `AllowedValues`, no `Anchors`, and an empty knowledge half.
//! A bundle from this source therefore **loads, pins and validates with zero metrics**, and answers
//! no certified question - which is issue #115's shape and the whole reason the declaration exists:
//! a deployment whose whole model is a physical schema must not be told it has metrics it does not.
//!
//! # What is built here, and what is NOT
//!
//! This crate contains the conversion [`RdbmsCatalog`] applies to dictionary records, and it is
//! tested against a fake reader that serves a recorded dictionary - the port gets a fake,
//! not mocked SQL (`github.com/telekom/sutura#151`'s thing 4). Since #972, it also contains the
//! live implementors over a Postgres ([`postgres_reader`]) and an Oracle ([`oracle_reader`])
//! documentation schema, behind default-off `live` and `live-oracle` features so the library
//! closure stays domain + thiserror and no build links either driver's stack without asking for
//! it.
//!
//! **The fake dominates the suite; the live reader is the production half, feature-gated.**
//! [`DictionaryReader`] is the seam they implement ([`fixture::FixtureReader`] the recorded corpus,
//! [`AnyDictionaryReader`] a real connection), and the conversion is the same for each.
//! A composition root that links the `live` feature serves `catalog.kind: rdbms`; a build without
//! it refuses by name. (Its only other dependant is `sutura-app`, as a dev-dependency.)

pub mod descriptions;
mod relationships;
pub use relationships::{
    DeclarationError, DeclaredRelationship, Relationship, RelationshipError, SingleColumnTargetUniqueness, TargetKeyKind,
};
#[cfg(feature = "live")]
mod documentation;
pub mod fixture;
mod native;
pub use native::{NativeAllowlist, NativeLoad, NativeSelectionError};
#[cfg(feature = "live-oracle")]
pub mod oracle_reader;
#[cfg(feature = "live")]
pub mod postgres_reader;

/// The live reader a declared `connection.dialect` selects, so one [`RdbmsCatalog`] type holds
/// either without a trait object.
#[cfg(feature = "live")]
#[derive(Debug)]
pub enum AnyDictionaryReader {
    /// The Postgres documentation-schema reader.
    Postgres(Box<crate::postgres_reader::PostgresReader>),
    /// The Oracle documentation-schema reader, behind the `live-oracle` feature.
    #[cfg(feature = "live-oracle")]
    Oracle(Box<crate::oracle_reader::OracleReader>),
}

#[cfg(feature = "live")]
impl AnyDictionaryReader {
    pub fn with_relationships_view(self, view: Option<&str>) -> Result<Self, crate::postgres_reader::InvalidReaderConfig> {
        match self {
            Self::Postgres(reader) => reader
                .with_relationships_view(view)
                .map(|reader| Self::Postgres(Box::new(reader))),
            #[cfg(feature = "live-oracle")]
            Self::Oracle(reader) => reader
                .with_relationships_view(view)
                .map(|reader| Self::Oracle(Box::new(reader))),
        }
    }

    #[must_use]
    pub fn with_relationships(self, relationships: Vec<DeclaredRelationship>) -> Self {
        match self {
            Self::Postgres(reader) => Self::Postgres(Box::new(reader.with_relationships(relationships))),
            #[cfg(feature = "live-oracle")]
            Self::Oracle(reader) => Self::Oracle(Box::new(reader.with_relationships(relationships))),
        }
    }

    pub const fn bounds(&self) -> DictionaryBounds {
        match self {
            Self::Postgres(reader) => reader.bounds(),
            #[cfg(feature = "live-oracle")]
            Self::Oracle(reader) => reader.bounds(),
        }
    }
}

#[cfg(feature = "live")]
impl DictionaryReader for AnyDictionaryReader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        match self {
            Self::Postgres(reader) => reader.read_dictionary(),
            #[cfg(feature = "live-oracle")]
            Self::Oracle(reader) => reader.read_dictionary(),
        }
    }
}

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::num::NonZeroU64;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Audience, Column, Definitions, Description, Model};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{
    ColumnName, DatasetName, InvalidIdentifier, ModelName, ProjectName, QualifiedTable, SourceName, TableName, TableQualifier,
};
use sutura_domain::pinned::{CatalogKind, DefinitionVersion, PinnedDefinitions, SemanticCatalog};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);
/// One parsed physical address and the semantic model built over it.
type ConvertedModel = (QualifiedTable, Model);

/// Refuses whitespace before the shared parser can trim it from a physical identifier.
fn exact_physical_identifier(raw: &str) -> Result<&str, InvalidIdentifier> {
    if let Some(offending) = raw.chars().find(|character| character.is_whitespace()) {
        return Err(InvalidIdentifier::IllegalCharacter { offending });
    }
    Ok(raw)
}

/// Where a dictionary's records come from.
///
/// **The fake seam.** Everything above this trait is decided and tested against a recorded
/// dictionary served by [`fixture::FixtureReader`]. The feature-gated
/// [`postgres_reader::PostgresReader`] reads a declared documentation schema over a Postgres
/// socket and maps its failures into [`RdbmsError::Read`]. Both use the same conversion.
///
/// Composite foreign keys carry every ordered column pair and the complete referenced key.
/// Native readers omit constraints whose enforcement or complete target uniqueness is unproven.
pub trait DictionaryReader {
    /// Reads the deployment's dictionary, in whatever shape this crate defines.
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError>;
}

mod error;
pub use error::RdbmsError;

/// A catalog read from an RDBMS dictionary.
///
/// Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
/// one per composition and a generic keeps the chosen reader visible. It carries the declared name
/// and version it is recorded under, the same way the other catalog adapters carry their own.
#[derive(Debug, Clone)]
pub struct RdbmsCatalog<R> {
    name: SourceName,
    source_alias: SourceName,
    version: DefinitionVersion,
    reader: R,
    bounds: Option<DictionaryBounds>,
    audience: Option<Audience>,
}

impl<R> RdbmsCatalog<R> {
    /// Opens a catalog over a dictionary reader.
    pub fn new(name: SourceName, version: DefinitionVersion, reader: R) -> Self {
        let source_alias = name.clone();
        Self {
            name,
            source_alias,
            version,
            reader,
            bounds: None,
            audience: None,
        }
    }

    /// Changes the reader without changing the catalog declaration.
    pub fn map_reader<S>(self, map: impl FnOnce(R) -> S) -> RdbmsCatalog<S> {
        RdbmsCatalog {
            name: self.name,
            source_alias: self.source_alias,
            version: self.version,
            reader: map(self.reader),
            bounds: self.bounds,
            audience: self.audience,
        }
    }

    /// Binds models from this dictionary to a separately declared data source.
    #[must_use]
    pub fn with_source_alias(mut self, source_alias: SourceName) -> Self {
        self.source_alias = source_alias;
        self
    }

    /// Declares who may see every inferred model. Without it, verified callers see none.
    #[must_use]
    pub fn with_audience(mut self, audience: Audience) -> Self {
        self.audience = Some(audience);
        self
    }

    /// Bounds every decoded dictionary, including fake readers; absence leaves it unchecked here.
    /// Live readers also enforce bounds inline: Postgres bills already allocated Arrow batches
    /// (and limits fetched rows); Oracle bills streamed rows. This is a post-decode check, not
    /// a memory cap for the network read.
    #[must_use]
    pub const fn with_bounds(mut self, bounds: DictionaryBounds) -> Self {
        self.bounds = Some(bounds);
        self
    }
}

impl<R: DictionaryReader> RdbmsCatalog<R> {
    fn convert_relationship(
        relationship: &Relationship,
        models_by_table: &BTreeMap<QualifiedTable, ModelName>,
    ) -> Result<sutura_domain::catalog::Relationship, RdbmsError> {
        relationships::convert::<R>(relationship, models_by_table)
    }

    /// Loads once, returning the matching allowlist for composition before publication.
    pub fn load_with_allowlist(&self) -> Result<NativeLoad, RdbmsError> {
        native::load(self)
    }

    /// The conversion: a [`Dictionary`] into the two halves of a bundle.
    ///
    /// Kept as a method that takes a [`Dictionary`] so the whole decision half is testable against
    /// a reader that returns one, and a reader's only job is to produce a dictionary.
    fn assemble(&self, dictionary: &Dictionary) -> Result<Content, RdbmsError> {
        if dictionary.tables.is_empty() {
            return Err(RdbmsError::NoVisibleTables);
        }
        if let Some(bounds) = self.bounds {
            let rows = dictionary.tables.len() as u64 + dictionary.relationships.len() as u64;
            if rows > bounds.max_rows().get() {
                return Err(RdbmsError::ExceedsBounds {
                    rows,
                    limit: bounds.max_rows(),
                });
            }
        }
        let mut models = Vec::with_capacity(dictionary.tables.len());
        let mut models_by_table = BTreeMap::new();
        for table in &dictionary.tables {
            let (physical_table, model) = self.convert_model(table)?;
            match models_by_table.entry(physical_table) {
                Entry::Vacant(slot) => {
                    slot.insert(model.name().clone());
                }
                Entry::Occupied(taken) => {
                    return Err(RdbmsError::DuplicateTable {
                        table: taken.key().to_string(),
                    });
                }
            }
            models.push(model);
        }
        let mut relationships = Vec::with_capacity(dictionary.relationships.len());
        for relationship in &dictionary.relationships {
            relationships.push(Self::convert_relationship(relationship, &models_by_table)?);
        }

        let definitions =
            Definitions::assemble(models, relationships, Vec::new()).map_err(|cause| RdbmsError::Inconsistent { cause })?;
        Ok((definitions, Knowledge::none()))
    }

    /// A table record into a [`Model`].
    fn convert_model(&self, table: &Table) -> Result<ConvertedModel, RdbmsError> {
        let physical_table = Self::convert_table_address(table.address())?;
        let name = ModelName::parse(table.model()).map_err(|cause| RdbmsError::ModelName {
            table: physical_table.to_string(),
            model: table.model().to_owned(),
            cause,
        })?;
        let mut columns = Vec::with_capacity(table.columns.len());
        for column in &table.columns {
            let column_name = exact_physical_identifier(column)
                .and_then(ColumnName::parse)
                .map_err(|cause| RdbmsError::ColumnName {
                    table: physical_table.to_string(),
                    column: column.clone(),
                    cause,
                })?;
            let metadata = table.column_metadata(column);
            let column = Column::from_metadata(
                column_name,
                metadata.and_then(ColumnMetadata::data_type),
                metadata.and_then(ColumnMetadata::description),
                None,
            )
            .map_err(|refusal| {
                let (column, cause) = refusal.into_parts();
                RdbmsError::ColumnDescription {
                    table: physical_table.to_string(),
                    column,
                    cause,
                }
            })?;
            columns.push(
                column
                    .with_text_policy(metadata.map_or_default(|metadata| metadata.text.clone()))
                    .with_collation(metadata.map_or_default(|metadata| metadata.collation.clone())),
            );
        }
        let description = table
            .description()
            .map(|raw| {
                Description::parse(raw).map_err(|cause| RdbmsError::Description {
                    table: physical_table.to_string(),
                    cause,
                })
            })
            .transpose()?
            .unwrap_or_default();
        let primary_key = table
            .primary_key()
            .iter()
            .map(|column| {
                exact_physical_identifier(column)
                    .and_then(ColumnName::parse)
                    .map_err(|cause| RdbmsError::ColumnName {
                        table: physical_table.to_string(),
                        column: column.clone(),
                        cause,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut model = Model::new(name, self.source_alias.clone(), physical_table.clone(), columns, description)
            .with_primary_key(primary_key)
            .map_err(|cause| RdbmsError::Inconsistent { cause })?;
        if let Some(audience) = &self.audience {
            model = model.with_audience(audience.clone());
        }
        Ok((physical_table, model))
    }

    fn convert_table_address(address: &TableAddress) -> Result<QualifiedTable, RdbmsError> {
        let rendered = address.to_string();
        let table = exact_physical_identifier(address.table())
            .and_then(TableName::parse)
            .map_err(|cause| RdbmsError::TableName {
                table: rendered.clone(),
                cause,
            })?;
        let schema = exact_physical_identifier(address.schema())
            .and_then(DatasetName::parse)
            .map_err(|cause| RdbmsError::SchemaName {
                table: rendered.clone(),
                schema: address.schema().to_owned(),
                cause,
            })?;
        let qualifier = if let Some(catalog) = address.catalog() {
            let catalog = exact_physical_identifier(catalog)
                .and_then(ProjectName::parse)
                .map_err(|cause| RdbmsError::CatalogName {
                    table: rendered,
                    catalog: catalog.to_owned(),
                    cause,
                })?;
            TableQualifier::in_project(catalog, schema)
        } else {
            TableQualifier::in_dataset(schema)
        };
        Ok(QualifiedTable::new(Some(qualifier), table))
    }
}

impl<R> SemanticCatalog for RdbmsCatalog<R>
where
    R: DictionaryReader,
{
    type Error = RdbmsError;

    /// A **declaring** adapter, measured against its own declaration rather than the golden oracle.
    const KIND: CatalogKind = CatalogKind::Declaring;
    /// Provides `Structure`, `Descriptions`, `Relationships`, `ColumnTypes` and
    /// `ColumnDescriptions` - exactly what a dictionary *can* yield - and nothing else.
    ///
    /// `Structure` is unconditional; the other four are declared-and-conditional. A native
    /// relationship needs complete primary/unique target-key evidence, including every ordered
    /// composite pair. Missing evidence refuses conversion with [`RdbmsError::TargetUniquenessUnknown`];
    /// live readers omit constraints that lack the required enforcement and validation. Operator
    /// assertions instead require visible endpoints and compatible types before conversion.
    /// A schema with no foreign key lawfully carries no relationship; optional table and column
    /// comments likewise need not be present for structure to load.
    ///
    /// What is declared is nothing more. No
    /// [`Cardinality`](sutura_domain::capabilities::DefinitionKind::Cardinality) - a foreign key
    /// vouches for no fan-out in the dangerous direction, and a dictionary carries no metric for a
    /// dimension to be reached `via` one, so `produced` observes `Cardinality` absent and the
    /// declaration agrees. No `Metrics`, no `Grains`, no `RequiredFilters`, no `AllowedValues`, no
    /// `Anchors`. `primary_key` evidence rides under `Structure` rather than its own kind - see
    /// [`sutura_domain::catalog::Model::with_primary_key`]'s own doc for why it is evidence and not
    /// a declarable capability. **The knowledge half is empty for the same reason
    /// `sutura-catalog-datahub`'s is**: nothing here reads a glossary-like aspect, so a standalone
    /// bundle carries no `Knowledge` referent for a phrase or a caveat to attach to.
    ///
    /// Written as `of([..])` plus `and_may_provide([..])` with two explicit lists, the way an
    /// adapter over a fixed external schema must, so a tenth definition kind or a fifth knowledge
    /// capability leaves this declaration alone rather than silently widening it.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure]).and_may_provide([
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
                DefinitionKind::ColumnTypes,
                DefinitionKind::ColumnDescriptions,
            ]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        self.load_with_allowlist().map(|(pinned, _)| pinned)
    }
}

/// A table's physical address as a dictionary reports it.
///
/// A schema is required because it is the first part that distinguishes same-named tables in one
/// database. The catalog is optional because not every target renders a three-part table path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAddress {
    catalog: Option<String>,
    schema: String,
    table: String,
}

impl TableAddress {
    /// A physical table address, split into the dictionary fields that own its identity.
    pub const fn new(catalog: Option<String>, schema: String, table: String) -> Self {
        Self { catalog, schema, table }
    }

    /// A table in a schema of the connected catalog.
    pub const fn in_schema(schema: String, table: String) -> Self {
        Self::new(None, schema, table)
    }

    /// The catalog above the schema, when the dictionary reports one for generated statements.
    #[inline]
    pub fn catalog(&self) -> Option<&str> {
        self.catalog.as_deref()
    }

    /// The schema immediately above the table.
    #[inline]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// The table's own name.
    #[inline]
    pub fn table(&self) -> &str {
        &self.table
    }
}

impl core::fmt::Display for TableAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(catalog) = self.catalog() {
            write!(f, "{catalog}.")?;
        }
        write!(f, "{}.{}", self.schema, self.table)
    }
}

/// What a dictionary's own `information_schema`/catalog view says about one column beyond its
/// name: its declared data type, and a column comment if a human wrote one.
///
/// A separate type from the domain's [`sutura_domain::catalog::Column`] rather than that type
/// itself, because this crate's own identifiers are still bare dictionary strings at this point -
/// the same reason [`Table`]'s own fields are `String` rather than [`sutura_domain::model::ColumnName`].
/// [`RdbmsCatalog::convert_model`] is where the parse happens for all of them together.
///
/// **`datahub::document` and `openmetadata::document` declare the identical two fields, and stay
/// separate on purpose.** Each is that adapter's own reading of a wire shape this crate has no
/// business depending on - one adapter importing another's type crosses the boundary
/// `sutura-catalog-*` crates are not supposed to cross, for a coincidence of shape between three
/// sources whose actual dictionaries have no reason to keep matching.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnMetadata {
    data_type: Option<String>,
    description: Option<String>,
    text: sutura_domain::catalog::TextPolicy,
    collation: sutura_domain::catalog::Collation,
}

impl ColumnMetadata {
    pub const fn new(data_type: Option<String>, description: Option<String>) -> Self {
        Self {
            data_type,
            description,
            text: sutura_domain::catalog::TextPolicy::empty(),
            collation: sutura_domain::catalog::Collation::Unknown,
        }
    }

    /// The collation a reader's own dictionary evidence established for this column.
    #[must_use]
    pub fn with_collation(mut self, collation: sutura_domain::catalog::Collation) -> Self {
        self.collation = collation;
        self
    }

    #[inline]
    pub const fn collation(&self) -> &sutura_domain::catalog::Collation {
        &self.collation
    }

    #[inline]
    pub fn data_type(&self) -> Option<&str> {
        self.data_type.as_deref()
    }

    #[inline]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The text policy, including any raw-prefilter acknowledgement, a reader or native selection
    /// attached to this column.
    #[inline]
    pub const fn text(&self) -> &sutura_domain::catalog::TextPolicy {
        &self.text
    }
}

/// One semantic model, the physical table it selects, and the prose written against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    model: String,
    address: TableAddress,
    columns: Vec<String>,
    description: Option<String>,
    column_metadata: BTreeMap<String, ColumnMetadata>,
    primary_key: Vec<String>,
}

impl Table {
    /// A semantic model over a physical table.
    pub const fn new(model: String, address: TableAddress, columns: Vec<String>, description: Option<String>) -> Self {
        Self {
            model,
            address,
            columns,
            description,
            column_metadata: BTreeMap::new(),
            primary_key: Vec::new(),
        }
    }

    /// Attaches per-column type and comment evidence, keyed by the dictionary's own column spelling.
    ///
    /// A column named here that is not in [`Self::columns`] is dropped rather than refused: the
    /// conversion reads metadata only for a column it is already about to declare, and a stray key
    /// says nothing this crate's error vocabulary is set up to report against a table.
    #[must_use]
    pub fn with_column_metadata(mut self, metadata: impl IntoIterator<Item = (String, ColumnMetadata)>) -> Self {
        self.column_metadata = metadata.into_iter().collect();
        self
    }

    /// Declares which of this table's columns the dictionary's own primary or unique-key constraint
    /// names - evidence only, the same as [`sutura_domain::catalog::Model::with_primary_key`], which
    /// is where this arrives once converted.
    ///
    /// **Uncorrelated with [`SingleColumnTargetUniqueness`], and that is stated rather than
    /// reconciled.** A [`Relationship`]'s target-uniqueness evidence licenses one specific foreign
    /// key's `ManyToOne` direction; this evidence describes the table's OWN key, independent of
    /// whether anything references it. Nothing here checks the two against each other - a column
    /// this method names could be the primary key, a unique key that is not the primary one, or
    /// (a reader's bug) neither, and this type cannot tell which from the name alone. A reader is
    /// the only thing that could keep them consistent, because only a reader sees the dictionary's
    /// own labelling of which constraint is which.
    #[must_use]
    pub fn with_primary_key(mut self, primary_key: Vec<String>) -> Self {
        self.primary_key = primary_key;
        self
    }

    /// The semantic model name assigned to this table.
    #[inline]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The physical table address, as the dictionary spells each part.
    #[inline]
    pub const fn address(&self) -> &TableAddress {
        &self.address
    }

    /// The columns the table exposes.
    #[inline]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The table's comment, if a human wrote one.
    #[inline]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// One column's type/comment evidence, by the dictionary's own spelling of its name.
    #[inline]
    pub fn column_metadata(&self, column: &str) -> Option<&ColumnMetadata> {
        self.column_metadata.get(column)
    }

    /// Which columns the dictionary's own constraint names as this table's primary key.
    #[inline]
    pub fn primary_key(&self) -> &[String] {
        &self.primary_key
    }
}

/// A dictionary: the tables, and the relationships a foreign key between them records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dictionary {
    tables: Vec<Table>,
    relationships: Vec<Relationship>,
    native: bool,
}

impl Dictionary {
    /// Checks every declared pair against selected endpoints and compatible known types.
    pub fn apply_relationship_declarations(
        self,
        declarations: &[DeclaredRelationship],
        dialect: sutura_domain::warehouse::schema::TypeDialect,
        unbounded_number: Option<sutura_domain::warehouse::schema::UnboundedNumber>,
    ) -> Result<Self, RdbmsError> {
        relationships::declared::apply(self, declarations, dialect, unbounded_number)
    }

    /// Applies ordered prose without changing visible structure; counts ignored column rows.
    pub fn overlay_descriptions(
        self,
        sources: &[descriptions::DescriptionOverlay],
    ) -> Result<descriptions::OverlaidDictionary, RdbmsError> {
        descriptions::apply(self, sources)
    }

    /// A dictionary assembled from what a reader fetched.
    pub const fn new(tables: Vec<Table>, relationships: Vec<Relationship>) -> Self {
        Self {
            tables,
            relationships,
            native: false,
        }
    }

    /// Applies native selection before the dictionary crosses its reader port.
    pub fn native(
        self,
        selection: &sutura_domain::catalog::DictionarySelection,
        dialect: sutura_domain::warehouse::schema::TypeDialect,
    ) -> Result<Self, RdbmsError> {
        self.native_with_unbounded_number(selection, dialect, None)
    }

    /// Native selection with the source's explicitly declared unbounded NUMBER representation.
    pub fn native_with_unbounded_number(
        self,
        selection: &sutura_domain::catalog::DictionarySelection,
        dialect: sutura_domain::warehouse::schema::TypeDialect,
        unbounded_number: Option<sutura_domain::warehouse::schema::UnboundedNumber>,
    ) -> Result<Self, RdbmsError> {
        let mut selected = crate::native::select(self, selection, dialect, unbounded_number)?;
        selected.native = true;
        Ok(selected)
    }

    /// The tables the dictionary names.
    #[inline]
    pub fn tables(&self) -> &[Table] {
        &self.tables
    }

    /// The relationships a foreign key records.
    #[inline]
    pub fn relationships(&self) -> &[Relationship] {
        &self.relationships
    }
}

/// What one dictionary read may spend: a row cap and a byte cap.
///
/// Both caps are non-zero by type, including at config load; the reader chooses absent defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictionaryBounds {
    max_rows: NonZeroU64,
    max_bytes: NonZeroU64,
}

impl DictionaryBounds {
    #[must_use]
    pub const fn new(max_rows: NonZeroU64, max_bytes: NonZeroU64) -> Self {
        Self { max_rows, max_bytes }
    }

    #[inline]
    #[must_use]
    pub const fn max_rows(&self) -> NonZeroU64 {
        self.max_rows
    }

    #[inline]
    #[must_use]
    pub const fn max_bytes(&self) -> NonZeroU64 {
        self.max_bytes
    }
}

#[cfg(all(test, feature = "live"))]
mod collation_live_tests;
#[cfg(test)]
mod collation_tests;
#[cfg(test)]
mod tests;
