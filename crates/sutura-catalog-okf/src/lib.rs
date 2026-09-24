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
use std::io::Read as _;
use std::path::{Path, PathBuf};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, InconsistentDefinitions, InvalidDescription, Model};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The extensions a Table Schema catalog document may carry.
const DOCUMENT_EXTENSIONS: &[&str] = &["yaml", "yml"];

/// The most documents a catalog root may hold - a startup bound, walk refused as soon as it crosses.
const MAX_CATALOG_DOCUMENTS: usize = 1_000;
/// The most bytes a catalog root's documents may sum to - a startup bound, enforced on the READ
/// itself (each file is opened once, its `metadata()` on that handle checked for size and, before
/// the read, for being a regular file, and the read passes through `Read::take(remaining + 1)`), so
/// a document that crosses the aggregate bound is refused rather than allocated even if it grows or
/// is swapped after the walk. Mirrors `sutura-catalog-local`'s `MAX_CATALOG_BYTES` for the same
/// reason that crate has one: a served catalog directory is operator-mounted, and an unbounded
/// aggregate read is a startup cost nobody asked to pay.
const MAX_CATALOG_BYTES: u64 = 16 * 1024 * 1024;

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
        if !self.root.is_dir() {
            return Err(OkfCatalogError::NotADirectory { path: self.root.clone() });
        }
        let mut found = BTreeSet::new();
        let mut pending = vec![self.root.clone()];
        while let Some(directory) = pending.pop() {
            let entries = std::fs::read_dir(&directory).map_err(|cause| OkfCatalogError::Io {
                path: directory.clone(),
                cause,
            })?;
            for entry in entries {
                let entry = entry.map_err(|cause| OkfCatalogError::Io {
                    path: directory.clone(),
                    cause,
                })?;
                let path = entry.path();
                let kind = entry.file_type().map_err(|cause| OkfCatalogError::Io {
                    path: path.clone(),
                    cause,
                })?;
                #[expect(
                    clippy::filetype_is_file,
                    reason = "a catalog document is a regular file - a link, a socket or a device node is not, and skipping those is the point"
                )]
                let is_document = kind.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| DOCUMENT_EXTENSIONS.contains(&ext));
                if kind.is_dir() {
                    pending.push(path);
                } else if is_document {
                    found.insert(path);
                    if found.len() > MAX_CATALOG_DOCUMENTS {
                        return Err(OkfCatalogError::TooManyDocuments {
                            path: self.root.clone(),
                            found: found.len(),
                            limit: MAX_CATALOG_DOCUMENTS,
                        });
                    }
                }
            }
        }
        if found.is_empty() {
            return Err(OkfCatalogError::Empty { path: self.root.clone() });
        }
        Ok(found.into_iter().collect())
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
        // Columns with a duplicate `name` are refused, not silently deduplicated: `BTreeSet` would
        // swallow the second one, and a column set that shrinks between reads is a digest that lies.
        let mut columns = Vec::with_capacity(descriptor.fields.len());
        let mut seen = BTreeSet::new();
        for field in descriptor.fields {
            let column = ColumnName::parse(field.name.as_str()).map_err(|cause| OkfCatalogError::InvalidColumn {
                path: path.to_path_buf(),
                cause,
            })?;
            if !seen.insert(column.clone()) {
                return Err(OkfCatalogError::DuplicateColumn {
                    path: path.to_path_buf(),
                    column,
                });
            }
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
        Ok(Model::new(
            name,
            self.name.clone(),
            table,
            columns.into_iter().collect(),
            description,
        ))
    }
    /// Reads every descriptor and assembles the bundle.
    ///
    /// The byte bound is enforced on the READ itself, not on a `stat` taken separately from it, so
    /// a document that grows, or is swapped for a larger or non-regular file, between the walk and
    /// the read cannot slip past the bound. Each file is opened ONCE and everything is read through
    /// that same handle: the handle's own `metadata()` (an `fstat`, so it is the file actually about
    /// to be read) is checked for being a regular file - a symlink, device or fifo is refused rather
    /// than followed - and its `len()` is checked against the remaining budget; then the read is
    /// capped with `Read::take(remaining + 1)` and a file that still delivers more than `remaining`
    /// bytes is refused as [`OkfCatalogError::TooLarge`]. The `take` is what makes the refusal hold
    /// against a file that lies about its size or grows mid-read; the `metadata` len check is the
    /// fast path that keeps a legitimately-oversized file from being read at all.
    fn read_all(&self) -> Result<Content, OkfCatalogError> {
        let mut models = Vec::new();
        let mut total_bytes: u64 = 0;
        for path in self.documents()? {
            let mut file = std::fs::File::open(&path).map_err(|cause| OkfCatalogError::Io {
                path: path.clone(),
                cause,
            })?;
            // `fstat` on the handle we are about to read from: this is the file actually being read,
            // not a separately-named path. A non-regular file is refused here rather than followed -
            // the walk already skips links, but a document swapped for a link to `/dev/zero` after
            // the walk would otherwise open a zero-length device and allocate forever.
            let metadata = file.metadata().map_err(|cause| OkfCatalogError::Io {
                path: path.clone(),
                cause,
            })?;
            if !metadata.is_file() {
                return Err(OkfCatalogError::NotARegularFile { path });
            }
            let remaining = MAX_CATALOG_BYTES - total_bytes.min(MAX_CATALOG_BYTES);
            if metadata.len() > remaining {
                let found = total_bytes.saturating_add(metadata.len());
                return Err(OkfCatalogError::TooLarge {
                    path: self.root.clone(),
                    document: path,
                    found,
                    limit: MAX_CATALOG_BYTES,
                });
            }
            let mut text = String::new();
            file.by_ref()
                .take(remaining.saturating_add(1))
                .read_to_string(&mut text)
                .map_err(|cause| OkfCatalogError::Io {
                    path: path.clone(),
                    cause,
                })?;
            // `take` caps the read; if the file actually held more than `remaining` bytes, what
            // arrived still is - so re-check the length of what was READ, which closes the case of a
            // file that grew between the `fstat` and the read.
            if text.len() as u64 > remaining {
                let found = total_bytes.saturating_add(text.len() as u64);
                return Err(OkfCatalogError::TooLarge {
                    path: self.root.clone(),
                    document: path,
                    found,
                    limit: MAX_CATALOG_BYTES,
                });
            }
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
    /// running total. The bound is enforced on the read itself ([`OkfCatalog::read_all`]): the
    /// refusing total comes from the handle's own `metadata()` before the read, or from what the
    /// capped read actually delivered if a file grew in between.
    #[error("the catalog at {path} holds more than {limit} bytes of documents (the read stopped at {document}, {found} found)")]
    TooLarge {
        path: PathBuf,
        document: PathBuf,
        found: u64,
        limit: u64,
    },
    /// The document the walk named is not a regular file when it comes to be read.
    ///
    /// The walk refuses nothing on kind besides skipping links, devices and other non-files, but
    /// this is the handle actually about to be read: a document swapped for a symlink to a device
    /// after the walk would otherwise be followed - a zero-length device reporting `len 0` and
    /// reading forever. Refusing it here closes that path with no second, path-based open.
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
/// The properties the adapter does not use - `primaryKey`, `foreignKeys`, `constraints`, the field
/// `type`/`format` - are accepted as part of the published vocabulary and deliberately not surfaced:
/// they are value-level or structural facts that licence neither a measure nor a join in this model
/// (`docs/what-okf-can-carry.md`).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TableSchema {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[expect(
        dead_code,
        reason = "part of the accepted OKF vocabulary, read only to accept and refuse faithfully; the adapter deliberately does not surface missing-value or key declarations as definitions"
    )]
    #[serde(default)]
    missing_values: Vec<String>,
    #[expect(
        dead_code,
        reason = "see `missing_values` - a key declaration is not a measure or a join here"
    )]
    #[serde(default)]
    primary_key: Option<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "see `missing_values` - a foreign key declares no cardinality and so licences no relationship (`docs/what-okf-can-carry.md`)"
    )]
    #[serde(default)]
    foreign_keys: Vec<serde_norway::Value>,
    fields: Vec<Field>,
}

/// One `fields` entry of a Table Schema descriptor.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Field {
    name: String,
    #[expect(
        dead_code,
        reason = "a field's own prose is accepted but not surfaced - the model's one description comes from the descriptor's own `title`/`description`"
    )]
    #[serde(default)]
    title: Option<String>,
    #[expect(dead_code, reason = "see `title`")]
    #[serde(default)]
    description: Option<String>,
    #[expect(dead_code, reason = "see `title`")]
    #[serde(default)]
    example: Option<String>,
    #[expect(
        dead_code,
        reason = "a column type is a data type, not a measure (`docs/what-okf-can-carry.md`)"
    )]
    #[serde(rename = "type", default)]
    r#type: Option<String>,
    #[expect(
        dead_code,
        reason = "see `type` - a format is a physical representation hint, not a semantic predicate"
    )]
    #[serde(default)]
    format: Option<String>,
    #[expect(
        dead_code,
        reason = "see `type` - constraints are value validation over the data file, not definitional filters"
    )]
    #[serde(default)]
    constraints: Option<serde_norway::Value>,
    #[expect(
        dead_code,
        reason = "see `type` - an rdf type annotates a column, it does not define a measure"
    )]
    #[serde(default)]
    rdf_type: Option<String>,
}

impl SemanticCatalog for OkfCatalog {
    type Error = OkfCatalogError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    fn capabilities() -> MetadataCapabilities {
        // Exactly what this adapter produces and nothing more: the physical model (with, per model,
        // a non-empty description) and the descriptions. Everything else is a deliberate, declared
        // absence - the metric, the required filter, the grain, the allowlist, the anchor, the
        // relationship, and the referent-bearing knowledge.
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]),
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
