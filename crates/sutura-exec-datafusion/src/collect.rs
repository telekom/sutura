//! The result side of this adapter: what the engine hands back, and what a domain row is.
//!
//! # The seam
//!
//! [`crate::translate`] is the half that never reads a result - a plan becomes expressions there,
//! and nothing in it has seen a row. This is the mirror half: an Arrow schema is checked against the
//! plan's labels. Between the two, `lib.rs` keeps what neither of them is about - the session, the
//! runtime, attaching a file, and executing.
//!
//! **The value mapping used to be here and is not any more.** `docs/adr/0039` moved it to
//! `sutura_domain::warehouse::arrow`, because an Arrow array becoming a `Value` was implemented
//! three times - here, in the `DuckDB` adapter, and in `BigQuery`'s via a cast to text - and the
//! agreement between those copies is a correctness property rather than tidiness. What is left here
//! is the half that reads a PLAN's labels, which is this adapter's and no other's.
//!
//! It is its own file for the reason `translate.rs` is: `lib.rs` is at the 1000-line gate, and the
//! gate's answer to that is to split the file rather than to shorten the change. The seam is the one
//! `lib.rs` already named in prose before there was a file on either side of it.
//!

use datafusion::common::{Column, DFSchema};
use datafusion::logical_expr::Expr;

use crate::DataFusionError;

/// The aliased projection, and the expressions to order the result by.
///
/// Named rather than written out, because the two lists are produced together and consumed one line
/// apart: separating them into two passes over the same schema is how they would come to disagree
/// about which position is a grouped one.
pub(crate) type Projected = (Vec<Expr>, Vec<Expr>);

/// The final projection, and the expressions to order by.
///
/// The projection references the aggregate's own output fields rather than re-stating the grouped
/// expressions, because after aggregating there is no `orders.order_date` left to truncate - the
/// truncated value *is* a field. Each is aliased so the result labels are exactly
/// `QueryPlan::result_labels` in order: the keys, then the time bucket, then the measure.
///
/// Ordering is by the projected label columns, unaliased, for the grouped positions only. That is
/// the SQL path's "order by what it grouped by", and it is what makes two runs of one question
/// return rows in one order - which a differential test over row order depends on.
pub(crate) fn outputs(schema: &DFSchema, labels: &[String], group_count: usize) -> Result<Projected, DataFusionError> {
    if schema.fields().len() != labels.len() {
        return Err(DataFusionError::SchemaMismatch {
            expected: labels.to_vec(),
            actual: schema.fields().iter().map(|f| String::from(f.name().as_str())).collect(),
        });
    }
    let mut projection = Vec::with_capacity(labels.len());
    let mut ordering = Vec::with_capacity(group_count);
    for (index, ((qualifier, field), label)) in schema.iter().zip(labels.iter()).enumerate() {
        let reference = Expr::Column(Column::new(qualifier.cloned(), field.name().as_str()));
        projection.push(reference.alias(label.as_str()));
        if index < group_count {
            // Unqualified: an aliased projection field carries no qualifier, so this is the name the
            // sort resolves against.
            ordering.push(Expr::Column(Column::new_unqualified(label.as_str())));
        }
    }
    Ok((projection, ordering))
}
