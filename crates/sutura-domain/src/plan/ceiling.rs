//! The row ceilings a deployment configures, and the pair a service carries.
//!
//! [`RowCeiling`](crate::plan::RowCeiling) bounds the set a `top` is ranked over. This module holds
//! the other configured bound: how many rows a federated answer may return whole.

use crate::plan::{MAX_ROWS, RowCeiling};

/// How many rows a federated answer may return before it is refused - `github.com/telekom/sutura#828`.
///
/// **A refusal and never a truncation:** the combined answer is counted after the combine, and one
/// row over this is `RefusalReason::ResultTooLarge`, naming this number. [`Self::DEFAULT`] is
/// [`MAX_ROWS`], so a deployment that configures nothing is bounded as it always was. A `top` answer
/// is held to it too, over the combined set it is ranked from, after [`RowCeiling`].
///
/// **Its own type, with a maximum, and [`RowCeiling`] stays as it is.** A bound an operator can
/// raise has to have a ceiling in code, or "configure it high enough" is how a replica holds an
/// unbounded answer. [`Self::MAX`] is a round number stated as one, not a measured one: past it the
/// 8 MiB `ResponseByteLimit` refuses any row wider than eight bytes anyway. `RowCeiling` is not
/// bounded above, which is an asymmetry kept deliberately - a startup refusal on an existing key is a
/// breaking change, and this key is new.
///
/// **The limit:** this bounds ROWS and not memory. The check runs after the combine, so what holds
/// the replica's memory down is still the working-set ceiling. The single-source row cap stays the
/// compiled [`MAX_ROWS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FederatedRowCeiling(u32);

/// Why a federated row ceiling did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidFederatedRowCeiling {
    /// Zero reads as "unlimited" to whoever wrote it, and would refuse every federated answer.
    #[error("a federated row ceiling of zero would refuse every federated answer rather than mean unlimited")]
    Zero,
    /// More than [`FederatedRowCeiling::MAX`].
    #[error("a federated row ceiling above {max} rows is refused")]
    AboveTheMaximum { max: u32 },
}

impl FederatedRowCeiling {
    /// [`MAX_ROWS`], restated - what every deployment had before this type existed.
    pub const DEFAULT: Self = Self(MAX_ROWS);

    /// The most rows any deployment may configure.
    pub const MAX: u32 = 1_000_000;

    /// Parses an operator-chosen ceiling.
    pub const fn parse(rows: u32) -> Result<Self, InvalidFederatedRowCeiling> {
        if rows == 0 {
            return Err(InvalidFederatedRowCeiling::Zero);
        }
        if rows > Self::MAX {
            return Err(InvalidFederatedRowCeiling::AboveTheMaximum { max: Self::MAX });
        }
        Ok(Self(rows))
    }

    #[inline]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Both configured row ceilings, carried together so a service hands one argument down rather than
/// two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowCeilings {
    top: RowCeiling,
    federated: FederatedRowCeiling,
}

impl RowCeilings {
    /// Both defaults - the behaviour every deployment had before either was configurable.
    pub const DEFAULT: Self = Self {
        top: RowCeiling::DEFAULT,
        federated: FederatedRowCeiling::DEFAULT,
    };

    #[inline]
    pub const fn new(top: RowCeiling, federated: FederatedRowCeiling) -> Self {
        Self { top, federated }
    }

    /// The ceiling a `top` answer is ranked over.
    #[inline]
    pub const fn top(self) -> RowCeiling {
        self.top
    }

    /// The ceiling a federated answer may return whole.
    #[inline]
    pub const fn federated(self) -> FederatedRowCeiling {
        self.federated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_compiled_row_cap() {
        assert_eq!(FederatedRowCeiling::DEFAULT.get(), MAX_ROWS);
    }

    #[test]
    fn zero_and_above_the_maximum_are_refused_and_the_maximum_is_not() {
        assert_eq!(FederatedRowCeiling::parse(0), Err(InvalidFederatedRowCeiling::Zero));
        assert_eq!(
            FederatedRowCeiling::parse(FederatedRowCeiling::MAX + 1),
            Err(InvalidFederatedRowCeiling::AboveTheMaximum {
                max: FederatedRowCeiling::MAX
            })
        );
        assert_eq!(
            FederatedRowCeiling::parse(FederatedRowCeiling::MAX).map(FederatedRowCeiling::get),
            Ok(FederatedRowCeiling::MAX)
        );
    }

    #[test]
    fn the_top_ceiling_has_no_maximum() {
        assert_eq!(RowCeiling::parse(u32::MAX).map(RowCeiling::get), Ok(u32::MAX));
    }
}
