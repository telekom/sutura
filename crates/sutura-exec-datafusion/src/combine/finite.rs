//! The combine's last refusal before it answers: a measure that is not a finite number.

use datafusion::arrow::array::{Array as _, Float64Array};
use datafusion::arrow::datatypes::DataType;
use sutura_domain::warehouse::ResultBatches;

use super::CombineError;

/// Refuses an answer whose measure is not a finite number.
///
/// **One pass over the measure columns, and only the ones that are floats.** The measures are the
/// answer's last `measures` columns by construction - [`answer_labels`](super::answer_labels) states the order, and a
/// drill-across answer carries one per arm - and only a `Float64` measure can be non-finite: an
/// exact decimal cannot represent one.
///
/// **Why the combiner refuses this rather than letting the presentation edge do it.**
/// `ResultBatches::to_rows` already refuses a non-finite double, so the answer would be refused
/// either way - but as `sutura_app::ServiceError::Unreadable`, which reaches a caller as a
/// data-system failure. It is neither: a definition that declared `fails` for a zero denominator
/// asked for exactly this, and a caller told to retry it retries forever. So the refusal is taken
/// here, where it can be classified as
/// [`FederatedAnswerRefusal::NonFinite`](sutura_domain::plan::FederatedAnswerRefusal::NonFinite).
pub(super) fn refuse_non_finite(answered: &ResultBatches, measures: usize) -> Result<(), CombineError> {
    let width = answered.schema().fields().len();
    for measure in width.saturating_sub(measures)..width {
        let Some(field) = answered.schema().fields().get(measure) else {
            continue;
        };
        if *field.data_type() != DataType::Float64 {
            continue;
        }
        for batch in answered.batches() {
            let Some(column) = batch.columns().get(measure) else {
                continue;
            };
            let Some(floats) = column.as_any().downcast_ref::<Float64Array>() else {
                continue;
            };
            if floats.iter().flatten().any(|value| !value.is_finite()) {
                return Err(CombineError::NonFinite);
            }
        }
    }
    Ok(())
}
