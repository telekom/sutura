use super::*;
use sutura_domain::warehouse::Value;

/// The wire bytes for a `NUMERIC`: two bytes each of digit count, weight, sign and display
/// scale, then the base-10000 digits. Built big-endian exactly as the documented format.
#[expect(
    clippy::big_endian_bytes,
    reason = "the NUMERIC wire format is documented big-endian, which is exactly what the test \
              helper writes"
)]
fn numeric_bytes(digits: &[u16], weight: i16, sign: u16, dscale: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 2 * digits.len());
    out.extend_from_slice(&u16::try_from(digits.len()).unwrap_or(0).to_be_bytes());
    out.extend_from_slice(&weight.to_be_bytes());
    out.extend_from_slice(&sign.to_be_bytes());
    out.extend_from_slice(&dscale.to_be_bytes());
    for &digit in digits {
        out.extend_from_slice(&digit.to_be_bytes());
    }
    out
}

fn decode(digits: &[u16], weight: i16, sign: u16, dscale: u16) -> PgNumeric {
    decode_numeric(&numeric_bytes(digits, weight, sign, dscale)).expect("a valid NUMERIC decodes")
}

#[test]
fn an_integral_numeric_that_fits_is_an_integer_cell() {
    // 300 as NUMERIC(,0): one base-10000 digit, weight 0.
    assert_eq!(
        numeric_cell(&decode(&[300], 0, 0x0000, 0), "total").unwrap(),
        Value::Integer(300)
    );
    // 10000 = 1·10000^1, weight 1.
    assert_eq!(
        numeric_cell(&decode(&[1], 1, 0x0000, 0), "total").unwrap(),
        Value::Integer(10_000)
    );
}

#[test]
fn a_fractional_numeric_is_exact_text() {
    // 100.5 = 100·10000^0 + 5000·10000^-1, declared scale 1.
    assert_eq!(
        numeric_cell(&decode(&[100, 5000], 0, 0x0000, 1), "mean").unwrap(),
        Value::Text(String::from("100.5"))
    );
    // The declared scale renders 5000·10000^-1 as 0.5, not 0.5000.
    assert_eq!(
        numeric_cell(&decode(&[5000], -1, 0x0000, 1), "mean").unwrap(),
        Value::Text(String::from("0.5"))
    );
    // The wire's declared scale is preserved exactly.
    assert_eq!(
        numeric_cell(&decode(&[100], 0, 0x0000, 2), "mean").unwrap(),
        Value::Text(String::from("100.00"))
    );
    // The absent 10^-4 group implied by weight -2 is still part of the value.
    assert_eq!(
        numeric_cell(&decode(&[1000], -2, 0x0000, 5), "mean").unwrap(),
        Value::Text(String::from("0.00001"))
    );
}

#[test]
fn a_negative_numeric_keeps_its_sign_exactly() {
    assert_eq!(
        numeric_cell(&decode(&[300], 0, 0x4000, 0), "total").unwrap(),
        Value::Integer(-300)
    );
    assert_eq!(
        numeric_cell(&decode(&[100, 5000], 0, 0x4000, 1), "mean").unwrap(),
        Value::Text(String::from("-100.5"))
    );
}

#[test]
fn a_non_finite_numeric_is_refused_as_a_non_finite_cell() {
    assert!(matches!(
        numeric_cell(&decode(&[0], 0, 0xC000, 0), "mean"),
        Err(PostgresError::NotFinite { .. })
    ));
    assert!(matches!(
        numeric_cell(&decode(&[0], 0, 0xD000, 2), "mean"),
        Err(PostgresError::NotFinite { .. })
    ));
}

#[test]
fn a_numeric_wider_than_i64_stays_exact_text() {
    // 10^20 is beyond i64 and must not be rounded or refused.
    assert_eq!(
        numeric_cell(&decode(&[1], 5, 0x0000, 0), "total").unwrap(),
        Value::Text(String::from("100000000000000000000"))
    );
}

#[test]
fn a_truncated_numeric_header_is_a_decoder_error() {
    let short = decode_numeric(&[0, 1, 0]).expect_err("fewer than the eight header bytes");
    assert!(short.to_string().contains("shorter"), "{short}");
    // Eight header bytes but claims a digit it does not carry.
    let missing_digit = decode_numeric(&[0, 1, 0, 0, 0, 0, 0, 0]).expect_err("claims a digit that is not there");
    assert!(missing_digit.to_string().contains("value was truncated"), "{missing_digit}");
}

#[test]
fn pg_date_round_trips_through_the_epoch_offset() {
    // The driver's epoch (2000-01-01) is day 0 in its own numbering.
    assert_eq!(PgDate { days: 0 }.to_domain_days(), 10_957);
    // The domain epoch (1970-01-01) is the driver's -10957.
    assert_eq!(PgDate::from_domain(0).days, -10_957);
    assert_eq!(PgDate::from_domain(0).to_domain_days(), 0);
}

#[test]
fn a_statement_timeout_is_a_u32_ceiling_or_it_is_refused() {
    // The tuning value becomes a `SET statement_timeout = N` line verbatim, so it is a typed
    // ceiling at the boundary: a number that fits parses...
    assert_eq!(parse_statement_timeout("15000").expect("a number parses"), 15_000);
    assert_eq!(parse_statement_timeout("0").expect("zero is a valid timeout"), 0);
    assert_eq!(
        parse_statement_timeout(&u32::MAX.to_string()).expect("the ceiling parses"),
        u32::MAX
    );
    // ...and anything that cannot be a `u32` is refused rather than reaching the statement.
    // `u32::MAX + 1` is the ceiling's far side, and decimals are refused rather than truncated.
    assert!(matches!(
        parse_statement_timeout("not-a-number"),
        Err(PostgresError::InvalidStatementTimeout { .. })
    ));
    assert!(matches!(
        parse_statement_timeout("4294967296"),
        Err(PostgresError::InvalidStatementTimeout { .. })
    ));
    assert!(matches!(
        parse_statement_timeout("15000.5"),
        Err(PostgresError::InvalidStatementTimeout { .. })
    ));
}
