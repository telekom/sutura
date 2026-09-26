#![forbid(unsafe_code)]
//! A [`SemanticCatalog`] over a directory of OKF Frictionless Table Schema descriptors.
//!
//! One YAML file per physical table, each a [Table Schema](https://specs.frictionlessdata.io/table-schema/)
//! descriptor: the `fields[].name` column set is the model's columns, the descriptor's `title` or
//! `description` is the model's description, and the file's own stem is the table (and model) name -
//! because the Table Schema vocabulary declares no table name of its own, and a file name is the one
//! thing an author wrote whose stem is deterministic and unique in a directory.
//!
//! `docs/what-okf-can-carry.md` is the finding that decides this adapter's whole shape. The OKF
//! vocabulary (Table Schema version 1) is **strictly narrower than Wren** - it carries a table's
//! column set and free-text descriptions, and nothing of a metric layer. A `foreignKey` declares no
//! cardinality, so it can licence no relationship (a relationship carries a required `JoinType`,
//! and Table Schema gives no way to choose one) - which is why this adapter **declares**
//! `Relationships` out rather than supplying an unlicensed one. What it provides is exactly
//! [`DefinitionKind::Structure`] and [`DefinitionKind::Descriptions`]; everything else - the metric,
//! the required filter, the grain, the allowlist, the anchor, and the referent-bearing knowledge - is
//! a deliberate, declared absence. Being a `declaring` source measured against that declaration is
//! the whole point of the [`CatalogKind::Declaring`] class.
//!
//! Three properties of the load worth stating, because each is a mechanism rather than a wish:
//!
//! **The walk is sorted and refused-when-empty.** `read_dir` returns entries in filesystem order, and
//! a digest that moves between two runs over unchanged files is a digest nobody can act on, so the
//! walk collects into a `BTreeSet` and yields sorted. An empty or missing directory is an error
//! ([`OkfCatalogError::Empty`], [`OkfCatalogError::NotADirectory`]) rather than a silently-empty
//! catalog, for the same reason `sutura-catalog-local` refuses one: a mistyped root that happens to
//! exist would otherwise load a catalog with no models and its declaration would then claim
//! `Structure` that `produced` does not observe.
//!
//! **A descriptor without a self-report must fail, not default.** Each model is required to carry a
//! non-empty `title` or `description`, because the adapter declares [`DefinitionKind::Descriptions`]
//! and `MetadataCapabilities::produced` observes that kind only through a non-empty description - so
//! a document that supplied none would make the adapter's own declaration unfaithful. The requirement
//! is a refusal ([`OkfCatalogError::MissingDescription`]), not a default, which is this repository's
//! rule that a default is indistinguishable from a decision.
//!
//! **`deny_unknown_fields` at every depth, and validated names.** A Table Schema descriptor that
//! carries a key this adapter does not read is refused rather than silently ignored, and each column,
//! table and model name is parsed through the domain's validated newtypes - so a document cannot
//! write past a `parse`. A duplicate column name (two `fields` entries with one `name`) is an error,
//! because `BTreeSet` deduplicates and a silently-shrinking column set is a digest that lies.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Column, Definitions, Description, InconsistentDefinitions, InvalidDescription, Model};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The extensions a Table Schema catalog document may carry.
const DOCUMENT_EXTENSIONS: &[&str] = &["yaml", "yml"];

/// The two halves of a bundle's content, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// A catalog read from a directory of Table Schema descriptors.
///
/// Like the local catalog's `LocalCatalog`, it carries a declared NAME, a root and a version: the
/// name is the key the contribution manifest records this contributor under, the root is the
/// directory of descriptors, and the version identifies which snapshot of that directory this is.
#[derive(Debug, Clone)]
pub struct OkfCatalog {
    name: SourceName,
    root: PathBuf,
    version: DefinitionVersion,
}

impl OkfCatalog {
    /// Points a catalog at a directory of Table Schema descriptors.
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

    /// Every descriptor under the root, in sorted order.
    ///
    /// Sorted through a `BTreeSet`, so the walk is a function of the tree rather than of the
    /// filesystem. Files that are not a `yaml`/`yml` document are skipped rather than refused, so a
    /// stray `README.md` does not break a load. A regular file with the right extension and the wrong
    /// content still fails loudly at deserialisation.
    fn documents(&self) -> Result<Vec<PathBuf>, OkfCatalogError> {
        sutura_bounded_read::walk(&self.root, DOCUMENT_EXTENSIONS, sutura_bounded_read::MAX_CATALOG_DOCUMENTS)
            .map_err(map_walk_error)
    }

    /// Turns one Table Schema descriptor file on disk into a domain [`Model`].
    fn descriptor_to_model(&self, path: &Path, text: &str) -> Result<Model, OkfCatalogError> {
        let descriptor: TableSchema = serde_norway::from_str(text).map_err(|cause| OkfCatalogError::Malformed {
            path: path.to_path_buf(),
            cause,
        })?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| OkfCatalogError::Unnamed {
                path: path.to_path_buf(),
            })?;
        let table = TableName::parse(stem).map_err(|cause| OkfCatalogError::InvalidName {
            path: path.to_path_buf(),
            cause,
        })?;
        let name = ModelName::parse(stem).map_err(|cause| OkfCatalogError::InvalidName {
            path: path.to_path_buf(),
            cause,
        })?;
        // Columns with a duplicate `name` are refused, not silently deduplicated: a column map would
        // swallow the second one, and a column set that shrinks between reads is a digest that lies.
        let mut columns = Vec::with_capacity(descriptor.fields.len());
        let mut seen = BTreeSet::new();
        for field in descriptor.fields {
            let column_name = ColumnName::parse(field.name.as_str()).map_err(|cause| OkfCatalogError::InvalidColumn {
                path: path.to_path_buf(),
                cause,
            })?;
            if !seen.insert(column_name.clone()) {
                return Err(OkfCatalogError::DuplicateColumn {
                    path: path.to_path_buf(),
                    column: column_name,
                });
            }
            // `type` is the field's logical data type (`string`, `integer`, `number`, …) - descriptive
            // text quoted into `Column::data_type`, never a measure. A type this adapter cannot
            // represent is dropped rather than refused - `Column::from_metadata`'s own doc.
            //
            // `description`, falling back to `title` the same way the model's own does - a field may
            // carry either or neither, and this adapter has no third source of column prose.
            let field_description = field
                .description
                .or(field.title)
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty());
            let column = Column::from_metadata(
                column_name.clone(),
                field.r#type.as_deref(),
                field_description.as_deref(),
                None,
            )
            .map_err(|cause| OkfCatalogError::InvalidColumnDescription {
                path: path.to_path_buf(),
                column: column_name.clone(),
                cause,
            })?;
            columns.push(column);
        }
        // A model without a title or description would make the declared `Descriptions` unproduced,
        // so it is refused rather than defaulted.
        let description = descriptor
            .description
            .or(descriptor.title)
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| OkfCatalogError::MissingDescription {
                path: path.to_path_buf(),
            })?;
        let description = Description::parse(description).map_err(|cause| OkfCatalogError::InvalidDescription {
            path: path.to_path_buf(),
            cause,
        })?;
        let model = Model::new(name, self.name.clone(), table, columns, description);
        // `primaryKey` is a Table Schema field or field list; evidence only, per `Model::with_primary_key`'s
        // own doc - it licenses no join and nothing here re-derives a cardinality from it.
        let primary_key =
            primary_key_columns(descriptor.primary_key.as_ref()).map_err(|cause| OkfCatalogError::InvalidPrimaryKey {
                path: path.to_path_buf(),
                cause,
            })?;
        model
            .with_primary_key(primary_key)
            .map_err(|cause| OkfCatalogError::Inconsistent { cause })
    }
    /// Reads every descriptor and assembles the bundle.
    ///
    /// The byte bound is enforced on the READ itself, not on a `stat` taken separately from it -
    /// each file is opened ONCE and everything is read through that same handle, so a document
    /// that grows, or is swapped, between the walk and the read cannot slip past the bound. The
    /// refusal paths and their exact reach are [`sutura_bounded_read::read_document`]'s contract.
    fn read_all(&self) -> Result<Content, OkfCatalogError> {
        let mut models = Vec::new();
        let mut total_bytes: u64 = 0;
        for path in self.documents()? {
            // ONE open per descriptor, still, and everything about the file decided from the handle
            // that is actually read - the read, the regular-file check, the byte budget and the
            // post-read recheck all live in `sutura_bounded_read::read_document`, on the ONE handle
            // it opened. The refusal half, the `O_NOFOLLOW` / `O_NONBLOCK` / `O_CLOEXEC` flags, is
            // that crate's open.
            let text = sutura_bounded_read::read_document(&self.root, &path, total_bytes).map_err(map_read_error)?;
            total_bytes += text.len() as u64;
            models.push(self.descriptor_to_model(&path, &text)?);
        }
        let definitions =
            Definitions::assemble(models, Vec::new(), Vec::new()).map_err(|cause| OkfCatalogError::Inconsistent { cause })?;
        // This adapter supplies no referent-bearing knowledge. `KnowledgeInput::none()` assembles an
        // empty knowledge that declares nothing - which is what `capabilities()` says it declares.
        let knowledge = Knowledge::assemble(&definitions, KnowledgeInput::none())
            .map_err(|cause| OkfCatalogError::UncheckableKnowledge { cause })?;
        Ok((definitions, knowledge))
    }
}

/// Maps a [`sutura_bounded_read::WalkError`] into this adapter's own refusal variants, carrying the
/// same message each variant rendered before this crate existed.
fn map_walk_error(cause: sutura_bounded_read::WalkError) -> OkfCatalogError {
    match cause {
        sutura_bounded_read::WalkError::NotADirectory { path } => OkfCatalogError::NotADirectory { path },
        sutura_bounded_read::WalkError::Io { path, cause } => OkfCatalogError::Io { path, cause },
        sutura_bounded_read::WalkError::TooManyDocuments { path, found, limit } => {
            OkfCatalogError::TooManyDocuments { path, found, limit }
        }
        sutura_bounded_read::WalkError::Empty { path } => OkfCatalogError::Empty { path },
    }
}

/// Maps a [`sutura_bounded_read::ReadError`] into this adapter's own refusal variants. `TooLarge`
/// names the document that crossed the bound with `document` and the catalog root with `root`, the
/// same roles the message text gives them.
fn map_read_error(cause: sutura_bounded_read::ReadError) -> OkfCatalogError {
    match cause {
        sutura_bounded_read::ReadError::Open { path, cause } => OkfCatalogError::Open { path, cause },
        sutura_bounded_read::ReadError::NotARegularFile { path } => OkfCatalogError::NotARegularFile { path },
        sutura_bounded_read::ReadError::TooLarge {
            root,
            document,
            found,
            limit,
        } => OkfCatalogError::TooLarge {
            path: root,
            document,
            found,
            limit,
        },
        sutura_bounded_read::ReadError::Io { path, cause } => OkfCatalogError::Io { path, cause },
    }
}

/// Why a directory could not be read as an OKF catalog.
///
/// Every variant carries the path, because a catalog is many files and a message that names no file
/// sends a reader to read all of them.
#[derive(Debug, thiserror::Error)]
pub enum OkfCatalogError {
    #[error("the catalog root {path} is not a directory")]
    NotADirectory { path: PathBuf },
    #[error("could not read {path}")]
    Io {
        path: PathBuf,
        #[source]
        cause: std::io::Error,
    },
    /// An open or `fstat` of a descriptor failed in a way the OS described but [`Self::Io`]'s
    /// wording does not: a swapped symlink refuses with `ELOOP` and a swapped FIFO with
    /// `ENXIO`, and neither is "could not read".
    #[error("could not open {path}: {cause}")]
    Open {
        path: PathBuf,
        #[source]
        cause: rustix::io::Errno,
    },
    #[error("the document {path} is not a Table Schema descriptor")]
    Malformed {
        path: PathBuf,
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the document {path} has no file name to name its table")]
    Unnamed { path: PathBuf },
    #[error("the table name of {path} is not a name")]
    InvalidName {
        path: PathBuf,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    #[error("a column of {path} is not a name")]
    InvalidColumn {
        path: PathBuf,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    #[error("the descriptor {path} lists the column {column} twice")]
    DuplicateColumn { path: PathBuf, column: ColumnName },
    #[error("the descriptor {path} carries neither a title nor a description to describe the model")]
    MissingDescription { path: PathBuf },
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
    #[error("the primaryKey of {path} is not a column name or a list of them")]
    InvalidPrimaryKey {
        path: PathBuf,
        #[source]
        cause: InvalidPrimaryKeyShape,
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
    /// The read of this document would push the running total over `MAX_CATALOG_BYTES`.
    ///
    /// `path` is the catalog root, matching `TooManyDocuments` and `Empty` above - the rendered
    /// text names "the catalog", so the path in it has to be the catalog's, not one file's.
    /// `document` is the one whose bytes pushed the running total past `limit`. `found` is that
    /// running total. The bound is enforced on the read itself ([`OkfCatalog::read_all`] and
    /// [`sutura_bounded_read::read_document`]): the refusing total comes from the handle's own `metadata()` before the
    /// read, or from what the capped read actually delivered if a file grew in between.
    #[error("the catalog at {path} holds more than {limit} bytes of documents (the read stopped at {document}, {found} found)")]
    TooLarge {
        path: PathBuf,
        document: PathBuf,
        found: u64,
        limit: u64,
    },
    /// The document the walk named is not a regular file when it comes to be read.
    ///
    /// The walk skips links and non-files; this refuses a document that became a non-regular file
    /// after the walk. It is the handle actually about to be read, refused before any bytes are:
    /// a device node opens under `O_NONBLOCK` without blocking and is refused here rather than
    /// read. A symlink swapped in after the walk - to a regular file or anything else - never
    /// reaches this check at all: `O_NOFOLLOW` on the open refuses it with `ELOOP` first. A
    /// swapped FIFO is refused the same way a device is - `O_NONBLOCK` makes its open return
    /// rather than block, and this check then refuses the non-regular handle.
    #[error("the document {path} is not a regular file - refused rather than read as one")]
    NotARegularFile { path: PathBuf },
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

/// The serde shape of a Table Schema descriptor, as the OKF vocabulary defines it.
///
/// `deny_unknown_fields` at this depth and within each field is the fidelity that a Table Schema
/// document which carries a key the adapter does not read still fails the load rather than vanishing.
/// `primaryKey` is now evidence on the model (`Model::with_primary_key`); `foreignKeys`,
/// `constraints`, `format` and `rdfType` remain accepted and unsurfaced - structural or value-level
/// facts that licence neither a measure nor a join (`docs/what-okf-can-carry.md`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TableSchema {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[expect(
        dead_code,
        reason = "part of the accepted OKF vocabulary, read only to accept and refuse faithfully; the adapter deliberately does not surface a missing-value declaration as a definition"
    )]
    #[serde(default)]
    missing_values: Vec<String>,
    /// A field or field list uniquely identifying each row - primary-key evidence only, per
    /// `Model::with_primary_key`'s own doc. Not a `foreignKeys`-style structural fact: it says
    /// nothing about a join and licenses none.
    #[serde(default)]
    primary_key: Option<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "a foreign key declares no cardinality and so licences no relationship (`docs/what-okf-can-carry.md`)"
    )]
    #[serde(default)]
    foreign_keys: Vec<serde_norway::Value>,
    fields: Vec<Field>,
}

/// One `fields` entry of a Table Schema descriptor.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Field {
    /// Human-readable, and the fallback for `description` - same precedence as the model's own.
    #[serde(default)]
    title: Option<String>,
    description: Option<String>,
    #[expect(dead_code, reason = "a sample value, not prose or structure")]
    #[serde(default)]
    example: Option<String>,
    name: String,
    /// The logical data type (`string`, `integer`, `number`, …) - descriptive text quoted into
    /// `Column::data_type`, never a measure (`docs/what-okf-can-carry.md`).
    #[serde(rename = "type", default)]
    r#type: Option<String>,
    #[expect(dead_code, reason = "a format is a physical representation hint, not a semantic predicate")]
    #[serde(default)]
    format: Option<String>,
    #[expect(
        dead_code,
        reason = "constraints are value validation over the data file, not definitional filters"
    )]
    #[serde(default)]
    constraints: Option<serde_norway::Value>,
    #[expect(dead_code, reason = "an rdf type annotates a column, it does not define a measure")]
    #[serde(default)]
    rdf_type: Option<String>,
}

/// Why a `primaryKey` value could not be read as a column name or a list of them.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidPrimaryKeyShape {
    /// Neither a YAML string nor a sequence of strings - the two shapes the Table Schema
    /// specification allows.
    #[error("primaryKey must be a string or a list of strings")]
    NotAStringOrList,
    #[error("a primaryKey entry is not a usable column name: {cause}")]
    Column {
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
}

/// Reads a Table Schema `primaryKey` - absent, a bare string, or a list of strings - into the
/// column names it names.
fn primary_key_columns(raw: Option<&serde_norway::Value>) -> Result<Vec<ColumnName>, InvalidPrimaryKeyShape> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let names: Vec<&str> = match raw {
        serde_norway::Value::String(name) => vec![name.as_str()],
        serde_norway::Value::Sequence(entries) => entries
            .iter()
            .map(|entry| entry.as_str().ok_or(InvalidPrimaryKeyShape::NotAStringOrList))
            .collect::<Result<_, _>>()?,
        _ => return Err(InvalidPrimaryKeyShape::NotAStringOrList),
    };
    names
        .into_iter()
        .map(|name| ColumnName::parse(name).map_err(|cause| InvalidPrimaryKeyShape::Column { cause }))
        .collect()
}

impl SemanticCatalog for OkfCatalog {
    type Error = OkfCatalogError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    fn capabilities() -> MetadataCapabilities {
        // Exactly what this adapter produces and nothing more: the physical model (with, per model,
        // a non-empty description) and the descriptions. Everything else is a deliberate, declared
        // absence - the metric, the required filter, the grain, the allowlist, the anchor, the
        // relationship, and the referent-bearing knowledge. `ColumnTypes` and `ColumnDescriptions`
        // are declared-and-empty may-provide: a Table Schema field's `type` and `description` are
        // both `#[serde(default)]`, so whether a given descriptor carries either is the author's
        // choice per field rather than a structural guarantee this adapter can vouch for.
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions])
                .and_may_provide([DefinitionKind::ColumnTypes, DefinitionKind::ColumnDescriptions]),
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
        .map_err(|cause| OkfCatalogError::Digest { cause })
    }
}
