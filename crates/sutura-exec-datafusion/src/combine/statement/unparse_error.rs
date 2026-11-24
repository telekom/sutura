//! Classify a `DataFusionError` the unparser or the engine raised, past its `Context`/`Diagnostic`/
//! `Shared`/`Collection` wrapper variants, so `statement.rs` can route it to a typed refusal.

use datafusion::common::{DataFusionError, SchemaError};
use datafusion::sql::unparser::{UnprojectableAggregateColumn, UnquotableName};

/// Whether `cause` is the unparser's own refusal to name a column back against an `Aggregate` it
/// rewrote - an internal planner gap, not a result the source produced or a database failure, so
/// the caller is told not to retry rather than reading a generic `StatementFailed`.
pub(super) fn unprojected_agg(cause: &DataFusionError) -> bool {
    match cause {
        DataFusionError::External(cause) => cause.downcast_ref::<UnprojectableAggregateColumn>().is_some(),
        DataFusionError::Context(_, cause) | DataFusionError::Diagnostic(_, cause) => unprojected_agg(cause),
        DataFusionError::Shared(cause) => unprojected_agg(cause),
        DataFusionError::Collection(causes) => !causes.is_empty() && causes.iter().all(unprojected_agg),
        _ => false,
    }
}

/// Whether `cause` is the unparser's or a text-policy view's refusal of a name it could not quote.
pub(super) fn unquotable(cause: &DataFusionError) -> bool {
    match cause {
        DataFusionError::External(cause) => cause.downcast_ref::<UnquotableName>().is_some(),
        DataFusionError::Context(_, cause) | DataFusionError::Diagnostic(_, cause) => unquotable(cause),
        DataFusionError::Shared(cause) => unquotable(cause),
        _ => false,
    }
}

pub(super) fn field_not_found(cause: &DataFusionError) -> bool {
    match cause {
        DataFusionError::SchemaError(schema, _) => matches!(schema.as_ref(), SchemaError::FieldNotFound { .. }),
        DataFusionError::Context(_, cause) | DataFusionError::Diagnostic(_, cause) => field_not_found(cause),
        DataFusionError::Shared(cause) => field_not_found(cause),
        DataFusionError::Collection(causes) => !causes.is_empty() && causes.iter().all(field_not_found),
        _ => false,
    }
}
