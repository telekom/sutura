#![forbid(unsafe_code)]
//! **EXPERIMENTAL.** API-only seam for an `OpenAPI`-described declaring catalog.
//!
//! [`Contract::parse`] refuses every input, including `OpenAPI` 3.0/3.1. No contract can be
//! constructed, so [`OpenApiCatalog::load`] is unreachable. No API is fetched or catalog loaded.
//! The public signatures and fixture reader let the later implementation's tests run against
//! this refusing baseline. Fixture data is synthetic or a public-document subset, not live evidence.

pub mod fixture;
mod mapping;

use std::convert::Infallible;
use std::time::Instant;

pub use mapping::{OperationId, ParameterName};
use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{InconsistentDefinitions, InvalidDescription};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, KnowledgeCapabilities};
use sutura_domain::model::{ColumnName, InvalidIdentifier, ModelName, RelationshipName, SourceName};
use sutura_domain::pinned::{CatalogKind, DefinitionVersion, PinnedDefinitions, SemanticCatalog};

/// The document byte cap, checked before JSON parsing.
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
/// The mapping byte cap reserved for the implementation.
pub const MAX_MAPPING_BYTES: usize = 64 * 1024;
/// The listing entry cap reserved for the implementation.
pub const MAX_ENTRIES: usize = 100_000;
/// The cursor byte cap reserved for the implementation.
pub const MAX_CURSOR_BYTES: usize = 2048;

/// Why a document and a mapping do not make a catalog. Checked at construction, before any request.
///
/// No variant carries a token or an answer; a document value is repeated at most 64 bytes long.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("the {what} is larger than the {limit}-byte cap")]
    TooLarge { what: &'static str, limit: usize },
    #[error("the document is not JSON")]
    Document {
        #[source]
        cause: serde_json::Error,
    },
    #[error("the mapping is not in the mapping vocabulary")]
    Mapping {
        #[source]
        cause: serde_norway::Error,
    },
    #[error("the document is not OpenAPI 3.0 or 3.1 (found {found:?})")]
    UnsupportedVersion { found: Option<String> },
    #[error("the document references {reference:?}, which is not a local `#/` reference; nothing outside the document is read")]
    ExternalReference { reference: String },
    #[error("the document references {reference:?}, which it does not hold")]
    UnresolvedReference { reference: String },
    #[error("the document nests references deeper than {limit}")]
    ReferenceTooDeep { limit: usize },
    #[error("the document has no operation {operation}")]
    UnknownOperation { operation: OperationId },
    #[error("the document has more than one operation {operation}")]
    DuplicateOperation { operation: OperationId },
    #[error("the operation {operation} is not a GET; the catalog only reads")]
    NotAGet { operation: OperationId },
    #[error("the operation {operation} needs a credential other than a bearer token")]
    UnsupportedSecurity { operation: OperationId },
    #[error("the operation {operation} declares no 200 JSON answer schema")]
    NoJsonAnswer { operation: OperationId },
    /// The path is not literal unreserved segments and whole-segment parameters, so the adapter
    /// cannot prove the request stays on the declared endpoint.
    #[error("the path of {operation} is not one this adapter builds; it could leave the declared endpoint")]
    OffOriginPath { operation: OperationId },
    #[error("the operation {operation} needs the parameter {parameter}, which the mapping does not bind")]
    UnboundParameter { operation: OperationId, parameter: String },
    #[error("the parameter {parameter} is not a {location} parameter of {operation}")]
    UndeclaredParameter {
        operation: OperationId,
        parameter: ParameterName,
        location: &'static str,
    },
    #[error("the mapping binds the parameter {parameter}, which no mapped operation's path has")]
    UnusedParameter { parameter: ParameterName },
    #[error("the answer schema of {operation} declares nothing at {pointer:?}")]
    NotInSchema { operation: OperationId, pointer: String },
    #[error("the answer schema of {operation} does not declare {expected} at {pointer:?}")]
    NotShaped {
        operation: OperationId,
        pointer: String,
        expected: &'static str,
    },
}

/// A checked mapping. This API-only baseline cannot construct one.
#[derive(Debug)]
pub struct Contract {
    unavailable: Infallible,
}

impl Contract {
    /// Refuses every document version; the mapping is not parsed in this API-only baseline.
    pub fn parse(document: &str, _mapping: &str) -> Result<Self, ContractError> {
        if document.len() > MAX_DOCUMENT_BYTES {
            return Err(ContractError::TooLarge {
                what: "document",
                limit: MAX_DOCUMENT_BYTES,
            });
        }
        let root: serde_json::Value = serde_json::from_str(document).map_err(|cause| ContractError::Document { cause })?;
        let found = root
            .get("openapi")
            .and_then(serde_json::Value::as_str)
            .map(|version| format!("openapi {}", shown(version)))
            .or_else(|| {
                root.get("swagger")
                    .map(|version| format!("swagger {}", shown(version.as_str().unwrap_or("?"))))
            });
        // ponytail: no supported versions until the contract implementation arrives.
        Err(ContractError::UnsupportedVersion { found })
    }
}

/// Where the answers come from: one `GET` of a [`Target`] on the declared endpoint.
///
/// The fake seam: [`fixture::FixtureReader`] serves recorded answers.
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

/// A declaring catalog whose checked contract is unavailable in this API-only baseline.
#[derive(Debug)]
pub struct OpenApiCatalog<R> {
    _name: SourceName,
    _version: DefinitionVersion,
    _source: SourceName,
    contract: Contract,
    _reader: R,
}

impl<R: AnswerReader> OpenApiCatalog<R> {
    pub const fn new(name: SourceName, version: DefinitionVersion, source: SourceName, contract: Contract, reader: R) -> Self {
        Self {
            _name: name,
            _version: version,
            _source: source,
            contract,
            _reader: reader,
        }
    }
}

impl<R: AnswerReader> SemanticCatalog for OpenApiCatalog<R> {
    type Error = OpenApiError;

    const KIND: CatalogKind = CatalogKind::Declaring;

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
        match self.contract.unavailable {}
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
