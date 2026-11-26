use sutura_domain::catalog::{InvalidDictionaryPattern, InvalidDictionarySelection};
use sutura_domain::model::{InvalidIdentifier, SourceName};

use super::{
    DictionarySource, InvalidConnection, InvalidDescriptionSource, InvalidDocumentationSchema, PredicateOperator, relationships,
};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRdbmsCatalog {
    #[error("invalid relationship declaration: {cause}")]
    Relationship {
        #[source]
        cause: relationships::InvalidRelationship,
    },
    #[error("invalid description source: {cause}")]
    Descriptions {
        #[source]
        cause: InvalidDescriptionSource,
    },
    #[error("relationship on_missing_endpoint must be one of refuse, omit")]
    DefaultOnMissingEndpoint,
    #[error("on_unsupported_column must be one of refuse, omit")]
    UnsupportedColumnPolicy,
    #[error("on_empty_table must be one of refuse, omit")]
    EmptyTablePolicy,
    #[error("on_invalid_comment must be one of refuse, omit")]
    InvalidCommentPolicy,
    #[error("invalid column text policy: {cause}")]
    TextPolicy {
        cause: sutura_domain::catalog::InvalidTextPolicy,
    },
    #[error("the inferred models' audience is invalid")]
    Audience {
        #[source]
        cause: crate::audience::InvalidCatalogAudience,
    },
    #[error("`{key}` is required for `kind: rdbms` and was not written")]
    Missing { key: &'static str },
    #[error("native include/exclude keys require dictionary_source: native_dictionary")]
    NativeKeysOnView,
    #[error("native dictionaries read no documentation environment, schema, live-row predicate or relationship view")]
    ViewKeysOnNative,
    #[error("a native dictionary pattern is invalid")]
    Pattern {
        #[source]
        cause: InvalidDictionaryPattern,
    },
    #[error("native dictionary selection is invalid")]
    Selection {
        #[source]
        cause: InvalidDictionarySelection,
    },
    #[error("`environment` is empty - write the key that selects this deployment's dictionary rows")]
    EmptyEnvironment,
    #[error("`source_alias` is not a source name")]
    SourceAlias {
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`source_alias` is `{alias}`, which names no declared source")]
    UnknownSourceAlias { alias: SourceName },
    #[error("`dictionary_schema` is not usable: {cause}")]
    DictionarySchema {
        #[source]
        cause: InvalidDocumentationSchema,
    },
    #[error("`live_row_predicate.column` is not a column name")]
    PredicateColumn {
        #[source]
        cause: InvalidIdentifier,
    },
    #[error("`live_row_predicate.operator` is `{found}`, not one of: {}", PredicateOperator::known())]
    UnknownPredicateOperator { found: String },
    #[error("`live_row_predicate.operator` is `equals` and no `value` was written")]
    PredicateValueMissing,
    #[error("`live_row_predicate.operator` is `{operator}`, which reads no `value` - remove it")]
    PredicateValueUnexpected { operator: &'static str },
    #[error("`{key}` is 0, which would refuse every read rather than bound one - remove it for the reader's default")]
    ZeroBound { key: &'static str },
    #[error("`connection` is not usable")]
    Connection {
        #[source]
        cause: InvalidConnection,
    },
    #[error("`dictionary_source` is `{found}`, not one of: {}", DictionarySource::known())]
    DictionarySourceUnknown { found: String },
}
