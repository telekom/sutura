use std::num::NonZeroU64;

use sutura_domain::model::ColumnName;

use crate::NativeSelectionError;

/// Why a dictionary could not be read as a catalog.
///
/// Every variant is a typed contract rather than a message; the message is for a human and the
/// variant is what a caller can branch on. The mapping variants carry the table or column they were
/// refused on, because a dictionary is many rows and "invalid identifier" with no table name sends
/// a reader back to all of them.
#[derive(Debug, thiserror::Error)]
pub enum RdbmsError {
    #[error("declared relationship refused: {cause}")]
    Declaration {
        #[from]
        cause: crate::DeclarationError,
    },
    #[error("the relationship column pairs are invalid: {cause}")]
    RelationshipKeys {
        #[source]
        cause: sutura_domain::catalog::InvalidJoinKeys,
    },
    #[error("the relationship metadata is invalid: {cause}")]
    Relationship {
        #[from]
        cause: crate::RelationshipError,
    },
    #[error("native dictionary selection refused the load")]
    Native {
        #[from]
        cause: NativeSelectionError,
    },
    /// The reader could not fetch the dictionary.
    #[error("reading the dictionary failed: {0}")]
    Read(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// The listener redirected the login to an address the connection does not declare. Refused
    /// before authentication.
    #[error("the listener redirected the login to an address this connection does not declare")]
    RedirectRefused,
    /// The reader returned no table, so this bundle cannot honour its required `Structure` claim.
    #[error("the dictionary contains no visible table")]
    NoVisibleTables,
    /// A table's semantic name did not parse as a model name.
    #[error("model {model} for table {table} is not a usable model name: {cause}")]
    ModelName {
        table: String,
        model: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A table's catalog did not parse as the top part of a qualified table name.
    #[error("catalog {catalog} of table {table} is not usable: {cause}")]
    CatalogName {
        table: String,
        catalog: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A table's schema did not parse as the middle part of a qualified table name.
    #[error("schema {schema} of table {table} is not usable: {cause}")]
    SchemaName {
        table: String,
        schema: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A physical table's own name did not parse.
    #[error("physical table {table} is not usable: {cause}")]
    TableName {
        table: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// Two semantic models named the same physical table.
    #[error("physical table {table} is declared twice")]
    DuplicateTable { table: String },
    /// A foreign key named a physical table absent from the dictionary.
    #[error("foreign key endpoint {table} is not a table in the dictionary")]
    UnknownRelationshipTable { table: String },
    /// A column's name did not parse.
    #[error("column {column} on table {table} is not a usable column name: {cause}")]
    ColumnName {
        table: String,
        column: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A foreign key's name did not parse.
    #[error("foreign key {relationship} is not a usable relationship name: {cause}")]
    RelationshipName {
        relationship: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A table description did not pass the authored-prose rule.
    #[error("the description of table {table} is not usable: {cause}")]
    Description {
        table: String,
        #[source]
        cause: sutura_domain::catalog::InvalidDescription,
    },
    /// A column comment did not pass the authored-prose rule.
    #[error("the comment on column {column} of table {table} is not usable: {cause}")]
    ColumnDescription {
        table: String,
        column: ColumnName,
        #[source]
        cause: sutura_domain::catalog::InvalidDescription,
    },
    /// The referenced column had no single-column primary or unique-key evidence.
    #[error("referenced column {table}.{column} has no single-column primary or unique-key evidence")]
    TargetUniquenessUnknown { table: String, column: String },
    /// The assembled definitions did not hold together.
    #[error("the dictionary definitions do not hold together: {cause}")]
    Inconsistent {
        #[source]
        cause: sutura_domain::catalog::InconsistentDefinitions,
    },
    /// Pinning failed.
    #[error("the dictionary bundle could not be pinned: {cause}")]
    Digest {
        #[source]
        cause: sutura_domain::definitions::NotDigestible,
    },
    /// The dictionary carries more tables and relationships than the declared row cap.
    ///
    /// Checked on the decoded dictionary, whatever the reader did. The byte cap is not checked here:
    /// a [`crate::Dictionary`] carries no byte count, so it is the reader's to hold against the response.
    #[error("the dictionary carries {rows} entries, over the declared maximum of {limit}")]
    ExceedsBounds { rows: u64, limit: NonZeroU64 },
}
