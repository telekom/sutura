//! A question's nested clauses - its period, its filters and its `top` - as a tool call carries
//! them, and what each hands the domain's parse.

use sutura_runtime::relative_range::{LastWire, RangeResolutionError, WallClock, resolve_range};

/// A resolved period's first included and first excluded ISO day.
type Period = (String, String);

/// A `top` clause: rank by `by`, in `direction`, keep the first `n`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TopArgs {
    /// How many groups to return. Must be positive, and no more than this deployment will
    /// certify - a larger count is the same refusal an unbounded question over that many groups
    /// already gets.
    n: u32,
    /// `metric` ranks by the question's own measure; `period` ranks by the time bucket.
    by: String,
    /// `desc` for the largest first, `asc` for the smallest.
    direction: String,
}

/// A half-open period: `start` is included, `end` is not. Either an absolute period
/// (`start`/`end`) or a period relative to today (`last`) - never both, never neither.
///
/// Half-open at every grain, which is what makes a month `[2026-06-01, 2026-07-01)` rather than a
/// last day that differs per month. Both dates are ISO `YYYY-MM-DD`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeArgs {
    /// The first day included, as `YYYY-MM-DD` - for example `2026-06-01`. Mutually exclusive
    /// with `last`.
    start: Option<String>,
    /// The first day NOT included, as `YYYY-MM-DD` - for example `2026-07-01`. Mutually exclusive
    /// with `last`.
    end: Option<String>,
    /// A period ending today (or yesterday, unless `include_current`), resolved against this
    /// deployment's own clock. Mutually exclusive with `start`/`end`.
    last: Option<LastArgs>,
}

/// A count of calendar periods before today, resolved at request time rather than authored as
/// dates - `telekom/sutura#778`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LastArgs {
    /// How many `unit`s back. Must not be zero.
    count: u32,
    /// One of `day`, `week`, `month`, `quarter` or `year`.
    unit: String,
    /// Whether today's own, possibly partial, period is included.
    #[serde(default)]
    include_current: bool,
}

/// One filter: a dimension, and what it must - or must not - equal.
///
/// Tagged by `op`, mirroring `sutura_domain::query::Filter` - `github.com/telekom/sutura#968`. Every
/// value, in either shape, is still checked against the metric's own allowlist; `In`/`NotIn` widen
/// the predicate's shape and never the source of a value.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FilterArgs {
    /// The dimension equals this one value.
    Eq {
        /// The dimension to filter on, by the name the metric declares it under.
        dimension: String,
        /// The value it must equal. Where the catalog declares an allowlist, only a declared
        /// value is accepted.
        value: String,
    },
    /// The dimension equals one of these values. At least one.
    In {
        /// The dimension to filter on, by the name the metric declares it under.
        dimension: String,
        #[schemars(length(min = 1))]
        values: Vec<String>,
    },
    /// The dimension equals none of these values. At least one. A row whose value for this
    /// dimension is unmatched/NULL (an unresolved join key) is excluded, not included.
    NotIn {
        /// The dimension to filter on, by the name the metric declares it under.
        dimension: String,
        #[schemars(length(min = 1))]
        values: Vec<String>,
    },
}

impl TopArgs {
    pub(super) fn raw(&self) -> sutura_domain::question::RawTop<'_> {
        sutura_domain::question::RawTop::new(self.n, &self.by, &self.direction)
    }
}

impl RangeArgs {
    /// Resolves the period against `clock`.
    pub(super) fn resolve(self, clock: &impl WallClock) -> Result<Period, RangeResolutionError> {
        let last = self
            .last
            .map(|last| LastWire::new(last.count, last.unit, last.include_current));
        resolve_range(clock, self.start, self.end, last)
    }
}
