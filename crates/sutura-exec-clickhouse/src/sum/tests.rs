use serde_json::json;
use sutura_domain::warehouse::{Real, Value};

use super::{SumTextError, SumType, read};

const NO_DECIMAL: &str = "0.00000000000000000000000000000000000000";

#[test]
fn a_type_name_says_what_the_total_is() {
    assert_eq!(SumType::parse("Int128"), Some(SumType::Integer));
    assert_eq!(SumType::parse("Nullable(Float64)"), Some(SumType::Float));
    assert_eq!(SumType::parse("Decimal(38, 2)"), Some(SumType::Decimal { scale: 2 }));
    assert_eq!(
        SumType::parse("Nullable(Decimal(76, 38))"),
        Some(SumType::Decimal { scale: 38 })
    );
    assert_eq!(SumType::parse("String"), None);
    assert_eq!(SumType::parse("Decimal(38)"), None);
}

#[test]
fn a_type_the_sum_slots_cannot_hold_exactly_is_refused_by_name() {
    assert_eq!(SumType::parse("UInt128"), None);
    assert_eq!(SumType::parse("Int256"), None);
    assert_eq!(SumType::parse("Decimal(76, 39)"), None);
    assert_eq!(read(json!(["UInt128", null, null, null])), Err(SumTextError::Malformed));
    assert_eq!(
        read(json!(["Decimal(76, 50)", null, "1.5", null])),
        Err(SumTextError::Malformed)
    );
}

#[test]
fn an_integer_total_is_an_integer_where_it_fits_and_its_exact_digits_where_it_does_not() {
    let integer = SumType::Integer;
    assert_eq!(integer.value("3"), Ok(Value::Integer(3)));
    assert_eq!(integer.value("-9223372036854775808"), Ok(Value::Integer(i64::MIN)));
    assert_eq!(
        integer.value("170141183460469231731687303715884105727"),
        Ok(Value::Text(String::from("170141183460469231731687303715884105727")))
    );
    assert_eq!(integer.value("3.5"), Err(SumTextError::Malformed));
}

#[test]
fn a_decimal_total_gets_its_scale_back_and_keeps_every_digit() {
    let money = SumType::Decimal { scale: 2 };
    assert_eq!(money.value("3"), Ok(Value::Text(String::from("3.00"))));
    assert_eq!(money.value("11.5"), Ok(Value::Text(String::from("11.50"))));
    assert_eq!(money.value("-0.07"), Ok(Value::Text(String::from("-0.07"))));
    assert_eq!(
        money.value("155.00000000000000000000000000000000000000"),
        Ok(Value::Text(String::from("155.00")))
    );
    assert_eq!(money.value("1.234"), Err(SumTextError::Malformed));
    assert_eq!(
        money.value("1.00000000000000000000000000000000000001"),
        Err(SumTextError::Malformed)
    );
    assert_eq!(SumType::Decimal { scale: 0 }.value("300.0"), Ok(Value::Integer(300)));
    assert_eq!(
        SumType::Decimal { scale: 0 }.value("99999999999999999999"),
        Ok(Value::Text(String::from("99999999999999999999")))
    );
}

#[test]
fn a_float_total_is_a_real_and_a_non_finite_one_is_refused() {
    assert_eq!(SumType::Float.value("3"), Ok(Value::Real(Real::parse(3.0).expect("finite"))));
    assert!(matches!(SumType::Float.value("inf"), Err(SumTextError::NotFinite(_))));
    assert!(matches!(SumType::Float.value("nan"), Err(SumTextError::NotFinite(_))));
    assert_eq!(SumType::Float.value("three"), Err(SumTextError::Malformed));
}

#[test]
fn the_total_is_read_from_the_slot_the_type_name_points_at() {
    let wide = "170141183460469231731687303715884105727";
    assert_eq!(read(json!(["Int128", wide, null, null])), Ok(Value::Text(String::from(wide))));
    assert_eq!(
        read(json!([
            "Decimal(38, 2)",
            null,
            "11.50000000000000000000000000000000000000",
            null
        ])),
        Ok(Value::Text(String::from("11.50")))
    );
    assert_eq!(
        read(json!(["Nullable(Float64)", null, null, 4])),
        Ok(Value::Real(Real::parse(4.0).expect("finite")))
    );
    assert!(matches!(
        read(json!(["Float64", null, null, "inf"])),
        Err(SumTextError::NotFinite(_))
    ));
}

#[test]
fn a_total_that_is_not_in_its_slot_is_null_when_the_slot_is_and_refused_when_it_is_not_a_total() {
    assert_eq!(read(json!(["Int128", null, null, null])), Ok(Value::Null));
    assert_eq!(read(json!(["Decimal(38, 2)", null, null, null])), Ok(Value::Null));
    assert_eq!(read(json!(["Int128", true, null, null])), Err(SumTextError::Malformed));
    assert_eq!(read(json!(["Int128"])), Err(SumTextError::Malformed));
    assert_eq!(read(json!("Int128")), Err(SumTextError::Malformed));
    assert_eq!(
        read(json!(["Decimal(38, 2)", null, NO_DECIMAL, null])),
        Ok(Value::Text(String::from("0.00")))
    );
}
