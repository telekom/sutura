//! What a wren item became, once its name and shape are recognised.
//!
//! Every field here already parsed - a [`sutura_domain::model::ModelName`], not a `String` a
//! render function might quote wrong - so [`super::render`] only has to spell YAML, never decide
//! whether a name is one.

use sutura_domain::model::{Aggregate, ColumnName, DimensionName, MetricName, ModelName, RelationshipName};

pub(crate) struct PlannedColumn {
    pub(crate) name: ColumnName,
    pub(crate) data_type: String,
    pub(crate) nullable: bool,
}

pub(crate) struct PlannedModel {
    pub(crate) name: ModelName,
    /// The dotted table path text, already composed from wren's `tableReference` parts - handed to
    /// the loader's own `QualifiedTable::parse` unchanged, so this converter carries no second
    /// parser for it.
    pub(crate) table: String,
    pub(crate) columns: Vec<PlannedColumn>,
    pub(crate) primary_key: Option<ColumnName>,
}

/// One term - one aggregate over one column - in a metric this converter is about to write.
pub(crate) struct PlannedTerm {
    pub(crate) aggregate: Aggregate,
    pub(crate) column: ColumnName,
}

pub(crate) enum PlannedComputation {
    Simple(PlannedTerm),
    Ratio {
        numerator: PlannedTerm,
        denominator: PlannedTerm,
    },
}

pub(crate) struct PlannedRelationship {
    pub(crate) name: RelationshipName,
    pub(crate) origin_model: ModelName,
    pub(crate) origin_column: ColumnName,
    pub(crate) target_model: ModelName,
    pub(crate) target_column: ColumnName,
    /// `sutura_domain::model::JoinType::as_str`'s own spelling: `one_to_one`, `one_to_many` or
    /// `many_to_one`. `many_to_many` never reaches this type - that shape has no variant to hold it
    /// and is refused before a [`PlannedRelationship`] exists.
    pub(crate) join_type: &'static str,
}

#[derive(Clone)]
pub(crate) struct PlannedDimension {
    pub(crate) name: DimensionName,
    pub(crate) column: ColumnName,
}

pub(crate) struct PlannedMetric {
    pub(crate) name: MetricName,
    pub(crate) model: ModelName,
    pub(crate) computation: PlannedComputation,
    pub(crate) time_column: ColumnName,
    pub(crate) dimensions: Vec<PlannedDimension>,
    /// The wren cube and measure this metric came from, for the document's own prose.
    pub(crate) cube: String,
    pub(crate) measure: String,
}
