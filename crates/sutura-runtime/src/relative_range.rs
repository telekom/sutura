//! A relative time range - `last: { count, unit }` - resolved against this deployment's own wall
//! clock into the absolute [`TimeRange`] the domain already knows how to hold. `telekom/sutura#778`.
//!
//! **Not `docs/adr/0029`'s clock.** That one is monotonic (`std::time::Instant`) and exists for a
//! deadline that must not be fooled by a wall-clock jump; the domain reads no clock of either kind.
//! This one answers "what calendar day is it", which a monotonic instant cannot say, and it lives
//! here rather than in `sutura-domain` for the same reason the rest of this crate does: two
//! transports need the identical implementation rather than each re-deriving it (see this crate's
//! own doc comment).
//!
//! [`WallClock`] is the port, [`SystemClock`] its first and only shipping implementor - the same
//! shape as `sutura_exec_bigquery::sts::UnixClock`/`SystemClock`, for the same reason: everything
//! [`resolve_range`] decides is exercised against a fixed clock in a test, so an ambient
//! `SystemTime::now()` inside the resolution would make the outcome a function of the day the test
//! ran on.
//!
//! **The limit.** "Last week" resolves against THIS process's calendar date; a caller in another
//! timezone gets the server's week, not their own. Nothing here reads a timezone, and nothing asks
//! for one yet.

use core::num::NonZeroU32;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use sutura_domain::calendar::{Date, InvalidDate, TimeRange};

/// Where "today" comes from when a caller's range is relative rather than absolute.
pub trait WallClock {
    /// This process's own calendar date, right now.
    fn today(&self) -> Result<Date, ClockUnavailable>;
}

/// The wall clock: the shipping [`WallClock`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl WallClock for SystemClock {
    fn today(&self) -> Result<Date, ClockUnavailable> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|cause| ClockUnavailable::Read { cause })?;
        // Saturating rather than a third error variant for a case that cannot occur before the
        // Gregorian calendar runs out of representable years anyway: `Date::from_days_since_epoch`
        // rejects an out-of-range day count on its own, by name, via `InvalidDate::YearOutOfRange`.
        let days = i32::try_from(elapsed.as_secs().checked_div(86_400).unwrap_or(0)).unwrap_or(i32::MAX);
        Date::from_days_since_epoch(days).map_err(|cause| ClockUnavailable::NotADate { cause })
    }
}

/// Why this process could not say what today is.
#[derive(Debug, thiserror::Error)]
pub enum ClockUnavailable {
    /// The system clock reads before the Unix epoch.
    #[error("this process's clock reads before 1970-01-01")]
    Read {
        #[source]
        cause: SystemTimeError,
    },
    /// The system clock's day count falls outside the years [`Date`] can hold.
    #[error("this process's clock does not fall on a representable date")]
    NotADate {
        #[source]
        cause: InvalidDate,
    },
}

/// The closed vocabulary a relative range's `unit` may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeUnit {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

/// `unit` named none of the five accepted values.
///
/// Carries nothing: the set is fixed and finite, so the message names all five instead of echoing
/// back the value that did not match - the same choice `sutura_domain::model::Grain`'s own parse
/// failure makes, and for the same reason.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("`unit` must be one of `day`, `week`, `month`, `quarter` or `year`")]
pub struct UnknownRelativeUnit;

impl RelativeUnit {
    fn parse(raw: &str) -> Result<Self, UnknownRelativeUnit> {
        match raw {
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            "quarter" => Ok(Self::Quarter),
            "year" => Ok(Self::Year),
            _ => Err(UnknownRelativeUnit),
        }
    }
}

/// A transport's own `last` fields, extracted as plain values - the wire struct's job stops at
/// deserializing them; everything from here on is the one resolver both transports call.
///
/// Fields are private: this is a library crate's own public type, and a `pub` field would let a
/// caller build one directly rather than through [`Self::new`] - which holds no invariant of its
/// own today, but a struct literal bypassing a constructor is exactly the shape that stops holding
/// one the day it gains one.
#[derive(Debug, Clone)]
pub struct LastWire {
    count: u32,
    unit: String,
    include_current: bool,
}

impl LastWire {
    pub const fn new(count: u32, unit: String, include_current: bool) -> Self {
        Self {
            count,
            unit,
            include_current,
        }
    }
}

/// Why a `range` was neither a valid absolute period nor a valid relative one.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidRangeShape {
    /// Both `start`/`end` and `last` were given, or neither was.
    #[error("`range` must be either `start`/`end` or `last`, not both and not neither")]
    AmbiguousOrMissing,
    /// `last.count` was zero, which names no period.
    #[error("`last.count` must not be zero")]
    ZeroCount,
    /// `last.unit` named none of the five accepted values.
    #[error(transparent)]
    Unit(#[from] UnknownRelativeUnit),
}

/// Which of the two shapes a `range` used, once the ambiguity above is ruled out.
enum RangeChoice {
    Absolute {
        start: String,
        end: String,
    },
    Relative {
        count: NonZeroU32,
        unit: RelativeUnit,
        include_current: bool,
    },
}

fn range_choice(start: Option<String>, end: Option<String>, last: Option<LastWire>) -> Result<RangeChoice, InvalidRangeShape> {
    match (start, end, last) {
        (Some(start), Some(end), None) => Ok(RangeChoice::Absolute { start, end }),
        (None, None, Some(last)) => {
            let count = NonZeroU32::new(last.count).ok_or(InvalidRangeShape::ZeroCount)?;
            let unit = RelativeUnit::parse(&last.unit)?;
            Ok(RangeChoice::Relative {
                count,
                unit,
                include_current: last.include_current,
            })
        }
        _ => Err(InvalidRangeShape::AmbiguousOrMissing),
    }
}

/// The pair a resolved window's endpoints come back as, named so a `Result` naming it stays under
/// `clippy::type_complexity`.
type DateWindow = (Date, Date);

/// Why a `range` could not become the two ISO dates [`resolve_range`] hands to
/// `sutura_domain::question::parse_query`.
#[derive(Debug, thiserror::Error)]
pub enum RangeResolutionError {
    /// The wire shape itself - see [`InvalidRangeShape`].
    #[error(transparent)]
    Shape(#[from] InvalidRangeShape),
    /// A relative range needed the clock and could not read it.
    #[error("this deployment's clock could not be read")]
    Clock(#[source] ClockUnavailable),
    /// A relative range resolved to a boundary outside the years [`Date`] can hold.
    #[error("the resolved range falls outside the dates this system can represent")]
    OutOfRange(#[source] InvalidDate),
    /// Unreachable: `count` is a `NonZeroU32`, so every window [`shift`] builds spans at least one
    /// day and `start` is always strictly before `end`. Answered rather than unwrapped because
    /// `unwrap_used` is denied - a panic reachable from a caller's `count` is exactly what that ban
    /// exists to prevent, so the impossible alternative gets its own name and keeps its real cause
    /// instead of being discarded into a borrowed one.
    #[error(transparent)]
    Empty(#[from] sutura_domain::calendar::InvalidTimeRange),
}

/// The pair of ISO dates [`resolve_range`] hands to `sutura_domain::question::parse_query`.
type ResolvedRange = (String, String);

/// The one place both transports turn a `range` - absolute or relative - into the two ISO dates
/// `sutura_domain::question::parse_query` already knows how to certify.
///
/// `Query` does not change: this resolves BEFORE that function is ever called, so a relative range
/// is indistinguishable from an absolute one by the time the domain sees it.
pub fn resolve_range<C>(
    clock: &C,
    start: Option<String>,
    end: Option<String>,
    last: Option<LastWire>,
) -> Result<ResolvedRange, RangeResolutionError>
where
    C: WallClock,
{
    match range_choice(start, end, last)? {
        RangeChoice::Absolute { start, end } => Ok((start, end)),
        RangeChoice::Relative {
            count,
            unit,
            include_current,
        } => {
            let today = clock.today().map_err(RangeResolutionError::Clock)?;
            let range = shift(today, count, unit, include_current)?;
            Ok((range.start().to_iso(), range.end().to_iso()))
        }
    }
}

/// The absolute window `count` `unit`s back from `today`, extended to include `today` itself when
/// `include_current`.
fn shift(today: Date, count: NonZeroU32, unit: RelativeUnit, include_current: bool) -> Result<TimeRange, RangeResolutionError> {
    let count = core::num::NonZeroI64::from(count).get();
    let (start, end) = match unit {
        RelativeUnit::Day => fixed_length_window(today, count, 1, include_current),
        RelativeUnit::Week => fixed_length_window(today, count, 7, include_current),
        RelativeUnit::Month => calendar_window(today, count, 1, include_current),
        RelativeUnit::Quarter => calendar_window(today, count, 3, include_current),
        RelativeUnit::Year => calendar_window(today, count, 12, include_current),
    }
    .map_err(RangeResolutionError::OutOfRange)?;
    TimeRange::new(start, end).map_err(RangeResolutionError::from)
}

/// A window of exactly `count` fixed-length periods (`unit_days` each) ending at `today`, or at
/// `today` plus one day when `include_current` - "the last N days/weeks", no calendar alignment.
fn fixed_length_window(today: Date, count: i64, unit_days: i64, include_current: bool) -> Result<DateWindow, InvalidDate> {
    let span = count.saturating_mul(unit_days);
    let end_days = i64::from(today.days_since_epoch()).saturating_add(i64::from(include_current));
    let start_days = end_days.saturating_sub(span);
    let end = Date::from_days_since_epoch(i32::try_from(end_days).unwrap_or(i32::MAX))?;
    let start = Date::from_days_since_epoch(i32::try_from(start_days).unwrap_or(i32::MIN))?;
    Ok((start, end))
}

/// A window of exactly `count` whole `unit_months`-month periods (month: 1, quarter: 3, year: 12)
/// ending at the calendar boundary containing `today`, or extended to `today` itself when
/// `include_current` - "the whole month/quarter/year before this one", or "so far" this one.
fn calendar_window(today: Date, count: i64, unit_months: i64, include_current: bool) -> Result<DateWindow, InvalidDate> {
    let boundary = period_start(today, unit_months)?;
    let end = if include_current {
        Date::from_days_since_epoch(i32::try_from(i64::from(today.days_since_epoch()).saturating_add(1)).unwrap_or(i32::MAX))?
    } else {
        boundary
    };
    // The current period counts toward `count` once it is included, so only `count - 1` whole
    // periods are stepped back beyond the boundary.
    let periods_back = if include_current { count.saturating_sub(1) } else { count };
    let start = month_start(boundary, periods_back.saturating_mul(unit_months).saturating_neg())?;
    Ok((start, end))
}

/// The first day of the `unit_months`-sized calendar period containing `today` - the month, the
/// quarter or the year it falls in, per `unit_months`.
fn period_start(today: Date, unit_months: i64) -> Result<Date, InvalidDate> {
    let month0 = i64::from(today.month()) - 1;
    let period_index = month0.div_euclid(unit_months);
    month_start(today, period_index.saturating_mul(unit_months).saturating_sub(month0))
}

/// The first day of the month `delta_months` away from `from`'s own month. Always day one, so
/// there is nothing to clamp: unlike shifting an arbitrary day-of-month, a month's first day is
/// valid in every month.
fn month_start(from: Date, delta_months: i64) -> Result<Date, InvalidDate> {
    let month0 = i64::from(from.month()) - 1 + delta_months;
    let year = i64::from(from.year()).saturating_add(month0.div_euclid(12));
    // Saturating rather than a new error variant, for the reason `SystemClock::today` gives:
    // `Date::new` rejects an out-of-range year on its own.
    let year = i16::try_from(year).unwrap_or(if year < 0 { i16::MIN } else { i16::MAX });
    let month = u8::try_from(month0.rem_euclid(12).saturating_add(1)).unwrap_or(u8::MAX);
    Date::new(year, month, 1)
}

#[cfg(test)]
mod tests {
    use super::{ClockUnavailable, LastWire, RangeResolutionError, WallClock, resolve_range};
    use sutura_domain::calendar::Date;

    struct FixedClock(Date);

    impl WallClock for FixedClock {
        fn today(&self) -> Result<Date, ClockUnavailable> {
            Ok(self.0)
        }
    }

    struct BrokenClock;

    impl WallClock for BrokenClock {
        fn today(&self) -> Result<Date, ClockUnavailable> {
            Err(ClockUnavailable::NotADate {
                cause: Date::from_days_since_epoch(i32::MAX).expect_err("i32::MAX is not a date"),
            })
        }
    }

    fn last(count: u32, unit: &str, include_current: bool) -> LastWire {
        LastWire {
            count,
            unit: unit.to_owned(),
            include_current,
        }
    }

    #[test]
    fn a_month_excluding_today_is_the_whole_month_before() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let (start, end) = resolve_range(&clock, None, None, Some(last(1, "month", false))).expect("resolves");
        assert_eq!((start.as_str(), end.as_str()), ("2026-08-01", "2026-09-01"));
    }

    #[test]
    fn a_month_including_today_is_the_month_so_far() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let (start, end) = resolve_range(&clock, None, None, Some(last(1, "month", true))).expect("resolves");
        assert_eq!((start.as_str(), end.as_str()), ("2026-09-01", "2026-09-17"));
    }

    #[test]
    fn two_quarters_back_spans_two_whole_quarters() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let (start, end) = resolve_range(&clock, None, None, Some(last(2, "quarter", false))).expect("resolves");
        assert_eq!((start.as_str(), end.as_str()), ("2026-01-01", "2026-07-01"));
    }

    #[test]
    fn last_ninety_days_excludes_today() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let (start, end) = resolve_range(&clock, None, None, Some(last(90, "day", false))).expect("resolves");
        assert_eq!((start.as_str(), end.as_str()), ("2026-06-18", "2026-09-16"));
    }

    #[test]
    fn an_absolute_range_is_returned_unresolved() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let (start, end) =
            resolve_range(&clock, Some("2026-01-01".to_owned()), Some("2026-02-01".to_owned()), None).expect("resolves");
        assert_eq!((start.as_str(), end.as_str()), ("2026-01-01", "2026-02-01"));
    }

    #[test]
    fn both_start_end_and_last_is_ambiguous() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let error = resolve_range(
            &clock,
            Some("2026-01-01".to_owned()),
            Some("2026-02-01".to_owned()),
            Some(last(1, "month", false)),
        )
        .expect_err("both shapes at once is refused");
        assert!(matches!(error, RangeResolutionError::Shape(_)), "{error:?}");
    }

    #[test]
    fn neither_shape_is_also_ambiguous() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let error = resolve_range(&clock, None, None, None).expect_err("no shape at all is refused");
        assert!(matches!(error, RangeResolutionError::Shape(_)), "{error:?}");
    }

    #[test]
    fn a_zero_count_is_refused() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let error = resolve_range(&clock, None, None, Some(last(0, "month", false))).expect_err("zero names no period");
        assert!(matches!(error, RangeResolutionError::Shape(_)), "{error:?}");
    }

    #[test]
    fn an_unknown_unit_is_refused() {
        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let error = resolve_range(&clock, None, None, Some(last(1, "fortnight", false))).expect_err("not one of the five");
        assert!(matches!(error, RangeResolutionError::Shape(_)), "{error:?}");
    }

    /// The refusal proof for an unreadable clock, not just the happy path where it answers - a
    /// predicate exercised with no test of its refusal is the standard hole.
    #[test]
    fn an_unreadable_clock_refuses_a_relative_range() {
        let error = resolve_range(&BrokenClock, None, None, Some(last(1, "month", false))).expect_err("the clock never answers");
        assert!(matches!(error, RangeResolutionError::Clock(_)), "{error:?}");
    }
}
