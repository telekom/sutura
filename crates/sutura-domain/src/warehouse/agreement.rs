//! What it takes for two answers to one plan to be the SAME answer.
//!
//! One policy, so the differential legs cannot each have their own. Two of them did, and both were
//! wrong the same way: each compared cells through [`Value::render`], which is a *display* form and
//! therefore erases the variant. [`Value::Null`] and `Value::Text("null")` both render `null`, and
//! `Value::Integer(1)` and `Value::Text("1")` both render `1` - so a data system that answered a
//! cell as text where the engine answered it as a number, or as the word `null` where the engine
//! answered nothing at all, compared EQUAL. The live acceptance comparison inherited that, which is
//! why this is one shared module rather than a note on each copy.
//!
//! [`Value::render`] is not at fault and is not changed: an anchor is a certified NUMBER compared as
//! text on purpose, and one canonical display form is what keeps that comparison the same in every
//! place it is made. What was wrong is using a display form as an equality.
//!
//! # The four properties, each of which can be lost on its own
//!
//! - **The variant is part of the value.** The comparison key is per variant, so the two pairs above
//!   are disagreements.
//! - **Multiplicity survives.** Content is a MULTISET, not a set: a row answered twice by one side
//!   and once by the other is a disagreement, which a set cannot see.
//! - **The one approximation is named and scoped.** [`RealTolerance`] applies to [`Value::Real`] and
//!   to nothing else. An integer, a date-as-text and a text cell are compared exactly.
//! - **Order is a separate assertion.** [`agree_on_content`] says nothing about order and
//!   [`agree_on_order`] says nothing else, so *a wrong number* and *the right rows in the wrong
//!   order* stay two diagnoses rather than one. Call the content one first, or the first symptom of
//!   a wrong number is reported as a sort order.
//!
//! # What this does NOT decide
//!
//! Whether an order was promised at all. A plan that emits `ORDER BY` claims one, and a leg
//! comparing an answer to a plan without one should not call [`agree_on_order`]. Nothing here can
//! tell, because a [`RowSet`] does not carry the plan that produced it.
//!
//! # Two limits worth reading before citing this
//!
//! **A disagreement carries rows, which is the one place this module departs from the rule that an
//! error carries nothing sensitive.** A differential failure that does not name the row it found is
//! unusable, and the alternative - a boolean plus a hand-written diff at every call site - is the
//! duplication this module exists to remove. It is a deliberate exception with a narrow blast
//! radius: nothing on a serving path constructs one of these types.
//!
//! **That narrowness rests on the feature gate and on nothing stronger.** The module compiles only
//! under `cfg(test)` or the default-off `agreement` feature, so no shipped artefact contains it -
//! but `checks.shipped-features` establishes that kind of claim by reading crate NAMES out of the
//! binary, and this feature adds no crate. Cargo's own resolution is the mechanism; no gate would
//! fail if a composition root turned the feature on.

use std::collections::BTreeMap;

use crate::warehouse::{RowSet, Value};

/// How two [`Value::Real`] cells are compared, and the ONLY approximation in this module.
///
/// A type rather than a bare number, so a call site states the approximation it accepts. One
/// reviewed constant rather than a `parse`, so no call site can quietly choose a looser one: a
/// second legitimate tolerance is a second constant here, beside its own reason, in front of a
/// reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealTolerance {
    digits: usize,
}

impl RealTolerance {
    /// What the differential legs compare a float at: twelve digits after the point in exponential
    /// form, so thirteen significant digits.
    ///
    /// **Not a loosening, and the reason is what keeps it off every other variant.** Summing the
    /// same rows in a different order changes the last place of an `f64`, so comparing the full
    /// binary expansion asserts that both sides summed in the same order - which is not a property
    /// either side promises, and not what a differential test is for. It has fired for real on the
    /// example corpus. Twelve digits is far beyond any figure a metric reports and far short of the
    /// noise.
    pub const DIFFERENTIAL: Self = Self { digits: 12 };
}

/// One cell, as a comparable key that KEEPS ITS VARIANT.
///
/// The whole point of the module, in one type: two cells are the same cell when they are the same
/// variant carrying the same content, so no rendering can make a null equal the word `null`.
/// Private, because it is how the comparison is made rather than something a caller states - a
/// disagreement is reported as the [`Value`] it found.
///
/// [`Value::Real`] is the one variant that becomes text, at [`RealTolerance`]'s precision.
/// Quantising it is what makes approximate equality an equivalence relation and therefore usable as
/// a map key. Comparing floats pairwise is not one, and a multiset comparison needs one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Cell {
    Null,
    Integer(i64),
    Real(String),
    Text(String),
}

/// One cell, keyed.
fn cell(value: &Value, real: RealTolerance) -> Cell {
    match *value {
        Value::Null => Cell::Null,
        Value::Integer(v) => Cell::Integer(v),
        Value::Real(v) => Cell::Real(format!("{:.*e}", real.digits, v)),
        Value::Text(ref v) => Cell::Text(v.clone()),
    }
}

/// How often a row was answered, and one of the rows that answered it.
///
/// The example is kept so a disagreement can be reported as the [`Value`]s a reader recognises
/// rather than as the key the comparison was made on.
struct Occurrences {
    count: usize,
    example: Vec<Value>,
}

/// Every distinct row, and how many times it appears.
fn tally(rows: &RowSet, real: RealTolerance) -> BTreeMap<Vec<Cell>, Occurrences> {
    let mut seen: BTreeMap<Vec<Cell>, Occurrences> = BTreeMap::new();
    for row in rows.rows() {
        let key: Vec<Cell> = row.iter().map(|value| cell(value, real)).collect();
        seen.entry(key)
            .and_modify(|found| found.count = found.count.saturating_add(1))
            .or_insert_with(|| Occurrences {
                count: 1,
                example: row.clone(),
            });
    }
    seen
}

/// Why two answers do not carry the same content.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ContentDisagreement {
    /// The two sides labelled the result differently.
    ///
    /// Checked first, because everything below is per column and a differing label means the two
    /// sides are not answering about the same columns at all.
    #[error("one side labelled the result {left:?} and the other {right:?}")]
    Columns { left: Vec<String>, right: Vec<String> },
    /// One side answered a row a different number of times than the other.
    ///
    /// A `0` on either side is the ordinary case of a row one side did not answer at all. One
    /// variant covers both, because *this row appeared twice here and once there* is the same
    /// defect and a second variant for it would report the same thing two ways.
    #[error("one side answered a row {left} time(s) and the other {right} time(s): {row:?}")]
    Multiplicity { row: Vec<Value>, left: usize, right: usize },
}

/// Why two answers do not carry the same content in the same order.
///
/// Its own type rather than a variant of [`ContentDisagreement`], because it is a separate claim
/// with a separate diagnosis: content is *the answer is wrong*, and order is *the plan asked for an
/// order and one side did not give it*.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum OrderDisagreement {
    /// The two results are not even the same shape, so there is no position to compare.
    #[error(
        "one side answered {left_rows} row(s) of {left_columns} column(s) and the other {right_rows} of \
         {right_columns}"
    )]
    Shape {
        left_rows: usize,
        left_columns: usize,
        right_rows: usize,
        right_columns: usize,
    },
    /// One position holds two different cells.
    #[error("row {row}, column {column} ({label}): {left:?} against {right:?}")]
    Cell {
        row: usize,
        column: usize,
        label: String,
        left: Value,
        right: Value,
    },
}

/// WHAT the two sides answered: the labels, and the rows as a multiset.
///
/// Says nothing about the order - [`agree_on_order`] is that assertion, and the two are separate so
/// that a wrong number is never reported as a sort order. Call this one first.
pub fn agree_on_content(left: &RowSet, right: &RowSet, real: RealTolerance) -> Result<(), ContentDisagreement> {
    if left.columns() != right.columns() {
        return Err(ContentDisagreement::Columns {
            left: left.columns().to_vec(),
            right: right.columns().to_vec(),
        });
    }
    let (here, there) = (tally(left, real), tally(right, real));
    for (key, found) in &here {
        let theirs = there.get(key).map_or(0, |other| other.count);
        if theirs != found.count {
            return Err(ContentDisagreement::Multiplicity {
                row: found.example.clone(),
                left: found.count,
                right: theirs,
            });
        }
    }
    // The other direction, which the loop above cannot reach: a row only the right side answered
    // has no key on the left to iterate over.
    for (key, found) in &there {
        if !here.contains_key(key) {
            return Err(ContentDisagreement::Multiplicity {
                row: found.example.clone(),
                left: 0,
                right: found.count,
            });
        }
    }
    Ok(())
}

/// The ORDER: cell for cell, position for position.
///
/// A plan that emits `ORDER BY` claims an order, so two data systems answering one plan in two
/// orders is a defect whatever the reason. Whether the plan claimed one is the caller's to know.
///
/// **Two limits, and both are why [`agree_on_content`] is called first rather than by convention.**
/// This compares the SHAPE and not the labels, so two results of one width whose columns are named
/// differently are compared position by position here and reported as a
/// [`ContentDisagreement::Columns`] there. And a caller reaching only for this one gets no
/// multiplicity check, because a positional comparison of equal-height results cannot express one.
pub fn agree_on_order(left: &RowSet, right: &RowSet, real: RealTolerance) -> Result<(), OrderDisagreement> {
    if left.columns().len() != right.columns().len() || left.rows().len() != right.rows().len() {
        return Err(OrderDisagreement::Shape {
            left_rows: left.rows().len(),
            left_columns: left.columns().len(),
            right_rows: right.rows().len(),
            right_columns: right.columns().len(),
        });
    }
    for (index, (mine, theirs)) in left.rows().iter().zip(right.rows()).enumerate() {
        for (column, (here, there)) in mine.iter().zip(theirs).enumerate() {
            if cell(here, real) != cell(there, real) {
                return Err(OrderDisagreement::Cell {
                    row: index,
                    column,
                    // `RowSet::new` refuses a ragged result, so a row this wide has a label. The
                    // fallback names the impossible case rather than defaulting to an empty
                    // string, which would read as a column called nothing.
                    label: left
                        .columns()
                        .get(column)
                        .map_or_else(|| String::from("<no such column>"), Clone::clone),
                    left: here.clone(),
                    right: there.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ContentDisagreement, OrderDisagreement, RealTolerance, agree_on_content, agree_on_order};
    use crate::warehouse::{Real, RowSet, Value};

    /// Every test here compares at the differential legs' own tolerance, so what they establish is
    /// about the policy those legs use rather than about a precision invented for a test.
    const TOLERANCE: RealTolerance = RealTolerance::DIFFERENTIAL;

    fn one_column(label: &str, cells: Vec<Value>) -> RowSet {
        RowSet::new(vec![String::from(label)], cells.into_iter().map(|cell| vec![cell]).collect())
            .expect("a one-column result is rectangular")
    }

    fn real(v: f64) -> Value {
        Value::Real(Real::parse(v).expect("a finite number is a real"))
    }

    /// The first of the two cases the audit named, and the reason this module exists.
    ///
    /// `Value::render` answers `"null"` for both, so the comparator this replaced called a data
    /// system that returned the WORD null equal to one that returned no value at all. Mutation
    /// evidence: replace the key with `value.render()` and this test is the one that reddens.
    #[test]
    fn a_null_and_the_text_null_are_not_the_same_cell() {
        let absent = one_column("region", vec![Value::Null]);
        let spelled = one_column("region", vec![Value::Text(String::from("null"))]);
        assert_eq!(
            agree_on_content(&absent, &spelled, TOLERANCE),
            Err(ContentDisagreement::Multiplicity {
                row: vec![Value::Null],
                left: 1,
                right: 0,
            })
        );
        assert_eq!(
            agree_on_order(&absent, &spelled, TOLERANCE),
            Err(OrderDisagreement::Cell {
                row: 0,
                column: 0,
                label: String::from("region"),
                left: Value::Null,
                right: Value::Text(String::from("null")),
            })
        );
    }

    /// The second case the audit named.
    ///
    /// A number and the text of that number are not the same answer: one side counted and the other
    /// handed back a string, which is precisely the class a rendered comparison cannot see.
    #[test]
    fn an_integer_and_its_own_text_are_not_the_same_cell() {
        let counted = one_column("subscriptions", vec![Value::Integer(1)]);
        let spelled = one_column("subscriptions", vec![Value::Text(String::from("1"))]);
        assert!(agree_on_content(&counted, &spelled, TOLERANCE).is_err());
        assert!(agree_on_order(&counted, &spelled, TOLERANCE).is_err());
    }

    /// A real number and the text of it are not the same answer either.
    ///
    /// The rendered comparator distinguished these by accident - it wrote a `Real` in exponential
    /// form, so only a text cell spelling `1.000000000000e0` collided. The distinction is now the
    /// variant rather than the notation.
    #[test]
    fn a_real_and_its_exponential_text_are_not_the_same_cell() {
        let computed = one_column("mean", vec![real(1.0)]);
        let spelled = one_column("mean", vec![Value::Text(String::from("1.000000000000e0"))]);
        assert!(agree_on_content(&computed, &spelled, TOLERANCE).is_err());
    }

    /// Multiplicity, which a set comparison cannot hold.
    ///
    /// The same distinct rows on both sides, one of them answered twice by one side. A `sort` and a
    /// `dedup` would call these equal, and a duplicated row is a wrong answer.
    #[test]
    fn a_row_answered_twice_does_not_agree_with_the_same_row_answered_once() {
        let twice = one_column("region", vec![Value::Text(String::from("north")); 2]);
        let once = one_column("region", vec![Value::Text(String::from("north"))]);
        assert_eq!(
            agree_on_content(&twice, &once, TOLERANCE),
            Err(ContentDisagreement::Multiplicity {
                row: vec![Value::Text(String::from("north"))],
                left: 2,
                right: 1,
            })
        );
    }

    /// The approximation, and that it is an approximation.
    ///
    /// Two sums of the same rows in different orders differ in the LAST PLACE of an `f64`, which is
    /// what the tolerance is for. The nudge is a bit pattern rather than a second literal, because a
    /// literal that far out is either the same `f64` or one clippy calls excessive precision - and
    /// asserting the two are different values first is what keeps the agreement below from being
    /// about one number compared with itself.
    #[test]
    fn two_reals_a_last_place_apart_agree_and_a_coarser_difference_does_not() {
        let summed = 1.0_f64 / 3.0;
        let summed_the_other_way = f64::from_bits(summed.to_bits().saturating_add(1));
        assert_ne!(summed.to_bits(), summed_the_other_way.to_bits());
        let engine = one_column("mean", vec![real(summed)]);
        let source = one_column("mean", vec![real(summed_the_other_way)]);
        assert_eq!(agree_on_content(&engine, &source, TOLERANCE), Ok(()));
        // A difference the tolerance does NOT absorb, so it is a tolerance rather than a blanket.
        let coarser = one_column("mean", vec![real(0.333_334)]);
        assert!(agree_on_content(&engine, &coarser, TOLERANCE).is_err());
    }

    /// The tolerance reaches [`Value::Real`] and nothing else.
    ///
    /// An integer one apart is a wrong count however small the gap, and text is text. Without this
    /// the previous test would be evidence for a blanket epsilon.
    #[test]
    fn the_tolerance_does_not_reach_an_integer_or_a_text_cell() {
        let counted = one_column("subscriptions", vec![Value::Integer(1_000_000_000_000_000)]);
        let off_by_one = one_column("subscriptions", vec![Value::Integer(1_000_000_000_000_001)]);
        assert!(agree_on_content(&counted, &off_by_one, TOLERANCE).is_err());

        let exact = one_column("total", vec![Value::Text(String::from("123.450000000000001"))]);
        let rounded = one_column("total", vec![Value::Text(String::from("123.45"))]);
        assert!(agree_on_content(&exact, &rounded, TOLERANCE).is_err());
    }

    /// The order assertion is separate, and this is what separateness BUYS.
    ///
    /// The same rows in two orders: content agrees, order does not. One assertion for both would
    /// report a sort order as a wrong number, or a wrong number as a sort order, depending on which
    /// way it was written.
    #[test]
    fn the_same_rows_in_a_different_order_agree_on_content_and_not_on_order() {
        let engine = one_column(
            "region",
            vec![Value::Text(String::from("north")), Value::Text(String::from("south"))],
        );
        let source = one_column(
            "region",
            vec![Value::Text(String::from("south")), Value::Text(String::from("north"))],
        );
        assert_eq!(agree_on_content(&engine, &source, TOLERANCE), Ok(()));
        assert_eq!(
            agree_on_order(&engine, &source, TOLERANCE),
            Err(OrderDisagreement::Cell {
                row: 0,
                column: 0,
                label: String::from("region"),
                left: Value::Text(String::from("north")),
                right: Value::Text(String::from("south")),
            })
        );
    }

    /// Labels are content, and they are reported before any cell is compared.
    #[test]
    fn two_sides_that_labelled_the_result_differently_disagree_on_content() {
        let engine = one_column("region", vec![Value::Text(String::from("north"))]);
        let source = one_column("Region", vec![Value::Text(String::from("north"))]);
        assert_eq!(
            agree_on_content(&engine, &source, TOLERANCE),
            Err(ContentDisagreement::Columns {
                left: vec![String::from("region")],
                right: vec![String::from("Region")],
            })
        );
    }

    /// Two answers of different heights have no position to compare, and say so.
    #[test]
    fn results_of_different_shapes_are_reported_as_a_shape_rather_than_a_cell() {
        let one = one_column("region", vec![Value::Text(String::from("north"))]);
        let none = one_column("region", Vec::new());
        assert_eq!(
            agree_on_order(&one, &none, TOLERANCE),
            Err(OrderDisagreement::Shape {
                left_rows: 1,
                left_columns: 1,
                right_rows: 0,
                right_columns: 1,
            })
        );
    }

    /// The agreeing case, so none of the above is passing because everything disagrees.
    #[test]
    fn two_identical_answers_agree_on_content_and_on_order() {
        let rows = vec![
            vec![Value::Text(String::from("north")), Value::Integer(10), real(0.5)],
            vec![Value::Null, Value::Integer(20), real(1.5)],
        ];
        let columns = vec![String::from("region"), String::from("count"), String::from("share")];
        let engine = RowSet::new(columns.clone(), rows.clone()).expect("a rectangular result");
        let source = RowSet::new(columns, rows).expect("a rectangular result");
        assert_eq!(agree_on_content(&engine, &source, TOLERANCE), Ok(()));
        assert_eq!(agree_on_order(&engine, &source, TOLERANCE), Ok(()));
    }
}
