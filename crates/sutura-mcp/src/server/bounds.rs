//! Pre-allocation bounds on request values, read off the raw `serde_json` map before
//! `serde_json::from_value` allocates the typed collection.
//!
//! #1089 bounds the same counts downstream: `sutura_domain::question::parse_query` checks
//! `metrics.len() > MAX_METRICS` etc. *after* `from_value` has built the `Vec<String>`. The bound
//! there is the semantic one and stays; the bound here is the earlier, allocation-preventing one.
//! It fires through the same channel: a count over the wire limit returns the same
//! [`RefusalReason`] `parse_query` would, so a caller sees one refusal code from one limit, never
//! two controls reporting a bound the other can exceed.
//!
//! The statement-size bound has no downstream twin to conflict with: `RawStatement::parse` checks
//! `> MAX_RAW_STATEMENT_BYTES` on the deserialized `String`, and this fires the same
//! [`MalformedStatement`] code earlier, before the `String` is allocated.

use sutura_domain::query::{MAX_DIMENSIONS, MAX_FILTERS, MAX_METRICS, RefusalReason};
use sutura_domain::raw::MAX_RAW_STATEMENT_BYTES;

use crate::wire::MalformedStatement;

/// Reads the raw arguments and returns a [`RefusalReason`] if a question count exceeds its limit,
/// before `from_value` allocates the `Vec<String>`.
///
/// Returns `None` when the arguments are not an object or the field is absent or not an array, so a
/// missing field is left to `from_value`'s existing typed error (the `NotAnObject` path) rather than
/// to this bound. The refusal, when it fires, carries only the count and the limit - no caller text.
pub(crate) fn refused_for_over_cap(arguments: &serde_json::Value) -> Option<RefusalReason> {
    let map = arguments.as_object()?;
    if let Some(requested) = array_len(map, "metrics").filter(|&n| n > MAX_METRICS) {
        return Some(RefusalReason::TooManyMetrics {
            requested,
            limit: MAX_METRICS,
        });
    }
    if let Some(requested) = array_len(map, "dimensions").filter(|&n| n > MAX_DIMENSIONS) {
        return Some(RefusalReason::TooManyDimensions {
            requested,
            limit: MAX_DIMENSIONS,
        });
    }
    if let Some(requested) = array_len(map, "filters").filter(|&n| n > MAX_FILTERS) {
        return Some(RefusalReason::TooManyFilters {
            requested,
            limit: MAX_FILTERS,
        });
    }
    None
}

/// Reads the raw arguments and returns a [`MalformedStatement`] if the `statement` string exceeds
/// [`MAX_RAW_STATEMENT_BYTES`], before `from_value` allocates the `String`.
///
/// Returns `None` when the arguments are not an object or the field is absent or not a string, so a
/// missing field is left to `from_value`'s existing typed error rather than to this bound. The error
/// carries only the length and the limit - no caller text.
pub(crate) fn oversized_statement(arguments: &serde_json::Value) -> Option<MalformedStatement> {
    let len = arguments
        .as_object()?
        .get("statement")
        .and_then(serde_json::Value::as_str)?
        .len();
    if len > MAX_RAW_STATEMENT_BYTES {
        return Some(MalformedStatement::StatementTooLarge {
            len,
            limit: MAX_RAW_STATEMENT_BYTES,
        });
    }
    None
}

/// The length of a JSON array field, or `None` when the field is absent or not an array.
fn array_len(map: &serde_json::Map<String, serde_json::Value>, field: &str) -> Option<usize> {
    map.get(field).and_then(serde_json::Value::as_array).map(Vec::len)
}
