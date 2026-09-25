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

/// The most documents a catalog root may hold - a startup bound, walk refused as soon as it crosses.
const MAX_CATALOG_DOCUMENTS: usize = 1_000;
/// The most bytes a catalog root's documents may sum to - a startup bound, enforced on the READ
/// itself (each file is opened once, `fstat`'d on that handle for being a regular file and for its
/// size, and the read is done in chunks capped at the bytes the aggregate had left), so
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
    /// refusal paths and their exact reach are [`read_document`]'s contract.
    fn read_all(&self) -> Result<Content, OkfCatalogError> {
        let mut models = Vec::new();
        let mut total_bytes: u64 = 0;
        for path in self.documents()? {
            // ONE open per descriptor, still, and now one that does not follow a symlink into it
            // and does not block on what the walk did not see: `O_NOFOLLOW` makes a document
            // swapped for a symlink a refusal at open, `O_NONBLOCK` makes a swapped FIFO `ENXIO`
            // rather than a boot that never returns. What the opened handle IS is still checked
            // on the handle, below.
            //
            // "Opened once" itself is held by review, not by a test: a hand mutation that appends
            // a second, unguarded `std::fs::read_to_string(&path)` right after this block is not
            // killed by a swap-timing test - the window between the two back-to-back opens is
            // sub-microsecond, well under what even the multi-millisecond swap tests below need to
            // land reliably (measured across 3 separate `just test` runs against that mutation).
            let flags = rustix::fs::OFlags::RDONLY
                .union(rustix::fs::OFlags::NOFOLLOW)
                .union(rustix::fs::OFlags::NONBLOCK)
                .union(rustix::fs::OFlags::CLOEXEC);
            let fd = rustix::fs::open(&path, flags, rustix::fs::Mode::empty()).map_err(|cause| OkfCatalogError::Open {
                path: path.clone(),
                cause,
            })?;
            let ReadDocument { text, consumed } = read_document(&fd, &path, &self.root, total_bytes)?;
            total_bytes += consumed;
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
/// The text of one descriptor, and the number of bytes it consumed from the aggregate budget.
///
/// A plain struct rather than a `(String, u64)` return: the two are always read together, and the
/// named field keeps the "one document" boundary legible in [`OkfCatalog::read_all`].
#[derive(Debug)]
struct ReadDocument {
    text: String,
    consumed: u64,
}

/// Reads one descriptor's bytes against the aggregate byte bound.
///
/// Bounded on the OPENED HANDLE: the descriptor is `fstat`'d for being a regular file, its
/// size against the remaining budget is the fast path that refuses a legitimately-oversized
/// file before anything is read, and the read itself is done in chunks capped at the bytes
/// the aggregate had left - so a file that lies about its size, or grows while it is being
/// read, is refused as [`OkfCatalogError::TooLarge`] rather than allocated.
///
/// The OPEN is the other half, and it is where a document swapped after the walk is caught:
/// `O_NOFOLLOW` makes a final-component symlink a refusal at open rather than a read of
/// whatever it pointed at, and `O_NONBLOCK` makes a swapped FIFO `ENXIO` rather than an open
/// that blocks the boot before the `fstat` runs. What these do NOT refuse is a document
/// swapped for a regular file at a different path - the walk named a path, and the handle
/// opened is of whatever that path names now; the bound still holds, on the handle. `root`
/// is the catalog root, carried only so the `TooLarge` text can name the catalog as the
/// other variants do.
fn read_document(fd: impl rustix::fd::AsFd, path: &Path, root: &Path, total_bytes: u64) -> Result<ReadDocument, OkfCatalogError> {
    // `fstat` on the descriptor we are about to read from: this is the file actually being
    // read, not a separately-named path. A non-regular file that opens anyway - a device, for
    // one - is refused here rather than read; a swapped symlink and a swapped FIFO are refused
    // at the open itself (`O_NOFOLLOW` / `O_NONBLOCK`), before this check runs.
    let stat = rustix::fs::fstat(fd.as_fd()).map_err(|cause| OkfCatalogError::Open {
        path: path.to_path_buf(),
        cause,
    })?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
        return Err(OkfCatalogError::NotARegularFile {
            path: path.to_path_buf(),
        });
    }
    let remaining = MAX_CATALOG_BYTES - total_bytes.min(MAX_CATALOG_BYTES);
    if stat.st_size.cast_unsigned() > remaining {
        let found = total_bytes.saturating_add(stat.st_size.cast_unsigned());
        return Err(OkfCatalogError::TooLarge {
            path: root.to_path_buf(),
            document: path.to_path_buf(),
            found,
            limit: MAX_CATALOG_BYTES,
        });
    }
    // The read is on the descriptor itself, in bounded chunks: a document that grows while it
    // is being read cannot allocate past the bytes the aggregate had left, in any one chunk.
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = rustix::io::read(fd.as_fd(), &mut chunk).map_err(|cause| OkfCatalogError::Io {
            path: path.to_path_buf(),
            cause: cause.into(),
        })?;
        if n == 0 {
            break;
        }
        if buf.len() as u64 + n as u64 > remaining.saturating_add(1) {
            let found = total_bytes.saturating_add(remaining.saturating_add(1));
            return Err(OkfCatalogError::TooLarge {
                path: root.to_path_buf(),
                document: path.to_path_buf(),
                found,
                limit: MAX_CATALOG_BYTES,
            });
        }
        match chunk.get(..n) {
            Some(read) => buf.extend_from_slice(read),
            // `read` never reports more bytes than the chunk held; the bound is the one
            // guard if it ever did.
            None => {
                return Err(OkfCatalogError::TooLarge {
                    path: path.to_path_buf(),
                    document: path.to_path_buf(),
                    found: total_bytes.saturating_add(u64::try_from(n).unwrap_or(u64::MAX)),
                    limit: MAX_CATALOG_BYTES,
                });
            }
        }
    }
    let text = String::from_utf8(buf).map_err(|cause| OkfCatalogError::Io {
        path: path.to_path_buf(),
        cause: std::io::Error::new(std::io::ErrorKind::InvalidData, cause),
    })?;
    // `take` caps the read; if the file actually held more than `remaining` bytes, what arrived
    // still is - so re-check the length of what was READ, which closes the case of a file that
    // grew between the `fstat` and the read.
    if text.len() as u64 > remaining {
        let found = total_bytes.saturating_add(text.len() as u64);
        return Err(OkfCatalogError::TooLarge {
            path: root.to_path_buf(),
            document: path.to_path_buf(),
            found,
            limit: MAX_CATALOG_BYTES,
        });
    }
    let consumed = text.len() as u64;
    Ok(ReadDocument { text, consumed })
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
    /// [`read_document`]): the refusing total comes from the handle's own `metadata()` before the
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
