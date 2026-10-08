//! The pure halves of this adapter, hermetic: the tuning value's parse and the deadline clamp.

use crate::{PostgresError, parse_statement_timeout, statement_timeout_under};

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

#[test]
fn the_environment_moves_the_statement_timeout_only_in_a_dev_build() {
    assert_eq!(statement_timeout_under(true, Some("250")).expect("a dev build reads it"), 250);
    assert!(matches!(
        statement_timeout_under(true, Some("not-a-number")),
        Err(PostgresError::InvalidStatementTimeout { .. })
    ));
    assert_eq!(statement_timeout_under(true, None).expect("unset is the default"), 15_000);
    // A shipped build ignores the variable - even an unparseable one, which is not worth refusing
    // a boot over when nothing reads it.
    assert_eq!(statement_timeout_under(false, Some("250")).expect("ignored"), 15_000);
    assert_eq!(statement_timeout_under(false, Some("not-a-number")).expect("ignored"), 15_000);
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
