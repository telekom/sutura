//! `top`: an explicit order and a caller-chosen row limit, so a group-by over a wide dimension is
//! bounded rather than refused.
//!
//! **The problem this exists for.** Without it the planner fixes the order (the group keys,
//! `NULLS LAST`) and [`crate::plan::MAX_ROWS`] refuses a result past ten thousand rows - so "top
//! ten products by revenue" over a fifty-thousand-product dimension has no representable form:
//! the only question that exists asks for all fifty thousand and is refused. `top` is the bounded
//! form of that question.
//!
//! **The tie-break is not decoration.** [`Top`] ranks by [`TopBy::Metric`] or [`TopBy::Period`],
//! and a generator appends the plan's own group-key ordering after it - the same ordering a
//! question without `top` already gets. Without that second key, two rows tied on the first one
//! come back in an order nothing pins, and two executors disagree about which `n`th row survives.

use crate::plan::RowCeiling;

/// A caller-chosen order and row limit on a question's own result.
///
/// `n` is checked against [`crate::plan::RowCeiling`] where the question is resolved, not here:
/// this type carries only what the caller asked for, and a bound that depends on the deployment
/// does not belong on a value the caller alone constructs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Top {
    n: TopN,
    by: TopBy,
    direction: TopDirection,
}

impl Top {
    #[inline]
    pub const fn new(n: TopN, by: TopBy, direction: TopDirection) -> Self {
        Self { n, by, direction }
    }

    #[inline]
    pub const fn n(self) -> TopN {
        self.n
    }

    #[inline]
    pub const fn by(self) -> TopBy {
        self.by
    }

    #[inline]
    pub const fn direction(self) -> TopDirection {
        self.direction
    }
}

/// What to rank a `top` question's rows by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopBy {
    /// The question's own measure.
    Metric,
    /// The time bucket - the oldest or newest periods, rather than the largest or smallest values.
    Period,
}

/// Which way [`TopBy`] ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopDirection {
    Desc,
    Asc,
}

/// A positive row count. Zero asks for nothing, which is not what a caller who wrote `top` meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u32")]
pub struct TopN(u32);

/// Why a `top.n` did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTopN {
    #[error("top.n of zero asks for nothing")]
    Zero,
}

impl TopN {
    pub const fn parse(n: u32) -> Result<Self, InvalidTopN> {
        if n == 0 {
            return Err(InvalidTopN::Zero);
        }
        Ok(Self(n))
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Whether this request asks for more rows than `ceiling` certifies.
    ///
    /// Takes the ceiling rather than reading [`crate::plan::MAX_ROWS`] directly, so a deployment
    /// that configured [`crate::plan::RowCeiling`] is compared against its own number rather than
    /// the compiled default - `github.com/telekom/sutura#777`'s own record of the decision.
    #[inline]
    pub const fn exceeds_the_row_cap(self, ceiling: RowCeiling) -> bool {
        self.0 > ceiling.get()
    }
}

impl TryFrom<u32> for TopN {
    type Error = InvalidTopN;

    fn try_from(n: u32) -> Result<Self, Self::Error> {
        Self::parse(n)
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidTopN, TopN};

    #[test]
    fn a_zero_row_count_is_refused_rather_than_read_as_no_limit() {
        assert_eq!(TopN::parse(0), Err(InvalidTopN::Zero));
    }

    #[test]
    fn a_positive_row_count_round_trips() {
        assert_eq!(TopN::parse(10).expect("ten is a row count").get(), 10);
    }

    /// `TopN`'s `try_from = "u32"` and its derived `Serialize` agree on more than shape: a value
    /// written and read back is the same value. Exempted from the generated string corpus in
    /// `crates/sutura-domain/src/serialized_form_tests.rs` (`xtask/src/serde_parse/completeness.rs`'s
    /// `ASKED_ELSEWHERE`) because its canonical form is a JSON number, not a string that generator
    /// produces.
    #[test]
    fn a_top_n_round_trips_through_its_on_disk_shape() {
        for n in [1_u32, 3, crate::plan::MAX_ROWS, crate::plan::MAX_ROWS + 1, u32::MAX] {
            let parsed = TopN::parse(n).expect("a positive count is a row count");
            let value = serde_json::to_value(parsed).expect("a TopN serializes");
            let back: TopN = serde_json::from_value(value).expect("a TopN's own serialized form deserializes");
            assert_eq!(parsed, back);
        }
    }

    #[test]
    fn only_a_count_past_the_row_cap_exceeds_it() {
        let ceiling = crate::plan::RowCeiling::DEFAULT;
        assert!(
            !TopN::parse(ceiling.get())
                .expect("the cap itself is a row count")
                .exceeds_the_row_cap(ceiling)
        );
        assert!(
            TopN::parse(ceiling.get() + 1)
                .expect("one past the cap is still a row count")
                .exceeds_the_row_cap(ceiling)
        );
    }

    #[test]
    fn a_configured_ceiling_is_compared_against_rather_than_the_compiled_default() {
        // A count within the compiled default but past a NARROWER configured ceiling exceeds it -
        // the whole point of `RowCeiling` being an argument rather than a constant read in place.
        let narrower = crate::plan::RowCeiling::parse(5).expect("five is a row count");
        assert!(TopN::parse(10).expect("ten is a row count").exceeds_the_row_cap(narrower));
    }
}
