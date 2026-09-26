#![forbid(unsafe_code)]
//! A [`SemanticCatalog`] over a directory of Open Data Contract Standard v3 contract documents.
//!
//! One YAML document per contract, each an [ODCS v3 / Bitol](https://bitol.io/open-data-contract-standard/)
//! data contract: the `schema` array's [`SchemaObject`]s are the contract's physical tables, each
//! `properties[].name` is that table's column set, and a `SchemaObject.relationships` array (v3.1+)
//! records the joins between them. `docs/what-a-data-contract-can-carry.md` is the finding that
//! decides this adapter's whole shape.
//!
//! **The ODCS vocabulary is richer than OKF's on the schema side, and just as silent on the
//! measure.** It names primary keys, `required` and `unique` per column, and carries a free-text
//! per-column `classification`; from v3.1.0 it carries a foreign key whose single-column target
//! `primaryKey`/`unique` evidence can licence a join under the same rule `sutura-catalog-rdbms`
//! applies. What it never carries, at any version, is a metric entity - so an adapter over it
//! **declares** `Structure`; **may-provides** `Descriptions`, `ColumnTypes`, `ColumnDescriptions`
//! (each optional in the schema) and `Relationships` (v3.1+, and only where a relationship's single-
//! column target carries `primaryKey`/`unique` evidence); **never declares** `Cardinality` (a
//! contract vouches for no metric fan-out, and there is no metric to own a dimension here); and
//! **reports-not-defines** the quality rules, the SLA and the `classification` while **declaring**
//! `Metrics`, `RequiredFilters`, `Grains`, `AllowedValues` and `Anchors` absent. Being a
//! `declaring` source measured against that declaration is the whole point of the
//! [`CatalogKind::Declaring`] class.
//!
//! Three properties of the load worth stating, because each is a mechanism rather than a wish:
//!
//! **The walk is sorted and refused-when-empty.** `read_dir` returns entries in filesystem order,
//! and a digest that moves between two runs over unchanged files is a digest nobody can act on, so
//! the walk collects into a `BTreeSet` and yields sorted. An empty or missing directory is an error
//! ([`DataContractError::Empty`], [`DataContractError::NotADirectory`]) rather than a silently-empty
//! catalog, for the same reason the OKF and local catalogs refuse one. This walk and the read are
//! the bounded, single-open shape `sutura-catalog-okf` carries after `#1022`'s hardening - shared
//! now through one crate, `sutura_bounded_read`, rather than a third copy of one mechanism
//! (`github.com/telekom/sutura#1045`).
//!
//! **A contract without a self-report of its version is refused by name.** `apiVersion` and `kind`
//! are required by the standard and this adapter reads both: an unknown `apiVersion` (a `v2.x` or
//! a typo'd `v3`) and an unknown `kind` are refused rather than guessed at, because which fields a
//! contract may carry - in particular whether `relationships` exist - is a property of the version
//! it stamps itself with.
//!
//! **`deny_unknown_fields` at the depths this adapter decodes, and validated names.** A contract,
//! `SchemaObject`, `SchemaProperty` or relationship that carries a key this adapter does not read
//! is refused rather than silently ignored, and each column, table and model name is parsed
//! through the domain's validated newtypes. What this does NOT reach: `servers`, `quality`,
//! `description` (contract-level), `team`, `support`, `slaProperties` and `context` are accepted as
//! opaque `serde_norway::Value` subtrees and are never themselves schema-validated - reported and
//! ignored by declaration, `docs/what-a-data-contract-can-carry.md`. A duplicate column name (two
//! `properties` entries with one `name`) is an error, because `BTreeMap` would deduplicate and a
//! silently-shrinking column set is a digest that lies.
//!
//! **A relationship this adapter cannot safely convert is refused, not dropped.** A schema-level
//! `relationships` array present before v3.1.0 (invalid against the published schema for that
//! version, but structurally decodable by this adapter's own type) is refused by name
//! ([`DataContractError::RelationshipsBeforeV3_1`]), and a property-level relationship (v3.1+,
//! `from` implicit) is refused by name ([`DataContractError::PropertyLevelRelationshipUnsupported`])
//! rather than accepted as opaque and silently vanishing from the bundle.

mod document;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use document::{Contract, RelationshipDecl, RelationshipEndpoint, SchemaObject};
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{
    Column, Definitions, Description, InconsistentDefinitions, InvalidDescription, JoinKey, JoinKeys, Model,
    Relationship as DomainRelationship,
};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, InvalidIdentifier, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The extensions a data-contract document may carry.
const DOCUMENT_EXTENSIONS: &[&str] = &["yaml", "yml"];

/// The two halves of a bundle's content, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// A single contract's decoded models, raw relationships and per-model unique-column evidence.
///
/// The relationships stay un-converted across file boundaries because the target-uniqueness rule
/// needs the UNIQUE-colon evidence of a target model that may live in a different document of the
/// same directory - so the conversion is assembled over the whole walk's evidence, not per file.
struct ParsedContract {
    models: Vec<Model>,
    relationships: Vec<RawRelationship>,
    unique_columns: BTreeMap<ModelName, BTreeSet<ColumnName>>,
}

/// A catalog read from a directory of ODCS v3 contract documents.
///
/// Like the OKF and local catalogs, it carries a declared NAME, a root and a version: the name is
/// the key the contribution manifest records this contributor under, the root is the directory of
/// contracts, and the version identifies which snapshot of that directory this is.
#[derive(Debug, Clone)]
pub struct DataContractCatalog {
    name: SourceName,
    root: PathBuf,
    version: DefinitionVersion,
}

impl DataContractCatalog {
    /// Points a catalog at a directory of ODCS v3 contract documents.
    ///
    /// The version is supplied rather than derived, because what identifies a snapshot of a directory
    /// is not something the directory knows - it is a commit id or a tag that only the caller has.
    pub const fn new(name: SourceName, root: PathBuf, version: DefinitionVersion) -> Self {
        Self { name, root, version }
    }

    /// The declared name this contributor is recorded under in a bundle's contribution manifest.
    #[inline]
    pub const fn name(&self) -> &SourceName {
        &self.name
    }

    #[inline]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every contract under the root, in sorted order.
    ///
    /// Sorted through a `BTreeSet`, so the walk is a function of the tree rather than of the
    /// filesystem. Files that are not a `yaml`/`yml` document are skipped rather than refused, so a
    /// stray `README.md` does not break a load. A regular file with the right extension and the wrong
    /// content still fails loudly at deserialisation.
    fn documents(&self) -> Result<Vec<PathBuf>, DataContractError> {
        sutura_bounded_read::walk(&self.root, DOCUMENT_EXTENSIONS, sutura_bounded_read::MAX_CATALOG_DOCUMENTS)
            .map_err(map_walk_error)
    }

    /// Reads every contract and assembles the bundle.
    ///
    /// The byte bound is enforced on the READ itself, not on a `stat` taken separately from it -
    /// each file is opened ONCE, through `rustix` with `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, and
    /// everything is read through that same handle: a document swapped for a symlink after the
    /// walk is refused at open (`ELOOP`) rather than followed, a swapped FIFO is refused rather
    /// than blocking the open, and a document that grows between the walk and the read cannot slip
    /// past the bound - the same hardening `sutura-catalog-okf` and `sutura-catalog-local` carry,
    /// `#1022`. **The limit stated there still holds here:** a document swapped for a REGULAR file
    /// at the same path between the walk and the open is read as whatever that path names now,
    /// bounded, not refused - the walk named a path, and the handle opened is of whatever that path
    /// currently is. "Opened once" is held by review, not by a test, for the reason
    /// [`sutura_bounded_read::read_document`]'s own module doc now states: the window between two
    /// back-to-back opens is sub-microsecond, under what a swap-timing test can land reliably. The
    /// refusal paths and their exact reach are [`sutura_bounded_read::read_document`]'s contract.
    fn read_all(&self) -> Result<Content, DataContractError> {
        let mut models = Vec::new();
        let mut relationships = Vec::new();
        let mut unique_columns: BTreeMap<ModelName, BTreeSet<ColumnName>> = BTreeMap::new();
        let mut total_bytes: u64 = 0;
        for path in self.documents()? {
            // ONE open per contract, and everything about the file decided from the handle that is
            // actually read - the read, the regular-file check, the byte budget and the post-read
            // recheck all live in `sutura_bounded_read::read_document`, on the ONE handle it
            // opened. The refusal half, the `O_NOFOLLOW` / `O_NONBLOCK` / `O_CLOEXEC` flags, is that
            // crate's open; see its `read.rs` for why a document swapped for a symlink or a FIFO is
            // refused at open, and why the budget is enforced on the read itself.
            let text = sutura_bounded_read::read_document(&self.root, &path, total_bytes).map_err(map_read_error)?;
            total_bytes += text.len() as u64;
            let parsed = self.parse_contract(&path, &text)?;
            models.extend(parsed.models);
            relationships.extend(parsed.relationships);
            for (model, columns) in parsed.unique_columns {
                unique_columns.entry(model).or_default().extend(columns);
            }
        }

        let mut domain_relationships = Vec::with_capacity(relationships.len());
        for relationship in &relationships {
            domain_relationships.push(self.convert_relationship(relationship, &unique_columns)?);
        }
        let definitions = Definitions::assemble(models, domain_relationships, Vec::new())
            .map_err(|cause| DataContractError::Inconsistent { cause })?;
        // This adapter supplies no referent-bearing knowledge. `KnowledgeInput::none()` assembles an
        // empty knowledge that declares nothing - which is what `capabilities()` says it declares.
        let knowledge = Knowledge::assemble(&definitions, KnowledgeInput::none())
            .map_err(|cause| DataContractError::UncheckableKnowledge { cause })?;
        Ok((definitions, knowledge))
    }

    /// One contract document on disk into its models, raw relationships and unique-column evidence.
    fn parse_contract(&self, path: &Path, text: &str) -> Result<ParsedContract, DataContractError> {
        let contract: Contract = serde_norway::from_str(text).map_err(|cause| DataContractError::Malformed {
            path: path.to_path_buf(),
            cause,
        })?;
        // `apiVersion` and `kind` are how a contract stamps its own version; an unknown value is
        // refused by name because which fields the document may carry is a fact about that version.
        let version = ApiVersion::parse(&contract.api_version).map_err(|cause| DataContractError::UnsupportedVersion {
            path: path.to_path_buf(),
            cause,
        })?;
        if contract.kind != KIND_DATA_CONTRACT {
            return Err(DataContractError::UnknownKind {
                path: path.to_path_buf(),
                found: contract.kind,
            });
        }

        let mut models = Vec::with_capacity(contract.schema.len());
        let mut relationships = Vec::new();
        let mut unique_columns = BTreeMap::new();
        for object in contract.schema {
            models.push(self.convert_model(path, &object)?);
            let model_name = ModelName::parse(object.name.as_str()).map_err(|cause| DataContractError::InvalidName {
                path: path.to_path_buf(),
                cause,
            })?;
            // Collect the unique-column evidence (primaryKey OR unique) that a relationship's
            // target-tuniqueness rule reads, keyed by the model this object declares.
            let mut unique = BTreeSet::new();
            for property in &object.properties {
                let column = ColumnName::parse(property.name.as_str()).map_err(|cause| DataContractError::InvalidColumn {
                    path: path.to_path_buf(),
                    cause,
                })?;
                if property.unique == Some(true) || property.primary_key == Some(true) {
                    unique.insert(column);
                }
            }
            unique_columns.insert(model_name, unique);
            // Relationships exist only from v3.1.0. A v3.0/v3.0.x contract carrying a
            // `relationships` array is itself invalid against the published schema for that
            // version - refused by name here rather than silently dropped, the same "refused
            // rather than silently ignored" rule this module's own header states.
            if version >= ApiVersion::V3_1_0 {
                for declaration in &object.relationships {
                    relationships.push(Self::convert_relationship_ref(path, declaration)?);
                }
            } else if !object.relationships.is_empty() {
                return Err(DataContractError::RelationshipsBeforeV3_1 {
                    path: path.to_path_buf(),
                    version: contract.api_version,
                });
            }
            // Property-level relationships (v3.1+, `RelationshipPropertyLevel`) are accepted only
            // as an opaque shape to detect their presence - this adapter has no join-endpoint
            // context for a `from` implicit at the property level, and converting one silently
            // would mint a join nobody reviewed. A non-empty one is refused by name.
            for property in &object.properties {
                if !property.relationships.is_empty() {
                    return Err(DataContractError::PropertyLevelRelationshipUnsupported {
                        path: path.to_path_buf(),
                        column: property.name.clone(),
                    });
                }
            }
        }
        Ok(ParsedContract {
            models,
            relationships,
            unique_columns,
        })
    }

    /// One [`SchemaObject`] into a domain [`Model`].
    fn convert_model(&self, path: &Path, object: &SchemaObject) -> Result<Model, DataContractError> {
        let name = ModelName::parse(object.name.as_str()).map_err(|cause| DataContractError::InvalidName {
            path: path.to_path_buf(),
            cause,
        })?;
        // The physical table name is the object's `physicalName` (e.g. `table_1_2_0`) where the
        // author wrote one, falling back to its `name`; the model name is always the object's name,
        // which is the one field the standard requires on a SchemaObject.
        let table = object.physical_name.as_deref().unwrap_or(object.name.as_str());
        let table = TableName::parse(table).map_err(|cause| DataContractError::InvalidName {
            path: path.to_path_buf(),
            cause,
        })?;

        let mut columns = Vec::with_capacity(object.properties.len());
        let mut seen = BTreeSet::new();
        let mut primary_key = Vec::new();
        for property in &object.properties {
            let column_name = ColumnName::parse(property.name.as_str()).map_err(|cause| DataContractError::InvalidColumn {
                path: path.to_path_buf(),
                cause,
            })?;
            if !seen.insert(column_name.clone()) {
                return Err(DataContractError::DuplicateColumn {
                    path: path.to_path_buf(),
                    column: column_name,
                });
            }
            // `physicalType` is the source dialect's spelling of the column type (`VARCHAR(2)`,
            // `DOUBLE`, `INT`), quoted into `Column::data_type` the way OKF quotes its `type`; when
            // the author left the dialect spelling out, the coarse `logicalType` enum still supplies
            // a type. Neither is a measure (`docs/what-a-data-contract-can-carry.md`).
            let data_type = property.physical_type.as_deref().or(property.logical_type.as_deref());
            // Column prose is the property's `description`, falling back to `businessName` - the
            // same two free-text fields the finding's `ColumnDescriptions` row names.
            let column_description = property
                .description
                .as_deref()
                .or(property.business_name.as_deref())
                .map(str::trim)
                .filter(|text| !text.is_empty());
            let column = Column::from_metadata(
                column_name.clone(),
                data_type,
                column_description,
                // `required` (not-null) maps onto `Column.nullable`; an absent key stays "no claim"
                // rather than inheriting the schema's `false` default - the finding and issue #973
                // both state that an absent `required` is not a nullability decision.
                property.required.map(|required| !required),
            )
            .map_err(|cause| DataContractError::InvalidColumnDescription {
                path: path.to_path_buf(),
                column: column_name.clone(),
                cause,
            })?;
            // Primary key evidence: a column with `primaryKey: true` is part of the key, ordered by
            // `primaryKeyPosition` (1-based). Evidence only, per `Model::with_primary_key`'s own doc.
            if property.primary_key == Some(true) {
                primary_key.push((property.primary_key_position.unwrap_or(-1), column_name));
            }
            columns.push(column);
        }
        // Sort the primary-key columns by their declared position so a composite key's order is
        // deterministic; an author who left positions out sorts by column name as a stable tiebreak.
        primary_key.sort_by_key(|(position, column)| (*position, column.clone()));

        // Descriptions is a may-provide kind here, so an undecorated object is a faithful model
        // rather than a refusal: `Description::default()` carries no prose and the declaration's
        // conditional marking is what lets `checked_against` accept its absence.
        let description = object
            .description
            .as_deref()
            .or(object.business_name.as_deref())
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let description = match description {
            Some(text) => Description::parse(text).map_err(|cause| DataContractError::InvalidDescription {
                path: path.to_path_buf(),
                cause,
            })?,
            None => Description::default(),
        };
        let model = Model::new(name, self.name.clone(), table, columns, description);
        model
            .with_primary_key(primary_key.into_iter().map(|(_, column)| column))
            .map_err(|cause| DataContractError::Inconsistent { cause })
    }

    /// One schema-level `relationships[]` declaration into the raw relationships it records.
    ///
    /// ODCS only supports the `foreignKey` type, and a `from`/`to` endpoint may be a single
    /// `table.column` reference or an array for a composite key. Only single-column references are
    /// representable in the domain's [`DomainRelationship`], so a composite key is refused here the
    /// way `sutura-catalog-rdbms` refuses one from a dictionary.
    fn convert_relationship_ref(path: &Path, declaration: &RelationshipDecl) -> Result<RawRelationship, DataContractError> {
        if let Some(found) = declaration.r#type.as_deref()
            && found != FOREIGN_KEY_TYPE
        {
            return Err(DataContractError::UnknownRelationshipType {
                path: path.to_path_buf(),
                found: found.to_owned(),
            });
        }
        let origin = SingleColumnReference::parse(&declaration.from).map_err(|cause| cause.into_error(path))?;
        let target = SingleColumnReference::parse(&declaration.to).map_err(|cause| cause.into_error(path))?;
        Ok(RawRelationship { origin, target })
    }

    /// One raw relationship into a domain relationship, under the rdbms target-uniqueness rule.
    ///
    /// A single-column `primaryKey`/`unique` target column licenses the safe `ManyToOne` direction;
    /// without that evidence, loading refuses rather than asserting a join whose fan-out is unknown -
    /// exactly `sutura-catalog-rdbms`'s own `TargetUniquenessUnknown`.
    fn convert_relationship(
        &self,
        relationship: &RawRelationship,
        unique_columns: &BTreeMap<ModelName, BTreeSet<ColumnName>>,
    ) -> Result<DomainRelationship, DataContractError> {
        let origin_model = ModelName::parse(&relationship.origin.model).map_err(|cause| DataContractError::InvalidName {
            path: self.root.clone(),
            cause,
        })?;
        let target_model = ModelName::parse(&relationship.target.model).map_err(|cause| DataContractError::InvalidName {
            path: self.root.clone(),
            cause,
        })?;
        let origin_column = ColumnName::parse(&relationship.origin.column).map_err(|cause| DataContractError::InvalidColumn {
            path: self.root.clone(),
            cause,
        })?;
        let target_column = ColumnName::parse(&relationship.target.column).map_err(|cause| DataContractError::InvalidColumn {
            path: self.root.clone(),
            cause,
        })?;
        let target_unique = unique_columns
            .get(&target_model)
            .is_some_and(|columns| columns.contains(&target_column));
        if !target_unique {
            return Err(DataContractError::TargetUniquenessUnknown {
                table: relationship.target.model.clone(),
                column: relationship.target.column.clone(),
            });
        }
        let name = Self::relationship_name(&origin_model, &origin_column, &target_model, &target_column)?;
        Ok(DomainRelationship::new(
            name,
            origin_model,
            target_model,
            JoinType::ManyToOne,
            JoinKeys::single(JoinKey::Equal {
                origin: origin_column,
                target: target_column,
            }),
        ))
    }

    /// A bounded, deterministic name for an unnamed ODCS relationship.
    ///
    /// ODCS relationships carry no name of their own (only `type`/`from`/`to`/`customProperties`),
    /// so one is derived from the endpoints - truncated under the domain's 63-character limit and
    /// disambiguated by a stable fingerprint of the whole reference, the way `sutura-catalog-rdbms`
    /// names an unnamed foreign key.
    fn relationship_name(
        origin_model: &ModelName,
        origin_column: &ColumnName,
        target_model: &ModelName,
        target_column: &ColumnName,
    ) -> Result<RelationshipName, DataContractError> {
        let fingerprint = stable_fingerprint([
            origin_model.as_str(),
            origin_column.as_str(),
            target_model.as_str(),
            target_column.as_str(),
        ]);
        let mut prefix = format!("{origin_model}_{target_model}_fk");
        prefix.truncate(prefix.len().min(MAX_RELATIONSHIP_PREFIX_LEN));
        let namespaced = format!("{prefix}__{fingerprint:016x}");
        RelationshipName::parse(&namespaced).map_err(|cause| DataContractError::RelationshipName { cause })
    }
}

/// Leaves room for `__` and a 16-character fingerprint under the 63-character name limit.
const MAX_RELATIONSHIP_PREFIX_LEN: usize = 45;

/// The one `kind` value a contract may declare.
const KIND_DATA_CONTRACT: &str = "DataContract";
/// The one relationship `type` value a contract may declare.
const FOREIGN_KEY_TYPE: &str = "foreignKey";

/// A schema-level relationship's single-column endpoint reference, as the shorthand the standard
/// writes: `table_name.column_name`.
struct SingleColumnReference {
    model: String,
    column: String,
}

impl SingleColumnReference {
    /// Parses a single-column `table.column` reference (the `ShorthandReference` shape); refuses
    /// a composite (array) endpoint, which the domain's single-key relationship cannot represent,
    /// and a `FullyQualifiedReference` (no `.`, e.g. `schema/orders/properties/customer_id`) by
    /// its own name rather than folding it into the composite-key refusal it is not.
    fn parse(endpoint: &RelationshipEndpoint) -> Result<Self, ReferenceShapeUnsupported> {
        match endpoint {
            RelationshipEndpoint::Single(reference) => reference
                .split_once('.')
                .map(|(model, column)| Self {
                    model: model.to_owned(),
                    column: column.to_owned(),
                })
                .ok_or_else(|| ReferenceShapeUnsupported::NotShorthand(reference.clone())),
            RelationshipEndpoint::Composite(_) => Err(ReferenceShapeUnsupported::Composite),
        }
    }
}

/// Why a relationship endpoint could not be read as a single-column shorthand reference.
enum ReferenceShapeUnsupported {
    /// An array endpoint - a composite key, unrepresentable in the domain's single-key relationship.
    Composite,
    /// A string endpoint with no `.` - a `FullyQualifiedReference`, a shape this adapter does not
    /// resolve.
    NotShorthand(String),
}

impl ReferenceShapeUnsupported {
    fn into_error(self, path: &Path) -> DataContractError {
        match self {
            Self::Composite => DataContractError::CompositeKeyUnrepresentable {
                path: path.to_path_buf(),
            },
            Self::NotShorthand(reference) => DataContractError::UnsupportedReference {
                path: path.to_path_buf(),
                reference,
            },
        }
    }
}

/// A raw, still-unconverted relationship's two single-column endpoints.
struct RawRelationship {
    origin: SingleColumnReference,
    target: SingleColumnReference,
}

/// The versions of the ODCS v3 line this adapter reads, and the order they gate relationships.
///
/// Only the v3 line is accepted; an unknown or `v2.x` value is refused by name rather than guessed
/// at, because whether a contract carries a `relationships` array at all is a property of its
/// version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ApiVersion {
    V3_0_0,
    V3_0_1,
    V3_0_2,
    V3_1_0,
    V3_2_0,
}

impl ApiVersion {
    fn parse(raw: &str) -> Result<Self, UnsupportedApiVersion> {
        match raw.trim() {
            "v3.0.0" => Ok(Self::V3_0_0),
            "v3.0.1" => Ok(Self::V3_0_1),
            "v3.0.2" => Ok(Self::V3_0_2),
            "v3.1.0" => Ok(Self::V3_1_0),
            "v3.2.0" => Ok(Self::V3_2_0),
            other => Err(UnsupportedApiVersion::Unknown(other.to_owned())),
        }
    }
}

/// Why a contract's `apiVersion` could not be read.
///
/// Every variant carries the word, so a message names the value a reader wrote and the version
/// line this adapter reads.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UnsupportedApiVersion {
    #[error(
        "`{0}` does not name a data-contract version this adapter reads (only the v3.0.0, v3.0.1, v3.0.2, v3.1.0 and v3.2.0 tags of the Open Data Contract Standard)"
    )]
    Unknown(String),
}

/// Maps a [`sutura_bounded_read::WalkError`] into this adapter's own refusal variants, carrying the
/// same message each variant rendered before this crate existed.
fn map_walk_error(cause: sutura_bounded_read::WalkError) -> DataContractError {
    match cause {
        sutura_bounded_read::WalkError::NotADirectory { path } => DataContractError::NotADirectory { path },
        sutura_bounded_read::WalkError::Io { path, cause } => DataContractError::Io { path, cause },
        sutura_bounded_read::WalkError::TooManyDocuments { path, found, limit } => {
            DataContractError::TooManyDocuments { path, found, limit }
        }
        sutura_bounded_read::WalkError::Empty { path } => DataContractError::Empty { path },
    }
}

/// Maps a [`sutura_bounded_read::ReadError`] into this adapter's own refusal variants. `TooLarge`
/// names the document that crossed the bound with `document` and the catalog root with `root`, the
/// same roles the message text gives them.
fn map_read_error(cause: sutura_bounded_read::ReadError) -> DataContractError {
    match cause {
        sutura_bounded_read::ReadError::Open { path, cause } => DataContractError::Open { path, cause },
        sutura_bounded_read::ReadError::NotARegularFile { path } => DataContractError::NotARegularFile { path },
        sutura_bounded_read::ReadError::TooLarge {
            root,
            document,
            found,
            limit,
        } => DataContractError::TooLarge {
            path: root,
            document,
            found,
            limit,
        },
        sutura_bounded_read::ReadError::Io { path, cause } => DataContractError::Io { path, cause },
    }
}

/// Why a directory could not be read as a data-contract catalog.
///
/// Every variant carries the path, because a catalog is many files and a message that names no file
/// sends a reader to read all of them.
#[derive(Debug, thiserror::Error)]
pub enum DataContractError {
    #[error("the catalog root {path} is not a directory")]
    NotADirectory { path: PathBuf },
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    #[error("could not open {path}: {cause}")]
    Open {
        path: PathBuf,
        #[source]
        cause: rustix::io::Errno,
    },
    #[error("the document {path} is not an Open Data Contract Standard v3 document")]
    Malformed {
        path: PathBuf,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the document {path} declares an unsupported data-contract version: {cause}")]
    UnsupportedVersion {
        path: PathBuf,
        #[source]
        cause: UnsupportedApiVersion,
    },
    #[error("the document {path} declares an unknown kind: `{found}` (only `DataContract` is read)")]
    UnknownKind { path: PathBuf, found: String },
    #[error("the relationship in {path} has an unsupported type: `{found}` (only `foreignKey` is read)")]
    UnknownRelationshipType { path: PathBuf, found: String },
    #[error("a relationship in {path} is a composite (multi-column) key, which a single-column join cannot represent")]
    CompositeKeyUnrepresentable { path: PathBuf },
    #[error(
        "a relationship in {path} references `{reference}`, which is not the `table.column` shorthand this adapter resolves (a fully-qualified reference is unsupported)"
    )]
    UnsupportedReference { path: PathBuf, reference: String },
    #[error(
        "the document {path} declares a `relationships` array under apiVersion `{version}`, which is invalid before v3.1.0 - refused rather than silently dropped"
    )]
    RelationshipsBeforeV3_1 { path: PathBuf, version: String },
    #[error(
        "the column {column} of {path} declares a property-level relationship, which this adapter has no join-endpoint context to convert and refuses rather than silently drops"
    )]
    PropertyLevelRelationshipUnsupported { path: PathBuf, column: String },
    #[error("a model of {path} is not a name")]
    InvalidName {
        path: PathBuf,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("a column of {path} is not a name")]
    InvalidColumn {
        path: PathBuf,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the document {path} lists the column {column} twice")]
    DuplicateColumn { path: PathBuf, column: ColumnName },
    #[error("the description of {path} is not usable")]
    InvalidDescription {
        path: PathBuf,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the description of column {column} in {path} is not usable")]
    InvalidColumnDescription {
        path: PathBuf,
        column: ColumnName,
        #[source]
        cause: InvalidDescription,
    },
    #[error(
        "a relationship to column {column} of {table} has no single-column target-uniqueness evidence - refused rather than asserting a join whose fan-out is unknown"
    )]
    TargetUniquenessUnknown { table: String, column: String },
    #[error("a relationship name keyed off `{cause}` is not a name")]
    RelationshipName {
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    #[error("the catalog's knowledge does not hold together")]
    UncheckableKnowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    #[error("the catalog at {path} holds no documents")]
    Empty { path: PathBuf },
    #[error("the catalog at {path} holds more than {limit} documents ({found} found before the walk stopped)")]
    TooManyDocuments { path: PathBuf, found: usize, limit: usize },
    #[error("the catalog at {path} holds more than {limit} bytes of documents (the read stopped at {document}, {found} found)")]
    TooLarge {
        path: PathBuf,
        document: PathBuf,
        found: u64,
        limit: u64,
    },
    #[error("the document {path} is not a regular file - refused rather than read as one")]
    NotARegularFile { path: PathBuf },
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

/// A deterministic, non-cryptographic fingerprint for a relationship's canonical identity.
///
/// The `0xff` separator cannot occur in UTF-8, so adjacent fields remain unambiguous. A collision
/// remains a duplicate relationship and is refused by [`Definitions::assemble`]; it can never
/// replace an existing one. Identical to `sutura-catalog-rdbms`'s own, because a relationship's
/// identity is the endpoints, not the adapter that read them.
fn stable_fingerprint<'a>(parts: impl IntoIterator<Item = &'a str>) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut fingerprint = OFFSET_BASIS;
    for part in parts {
        for byte in part.bytes().chain(core::iter::once(0xff)) {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(PRIME);
        }
    }
    fingerprint
}

impl SemanticCatalog for DataContractCatalog {
    type Error = DataContractError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    fn capabilities() -> MetadataCapabilities {
        // Exactly what this adapter produces and nothing more: the physical model, unconditionally
        // (an empty directory is refused, so every bundle carries at least one). Descriptions,
        // ColumnTypes and ColumnDescriptions are declared-and-empty may-provide (a `SchemaObject`
        // may carry neither prose nor type on any column), and Relationships are may-provide on
        // v3.1+ under the single-column target-uniqueness rule. Everything else - the metric, the
        // required filter, the grain, the allowlist, the anchor, the cardinality and the referent-
        // bearing knowledge - is a deliberate, declared absence.
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure]).and_may_provide([
                DefinitionKind::Descriptions,
                DefinitionKind::ColumnTypes,
                DefinitionKind::ColumnDescriptions,
                DefinitionKind::Relationships,
            ]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let (definitions, knowledge) = self.read_all()?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| DataContractError::Digest { cause })
    }
}
