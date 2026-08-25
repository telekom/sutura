//! Dates, and the bounded range a question has to carry.
//!
//! Hand-rolled rather than taken from a date library, and that is a boundary decision rather than
//! taste: `cargo xtask check-boundaries` holds `sutura-domain` to an allowlist of `serde` and
//! `thiserror` over the whole transitive tree, so a date crate would have to be argued onto that
//! list. What is needed here is a calendar date with an ordering and one parser, which is less code
//! than the argument would be.
//!
//! No clock, no time zone, no instant. A grain is a calendar concept, and "the month of June" is
//! not a question about an offset from an epoch. When a time zone becomes necessary it arrives with
//! the data system that needs one, not before.

/// The years a date may fall in.
///
/// Bounded at both ends so the four-digit written form is the whole domain: a year outside it could
/// not round-trip through [`Date::to_iso`], and a value that cannot be written back is a value that
/// will be rendered wrong into a bind parameter exactly once.
const YEAR_RANGE: core::ops::RangeInclusive<i16> = 1..=9999;

/// A calendar date, with no time and no zone.
///
/// Construct it with [`Date::parse`] or [`Date::new`]. The fields are private and ordered
/// year-month-day so the derived `Ord` is chronological: a reordering of the declaration would
/// silently invert every comparison, which is why the ordering is asserted in a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
pub struct Date {
    year: i16,
    month: u8,
    day: u8,
}

/// Why a date was rejected.
///
/// Each variant carries what was wrong as typed fields rather than a formatted sentence, and the
/// numeric parse failure keeps its cause: a discarded cause is the difference between "the month is
/// not a number" and knowing which character stopped it.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidDate {
    /// Not `YYYY-MM-DD`. Exactly one layout is accepted, because a parser that guesses between
    /// `03-04-2026` and `2026-04-03` guesses wrong for half the world.
    #[error("a date must be written YYYY-MM-DD: {value:?}")]
    Malformed { value: String },
    /// A component was not a number at all.
    #[error("the {component} in {value:?} is not a number")]
    NotANumber {
        value: String,
        component: &'static str,
        #[source]
        cause: core::num::ParseIntError,
    },
    /// A year outside [`YEAR_RANGE`].
    #[error("a year must be between {} and {}, not {year}", YEAR_RANGE.start(), YEAR_RANGE.end())]
    YearOutOfRange { year: i16 },
    /// A month outside 1 to 12.
    #[error("a month must be between 1 and 12, not {month}")]
    MonthOutOfRange { month: u8 },
    /// A well-formed date that does not exist, such as the 30th of February.
    #[error("{year:04}-{month:02}-{day:02} is not a date: month {month:02} has {days_in_month} days")]
    NoSuchDay {
        year: i16,
        month: u8,
        day: u8,
        days_in_month: u8,
    },
}

/// Days per month, January first. February is the value for a common year; [`Date::days_in_month`]
/// corrects it.
const DAYS_PER_MONTH: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

impl Date {
    /// Builds a date, rejecting a day the month does not have.
    ///
    /// Parse rather than validate: once this returns `Ok`, nothing downstream re-checks, because the
    /// 30th of February is unrepresentable rather than merely unwelcome.
    pub fn new(year: i16, month: u8, day: u8) -> Result<Self, InvalidDate> {
        if !YEAR_RANGE.contains(&year) {
            return Err(InvalidDate::YearOutOfRange { year });
        }
        if !(1..=12).contains(&month) {
            return Err(InvalidDate::MonthOutOfRange { month });
        }
        let days_in_month = Self::days_in_month(year, month);
        if day == 0 || day > days_in_month {
            return Err(InvalidDate::NoSuchDay {
                year,
                month,
                day,
                days_in_month,
            });
        }
        Ok(Self { year, month, day })
    }

    /// Parses `YYYY-MM-DD`.
    ///
    /// The widths are fixed at four-two-two so a short year cannot be read as a long one: `26-06-01`
    /// is rejected rather than becoming the year 26. Fixed widths also mean a leading `-` cannot
    /// reach the number parser, so a negative component is a layout error rather than a date in the
    /// distant past.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidDate> {
        let raw = raw.as_ref().trim();
        let mut parts = raw.split('-');
        let (Some(y), Some(m), Some(d), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(InvalidDate::Malformed {
                value: String::from(raw),
            });
        };
        if y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return Err(InvalidDate::Malformed {
                value: String::from(raw),
            });
        }
        // Parsed straight into the target width rather than through a wider type: four digits
        // cannot exceed `i16::MAX` and two cannot exceed `u8::MAX`, so there is no narrowing
        // conversion here to get wrong.
        let year = y.parse::<i16>().map_err(|cause| InvalidDate::NotANumber {
            value: String::from(raw),
            component: "year",
            cause,
        })?;
        let month = m.parse::<u8>().map_err(|cause| InvalidDate::NotANumber {
            value: String::from(raw),
            component: "month",
            cause,
        })?;
        let day = d.parse::<u8>().map_err(|cause| InvalidDate::NotANumber {
            value: String::from(raw),
            component: "day",
            cause,
        })?;
        Self::new(year, month, day)
    }

    /// How many days a month has, leap years included.
    fn days_in_month(year: i16, month: u8) -> u8 {
        if month == 2 && Self::is_leap_year(year) {
            return 29;
        }
        // `month` is 1 to 12 at every call site, and the `get`/`unwrap_or` shape keeps the
        // `indexing_slicing` ban satisfied without an `expect`. A panic path reachable from a
        // catalog file is exactly what that lint exists to prevent, and 0 days makes any day
        // invalid, which is the safe direction to fail in.
        DAYS_PER_MONTH.get(usize::from(month.saturating_sub(1))).copied().unwrap_or(0)
    }

    /// The proleptic Gregorian leap rule.
    ///
    /// The century exceptions are the whole reason this is a function: dropping them makes 1900 and
    /// 2100 leap years, which is a wrong answer for one day in four hundred years and therefore one
    /// nobody finds by trying it.
    ///
    /// `rem_euclid` rather than `%`: the remainder operator is banned by the lint table, and for a
    /// year inside [`YEAR_RANGE`] the two agree exactly.
    const fn is_leap_year(year: i16) -> bool {
        year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
    }

    #[inline]
    pub const fn year(self) -> i16 {
        self.year
    }

    #[inline]
    pub const fn month(self) -> u8 {
        self.month
    }

    #[inline]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// A date from a count of days since 1970-01-01.
    ///
    /// Needed because a data system returns a truncated date as a day number, and the alternative
    /// was casting the column to text inside the generated statement, which would bake one dialect's
    /// idea of a date format into every dialect's SQL.
    ///
    /// Walks a year at a time rather than dividing. That is not naivety about performance: integer
    /// division and the remainder operator are both banned by the lint table, the loop runs at most a
    /// few hundred times for any date this type can hold, and the leap rule stays in one place
    /// instead of being re-derived as a correction term.
    pub fn from_days_since_epoch(days: i32) -> Result<Self, InvalidDate> {
        const EPOCH_YEAR: i16 = 1970;
        let mut year = EPOCH_YEAR;
        let mut remaining = days;
        while remaining < 0 {
            year = year.saturating_sub(1);
            if !YEAR_RANGE.contains(&year) {
                return Err(InvalidDate::YearOutOfRange { year });
            }
            remaining = remaining.saturating_add(Self::days_in_year(year));
        }
        loop {
            let length = Self::days_in_year(year);
            if remaining < length {
                break;
            }
            remaining = remaining.saturating_sub(length);
            year = year.saturating_add(1);
            if !YEAR_RANGE.contains(&year) {
                return Err(InvalidDate::YearOutOfRange { year });
            }
        }
        let mut month: u8 = 1;
        loop {
            let length = i32::from(Self::days_in_month(year, month));
            if remaining < length {
                break;
            }
            remaining = remaining.saturating_sub(length);
            month = month.saturating_add(1);
            if month > 12 {
                // Unreachable: the year loop above left fewer days than the year holds. Written as a
                // branch rather than an `expect` because the no-panic ban is not conditional on the
                // loop above having been right.
                return Err(InvalidDate::MonthOutOfRange { month });
            }
        }
        // Saturating rather than fallible: `remaining` is less than the month length here, so it
        // always fits, and a saturated 255 would be rejected by `new` as a day no month has. That
        // keeps the conversion from needing an error variant for a case it cannot reach.
        let day = u8::try_from(remaining.saturating_add(1)).unwrap_or(u8::MAX);
        Self::new(year, month, day)
    }

    /// How many days a year has.
    const fn days_in_year(year: i16) -> i32 {
        if Self::is_leap_year(year) { 366 } else { 365 }
    }

    /// `YYYY-MM-DD`, which is what a bind parameter carries.
    pub fn to_iso(self) -> String {
        let (year, month, day) = (self.year, self.month, self.day);
        format!("{year:04}-{month:02}-{day:02}")
    }
}

impl TryFrom<String> for Date {
    type Error = InvalidDate;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(raw)
    }
}

impl core::fmt::Display for Date {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_iso())
    }
}

/// A bounded, half-open interval of dates: `start` included, `end` excluded.
///
/// **Bounded is the invariant, and it is why this type exists rather than a pair of `Option`s.** An
/// unbounded range is a table scan with a plausible name, and it is the shape a manipulated agent
/// asks for. There is no constructor that omits an endpoint.
///
/// Half-open rather than inclusive because a month is `[2026-06-01, 2026-07-01)` at every grain and
/// in every dialect, while an inclusive end needs a different last day per month and per grain. One
/// of those two conventions produces off-by-one bugs at month boundaries and the other does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "TimeRangeInput")]
pub struct TimeRange {
    start: Date,
    end: Date,
}

/// The deserialization shape for [`TimeRange`], so `serde(try_from)` has a struct to read into.
///
/// It exists only to be converted. Without it the derived `Deserialize` would write into
/// `TimeRange`'s private fields and skip the emptiness check below, and a catalog or question file
/// is exactly the input that check exists for.
#[derive(serde::Deserialize)]
struct TimeRangeInput {
    start: Date,
    end: Date,
}

/// Why a range was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidTimeRange {
    /// `end` is at or before `start`.
    ///
    /// Refused rather than normalised by swapping. A swapped range means the caller believes
    /// something we do not, and answering a different question than the one asked is worse than
    /// refusing: an empty result reads as "there was no revenue in June".
    #[error("a range must contain at least one day: {start} to {end} is empty")]
    Empty { start: Date, end: Date },
}

impl TimeRange {
    /// Builds a half-open range, rejecting an empty one.
    pub fn new(start: Date, end: Date) -> Result<Self, InvalidTimeRange> {
        if end <= start {
            return Err(InvalidTimeRange::Empty { start, end });
        }
        Ok(Self { start, end })
    }

    #[inline]
    pub const fn start(self) -> Date {
        self.start
    }

    #[inline]
    pub const fn end(self) -> Date {
        self.end
    }
}

impl TryFrom<TimeRangeInput> for TimeRange {
    type Error = InvalidTimeRange;

    fn try_from(input: TimeRangeInput) -> Result<Self, Self::Error> {
        Self::new(input.start, input.end)
    }
}

impl core::fmt::Display for TimeRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{}, {})", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::{Date, InvalidDate, InvalidTimeRange, TimeRange};

    fn date(raw: &str) -> Date {
        Date::parse(raw).expect("a test date is a date")
    }

    #[test]
    fn an_iso_date_parses_and_round_trips() {
        let d = date("2026-06-01");
        assert_eq!((d.year(), d.month(), d.day()), (2026, 6, 1));
        assert_eq!(d.to_iso(), "2026-06-01");
        assert_eq!(d.to_string(), "2026-06-01");
    }

    #[test]
    fn the_widths_are_fixed_so_a_short_year_is_not_guessed() {
        // The bug this prevents: accepting `26-06-01` and reading the year as 26 puts the date two
        // thousand years off, and every bounded predicate built from it silently matches nothing,
        // which reads as "there is no data" rather than as a mistake.
        assert_eq!(
            Date::parse("26-06-01").unwrap_err(),
            InvalidDate::Malformed {
                value: String::from("26-06-01")
            }
        );
        assert_eq!(
            Date::parse("2026-6-1").unwrap_err(),
            InvalidDate::Malformed {
                value: String::from("2026-6-1")
            }
        );
    }

    #[test]
    fn a_negative_component_is_a_layout_error_not_a_date() {
        // Fixed widths are what stop `-` reaching the number parser. Without them `2026--6-01`
        // would parse a negative month and only fail later, in a range check that reports the wrong
        // thing.
        assert_eq!(
            Date::parse("2026--6-01").unwrap_err(),
            InvalidDate::Malformed {
                value: String::from("2026--6-01")
            }
        );
    }

    #[test]
    fn a_date_is_not_a_place_to_hide_sql() {
        // Dates reach the statement as bind parameters, but they are parsed here first: a type that
        // accepted arbitrary text would leave the parameterisation as the only defence, and then a
        // single generator that forgot to bind would be an injection.
        drop(Date::parse("2026-06-01' OR '1'='1").expect_err("a quote must not survive into a date"));
        drop(Date::parse("").expect_err("empty is not a date"));
        let malformed = Date::parse("20xx-06-01").expect_err("letters are not a year");
        assert!(matches!(malformed, InvalidDate::NotANumber { component: "year", .. }));
    }

    #[test]
    fn a_numeric_parse_failure_keeps_its_cause() {
        // `map_err(|_| ..)` is banned for a reason: the cause is what says whether the component
        // was empty, overflowed or held a letter, and the variant alone does not.
        let err = Date::parse("2026-ab-01").expect_err("letters are not a month");
        assert!(
            core::error::Error::source(&err).is_some(),
            "the parse failure must stay reachable as a source: {err:?}"
        );
    }

    #[test]
    fn a_day_the_month_does_not_have_is_rejected() {
        assert_eq!(
            Date::parse("2026-02-30").unwrap_err(),
            InvalidDate::NoSuchDay {
                year: 2026,
                month: 2,
                day: 30,
                days_in_month: 28,
            }
        );
        assert_eq!(
            Date::new(2026, 13, 1).unwrap_err(),
            InvalidDate::MonthOutOfRange { month: 13 }
        );
        drop(Date::new(2026, 6, 0).expect_err("there is no zeroth of June"));
    }

    #[test]
    fn a_year_that_could_not_be_written_back_is_rejected() {
        // `to_iso` renders four digits. A year outside the range would round-trip to a different
        // date, and the place that would go wrong is a bind parameter, silently.
        assert_eq!(Date::new(0, 6, 1).unwrap_err(), InvalidDate::YearOutOfRange { year: 0 });
        assert_eq!(Date::new(-5, 6, 1).unwrap_err(), InvalidDate::YearOutOfRange { year: -5 });
    }

    #[test]
    fn the_century_exceptions_to_the_leap_rule_hold() {
        // Dropping them makes 1900 and 2100 leap years: one wrong day per four hundred years, so
        // nobody finds it by trying it.
        assert_eq!(Date::new(2024, 2, 29).expect("2024 is a leap year").day(), 29);
        assert_eq!(
            Date::new(2000, 2, 29)
                .expect("2000 is a leap year, being divisible by 400")
                .day(),
            29
        );
        drop(Date::new(1900, 2, 29).expect_err("1900 is not a leap year"));
        drop(Date::new(2100, 2, 29).expect_err("2100 is not a leap year"));
        drop(Date::new(2026, 2, 29).expect_err("2026 is not a leap year"));
    }

    #[test]
    fn a_day_number_becomes_the_date_it_names() {
        // A data system returns a truncated date as days since the epoch, and this is the only
        // conversion. An off-by-one here would move every bucket in every answer by a day, which is
        // the sort of wrongness that looks like a data problem for a week.
        assert_eq!(Date::from_days_since_epoch(0).expect("day zero"), date("1970-01-01"));
        assert_eq!(Date::from_days_since_epoch(1).expect("day one"), date("1970-01-02"));
        assert_eq!(Date::from_days_since_epoch(31).expect("february"), date("1970-02-01"));
        assert_eq!(Date::from_days_since_epoch(365).expect("next year"), date("1971-01-01"));
        // 2026-06-01 and 2026-07-01: the fixture range, so the number the anchor test reads back.
        assert_eq!(Date::from_days_since_epoch(20_605).expect("june"), date("2026-06-01"));
        assert_eq!(Date::from_days_since_epoch(20_635).expect("july"), date("2026-07-01"));
    }

    #[test]
    fn a_day_number_crosses_a_leap_day_correctly() {
        // 2024 is a leap year. A conversion that assumed 365-day years would put every date after
        // February a day early from here on.
        let leap_day = date("2024-02-29");
        let day_number = 19_782_i32;
        assert_eq!(Date::from_days_since_epoch(day_number).expect("a real day"), leap_day);
        assert_eq!(
            Date::from_days_since_epoch(day_number.saturating_add(1)).expect("the next day"),
            date("2024-03-01")
        );
    }

    #[test]
    fn a_negative_day_number_walks_backwards() {
        assert_eq!(Date::from_days_since_epoch(-1).expect("the day before"), date("1969-12-31"));
        assert_eq!(Date::from_days_since_epoch(-365).expect("a year before"), date("1969-01-01"));
    }

    #[test]
    fn a_day_number_outside_the_year_range_is_rejected_rather_than_wrapping() {
        // The loop is bounded by YEAR_RANGE, so an absurd day count is an error rather than a very
        // long wait or a wrapped year.
        drop(Date::from_days_since_epoch(i32::MAX).expect_err("year 5 million is not a year"));
        drop(Date::from_days_since_epoch(i32::MIN).expect_err("neither is its negative"));
    }

    #[test]
    fn ordering_is_chronological_and_not_field_order_by_accident() {
        // The derived `Ord` reads the fields in declaration order. Reordering the struct would
        // invert every comparison the resolver makes, with no compile error anywhere.
        assert!(date("2026-01-31") < date("2026-02-01"));
        assert!(date("2025-12-31") < date("2026-01-01"));
        assert!(date("2026-06-01") < date("2026-06-02"));
    }

    #[test]
    fn a_range_needs_at_least_one_day() {
        // Refused rather than swapped: answering a different question than the one asked returns an
        // empty result, which reads as "there was no revenue" rather than as a mistake.
        assert_eq!(
            TimeRange::new(date("2026-07-01"), date("2026-06-01")).unwrap_err(),
            InvalidTimeRange::Empty {
                start: date("2026-07-01"),
                end: date("2026-06-01"),
            }
        );
        // Equal endpoints are the case a half-open range makes empty rather than one day long,
        // which is the boundary an inclusive convention gets wrong.
        assert_eq!(
            TimeRange::new(date("2026-06-01"), date("2026-06-01")).unwrap_err(),
            InvalidTimeRange::Empty {
                start: date("2026-06-01"),
                end: date("2026-06-01"),
            }
        );
        let r = TimeRange::new(date("2026-06-01"), date("2026-07-01")).expect("June is a range");
        assert_eq!(r.to_string(), "[2026-06-01, 2026-07-01)");
        assert_eq!(r.start(), date("2026-06-01"));
        assert_eq!(r.end(), date("2026-07-01"));
    }

    /// Deserialize without a format crate: the boundary gate allowlists none, and this tests the
    /// wiring rather than a parser.
    fn deserialize_date(raw: &str) -> Result<Date, serde::de::value::Error> {
        use serde::Deserialize as _;
        use serde::de::IntoDeserializer as _;

        Date::deserialize(String::from(raw).into_deserializer())
    }

    #[test]
    fn deserialization_goes_through_the_constructor() {
        // A catalog file and a question file both arrive this way, so the derive is the path that
        // matters: without `serde(try_from)` it writes past every check above.
        drop(deserialize_date("2026-02-30").expect_err("an impossible day must not deserialize"));
        drop(deserialize_date("nope").expect_err("prose must not deserialize into a date"));
        assert_eq!(
            deserialize_date("2026-06-01").expect("a real date still deserializes"),
            date("2026-06-01")
        );
    }
}
