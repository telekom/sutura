//! Classify a `DataFusionError` the unparser or the engine raised, past its `Context`/`Diagnostic`/
//! `Shared`/`Collection` wrapper variants, so `statement.rs` can route it to a typed refusal.

use datafusion::common::{DataFusionError, SchemaError};
use datafusion::sql::unparser::{UnprojectableAggregateColumn, UnquotableName};

/// The column name the unparser's own refusal to name back against an `Aggregate` it rewrote
/// names, if `cause` is that refusal - an internal planner gap, not a result the source produced
/// or a database failure, so the caller is told not to retry rather than reading a generic
/// `StatementFailed`.
pub(super) fn unprojected_agg(cause: &DataFusionError) -> Option<&str> {
    match cause {
        DataFusionError::External(cause) => cause
            .downcast_ref::<UnprojectableAggregateColumn>()
            .map(|cause| cause.column.as_str()),
        DataFusionError::Context(_, cause) | DataFusionError::Diagnostic(_, cause) => unprojected_agg(cause),
        DataFusionError::Shared(cause) => unprojected_agg(cause),
        DataFusionError::Collection(causes) => causes.iter().find_map(unprojected_agg),
        _ => None,
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

#[cfg(test)]
mod tests {
    use sutura_domain::plan::StatementStopped;
    use sutura_domain::raw::SqlConstruct;

    /// The unparser's own refusal to name a column back against an `Aggregate` it rewrote (the
    /// shape behind #2068: a two-level aggregate from `count(DISTINCT ..)`, still naming the
    /// inner column after some plan shape the fold in `fold_aggregate_rename_projection` does not
    /// reach) is an internal planner gap, not a database failure - the caller is told not to retry
    /// rather than reading the generic `StatementFailed`.
    #[test]
    fn an_unprojected_aggregate_is_named_as_an_unsupported_construct_not_a_generic_failure() {
        let cause =
            datafusion::common::DataFusionError::External(Box::new(datafusion::sql::unparser::UnprojectableAggregateColumn {
                column: "count(DISTINCT test.b)".to_owned(),
            }));
        let stopped: StatementStopped<()> = super::super::unwritten("a part did not unparse", &cause);
        assert!(
            matches!(stopped, StatementStopped::UnsupportedSyntax(SqlConstruct::AggregateRewrite)),
            "{stopped:?}"
        );
    }
}
