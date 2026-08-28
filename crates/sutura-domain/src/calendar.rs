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
///
/// **`try_from` and `into` are a pair, and one without the other was a real asymmetry.** This type
/// carried `try_from = "String"` alone, and `serde(try_from)` affects `Deserialize` only - so the
/// derived `Serialize` wrote the STRUCT, and a date this crate serialized was a date this crate's
/// own `Deserialize` rejected. Two places depend on the two halves agreeing: the digest in
/// [`crate::definitions`] is taken over the serialized form, so it has to be taken over the ISO text
/// a catalog author actually wrote rather than over a field layout that never appears in a file; and
/// a schema generated from this type describes a wire value the surface accepts as a string. Every
/// other type here with a canonical text form is written the same way - `TermRepr` in
/// [`crate::measure`] pairs them so that what a digest covers and what a catalog wrote are the same
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
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
    /// A year outside the range this type accepts, which is 1 to 9999 so the four-digit written
    /// form is the whole domain.
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

/// Whether a component of a written date is digits and nothing else.
///
/// Bytes rather than characters, and that pairs with how the widths are measured: [`Date::parse`]
/// checks `str::len`, which is a byte count, so a component that passed a four-byte width while
/// holding one multi-byte character has to fail here rather than be measured a second way.
/// `is_ascii_digit` is deliberately narrower than `char::is_numeric`, which is true for digits in
/// scripts the integer parser does not read.
fn is_all_ascii_digits(component: &str) -> bool {
    !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
}

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
    ///
    /// **A width alone was not enough, and that was a real hole.** `i16::from_str` and
    /// `u8::from_str` both accept a leading `+`, so `+026-06-01` measured four wide and parsed as
    /// the year 26 - exactly what the fixed width exists to refuse - and `2026-+6-+1` parsed as the
    /// 1st of June. Every byte of every component has to be an ASCII digit, so a sign cannot occupy
    /// the column a digit was supposed to be in.
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
        // A layout error, not a number that came out wrong, which is why it is `Malformed` and not
        // `NotANumber`: `+026` is not a four-digit year written badly, it is three digits and a
        // sign in a field that has room for four digits.
        if !(is_all_ascii_digits(y) && is_all_ascii_digits(m) && is_all_ascii_digits(d)) {
            return Err(InvalidDate::Malformed {
                value: String::from(raw),
            });
        }
        // Parsed straight into the target width rather than through a wider type: four digits
        // cannot exceed `i16::MAX` and two cannot exceed `u8::MAX`, so there is no narrowing
        // conversion here to get wrong.
        //
        // The two checks above leave nothing for these three to reject - four ASCII digits fit an
        // `i16` and two fit a `u8` - so `NotANumber` is unreachable from here BY CONSTRUCTION
        // rather than by accident. The variant stays because `from_str` still returns a `Result`,
        // and both ways of not handling one are banned in this workspace: `map_err(|_| ..)` throws
        // away the cause and `expect` is a panic path reachable from a catalog file.
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
    /// year inside the accepted range the two agree exactly.
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

    /// Days since 1970-01-01, which is how a columnar engine stores a date.
    ///
    /// The inverse of [`Date::from_days_since_epoch`], and the two are asserted to round-trip. It
    /// exists because an in-process engine takes a date as an `i32` day count rather than as text:
    /// there is no statement for a date literal to be written into, so the value is handed over as
    /// the number the column actually holds.
    ///
    /// Counted by walking years, for the same reason the inverse does: integer division and the
    /// remainder operator are both banned by the lint table, the loop runs at most a few hundred
    /// times for any date this type can hold, and the leap rule stays in one place.
    pub fn days_since_epoch(self) -> i32 {
        const EPOCH_YEAR: i16 = 1970;
        let mut days: i32 = 0;
        let mut year = EPOCH_YEAR;
        while year < self.year {
            days = days.saturating_add(Self::days_in_year(year));
            year = year.saturating_add(1);
        }
        while year > self.year {
            year = year.saturating_sub(1);
            days = days.saturating_sub(Self::days_in_year(year));
        }
        let mut month: u8 = 1;
        while month < self.month {
            days = days.saturating_add(i32::from(Self::days_in_month(self.year, month)));
            month = month.saturating_add(1);
        }
        days.saturating_add(i32::from(self.day).saturating_sub(1))
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

/// The written form, which is what `serde(into = "String")` serializes.
///
/// The same rendering as [`Date::to_iso`] and [`Display`](core::fmt::Display) rather than a second
/// one: a type with two ways of writing itself down eventually writes one of them into a digest and
/// the other into an answer.
impl From<Date> for String {
    fn from(date: Date) -> Self {
        date.to_iso()
    }
}

impl core::fmt::Display for Date {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_iso())
    }
}

/// A half-open interval of dates: `start` included, `end` excluded.
///
/// **Both endpoints are always present, and that is the whole of what this type promises.** There is
/// no constructor that omits one, so an unbounded range is unrepresentable rather than refused.
///
/// **What it does not promise is that the interval is small.** `[0001-01-01, 9999-12-31)` satisfies
/// every check here, and a predicate built from it reads the whole table - which is the shape a
/// manipulated agent asks for. An earlier version of this comment claimed the newtype prevented a
/// table scan; it prevents an *absent* bound, and nothing more. The size of the interval a *caller*
/// may ask for is capped where a caller's question is resolved, against
/// [`crate::query::MAX_RANGE_DAYS`], and refused as
/// [`crate::query::RefusalReason::TimeRangeTooLong`].
///
/// **The cap is deliberately not on this constructor**, and the reason is who each caller is. This
/// same type is also a metric's anchor range, authored in a catalog by the person who defines the
/// metric - not requested by an agent, not on the hot path, and executed once at startup. A catalog
/// author who wants a decade-long anchor is not the threat the cap exists for, and a hard maximum
/// here would make a governance decision about requests by constraining authorship. Use
/// [`TimeRange::days`] to measure a range; decide what is too long where you know whose range it is.
///
/// Half-open rather than inclusive because a month is `[2026-06-01, 2026-07-01)` at every grain and
/// in every dialect, while an inclusive end needs a different last day per month and per grain. One
/// of those two conventions produces off-by-one bugs at month boundaries and the other does not.
///
/// **No `into` beside the `try_from`, unlike [`Date`], and that is not the same omission.** A range's
/// wire form is a two-field mapping - `start` and `end`, which is how a catalog author writes one -
/// and both halves already agree on it: `Serialize` derives that mapping and `try_from` reads it back
/// through [`TimeRange::new`]. What this type does NOT have is a canonical text form to convert into.
/// [`Display`](core::fmt::Display) renders `[2026-06-01, 2026-07-01)` for a human reading a refusal,
/// and nothing parses that shape, so serializing into it would produce exactly the asymmetry the
/// `into` on `Date` exists to remove. The round trip that has to hold here is the mapping one, and it
/// is asserted as such.
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

    /// How many days the interval covers.
    ///
    /// Always at least 1, because the constructor refuses `end <= start`. This is the number a cost
    /// bound has to be expressed in: rows read are a function of how much history the date predicate
    /// admits, and *not* of the grain, which decides how the admitted rows are grouped afterwards.
    /// A year of history is a year of scanning whether it comes back as 365 buckets or as 1.
    ///
    /// Derived from [`Date::days_since_epoch`] rather than from a second piece of calendar
    /// arithmetic, so a leap year cannot be counted one way here and another way there.
    /// `saturating_sub` because the subtraction is checked-by-construction - `end > start`, and both
    /// day numbers are within the range a four-digit year can reach - and a saturated value would
    /// still be refused by any cap rather than wrapping into a small one.
    pub fn days(self) -> i32 {
        self.end.days_since_epoch().saturating_sub(self.start.days_since_epoch())
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
        // And the form that defeated the width check while satisfying it. `i16::from_str` and
        // `u8::from_str` accept a leading `+`, so `+026` is four bytes wide and parses as 26: the
        // sign padded the field out to the width instead of a digit, and the caller got a bounded
        // range two thousand years in the past that every predicate matches nothing in.
        assert_eq!(
            Date::parse("+026-06-01").unwrap_err(),
            InvalidDate::Malformed {
                value: String::from("+026-06-01")
            }
        );
        assert_eq!(
            Date::parse("2026-+6-+1").unwrap_err(),
            InvalidDate::Malformed {
                value: String::from("2026-+6-+1")
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
        // A layout error rather than a number that came out wrong: nothing but ASCII digits reaches
        // the number parser, so `20xx` is refused by shape and the variant says so.
        let malformed = Date::parse("20xx-06-01").expect_err("letters are not a year");
        assert!(
            matches!(malformed, InvalidDate::Malformed { .. }),
            "letters must be refused by layout, not by the number parser: {malformed:?}"
        );
    }

    #[test]
    fn a_component_that_is_not_digits_is_refused_before_the_number_parser_sees_it() {
        // Everything a component could hold besides four-or-two ASCII digits, and each one is a
        // layout error. `NotANumber` is therefore unreachable through `parse` by construction -
        // which is the point of the digit gate, not an oversight in it.
        for raw in ["2026-ab-01", "20xx-06-01", "+026-06-01", "2026-+6-01", "2026-06-+1"] {
            assert_eq!(
                Date::parse(raw).expect_err("not digits"),
                InvalidDate::Malformed {
                    value: String::from(raw)
                },
                "{raw}"
            );
        }
        // The variant survives anyway, because `from_str` still returns a `Result` and neither
        // `map_err(|_| ..)` nor `expect` is allowed to absorb one here. This asserts the half of it
        // that still matters: it carries its cause rather than a formatted sentence, so `#[source]`
        // cannot be dropped from the field without a failure here.
        let cause = "ab".parse::<u8>().expect_err("letters are not a number");
        let err = InvalidDate::NotANumber {
            value: String::from("2026-ab-01"),
            component: "month",
            cause,
        };
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
    fn a_date_round_trips_through_its_day_number() {
        // The two conversions are inverses, and the in-process engine depends on it: it is handed a
        // day count and its results come back as day counts, so a mismatch would move every bucket
        // in every answer by a fixed offset - the kind of wrong that looks like a data problem.
        for iso in [
            "1970-01-01",
            "1970-01-02",
            "1969-12-31",
            "2024-02-29",
            "2024-03-01",
            "2026-06-01",
            "2026-07-01",
            "1900-03-01",
            "2100-03-01",
        ] {
            let original = date(iso);
            let days = original.days_since_epoch();
            assert_eq!(
                Date::from_days_since_epoch(days).expect("a round trip is a date"),
                original,
                "{iso} did not survive {days}"
            );
        }
        // And the absolute values, so a consistent-but-shifted pair of conversions cannot pass.
        assert_eq!(date("1970-01-01").days_since_epoch(), 0);
        assert_eq!(date("1970-01-02").days_since_epoch(), 1);
        assert_eq!(date("1969-12-31").days_since_epoch(), -1);
        assert_eq!(date("2026-06-01").days_since_epoch(), 20_605);
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

    #[test]
    fn a_range_reports_the_span_a_cost_bound_is_expressed_in() {
        // `days()` is what `query::MAX_RANGE_DAYS` is compared against, so an off-by-one or a
        // mis-counted leap day here moves the governance boundary rather than a display value.
        fn span(start: &str, end: &str) -> i32 {
            TimeRange::new(date(start), date(end))
                .expect("a test range is a range")
                .days()
        }
        // One day is the smallest a range can be, because the constructor refuses an empty one.
        assert_eq!(span("2026-06-01", "2026-06-02"), 1);
        assert_eq!(span("2026-06-01", "2026-07-01"), 30);
        // A common year and a leap year, so the leap rule is counted rather than approximated.
        assert_eq!(span("2026-01-01", "2027-01-01"), 365);
        assert_eq!(span("2024-01-01", "2025-01-01"), 366);
        // The two ten-year windows that bracket the cap: three leap days is the longest ten calendar
        // years there is, which is the number `MAX_RANGE_DAYS` is set to.
        assert_eq!(span("2020-01-01", "2030-01-01"), 3653);
        assert_eq!(span("2021-01-01", "2031-01-01"), 3652);
        // And the whole domain, which is the range the "bounded" newtype accepts and a cap has to
        // refuse: about ten thousand years of history from one question.
        assert_eq!(span("0001-01-01", "9999-12-31"), 3_652_058);
    }

    /// Deserialize straight from a string, without a format in between: this asserts the wiring -
    /// that the derive routes through the constructor - rather than a parser's handling of quotes.
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

    #[test]
    fn a_date_serializes_into_the_shape_its_own_deserialize_accepts() {
        // THE ASYMMETRY THIS TEST EXISTS FOR. `serde(try_from)` affects `Deserialize` ONLY, so with
        // `try_from = "String"` alone the derived `Serialize` wrote the struct -
        // `{"year":2026,"month":6,"day":1}` - which this type's own `Deserialize` then refused,
        // because it wants a string. Nothing round-tripped a date in production, so it stayed
        // invisible; what it was quietly deciding is written down in two places. The digest in
        // `definitions` is taken over the SERIALIZED form, so it covered a field layout that appears
        // in no file rather than the ISO text an author wrote; and a schema derived from this type
        // would describe an object for a value the surface accepts as a string.
        let d = date("2026-06-01");
        let json = serde_json::to_string(&d).expect("a date serializes");
        assert_eq!(json, "\"2026-06-01\"", "the wire form is the written form, not the fields");
        assert_eq!(
            serde_json::from_str::<Date>(&json).expect("and deserializes from what it wrote"),
            d
        );
        // The property in the form the rule states it, through the text form rather than through a
        // format crate: `parse(to_string(x)) == x`.
        assert_eq!(Date::parse(d.to_string()).expect("its own written form is a date"), d);
        assert_eq!(String::from(d), d.to_iso(), "one rendering of a date, not two");
        // A date that needs padding in three places at once, because zero-padding is exactly what a
        // struct-shaped form loses and a text one has to keep.
        let padded = date("0001-02-03");
        assert_eq!(serde_json::to_string(&padded).expect("serializes"), "\"0001-02-03\"");
        assert_eq!(Date::parse(padded.to_string()).expect("padded is a date"), padded);
        // And the leap day, which is the value a round trip through a re-parse has to survive rather
        // than be normalised away.
        let leap = date("2024-02-29");
        assert_eq!(
            serde_json::from_str::<Date>(&serde_json::to_string(&leap).expect("serializes")).expect("deserializes"),
            leap
        );
    }

    #[test]
    fn a_range_round_trips_through_the_mapping_a_catalog_author_writes() {
        // A range's canonical form is the two-field mapping, not the `[start, end)` text: that text
        // is for a human reading a refusal and nothing parses it. So the round trip asserted here is
        // the one that has to hold - and before `Date` paired its `into` with its `try_from`, it
        // failed on the FIRST field, which is how a latent asymmetry in one type becomes a broken
        // round trip in every type that carries it.
        let range = TimeRange::new(date("2026-06-01"), date("2026-07-01")).expect("June is a range");
        let json = serde_json::to_string(&range).expect("a range serializes");
        assert_eq!(json, "{\"start\":\"2026-06-01\",\"end\":\"2026-07-01\"}");
        assert_eq!(
            serde_json::from_str::<TimeRange>(&json).expect("and deserializes from what it wrote"),
            range
        );
        // Back in through the constructor rather than past it, so a round trip cannot be the way an
        // empty range gets built.
        drop(
            serde_json::from_str::<TimeRange>("{\"start\":\"2026-07-01\",\"end\":\"2026-06-01\"}")
                .expect_err("a backwards range must not deserialize, round trip or not"),
        );
        // The human form is deliberately not the wire form, and this is the assertion that says so.
        assert_eq!(range.to_string(), "[2026-06-01, 2026-07-01)");
        assert_ne!(json, range.to_string());
    }
}
