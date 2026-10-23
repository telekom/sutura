//! The ceilings a deployment configures, and the one value a service carries them in.
//!
//! [`RowCeiling`](crate::plan::RowCeiling) bounds the set a `top` is ranked over. This module holds
//! the other configured bounds: how many rows a federated answer may return whole, and the three a
//! plan is held to - data systems, lookup legs and legs per answer.

use crate::plan::{MAX_LEGS, MAX_ROWS, RowCeiling};
use crate::query::MAX_LOOKUP_LEGS;

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

/// Why a plan ceiling - [`SourceCeiling`], [`LookupLegCeiling`] or [`LegCeiling`] - did not parse.
///
/// One error for the three, because the two ways to get one wrong are the same and the settings key
/// that names which one is the config refusal's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPlanCeiling {
    /// Zero reads as "unlimited" to whoever wrote it, and would refuse every question it bounds.
    #[error("a plan ceiling of zero would refuse every question it bounds rather than mean unlimited")]
    Zero,
    /// More than the ceiling's own `MAX`.
    #[error("a plan ceiling above {max} is refused")]
    AboveTheMaximum { max: usize },
}

/// `value` when it is one to `max`.
const fn bounded(value: usize, max: usize) -> Result<usize, InvalidPlanCeiling> {
    if value == 0 {
        return Err(InvalidPlanCeiling::Zero);
    }
    if value > max {
        return Err(InvalidPlanCeiling::AboveTheMaximum { max });
    }
    Ok(value)
}

/// The most distinct data systems one answer may read, for one metric or several -
/// `github.com/telekom/sutura#1532`.
///
/// Each data system is an identity the broker mints credentials for, so this bounds the one source
/// set an answer mints for. A plan over it is refused as
/// [`RefusalReason::PlanSpansTooManySources`](crate::query::RefusalReason::PlanSpansTooManySources),
/// naming this number, inside `compile` and so before anything is minted - a cell counts the mints
/// for one metric, and none does for several. [`Self::MAX`] is [`MAX_LEGS`]: every leg reads one
/// data system, so a ceiling above the leg cap could never refuse anything.
///
/// **The limit, for all three plan ceilings:** the planner checks them, not the plan types.
/// `FederatedPlan::new` and `DrillAcrossPlan::new` hold only the absolute [`MAX_LOOKUP_LEGS`] and
/// [`MAX_LEGS`], so a plan built outside `sutura-semantic` is held only to those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceCeiling(usize);

impl SourceCeiling {
    /// One fact leg beside three lookup legs - the fixed bound before this was a setting.
    pub const DEFAULT: Self = Self(4);

    /// The most data systems any deployment may configure.
    pub const MAX: usize = MAX_LEGS;

    /// Parses an operator-chosen ceiling.
    pub const fn parse(sources: usize) -> Result<Self, InvalidPlanCeiling> {
        match bounded(sources, Self::MAX) {
            Ok(sources) => Ok(Self(sources)),
            Err(invalid) => Err(invalid),
        }
    }

    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The most lookup legs one metric's plan may read - one per crossing relationship.
///
/// More is refused as
/// [`RefusalReason::PlanNeedsTooManyLookupLegs`](crate::query::RefusalReason::PlanNeedsTooManyLookupLegs),
/// naming this number. Each lookup leg reads its whole table that survives the question's own
/// filters, so this bounds how many such reads one metric starts; the working-set ceiling bounds
/// their bytes. [`Self::MAX`] is [`MAX_LOOKUP_LEGS`], what a plan type can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupLegCeiling(usize);

impl LookupLegCeiling {
    /// Three - the fixed bound before this was a setting.
    pub const DEFAULT: Self = Self(3);

    /// The most lookup legs any deployment may configure.
    pub const MAX: usize = MAX_LOOKUP_LEGS;

    /// Parses an operator-chosen ceiling.
    pub const fn parse(legs: usize) -> Result<Self, InvalidPlanCeiling> {
        match bounded(legs, Self::MAX) {
            Ok(legs) => Ok(Self(legs)),
            Err(invalid) => Err(invalid),
        }
    }

    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The most legs - statements against a data system - one answer may run, counting every leg of
/// every metric.
///
/// More is refused as
/// [`RefusalReason::QuestionNeedsTooManyLegs`](crate::query::RefusalReason::QuestionNeedsTooManyLegs),
/// naming this number. Each leg's result is held whole before the combine, so this bounds the
/// statements one question starts and the results it holds at once; the working-set ceiling bounds
/// their bytes. It is its own setting and does not follow [`LookupLegCeiling`]: several metrics
/// over the same two data systems run many legs and read few sources. [`Self::MAX`] is
/// [`MAX_LEGS`], what a plan type can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegCeiling(usize);

impl LegCeiling {
    /// Six - the fixed bound before this was a setting.
    pub const DEFAULT: Self = Self(6);

    /// The most legs any deployment may configure.
    pub const MAX: usize = MAX_LEGS;

    /// Parses an operator-chosen ceiling.
    pub const fn parse(legs: usize) -> Result<Self, InvalidPlanCeiling> {
        match bounded(legs, Self::MAX) {
            Ok(legs) => Ok(Self(legs)),
            Err(invalid) => Err(invalid),
        }
    }

    #[inline]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The three ceilings the planner holds a plan to, carried together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanCeilings {
    sources: SourceCeiling,
    lookup_legs: LookupLegCeiling,
    legs: LegCeiling,
}

impl PlanCeilings {
    /// Every default - the behaviour every deployment had before any was configurable.
    pub const DEFAULT: Self = Self {
        sources: SourceCeiling::DEFAULT,
        lookup_legs: LookupLegCeiling::DEFAULT,
        legs: LegCeiling::DEFAULT,
    };

    #[inline]
    pub const fn new(sources: SourceCeiling, lookup_legs: LookupLegCeiling, legs: LegCeiling) -> Self {
        Self {
            sources,
            lookup_legs,
            legs,
        }
    }

    /// The most distinct data systems one answer may read.
    #[inline]
    pub const fn sources(self) -> SourceCeiling {
        self.sources
    }

    /// The most lookup legs one metric's plan may read.
    #[inline]
    pub const fn lookup_legs(self) -> LookupLegCeiling {
        self.lookup_legs
    }

    /// The most legs one answer may run.
    #[inline]
    pub const fn legs(self) -> LegCeiling {
        self.legs
    }
}

/// Every configured ceiling, carried together so a service hands one argument down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceilings {
    top: RowCeiling,
    federated: FederatedRowCeiling,
    plan: PlanCeilings,
}

impl Ceilings {
    /// Every default - the behaviour every deployment had before any was configurable.
    pub const DEFAULT: Self = Self {
        top: RowCeiling::DEFAULT,
        federated: FederatedRowCeiling::DEFAULT,
        plan: PlanCeilings::DEFAULT,
    };

    #[inline]
    pub const fn new(top: RowCeiling, federated: FederatedRowCeiling, plan: PlanCeilings) -> Self {
        Self { top, federated, plan }
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

    /// The ceilings the planner holds a plan to.
    #[inline]
    pub const fn plan(self) -> PlanCeilings {
        self.plan
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

    #[test]
    fn the_plan_defaults_are_the_bounds_before_they_were_settings() {
        assert_eq!(
            [
                PlanCeilings::DEFAULT.sources().get(),
                PlanCeilings::DEFAULT.lookup_legs().get(),
                PlanCeilings::DEFAULT.legs().get()
            ],
            [4, 3, 6]
        );
        assert_eq!(Ceilings::DEFAULT.plan(), PlanCeilings::DEFAULT);
    }

    #[test]
    fn each_plan_ceiling_refuses_zero_and_one_above_its_maximum_and_takes_the_maximum() {
        assert_eq!([SourceCeiling::MAX, LookupLegCeiling::MAX, LegCeiling::MAX], [16, 8, 16]);
        let above = |max| Some(InvalidPlanCeiling::AboveTheMaximum { max });
        assert_eq!(SourceCeiling::parse(0), Err(InvalidPlanCeiling::Zero));
        assert_eq!(SourceCeiling::parse(SourceCeiling::MAX + 1).err(), above(SourceCeiling::MAX));
        assert_eq!(
            SourceCeiling::parse(SourceCeiling::MAX).map(SourceCeiling::get),
            Ok(SourceCeiling::MAX)
        );
        assert_eq!(LookupLegCeiling::parse(0), Err(InvalidPlanCeiling::Zero));
        assert_eq!(
            LookupLegCeiling::parse(LookupLegCeiling::MAX + 1).err(),
            above(LookupLegCeiling::MAX)
        );
        assert_eq!(
            LookupLegCeiling::parse(LookupLegCeiling::MAX).map(LookupLegCeiling::get),
            Ok(LookupLegCeiling::MAX)
        );
        assert_eq!(LegCeiling::parse(0), Err(InvalidPlanCeiling::Zero));
        assert_eq!(LegCeiling::parse(LegCeiling::MAX + 1).err(), above(LegCeiling::MAX));
        assert_eq!(LegCeiling::parse(LegCeiling::MAX).map(LegCeiling::get), Ok(LegCeiling::MAX));
    }
}
