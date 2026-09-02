//! How a measure federates: what descends into a leg, and the one computation that happens above
//! them.
//!
//! **This module is a classification and a rule, and nothing executes it.** There is no leg plan
//! type, no splitter and no combiner in this workspace yet, so nothing here has a production
//! caller: the same shape `.agents/skills/sutura/query-surface`'s built-and-not-wired inventory describes for the
//! authored-SQL hatch. It is stated here rather than left for a reader to discover, because a
//! classification that looks wired is worse than one that says it is not.
//!
//! **The problem it answers.** Grouping a fact leg by a remote join key is a strictly finer grouping
//! than the answer, so a combine above the legs has to aggregate again - and whether that is correct
//! depends entirely on the aggregate. `AVG` of `AVG`s is not the average, and two exact distinct
//! counts added together over-count every key the two legs share. Neither of those raises an error
//! anywhere: they are wrong numbers under a certified metric name, which is the failure mode this
//! repository exists to prevent.
//! `docs/adr/0007-federating-across-different-data-systems.md` is the finding and
//! `docs/adr/0009-the-plan-from-one-source-to-many.md` Decision 2 is the decision.
//!
//! **A measure that does not descend is not a refusal.** Decision 2 is explicit: where an aggregate
//! cannot be computed per leg and re-aggregated, the leg carries finer-grained rows and the
//! aggregate happens above, paying the processing cost. So there is no error type in this module and
//! no [`RefusalReason`](crate::query::RefusalReason) variant behind it - [`Descent`] is total over
//! the vocabulary, and its third variant is a plan rather than a decline.
//!
//! **Three exhaustive matches, and each of them is the mechanism.** [`Descent::of`] matches
//! [`Aggregate`], so a seventh aggregate cannot compile without stating which of the three classes
//! it is in; [`descend`] matches [`Term`], so a third term has to say the same thing; and
//! [`Federation::of`] matches [`Measure`], so a third shape does too. A `match` that has to gain an
//! arm is the whole content of this branch.
//!
//! **The division cannot happen in a leg, and that is a shape rather than a check.** The vocabulary
//! already splits into a [`Term`] - one number - and a [`Measure`] - one term or a ratio of two.
//! What a leg carries is a [`Carried`], and a `Carried` is built out of the *term* level: it has no
//! variant that divides and no field a [`ZeroDenominator`] fits in, at any depth. The only
//! [`ZeroDenominator`] in this module is on [`Above::Quotient`], which is the node above every leg.
//! The bug that closes is specific: `ZeroDenominator::Null` renders as `NULLIF(d, 0)`, so applied
//! *inside* a leg a subgroup with a zero denominator becomes null, the `SUM` above skips nulls, and
//! that subgroup's numerator is silently dropped from the answer instead of nulling it.
//!
//! What is deliberately NOT here: the join kind. Decision 2's other half - INNER for a remote
//! dimension carrying a filter, LEFT for one that does not - is a function of where the filters
//! went, which only a splitter can know. It belongs to the branch that builds one.

use crate::measure::{Measure, Term, ZeroDenominator};
use crate::model::{Aggregate, ColumnName};

/// An aggregate that descends as written, paired with the function that re-aggregates it above.
///
/// **Two aggregates rather than one, because they are not always the same one.** A `Count` pushed
/// into a leg is re-aggregated above with a `Sum`: adding the leg counts is the count, and counting
/// them again counts legs. That is the single most repeated arithmetic mistake in a hand-written
/// combine, and recording both halves is what stops it being restated at each call site.
///
/// **The fields are private and there is no public constructor**, so the only values of this type
/// are the ones [`Descent::of`] returns. That is what makes [`Carried::Aggregated`] unable to *name*
/// an aggregate the classification did not call pushable: `Avg` and `CountDistinct` have no
/// `Pushed` anywhere, so no caller can write one. **The limit, stated with the claim:** the
/// guarantee is module-scoped, since code in this file can write the struct literal - which is
/// exactly where the classification lives, and nowhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Pushed {
    push: Aggregate,
    combine: Aggregate,
}

impl Pushed {
    /// The aggregate the leg computes.
    #[inline]
    pub const fn push(self) -> Aggregate {
        self.push
    }

    /// The aggregate that re-aggregates the leg's column above.
    #[inline]
    pub const fn combine(self) -> Aggregate {
        self.combine
    }
}

/// An aggregate that does not descend at all, and runs above the legs on the rows they carried.
///
/// Private field and no public constructor, for [`Pushed`]'s reason: a [`Carried::Keys`] can only
/// name an aggregate [`Descent::of`] classified as non-descending, and it carries *which* one rather
/// than assuming `CountDistinct` is the only one it will ever be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Pulled {
    above: Aggregate,
}

impl Pulled {
    /// The aggregate the combine applies to the pulled-up rows.
    #[inline]
    pub const fn above(self) -> Aggregate {
        self.above
    }
}

/// How one aggregate of the closed vocabulary descends into a leg.
///
/// **Three variants, because there are three answers and not two.** An earlier version of the plan
/// named `combine_with() -> Option<Aggregate>`, and `Option` cannot say the third one: descends as
/// itself, descends as two columns, does not descend and travels as a grouping key. The compile
/// error a seventh aggregate produces is therefore *you have not said which of the three you are*
/// rather than *you have not said whether you can*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Descent {
    /// Pushable as written: one column in the leg, one function above it.
    AsWritten(Pushed),
    /// Pushable decomposed: two columns in the leg, divided once above.
    ///
    /// `Avg` is the only one today, and it travels as a `Sum` and a `Count`. The zero handling is
    /// part of the classification rather than of whoever builds the tree, because it is what makes
    /// the decomposition faithful: an `AVG` over no rows is null, so the division above a leg whose
    /// count is zero has to be null too.
    Decomposed {
        numerator: Pushed,
        denominator: Pushed,
        zero_denominator: ZeroDenominator,
    },
    /// Not pushable. The column travels as a grouping key and the aggregate runs above.
    ///
    /// `CountDistinct` is the only one, and it is the expensive kind: the leg carries one row per
    /// distinct key rather than one row per group, because two join keys can share a value and no
    /// re-aggregating function repairs a sum of exact distinct counts.
    AsGroupingKey(Pulled),
}

impl Descent {
    /// The total function from an aggregate to how it federates.
    ///
    /// Exhaustive over [`Aggregate`] by construction: this `match` is the mechanism the whole
    /// branch exists for, and a new variant of that closed enum does not compile until it appears
    /// here.
    #[inline]
    pub const fn of(aggregate: Aggregate) -> Self {
        match aggregate {
            // Sums add. Min and Max are their own re-aggregation.
            Aggregate::Sum => Self::AsWritten(Pushed {
                push: Aggregate::Sum,
                combine: Aggregate::Sum,
            }),
            // Not a `Count` above: adding leg counts is the count, counting them counts legs.
            Aggregate::Count => Self::AsWritten(Pushed {
                push: Aggregate::Count,
                combine: Aggregate::Sum,
            }),
            Aggregate::Min => Self::AsWritten(Pushed {
                push: Aggregate::Min,
                combine: Aggregate::Min,
            }),
            Aggregate::Max => Self::AsWritten(Pushed {
                push: Aggregate::Max,
                combine: Aggregate::Max,
            }),
            // The mean of means is not the mean, so the two halves travel and the division waits.
            Aggregate::Avg => Self::Decomposed {
                numerator: Pushed {
                    push: Aggregate::Sum,
                    combine: Aggregate::Sum,
                },
                denominator: Pushed {
                    push: Aggregate::Count,
                    combine: Aggregate::Sum,
                },
                zero_denominator: ZeroDenominator::Null,
            },
            // The exact answer needs the distinct keys themselves.
            Aggregate::CountDistinct => Self::AsGroupingKey(Pulled {
                above: Aggregate::CountDistinct,
            }),
        }
    }
}

/// What one leg carries for one number the answer needs.
///
/// **The type that cannot divide.** There is no `Quotient` variant here and no [`ZeroDenominator`]
/// reachable from one: a `Carried` is a [`Pushed`] or a [`Pulled`] over a [`ColumnName`], and none
/// of those three can hold one. That is the ratio rule as a shape rather than as a rule somebody
/// remembers - see this module's header for the wrong number it prevents.
///
/// The division that a leg cannot express does not compile:
///
/// ```compile_fail
/// use sutura_domain::federation::Carried;
/// use sutura_domain::measure::ZeroDenominator;
///
/// // There is no variant of `Carried` that divides, so this names nothing.
/// fn _per_leg(numerator: Carried, denominator: Carried, zero_denominator: ZeroDenominator) -> Carried {
///     Carried::Quotient { numerator, denominator, zero_denominator }
/// }
/// ```
///
/// Nor does the zero guard, which is the half that silently drops a subgroup when it is applied
/// inside a leg:
///
/// ```compile_fail
/// use sutura_domain::federation::{Carried, Pushed};
/// use sutura_domain::measure::ZeroDenominator;
/// use sutura_domain::model::ColumnName;
///
/// fn _guarded(pushed: Pushed, column: ColumnName, zero_denominator: ZeroDenominator) -> Carried {
///     Carried::Aggregated { pushed, column, zero_denominator }
/// }
/// ```
///
/// And the twin, so a rename cannot make either block pass vacuously: the division exists, one level
/// up, where every leg is already below it.
///
/// ```
/// use sutura_domain::federation::{Above, Carried};
/// use sutura_domain::measure::ZeroDenominator;
///
/// fn _above(numerator: Carried, denominator: Carried, zero_denominator: ZeroDenominator) -> Above {
///     Above::Quotient {
///         numerator: Box::new(Above::Total(numerator)),
///         denominator: Box::new(Above::Total(denominator)),
///         zero_denominator,
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Carried {
    /// One aggregate the leg computes and hands up as one column.
    Aggregated { pushed: Pushed, column: ColumnName },
    /// A conditional count the leg computes and hands up as one column.
    ///
    /// Its own variant rather than an [`Aggregated`](Self::Aggregated) with a `Count`, for the
    /// reason [`Term::CountIf`] is its own term: `COUNT(col)` counts non-null rows and would count
    /// the `false` ones too.
    CountIf { column: ColumnName },
    /// No aggregate in the leg at all: the column travels as a grouping key, and the aggregate
    /// [`Pulled`] names runs above the rows it carried.
    Keys { pulled: Pulled, column: ColumnName },
}

impl Carried {
    /// The column this leg reads.
    #[inline]
    pub const fn column(&self) -> &ColumnName {
        match *self {
            Self::Aggregated { ref column, .. } | Self::CountIf { ref column } | Self::Keys { ref column, .. } => column,
        }
    }

    /// The aggregate the combine applies to what this leg carried.
    ///
    /// One place, so no combiner has to restate it - and so `Count` pushed down, `Sum` above stays
    /// one decision rather than one per call site.
    #[inline]
    pub const fn combine(&self) -> Aggregate {
        match *self {
            Self::Aggregated { pushed, .. } => pushed.combine(),
            // A conditional count is a count: the leg counts and the combine adds.
            Self::CountIf { .. } => Aggregate::Sum,
            Self::Keys { pulled, .. } => pulled.above(),
        }
    }

    /// Does this leg carry rows at the fact grain rather than one row per group?
    ///
    /// The price of the pull-up, and the quantity worth logging: it is the difference between a leg
    /// returning one row per group and one row per distinct key.
    #[inline]
    pub const fn is_pulled_up(&self) -> bool {
        matches!(*self, Self::Keys { .. })
    }
}

/// The one computation above the legs.
///
/// **A tree rather than a pair, and the reason is a nested case that is representable today**:
/// `ratio(avg(x), count(y))` has an `Avg` numerator, and an `Avg` is itself a division, so the
/// above-step nests. Depth is bounded at two by the vocabulary - a [`Measure`] is at most a ratio of
/// terms, and a term contributes at most one division - so the [`Box`] is indirection for a
/// recursive type rather than an unbounded structure.
///
/// Every division in this workspace's federated path is one of these nodes. That is the property:
/// the numbers a leg produces are re-aggregated, and only then divided.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum Above {
    /// One column the legs carried, re-aggregated by [`Carried::combine`].
    Total(Carried),
    /// Two of those, divided once - above every leg, with the zero handling the definition asked
    /// for applied to the final denominator rather than to a leg's.
    Quotient {
        numerator: Box<Self>,
        denominator: Box<Self>,
        zero_denominator: ZeroDenominator,
    },
}

/// How a measure federates: the whole answer for one measure.
///
/// Produced by [`Federation::of`], which is total: every measure the closed vocabulary can express
/// federates, and the ones that cannot descend pull rows up instead of being declined.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Federation {
    above: Above,
}

impl Federation {
    /// Classifies one measure.
    ///
    /// Exhaustive over [`Measure`]'s shapes, and over [`Term`]'s terms through [`descend`]. A ratio
    /// becomes an [`Above::Quotient`] whose halves are pushed separately, which is the rule stated
    /// as the only tree this function can build.
    pub fn of(measure: &Measure) -> Self {
        let above = match *measure {
            Measure::Simple(ref term) => descend(term),
            Measure::Ratio {
                ref numerator,
                ref denominator,
                zero_denominator,
            } => Above::Quotient {
                numerator: Box::new(descend(numerator)),
                denominator: Box::new(descend(denominator)),
                zero_denominator,
            },
        };
        Self { above }
    }

    /// What happens once, above the legs.
    #[inline]
    pub const fn above(&self) -> &Above {
        &self.above
    }

    /// What every leg carries, in the order a reader would say the measure.
    ///
    /// The leaves of [`Above`], collected rather than stored twice: a second copy is a second thing
    /// to keep in step with the tree.
    pub fn carried(&self) -> Vec<&Carried> {
        let mut out = Vec::new();
        leaves(&self.above, &mut out);
        out
    }

    /// Does answering this measure need rows at the fact grain?
    ///
    /// True for anything reaching a [`Descent::AsGroupingKey`], which is `CountDistinct` today. This
    /// is the cost Decision 2 accepts rather than refuses, so it is a quantity to report and not a
    /// condition to fail on.
    pub fn pulls_up_rows(&self) -> bool {
        self.carried().iter().any(|carried| carried.is_pulled_up())
    }
}

/// How one term descends.
///
/// Exhaustive over [`Term`], which is the second of this module's three matches: a third term has to
/// say how it federates before this compiles.
pub fn descend(term: &Term) -> Above {
    match *term {
        // `COUNTIF` in one dialect, `SUM(CASE WHEN ..)` in another, and a count either way: it
        // descends as written and the combine adds the leg counts.
        Term::CountIf { ref column } => Above::Total(Carried::CountIf { column: column.clone() }),
        Term::Aggregate(ref inner) => match Descent::of(inner.aggregate()) {
            Descent::AsWritten(pushed) => Above::Total(Carried::Aggregated {
                pushed,
                column: inner.column().clone(),
            }),
            // Two columns off one term, and the division that turns them back into an average
            // happens here - above every leg, exactly once.
            Descent::Decomposed {
                numerator,
                denominator,
                zero_denominator,
            } => Above::Quotient {
                numerator: Box::new(Above::Total(Carried::Aggregated {
                    pushed: numerator,
                    column: inner.column().clone(),
                })),
                denominator: Box::new(Above::Total(Carried::Aggregated {
                    pushed: denominator,
                    column: inner.column().clone(),
                })),
                zero_denominator,
            },
            Descent::AsGroupingKey(pulled) => Above::Total(Carried::Keys {
                pulled,
                column: inner.column().clone(),
            }),
        },
    }
}

/// Collects an above-tree's leaves left to right.
fn leaves<'a>(above: &'a Above, out: &mut Vec<&'a Carried>) {
    match *above {
        Above::Total(ref carried) => out.push(carried),
        Above::Quotient {
            ref numerator,
            ref denominator,
            ..
        } => {
            leaves(numerator, out);
            leaves(denominator, out);
        }
    }
}

#[cfg(test)]
mod tests;
