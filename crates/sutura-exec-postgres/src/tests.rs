use super::*;
use sutura_domain::warehouse::Value;

/// Renders a typed error and every cause beneath it, one per line, so a chain-only fact (the
/// message a downstream `#[source]` carries, not this type's own `Display`) can be asserted on.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = std::error::Error::source(error);
    while let Some(cause) = cursor {
        out.push_str(" | caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

/// **The hermetic negative M2 in the review that shipped this file could delete without reddening
/// a single tier-backed cell.** `tests/tls.rs`'s tier always answers the SSL negotiation byte `S`,
/// so nothing there can observe what happens when a server answers `N` - the one byte that matters
/// for whether `connect_secured` actually makes the handshake mandatory, or merely sets it up to be
/// forgotten. This binds a real `TcpListener` instead: no tier, no feature, runs everywhere.
#[test]
fn a_server_that_declines_tls_is_refused_rather_than_used_in_plaintext() {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::time::Duration;

    use sutura_conformance::corpus;

    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener binds");
    let port = listener.local_addr().expect("a bound listener has a local address").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the driver dials this listener");
        // The whole of an SSLRequest: an 8-byte length-and-code message, nothing more, per
        // tokio-postgres's own `connect_tls`.
        let mut request = [0_u8; 8];
        stream
            .read_exact(&mut request)
            .expect("the driver sends the 8-byte SSLRequest before anything else");
        // `N` is the server declining TLS. Closed right after, rather than kept open: under a
        // `Prefer` fallback the driver would carry on in plaintext and wait on a startup response
        // nothing here answers - closing turns that into a fast, different failure rather than a
        // hang, so this cell still terminates if the mandatory-`Require` line is ever lost.
        stream.write_all(b"N").expect("declining TLS is one byte");
        drop(stream);
    });

    let directory = std::env::temp_dir().join(format!("sutura-pg-declines-tls-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
    let anchor_path = directory.join("root.pem");
    let issued = rcgen::generate_simple_self_signed([String::from("localhost")]).expect("a self-signed pair generates");
    std::fs::write(&anchor_path, issued.cert.pem()).expect("the anchor writes");
    let tls = crate::tls::client_config(&crate::tls::TlsAnchors::Bundle(anchor_path), None)
        .expect("a bundle with one certificate builds a verifier");

    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .dbname("sutura")
        .user("sutura")
        .password("unused")
        .connect_timeout(Duration::from_secs(5));

    let refused = PostgresWarehouse::connect_secured(
        corpus::source(),
        corpus::posture(),
        &config,
        Some(sutura_tls::Rotating::fixed(tls)),
    );
    server.join().expect("the fake server thread does not panic");
    let _ignored = std::fs::remove_dir_all(&directory);

    let error = refused.expect_err("a server that declines TLS must not be used in plaintext");
    assert!(matches!(error, PostgresError::Connect { .. }), "{error}");
    let chain = error_chain(&error);
    assert!(
        chain.contains("does not support TLS"),
        "the refusal must name why the handshake could not happen: {chain}"
    );
}

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

/// `deadline::deadline_statement_timeout_ms`'s clamp, hermetic - no tier, no connection: the
/// arithmetic is pure, and `crates/sutura-exec-postgres/tests/deadline.rs` is where the server's own
/// `57014` is measured instead of assumed.
mod deadline_statement_timeout {
    use std::time::{Duration, Instant};

    use sutura_domain::warehouse::deadline::{Budget, Deadline};

    use crate::deadline::deadline_statement_timeout_ms;

    fn deadline(millis_left: u64) -> Deadline {
        Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_millis(millis_left)).expect("a positive budget parses"),
        )
    }

    /// A budget under the ceiling narrows it - `docs/adr/0029`'s row: the request's own bound is
    /// what a caller configured, and a connection with a generous dev ceiling must not widen it back.
    ///
    /// **A range, not an exact value.** `deadline_statement_timeout_ms` reads `Instant::now()`
    /// itself, after this test already read it once to open the deadline - real, if tiny, elapsed
    /// time between the two, which `as_millis` truncates rather than rounds. An exact `300` is
    /// therefore not guaranteed; what the claim needs is *narrowed to close to the budget*, not to
    /// the ceiling.
    #[test]
    fn a_budget_under_the_ceiling_is_the_value_sent() {
        let sent = deadline_statement_timeout_ms(15_000, deadline(300)).get();
        assert!((295..=300).contains(&sent), "expected close to 300ms, got {sent}ms");
    }

    /// The connect-time ceiling is the outer bound: a budget wider than it does not widen the
    /// per-statement value past what the deployment configured at connect.
    #[test]
    fn a_budget_over_the_ceiling_is_clamped_to_it() {
        assert_eq!(deadline_statement_timeout_ms(300, deadline(60_000)).get(), 300);
    }

    /// A ceiling of zero is the tuning value's own *disabled* spelling (`parse_statement_timeout`
    /// accepts `"0"`), read as no ceiling at all - the request's own budget governs alone, and the
    /// clamp does not panic on a zero-to-zero range. A range, not an exact value, for
    /// `a_budget_under_the_ceiling_is_the_value_sent`'s reason.
    #[test]
    fn a_disabled_ceiling_does_not_narrow_the_budget() {
        let sent = deadline_statement_timeout_ms(0, deadline(300)).get();
        assert!((295..=300).contains(&sent), "expected close to 300ms, got {sent}ms");
    }

    /// Never zero, even for a deadline already spent - structurally now, `NonZeroU32` rather than a
    /// `.max(1)` floor - opened ten seconds in the past on a one-millisecond budget, so
    /// `remaining_at(Instant::now())` reads `None` with no sleep needed to get there.
    #[test]
    fn a_deadline_already_spent_is_never_a_zero_timeout() {
        let spent = Deadline::opened_at(
            Instant::now()
                .checked_sub(Duration::from_secs(10))
                .expect("ten seconds ago is representable"),
            Budget::parse(Duration::from_millis(1)).expect("one ms is a budget"),
        );
        assert_eq!(spent.remaining_at(Instant::now()), None, "the fixture must already be spent");
        assert_ne!(deadline_statement_timeout_ms(15_000, spent).get(), 0);
        assert_ne!(deadline_statement_timeout_ms(0, spent).get(), 0);
    }

    /// **Structural, not timing-observed** (`telekom/sutura#687`'s review, finding 1). A deadline
    /// opened `budget - 500µs` in the past has, at construction, exactly 500µs left - under 1ms, so
    /// `as_millis` truncates it to `0` in the `Some` arm. By the time this function's own
    /// `Instant::now()` runs, that 500µs may or may not have already elapsed too, flipping the
    /// reading to the `None` arm instead. Both arms must answer `1`, so the assertion holds
    /// whichever one actually fires - it is not a race the test can lose. The only OTHER existing
    /// cell over a spent deadline (`a_deadline_already_spent_is_never_a_zero_timeout`, ten seconds
    /// in the past) drives only the `None` arm, never the truncating `Some` arm - the one a
    /// `.max(1)` floor with no test on it left silently unproven.
    #[test]
    fn a_deadline_grazing_its_own_budget_is_exactly_one_ms_either_way() {
        let budget = Duration::from_millis(50);
        let elapsed = budget
            .checked_sub(Duration::from_micros(500))
            .expect("fifty milliseconds minus half a millisecond is representable");
        let opened = Instant::now()
            .checked_sub(elapsed)
            .expect("fifty milliseconds minus half a millisecond ago is representable");
        let deadline = Deadline::opened_at(opened, Budget::parse(budget).expect("fifty ms is a budget"));
        assert_eq!(deadline_statement_timeout_ms(15_000, deadline).get(), 1);
        assert_eq!(deadline_statement_timeout_ms(0, deadline).get(), 1);
    }

    /// **The overflow fallback saturates at `i32::MAX`, not `u32::MAX`** (`telekom/sutura#687`'s
    /// round-2 review, finding 2/1b): Postgres's own `statement_timeout` GUC is a signed `int`, and
    /// a fifty-day budget's milliseconds do not fit a `u32` (`50 * 86_400_000 > u32::MAX`), so with
    /// no ceiling to clamp it this drives `unwrap_or(POSTGRES_TIMEOUT_MAX_MS)` directly.
    #[test]
    fn an_overflowing_remaining_saturates_at_i32_max_not_u32_max() {
        let fifty_days = Deadline::opened_at(
            Instant::now(),
            Budget::parse(Duration::from_hours(1200)).expect("fifty days is a budget"),
        );
        assert_eq!(deadline_statement_timeout_ms(0, fifty_days).get(), 0x7FFF_FFFF);
    }
}

/// `crate::deadline::deadline_exceeded`'s `DeadlineSpent` arm, hermetic - the tier cell in
/// `tests/deadline.rs` (`a_caller_spent_while_waiting_for_the_lock_is_refused_locally`) is what
/// proves a spent deadline actually reaches this variant; this is only the mapping.
#[test]
fn a_local_deadline_spent_refusal_is_deadline_exceeded() {
    assert!(crate::deadline::deadline_exceeded(&PostgresError::DeadlineSpent));
}
