//! The two "too much data" checks [`answer`](crate::answer) and
//! [`answer_federated`](crate::federated) both apply to a result AFTER it executes, and neither
//! reaches a data system - split out of `lib.rs` for `cargo xtask max-lines`'s cap rather than for
//! thematic tidiness, the same reason `federated` is its own file.

/// Whether a result set came back with more rows than its plan capped it at.
///
/// **A governance control, so the direction it fails in is the whole of what this function is for.**
/// The comparison used to be written inline as
/// `rows.len() > usize::try_from(plan.max_rows()).unwrap_or(usize::MAX)`, which reads as a cap and
/// is a cap being lifted: a conversion that came back `Err` produced `usize::MAX`, and no result set
/// is longer than that, so the one refusal that stops a TRUNCATED total from being certified would
/// have been skipped. Unreachable on any target with 32-bit pointers or wider, and still the wrong
/// direction to have written down.
///
/// It compares in `u64` instead, where the plan's `u32` cap widens with `From` and cannot fail at
/// all. The count still needs a conversion, because neither direction between these two types is
/// infallible - `From<usize> for u64` does not exist, since a target with pointers wider than 64
/// bits would lose a count, and `From<u32> for usize` does not either, since a 16-bit target could
/// not hold the cap. What changed is which way the unreachable case falls: a count that does not fit
/// a `u64` is a count larger than any `u32` cap, so `u64::MAX` here is not a fallback that guesses,
/// it is the answer. The control refuses rather than opening.
///
/// Named rather than inline so the boundary is testable without a data system: the case that decides
/// a certification is one row over the cap, and reaching it through [`crate::answer`] means
/// fabricating ten thousand rows through a validated bundle.
pub(crate) fn exceeds_row_cap(returned: usize, max_rows: u32) -> bool {
    u64::try_from(returned).unwrap_or(u64::MAX) > u64::from(max_rows)
}

/// Whether a result set's rendered cells would occupy more bytes than
/// [`sutura_domain::query::ResponseByteLimit::DEFAULT`] permits, and the ceiling if so.
///
/// **The same governance shape as [`exceeds_row_cap`], one measurement further out - see the call
/// site's own comment for why the row cap cannot see this.** `RowSet::rendered_byte_len` is the
/// canonical cell text an anchor is compared against, not the wire bytes either transport's own
/// response body eventually wraps it in, and the two do NOT stay close - see
/// [`ResponseByteLimit::DEFAULT`](sutura_domain::query::ResponseByteLimit::DEFAULT) for the factor
/// measured between them and what it means for the number this compares against. Named, for the
/// reason `exceeds_row_cap` is: the boundary is testable without a data system.
pub(crate) fn exceeds_response_bound(rows: &sutura_domain::warehouse::RowSet) -> Option<u64> {
    let limit = sutura_domain::query::ResponseByteLimit::DEFAULT;
    (rows.rendered_byte_len() > limit.bytes()).then_some(limit.bytes())
}
