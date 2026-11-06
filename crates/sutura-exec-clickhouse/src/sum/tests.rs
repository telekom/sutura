use super::SumType;

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
}
