//! The lookup legs of a federated plan: one per crossing relationship, at most
//! [`MAX_LOOKUP_LEGS`], each naming the leg it joins onto.

use crate::plan::leg::LegPlan;
use crate::query::MAX_LOOKUP_LEGS;

/// One leg of a federated plan, named by its place: the fact leg, or a lookup leg by its position
/// in [`LookupLegs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum LegAt {
    /// The metric's own leg.
    Fact,
    /// The lookup leg at this position.
    Lookup(usize),
}

/// A lookup leg, the leg it joins onto, and whether a row it does not match survives.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LookupLeg {
    leg: LegPlan,
    onto: LegAt,
    include_unmatched: bool,
}

impl LookupLeg {
    /// A lookup leg that joins onto the fact leg.
    #[inline]
    #[must_use]
    pub const fn onto_fact(leg: LegPlan, include_unmatched: bool) -> Self {
        Self {
            leg,
            onto: LegAt::Fact,
            include_unmatched,
        }
    }

    /// A lookup leg that joins onto the lookup leg at `earlier`, which [`LookupLegs::parse`]
    /// requires to come before it.
    #[inline]
    #[must_use]
    pub const fn onto_lookup(leg: LegPlan, earlier: usize, include_unmatched: bool) -> Self {
        Self {
            leg,
            onto: LegAt::Lookup(earlier),
            include_unmatched,
        }
    }

    /// The leg this one reads.
    #[inline]
    #[must_use]
    pub const fn leg(&self) -> &LegPlan {
        &self.leg
    }

    /// The leg this one joins onto.
    #[inline]
    #[must_use]
    pub const fn onto(&self) -> LegAt {
        self.onto
    }

    /// Whether a row of the leg it joins onto that this leg does not match survives, with null keys
    /// from this leg: LEFT when `true`, INNER when `false`. `docs/adr/0009` decides the direction,
    /// once for each lookup leg.
    #[inline]
    #[must_use]
    pub const fn include_unmatched(&self) -> bool {
        self.include_unmatched
    }
}

/// One to [`MAX_LOOKUP_LEGS`] lookup legs, each joined onto the fact leg or an earlier lookup leg.
///
/// A leg can name only an earlier one, so the legs form a tree rooted at the fact leg and a cycle
/// cannot be written. A plan with no lookup leg or with more than the cap does not exist.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LookupLegs(Vec<LookupLeg>);

impl LookupLegs {
    /// The legs, in the order each may be joined onto by a later one.
    ///
    /// # Errors
    ///
    /// [`LookupLegsError`] for none, for more than [`MAX_LOOKUP_LEGS`], and for a leg that joins
    /// onto itself or a later leg.
    pub fn parse(legs: Vec<LookupLeg>) -> Result<Self, LookupLegsError> {
        if legs.is_empty() || legs.len() > MAX_LOOKUP_LEGS {
            return Err(LookupLegsError::Count { legs: legs.len() });
        }
        if let Some(leg) = legs
            .iter()
            .enumerate()
            .find_map(|(at, leg)| matches!(leg.onto, LegAt::Lookup(onto) if onto >= at).then_some(at))
        {
            return Err(LookupLegsError::NotOntoAnEarlierLeg { leg });
        }
        Ok(Self(legs))
    }

    /// One lookup leg, joined onto the fact leg.
    #[inline]
    #[must_use]
    pub fn one(leg: LegPlan, include_unmatched: bool) -> Self {
        Self(vec![LookupLeg::onto_fact(leg, include_unmatched)])
    }

    /// Every lookup leg, in plan order.
    #[inline]
    #[must_use]
    pub fn legs(&self) -> &[LookupLeg] {
        &self.0
    }
}

/// Why a list of lookup legs is not one a plan can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LookupLegsError {
    /// None, or more than the cap.
    #[error("a federated plan reads one to {MAX_LOOKUP_LEGS} lookup legs, not {legs}")]
    Count { legs: usize },
    /// The leg at `leg` joins onto itself or a leg after it.
    #[error("lookup leg {leg} joins onto a leg that does not come before it")]
    NotOntoAnEarlierLeg { leg: usize },
}

#[cfg(test)]
mod tests {
    use super::{LegAt, LookupLeg, LookupLegs, LookupLegsError, MAX_LOOKUP_LEGS};
    use crate::model::{SourceName, TableName};
    use crate::plan::PlanBindings;
    use crate::plan::leg::LegPlan;

    /// Inline rather than in a `lookup/tests.rs`: this file is new, so the causality gate's base
    /// tree loses the `mod lookup;` that declares it.
    fn leg() -> LegPlan {
        LegPlan::Lookup {
            source: SourceName::parse("geo").expect("a test source is a source"),
            metric: crate::model::MetricName::parse("revenue").expect("a test metric is a metric"),
            table: TableName::parse("dim_region").expect("a test table is a table").into(),
            keys: Vec::new(),
            bindings: PlanBindings::none(),
        }
    }

    #[test]
    fn lookup_legs_are_one_to_the_cap_and_name_only_an_earlier_parent() {
        let onto_fact = || LookupLeg::onto_fact(leg(), true);
        assert_eq!(MAX_LOOKUP_LEGS, 8, "the plan type carries eight lookup legs");
        assert_eq!(LookupLegs::parse(Vec::new()), Err(LookupLegsError::Count { legs: 0 }));
        let at_cap =
            LookupLegs::parse(std::iter::repeat_with(onto_fact).take(MAX_LOOKUP_LEGS).collect()).expect("the cap is carried");
        assert_eq!(at_cap.legs().len(), MAX_LOOKUP_LEGS);
        assert_eq!(
            LookupLegs::parse(std::iter::repeat_with(onto_fact).take(MAX_LOOKUP_LEGS + 1).collect()),
            Err(LookupLegsError::Count {
                legs: MAX_LOOKUP_LEGS + 1
            })
        );
        let chain = LookupLegs::parse(vec![onto_fact(), LookupLeg::onto_lookup(leg(), 0, false), onto_fact()])
            .expect("a leg onto an earlier leg is a tree");
        assert_eq!(
            chain.legs().iter().map(LookupLeg::onto).collect::<Vec<_>>(),
            [LegAt::Fact, LegAt::Lookup(0), LegAt::Fact]
        );
        assert_eq!(
            LookupLegs::parse(vec![LookupLeg::onto_lookup(leg(), 0, true)]),
            Err(LookupLegsError::NotOntoAnEarlierLeg { leg: 0 })
        );
        assert_eq!(
            LookupLegs::parse(vec![onto_fact(), LookupLeg::onto_lookup(leg(), 2, true), onto_fact()]),
            Err(LookupLegsError::NotOntoAnEarlierLeg { leg: 1 })
        );
    }
}
