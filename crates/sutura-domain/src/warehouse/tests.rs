//! The port's own vocabulary, held against itself: a parameter renders as a value, a cell is finite,
//! and a row set is as wide as it says.

use super::{MalformedRowSet, NotFinite, ParamValue, Real, RowSet, Value};
use crate::calendar::Date;

fn real(value: f64) -> Real {
    Real::parse(value).expect("a test literal is finite")
}

#[test]
fn a_parameter_renders_for_a_reader_and_not_as_sql() {
    // The rendering exists so `sutura compile` can show a plan. It must not look like something
    // to paste into a statement: text keeps its quotes so an empty or padded value is visible,
    // and nothing here escapes anything, because escaping is what a bind parameter replaces.
    assert_eq!(
        ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date")).render(),
        "2026-06-01"
    );
    assert_eq!(ParamValue::Text(String::from("north")).render(), "\"north\"");
    assert_eq!(ParamValue::Text(String::new()).render(), "\"\"");
    // A value that would be an injection if it were text in a statement renders visibly as a
    // value rather than as syntax.
    assert_eq!(ParamValue::Text(String::from("a' OR '1'='1")).render(), "\"a' OR '1'='1\"");
}

#[test]
fn a_ragged_result_set_is_rejected_once_rather_than_handled_everywhere() {
    // Every consumer reads cells by column position. Without this check each of them needs its
    // own fallback for a shape that must not exist, and under the `indexing_slicing` ban those
    // fallbacks are where a wrong value gets substituted for a missing one.
    assert_eq!(
        RowSet::new(vec![String::from("a"), String::from("b")], vec![vec![Value::Integer(1)]],).unwrap_err(),
        MalformedRowSet::RowWidth {
            row: 0,
            cells: 1,
            columns: 2,
        }
    );
}

#[test]
fn scalar_refuses_any_shape_that_is_not_one_cell() {
    // An anchor check reads this. If it returned the first cell of a three-row result, an
    // anchor would silently pass against a statement that grouped when it should not have.
    let one = RowSet::new(vec![String::from("v")], vec![vec![Value::Integer(7)]]).expect("one cell is a valid result");
    assert_eq!(one.scalar(), Some(&Value::Integer(7)));

    let two_rows = RowSet::new(
        vec![String::from("v")],
        vec![vec![Value::Integer(7)], vec![Value::Integer(8)]],
    )
    .expect("two rows is a valid result");
    assert_eq!(two_rows.scalar(), None);

    let two_columns = RowSet::new(
        vec![String::from("a"), String::from("b")],
        vec![vec![Value::Integer(7), Value::Integer(8)]],
    )
    .expect("two columns is a valid result");
    assert_eq!(two_columns.scalar(), None);

    let empty = RowSet::new(vec![String::from("v")], vec![]).expect("no rows is a result");
    assert_eq!(empty.scalar(), None);
}

#[test]
fn rendering_is_one_function_so_an_anchor_compares_the_same_way_everywhere() {
    assert_eq!(Value::Integer(197_122).render(), "197122");
    assert_eq!(Value::Text(String::from("north")).render(), "north");
    assert_eq!(Value::Null.render(), "null");
    // Shortest round-trip: an exact decimal comes back as one rather than as 0.30000000000000004.
    assert_eq!(Value::Real(real(0.3_f64)).render(), "0.3");
}

#[test]
fn a_cell_cannot_hold_a_number_that_is_not_one() {
    // THE BUG THIS EXISTS FOR. `Real` used to be a raw `f64`, so a ratio measure declaring
    // `zero_denominator: fails` answered the string "inf" under its own certified metric name:
    // both adapters cast the numerator to a floating type before dividing, so the division is
    // IEEE float division, and IEEE float division by zero does not fail. Nothing between the
    // data system and the caller looked at the value, because nothing had a place to.
    //
    // Asserted over all three of the class rather than over the one variant that exposed it: a
    // guard on the division would have left `-inf` and `NaN` representable.
    assert_eq!(
        Real::parse(f64::INFINITY).unwrap_err(),
        NotFinite::Infinite { value: f64::INFINITY }
    );
    assert_eq!(
        Real::parse(f64::NEG_INFINITY).unwrap_err(),
        NotFinite::Infinite {
            value: f64::NEG_INFINITY
        }
    );
    assert_eq!(Real::parse(f64::NAN).unwrap_err(), NotFinite::NotANumber);
    // The messages an adapter's error chain ends in, so the reader is told which of the three.
    assert_eq!(
        Real::parse(f64::INFINITY).unwrap_err().to_string(),
        "inf is not a finite number"
    );
    assert_eq!(
        Real::parse(f64::NEG_INFINITY).unwrap_err().to_string(),
        "-inf is not a finite number"
    );
    assert_eq!(Real::parse(f64::NAN).unwrap_err().to_string(), "NaN is not a number");

    // And what a real number still does, so this is not a test that would pass with every float
    // refused. Zero and the subnormals are finite, and a metric that legitimately answers zero
    // must not be caught by a check aimed at a division by it.
    for finite in [0.0_f64, -0.0_f64, 0.3_f64, f64::MIN, f64::MAX, f64::MIN_POSITIVE] {
        // Compared as bits rather than with `==`, which `float_cmp` bans for the reason it exists:
        // the assertion here is that the value came through UNCHANGED, and bit equality is that
        // claim exactly. It also keeps negative zero distinguishable from zero.
        assert_eq!(real(finite).get().to_bits(), finite.to_bits(), "{finite} is a finite number");
    }
}

#[test]
fn a_real_number_renders_the_same_way_wherever_it_is_formatted() {
    // `Value::render` is what an anchor is compared against and `{:.12e}` is what a differential
    // comparison between two engines uses. Both go through this one type, so neither can drift
    // into its own idea of what the number looks like.
    assert_eq!(format!("{}", real(0.3_f64)), "0.3");
    assert_eq!(format!("{:.12e}", real(190_007.333_333_333_34_f64)), "1.900073333333e5");
}
