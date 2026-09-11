//! What happens above the legs: re-aggregation, and the division that cannot happen inside one.
//!
//! **This is the seam the federated module is split along, and it is a contract rather than a
//! filing decision.** A leg comes back already aggregated by its own data system, so the answer's
//! number is whatever this module reduces those per-leg rows to, and the divide tree is applied to
//! the result. Everything the rest of the module may reach is [`Leaves`] and [`reaggregates`]:
//! which aggregates have a re-aggregating function, and how a leaf column's numeric type is
//! established, are this module's own and cannot be re-decided at a call site.
//!
//! **Why the two steps are one type.** [`Leaves::of`] and [`Leaves::measure`] have to agree on
//! order: the cursor each [`Above::Total`](crate::federation::Above::Total) node is read with walks
//! the same sequence [`Federation::carried`] collects its leaves in, and it starts at zero. That
//! agreement used to be a sentence beside a `&mut 0` written at the call site - so the cursor is a
//! local of [`Leaves::measure`] now, and a caller can neither start it elsewhere nor point it at a
//! list some other call produced.
//!
//! **The division cannot happen in a leg, which is why it happens here.** The divide tree carries
//! the only [`ZeroDenominator`](crate::measure::ZeroDenominator) in the federated path, and the
//! guard belongs above every leg's rows rather than inside one of them - a guard applied inside a
//! leg is the wrong number this shape exists to prevent.
//!
//! **What this module does not reach.** A measure that does not decompose has no re-aggregating
//! function at all, and [`reaggregates`] is the whole statement of which do; the splitter refuses
//! such a measure before a plan exists, so nothing here has to answer for it.

use std::cmp::Ordering;

use crate::federation::{Above, Federation};
use crate::measure::ZeroDenominator;
use crate::model::{Aggregate, MetricName};
use crate::warehouse::{Real, Value};

use super::FederatedFailure;

/// Whether the combine has a re-aggregating function for `aggregate`.
///
/// The one question the plan's constructor asks of this module, and deliberately the only one: it
/// learns *whether* a carried leaf re-aggregates, never *which* reduction it would get, so the
/// reduction table below stays this module's own. [`FederatedPlan::new`](super::FederatedPlan::new)
/// refuses a leaf this answers `false` for, which is what makes
/// [`FederatedFailure::UnsupportedAggregate`] unreachable through a plan that constructor built.
pub(super) const fn reaggregates(aggregate: Aggregate) -> bool {
    Reduction::of(aggregate).is_some()
}

/// One re-aggregated value per carried leaf, in [`Federation::carried`] order, bound to its tree.
///
/// Non-emptiness is neither claimed nor needed here. What the type does hold is the pairing - these
/// values, this order, this [`Above`] tree, a cursor starting at zero - which is why
/// [`measure`](Leaves::measure) is a method rather than a function taking those parts separately.
pub(super) struct Leaves<'a> {
    above: &'a Above,
    values: Vec<Value>,
}

impl<'a> Leaves<'a> {
    /// Re-aggregates every leaf across one group's rows, one value per leaf, in carried order.
    pub(super) fn of(
        federation: &'a Federation,
        leaf_rows: &[Vec<Value>],
        metric: &MetricName,
    ) -> Result<Self, FederatedFailure> {
        let values = federation
            .carried()
            .iter()
            .enumerate()
            .map(|(column, leaf)| aggregate(leaf.combine(), leaf_rows.iter().filter_map(|row| row.get(column)), metric))
            .collect::<Result<Vec<Value>, FederatedFailure>>()?;
        Ok(Self {
            above: federation.above(),
            values,
        })
    }

    /// The measure: the divide tree above these leaves, applied to them.
    pub(super) fn measure(&self, metric: &MetricName) -> Result<Value, FederatedFailure> {
        apply_above(self.above, &self.values, &mut 0, metric)
    }
}

/// Re-aggregates one leaf's already-aggregated values across a group.
///
/// **The only aggregates that arrive here are the ones a decomposable measure re-aggregates with.** A
/// `Count` leaf re-aggregates with a sum and is itself an integer; the splitter refuses a `Carried::Keys`
/// leaf entirely, so `combine` is a total, minimum or maximum over a list of numbers. A group with no
/// non-null value contributes null; a cell that is not a number, a column that is not one kind of
/// number, an overflow, or a non-finite total is a refusal, never a silent zero or null.
///
/// **The column is read before the aggregate is applied**, so each reduction below sees one numeric
/// type - see [`LeafColumn`], which is where that decision and its reason live.
fn aggregate<'a>(
    aggregate: Aggregate,
    values: impl Iterator<Item = &'a Value>,
    metric: &MetricName,
) -> Result<Value, FederatedFailure> {
    let reduction = Reduction::of(aggregate).ok_or(FederatedFailure::UnsupportedAggregate { aggregate })?;
    let Some(column) = LeafColumn::parse(values, aggregate)? else {
        return Ok(Value::Null);
    };
    match reduction {
        Reduction::Total => column.total(metric),
        Reduction::Least => Ok(column.extreme(Ordering::Less)),
        Reduction::Greatest => Ok(column.extreme(Ordering::Greater)),
    }
}

/// What a leaf column is reduced to, named rather than left as the aggregate it came from.
///
/// [`Reduction::of`] is the one definition of which aggregates the combine re-aggregates with, and
/// [`FederatedPlan::new`](super::FederatedPlan::new) is where a leaf naming any other one is refused,
/// so which reduction a
/// column gets is settled by the plan, before a single cell of it is read.
#[derive(Clone, Copy)]
enum Reduction {
    /// [`Aggregate::Sum`], which a `Count` leaf also re-aggregates with.
    Total,
    /// [`Aggregate::Min`].
    Least,
    /// [`Aggregate::Max`].
    Greatest,
}

impl Reduction {
    /// The reduction an aggregate re-aggregates with, or `None` for one that has none.
    ///
    /// Named arms rather than a wildcard, so a seventh [`Aggregate`] has to answer here.
    const fn of(aggregate: Aggregate) -> Option<Self> {
        match aggregate {
            Aggregate::Sum => Some(Self::Total),
            Aggregate::Min => Some(Self::Least),
            Aggregate::Max => Some(Self::Greatest),
            Aggregate::Count | Aggregate::Avg | Aggregate::CountDistinct => None,
        }
    }
}

/// One leaf column's non-null cells, once their single numeric type is established.
///
/// **Reading the whole column before any aggregate touches it is what makes the arithmetic exact
/// rather than checked**, and it removes two wrong numbers at once: `Sum` accumulated an integer
/// subtotal and a real one and returned only the real one, and `Min`/`Max` compared every cell as an
/// `f64`, so two integers a data system tells apart read as equal above `2^53` and the answer was
/// whichever arrived first. Now each aggregate sees one type and nothing widens an `i64` to add it or
/// to compare it.
///
/// A cell that is no kind of number is refused for **every** aggregate rather than inside two of
/// them: a lone `Text` cell used to be accepted as its own minimum without being read as a number,
/// and `DuckDB` returns a `DECIMAL` money column as one. A column carrying both numeric types is
/// [`FederatedFailure::MixedNumericLeaf`], which carries that reasoning.
///
/// Both variants are non-empty by construction: [`parse`](LeafColumn::parse) answers `None` for a
/// column with no non-null cell, because a group contributing nothing is a null and not a zero.
enum LeafColumn {
    /// Every non-null cell was a [`Value::Integer`].
    Integers(Vec<i64>),
    /// Every non-null cell was a [`Value::Real`], and so is already finite.
    Reals(Vec<Real>),
}

impl LeafColumn {
    /// One leaf column's cells, `None` for a column of nulls, or the reason it is neither.
    fn parse<'a>(values: impl Iterator<Item = &'a Value>, aggregate: Aggregate) -> Result<Option<Self>, FederatedFailure> {
        let mut integers: Vec<i64> = Vec::new();
        let mut reals: Vec<Real> = Vec::new();
        for value in values {
            match *value {
                Value::Null => {}
                Value::Integer(cell) => integers.push(cell),
                Value::Real(cell) => reals.push(cell),
                Value::Text(_) => {
                    return Err(FederatedFailure::NonNumericLeaf {
                        aggregate,
                        value: value.clone(),
                    });
                }
            }
        }
        match (integers.is_empty(), reals.is_empty()) {
            (false, true) => Ok(Some(Self::Integers(integers))),
            (true, false) => Ok(Some(Self::Reals(reals))),
            (true, true) => Ok(None),
            (false, false) => Err(FederatedFailure::MixedNumericLeaf { aggregate }),
        }
    }

    /// The column's total, in the column's own type.
    ///
    /// An integer column totals as `i64` and overflow is a refusal; a real column totals as `f64`,
    /// which is the float addition the mono path's own `SUM` performs, and a total that leaves the
    /// finite range is a refusal because [`Real`] cannot hold it.
    #[expect(
        clippy::float_arithmetic,
        reason = "the re-aggregation of a real-valued leg column sums real numbers by design"
    )]
    fn total(&self, metric: &MetricName) -> Result<Value, FederatedFailure> {
        match *self {
            Self::Integers(ref cells) => cells
                .iter()
                .try_fold(0_i64, |total, cell| total.checked_add(*cell))
                .map(Value::Integer)
                .ok_or(FederatedFailure::Overflow {
                    aggregate: Aggregate::Sum,
                }),
            Self::Reals(ref cells) => Real::parse(cells.iter().fold(0.0_f64, |total, cell| total + cell.get()))
                .map(Value::Real)
                .map_err(|_not_finite| FederatedFailure::NonFinite { metric: metric.clone() }),
        }
    }

    /// The column's least or greatest cell, in the column's own type.
    ///
    /// `wanted` is the ordering a cell must have against the incumbent to replace it: [`Ordering::Less`]
    /// for a minimum, [`Ordering::Greater`] for a maximum. Both comparisons are exact - an `i64` against
    /// an `i64`, and `total_cmp` over reals that [`Real`] has already established are finite.
    ///
    /// `reduce` answers `None` only for an empty column, which [`parse`](LeafColumn::parse) answers
    /// `None` for instead - so the `Value::Null` below is unreachable rather than a case.
    fn extreme(&self, wanted: Ordering) -> Value {
        match *self {
            Self::Integers(ref cells) => cells
                .iter()
                .copied()
                .reduce(|best, cell| if cell.cmp(&best) == wanted { cell } else { best })
                .map_or(Value::Null, Value::Integer),
            Self::Reals(ref cells) => cells
                .iter()
                .copied()
                .reduce(|best, cell| {
                    if cell.get().total_cmp(&best.get()) == wanted {
                        cell
                    } else {
                        best
                    }
                })
                .map_or(Value::Null, Value::Real),
        }
    }
}

/// Applies the divide tree above a group's re-aggregated leaves, returning the measure.
///
/// `cursor` walks the tree in the same order [`Federation::carried`] collects its leaves, so each
/// [`Above::Total`] node reads the leaf [`Leaves::of`] aggregated for it. [`Leaves::measure`] is the
/// only caller, so the cursor it starts is always at zero and always over that same list.
fn apply_above(above: &Above, aggregated: &[Value], cursor: &mut usize, metric: &MetricName) -> Result<Value, FederatedFailure> {
    match *above {
        Above::Total(_) => {
            let value = aggregated.get(*cursor).cloned();
            *cursor = cursor.saturating_add(1);
            Ok(value.unwrap_or(Value::Null))
        }
        Above::Quotient {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            let numerator = apply_above(numerator, aggregated, cursor, metric)?;
            let denominator = apply_above(denominator, aggregated, cursor, metric)?;
            divide(&numerator, &denominator, zero_denominator, metric)
        }
    }
}

/// One division, with the guard the definition asked for applied to the final denominator.
#[expect(clippy::float_arithmetic, reason = "a division of leg totals is float arithmetic by design")]
fn divide(
    numerator: &Value,
    denominator: &Value,
    zero_denominator: ZeroDenominator,
    metric: &MetricName,
) -> Result<Value, FederatedFailure> {
    let Some(num) = to_f64(numerator) else {
        return Ok(Value::Null);
    };
    let Some(den) = to_f64(denominator) else {
        return Ok(Value::Null);
    };
    if den == 0.0 {
        return match zero_denominator {
            ZeroDenominator::Null => Ok(Value::Null),
            ZeroDenominator::Fail => Err(FederatedFailure::NonFinite { metric: metric.clone() }),
        };
    }
    let real = Real::parse(num / den).map_err(|_not_finite| FederatedFailure::NonFinite { metric: metric.clone() })?;
    Ok(Value::Real(real))
}

/// A numeric cell as `f64`, or `None` for a cell no ratio can be taken over.
///
/// `#[expect]`-bounded: casting a wide integer to `f64` loses precision above `2^53`, which is
/// accepted **here and only here** because a ratio over leg totals is inherently floating-point and
/// [`divide`] is the one caller. It is not accepted for a total or a comparison - see
/// [`FederatedFailure::MixedNumericLeaf`] for the widening this path refuses instead.
///
/// Every variant is named rather than left to a wildcard, so a fifth [`Value`] has to answer here.
/// [`Value::Text`] is one of the two `None`s and is unreachable through [`apply_above`]: every value
/// it reads came from [`aggregate`], which refuses a text cell as [`FederatedFailure::NonNumericLeaf`].
#[expect(clippy::cast_precision_loss, reason = "a division reads leg totals as f64 by design")]
const fn to_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(cell) => Some(*cell as f64),
        Value::Real(cell) => Some(cell.get()),
        Value::Null | Value::Text(_) => None,
    }
}
