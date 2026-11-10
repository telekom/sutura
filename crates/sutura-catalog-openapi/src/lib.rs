#![forbid(unsafe_code)]
//! **EXPERIMENTAL.** A [`SemanticCatalog`] over any HTTP metadata API an `OpenAPI` 3.0 or 3.1
//! document describes, read as a **declaring** source - `github.com/telekom/sutura#1727`.
//!
//! The API is configured, not coded: a pinned copy of its `OpenAPI` document and a data-only
//! mapping file together name the `GET` operations that list the models (and, optionally, each
//! model's outgoing links), and where in each answer a name, a description, a column, a key or a
//! link sits, as JSON Pointers. [`Contract::parse`] checks the mapping against the document once,
//! before any request; [`ContractError`] names each way the two can disagree.
//!
//! # What it reads, and how far that is proven
//!
//! Proven against recorded fixtures only (`src/fixture/`): no live API is part of any check. The
//! models, their columns, the source's own column type word, descriptions, primary keys and
//! many-to-one joins onto a target's single-column primary key. Nothing else: no metric, grain,
//! required filter, allowlist, anchor or knowledge, because the vocabulary has no word for them.
//! `docs/what-an-openapi-catalog-can-carry.md` is the field-by-field finding.
//!
//! # What keeps the requests on the declared endpoint
//!
//! The adapter builds every request target itself ([`Target`]): a document path made only of
//! unreserved segments and whole-segment parameters, the mapping's fixed parameters (parsed path
//! segments), each model's own name (parsed by `ModelName::parse`, whose grammar cannot hold `/`,
//! `.`, `?`, `#` or `%`) and a percent-encoded query. The document's `servers` and every non-local
//! `$ref` are never followed, and the only paging style is a cursor sent back as a query value, so
//! no answer can name a URL to dial.
//!
//! # Limits
//!
//! - The catalog is read as the deployment: every caller sees what the deployment's token sees.
//! - `OpenAPI` 3.0 and 3.1 only, as JSON; a Swagger 2.0 document is refused by name.
//! - A link with no foreign-key column on this side (a to-many side) is not carried.
//! - The token is read once, when the reader is built: a token that expires is not renewed.

mod contract;
mod document;
pub mod fixture;
mod harvest;
#[cfg(feature = "http")]
pub mod http;
mod mapping;

use std::time::Instant;

pub use contract::{Contract, ContractError, DOCUMENT_FILE, MAPPING_FILE, MAX_MAPPING_BYTES};
pub use document::MAX_DOCUMENT_BYTES;
pub use harvest::{MAX_CURSOR_BYTES, MAX_ENTRIES};
pub use mapping::{OperationId, ParameterName};
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, InconsistentDefinitions, InvalidDescription};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, InvalidIdentifier, ModelName, RelationshipName, SourceName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// Where the answers come from: one `GET` of a [`Target`] on the declared endpoint.
///
/// The fake seam: [`fixture::FixtureReader`] serves recorded answers, and `http::HttpAnswerReader`
/// (behind the `http` feature) asks the declared endpoint.
pub trait AnswerReader {
    type Error: std::error::Error + Send + Sync + 'static;

    /// `started` is when this load began, so a reader that keeps a deadline keeps one for the
    /// whole load, every page and every per-model listing included.
    fn get(&self, target: &Target, started: Instant) -> Result<serde_json::Value, Self::Error>;
}

/// A request target: a same-origin path and its query, built only by this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target(String);

impl Target {
    /// `path` is a path [`Contract::parse`] built; each query value is percent-encoded.
    fn new(path: &str, query: &[(&ParameterName, &str)]) -> Self {
        let mut target = String::from(path);
        for (index, (name, value)) in query.iter().enumerate() {
            target.push(if index == 0 { '?' } else { '&' });
            target.push_str(name.as_str());
            target.push('=');
            for byte in value.bytes() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    target.push(char::from(byte));
                } else {
                    target.push('%');
                    for nibble in [byte >> 4, byte & 0x0f] {
                        target.push(char::from_digit(u32::from(nibble), 16).map_or('0', |digit| digit.to_ascii_uppercase()));
                    }
                }
            }
        }
        Self(target)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why the answers could not be read as a catalog.
///
/// No variant carries a token; a value taken from an answer is repeated at most 64 bytes long.
#[derive(Debug, thiserror::Error)]
pub enum OpenApiError {
    #[error("the reader could not answer {operation}")]
    Read {
        operation: OperationId,
        #[source]
        cause: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("an answer of {operation} holds nothing at {pointer:?} (on {on:?})")]
    Missing {
        operation: OperationId,
        pointer: String,
        on: Option<String>,
    },
    #[error("an answer of {operation} holds no {expected} at {pointer:?} (on {on:?})")]
    Unexpected {
        operation: OperationId,
        pointer: String,
        on: Option<String>,
        expected: &'static str,
    },
    #[error("the {kind} {value:?} on {on} is not a name")]
    Identifier {
        kind: &'static str,
        value: String,
        on: String,
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("the model {on} carries no description")]
    MissingDescription { on: String },
    #[error("the description of {on} is not usable")]
    Description {
        on: String,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the description of column {column} on {on} is not usable")]
    ColumnDescription {
        on: String,
        column: ColumnName,
        #[source]
        cause: InvalidDescription,
    },
    #[error("a listing of {operation} repeated a cursor it had already given")]
    RepeatedCursor { operation: OperationId },
    #[error("a page of {operation} held no entry while it named a next page")]
    NoProgress { operation: OperationId },
    #[error("a cursor of {operation} is longer than {limit} bytes")]
    CursorTooLong { operation: OperationId, limit: usize },
    #[error("a listing of {operation} returned more than {max} entries")]
    TooManyEntries { operation: OperationId, max: usize },
    #[error("the relationship {relationship} points at {target}, which has no single-column primary key here")]
    TargetKeyUnknown {
        relationship: RelationshipName,
        target: ModelName,
    },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    #[error("the catalog's knowledge does not hold together")]
    Knowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

/// A catalog read through one checked [`Contract`].
///
/// `source` is the `sources.<alias>` every model is on: the API describes one data system's tables.
#[derive(Debug)]
pub struct OpenApiCatalog<R> {
    name: SourceName,
    version: DefinitionVersion,
    source: SourceName,
    contract: Contract,
    reader: R,
}

impl<R: AnswerReader> OpenApiCatalog<R> {
    pub const fn new(name: SourceName, version: DefinitionVersion, source: SourceName, contract: Contract, reader: R) -> Self {
        Self {
            name,
            version,
            source,
            contract,
            reader,
        }
    }

    fn assemble(&self) -> Result<Content, OpenApiError> {
        let harvest = harvest::Harvest {
            contract: &self.contract,
            reader: &self.reader,
            source: &self.source,
            started: Instant::now(),
        };
        let models = harvest.models()?;
        let relationships = harvest.relationships(&models)?;
        let definitions =
            Definitions::assemble(models, relationships, Vec::new()).map_err(|cause| OpenApiError::Inconsistent { cause })?;
        let knowledge =
            Knowledge::assemble(&definitions, KnowledgeInput::none()).map_err(|cause| OpenApiError::Knowledge { cause })?;
        Ok((definitions, knowledge))
    }
}

impl<R: AnswerReader> SemanticCatalog for OpenApiCatalog<R> {
    type Error = OpenApiError;

    const KIND: CatalogKind = CatalogKind::Declaring;

    /// `Structure` and `Descriptions` always; column types, column descriptions and joins when the
    /// mapping names them and the answers carry them. No metric and no knowledge.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure, DefinitionKind::Descriptions]).and_may_provide([
                DefinitionKind::ColumnTypes,
                DefinitionKind::ColumnDescriptions,
                DefinitionKind::Relationships,
            ]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let (definitions, knowledge) = self.assemble()?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| OpenApiError::Digest { cause })
    }
}

/// At most 64 bytes of a document or answer value, cut on a character boundary.
fn shown(raw: &str) -> String {
    let mut end = raw.len().min(64);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    raw.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests;
