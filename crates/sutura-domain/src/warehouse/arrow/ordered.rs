//! A result put in order by one of its columns, compared on the typed Arrow values.
//!
//! **Typed, and that is the whole point.** [`ResultBatches::to_rows`] answers a decimal with a
//! fractional part as its exact TEXT, so an order taken over decoded rows is a byte order:
//! `"999.00"` before `"1000.00"` descending, and `"-1.50"` before `"-10.00"` ascending. An order
//! taken here compares the `Decimal128` integers themselves, and every other Arrow type by its own
//! value, through `arrow-ord`'s comparator - the same kernel the engine sorts with.

use arrow_ord::ord::make_comparator;
use arrow_schema::{ArrowError, SortOptions};
use arrow_select::concat::concat_batches;
use arrow_select::take::take_record_batch;

use super::ResultBatches;
use crate::query::{TopDirection, TopN};

/// Why a result could not be put in order.
///
/// Carries a column POSITION and Arrow's own message, never a cell, so it discloses no data.
#[derive(Debug, thiserror::Error)]
pub enum UnorderedResult {
    /// The result has no column at the position asked for.
    #[error("a result of {width} columns has no column at position {at} to order by")]
    NoSuchColumn { at: usize, width: usize },
    /// Arrow refused to concatenate, compare or take the result's own arrays.
    #[error("the result could not be ordered by its column at position {at}")]
    Arrow {
        at: usize,
        #[source]
        cause: ArrowError,
    },
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the order is a concern of its own beside the schema guard and the decode, and its kernels stay in this file"
)]
impl ResultBatches {
    /// The first `n` rows, ordered by the column at `at` in `direction`.
    ///
    /// **Nulls last in either direction**, the ordered-result contract `sutura_sql::generate`'s
    /// `ordered_nulls_last` states for the rendered path. **Stable**: rows the column ties keep the
    /// order they arrived in, which for a combined answer is its ascending key order.
    ///
    /// # Errors
    ///
    /// [`UnorderedResult::NoSuchColumn`] for a position past the schema, and
    /// [`UnorderedResult::Arrow`] for a column type `arrow-ord` cannot compare.
    pub fn first_by(self, at: usize, direction: TopDirection, n: TopN) -> Result<Self, UnorderedResult> {
        let arrow = |cause| UnorderedResult::Arrow { at, cause };
        let width = self.schema.fields().len();
        let all = concat_batches(&self.schema, &self.batches).map_err(arrow)?;
        let column = all.columns().get(at).ok_or(UnorderedResult::NoSuchColumn { at, width })?;
        let options = SortOptions {
            descending: matches!(direction, TopDirection::Desc),
            nulls_first: false,
        };
        let compare = make_comparator(column, column, options).map_err(arrow)?;
        let mut order: Vec<usize> = (0..all.num_rows()).collect();
        order.sort_by(|&left, &right| compare(left, right));
        order.truncate(usize::try_from(n.get()).unwrap_or(usize::MAX));
        let indices = arrow_array::UInt64Array::from_iter_values(order.into_iter().map(|row| row as u64));
        let first = take_record_batch(&all, &indices).map_err(arrow)?;
        Ok(Self {
            schema: self.schema,
            rows: first.num_rows(),
            batches: vec![first],
            executions: self.executions,
        })
    }
}
