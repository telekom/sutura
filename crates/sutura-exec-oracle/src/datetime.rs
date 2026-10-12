//! Oracle's `DATE` and `TIMESTAMP` values, to and from the domain's.

use sutura_domain::calendar::Date;

use crate::OracleError;

/// A domain [`Date`], as the driver's `OracleTimestamp::new_date` wants it - via the date's own ISO
/// text rather than a day-offset arithmetic, because [`Date`] exposes no field accessors and its ISO
/// form is fixed-width.
pub(crate) fn oracle_date(date: Date) -> oracledb::OracleTimestamp {
    let iso = date.to_iso();
    let mut parts = iso.split('-');
    let year: i16 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let month: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let day: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    oracledb::OracleTimestamp::new_date(year, month, day)
}

const MICROS_PER_SECOND: i64 = 1_000_000;
const MICROS_PER_DAY: i64 = 86_400 * MICROS_PER_SECOND;

/// A `DATE`, `TIMESTAMP` or `TIMESTAMP WITH TIME ZONE` value as the driver's Arrow batch hands it -
/// `micros` since the epoch, in UTC for a zoned value, beside that value's own `offset_minutes` -
/// to its domain text.
///
/// Midnight with no fraction answers the ISO day - the reverse of [`oracle_date`], and how the
/// domain's Arrow reader renders a Postgres `DATE`. Any other time answers the ISO-8601 date-time
/// `YYYY-MM-DDTHH:MM:SS`, then `.` and the fraction with its trailing zeros trimmed when it is not
/// zero. A zoned value is the wall-clock time in its own offset, then `+HH:MM` or `-HH:MM`, and is
/// never shortened to a day.
pub(crate) fn date_text(micros: i64, offset_minutes: Option<i16>) -> Result<String, OracleError> {
    let local = micros + i64::from(offset_minutes.unwrap_or(0)) * 60 * MICROS_PER_SECOND;
    let day = i32::try_from(local.div_euclid(MICROS_PER_DAY))
        .ok()
        .and_then(|days| Date::from_days_since_epoch(days).ok())
        .ok_or_else(|| OracleError::UnsupportedType {
            column: String::from("<date>"),
            oracle_type: "a date this build cannot represent",
        })?;
    let into_day = local.rem_euclid(MICROS_PER_DAY);
    if offset_minutes.is_none() && into_day == 0 {
        return Ok(day.to_iso());
    }
    let (seconds, fraction) = (into_day.div_euclid(MICROS_PER_SECOND), into_day.rem_euclid(MICROS_PER_SECOND));
    let fraction = if fraction == 0 {
        String::new()
    } else {
        String::from(format!(".{fraction:06}").trim_end_matches('0'))
    };
    let zone = offset_minutes.map_or_else(String::new, |offset| {
        let sign = if offset < 0 { '-' } else { '+' };
        let offset = offset.unsigned_abs();
        format!("{sign}{:02}:{:02}", offset.div_euclid(60), offset.rem_euclid(60))
    });
    let (hour, minute, second) = (
        seconds.div_euclid(3600),
        seconds.div_euclid(60).rem_euclid(60),
        seconds.rem_euclid(60),
    );
    Ok(format!("{day}T{hour:02}:{minute:02}:{second:02}{fraction}{zone}"))
}

/// A `TIMESTAMP WITH LOCAL TIME ZONE` column, refused by name: the driver hands it in the session
/// zone with no offset.
pub(crate) fn local_zone_refused(column: &str) -> OracleError {
    OracleError::UnsupportedType {
        column: String::from(column),
        oracle_type: "TIMESTAMP WITH LOCAL TIME ZONE; the driver hands it in the session zone with no offset",
    }
}

#[cfg(test)]
mod tests {
    use super::date_text;

    #[test]
    fn a_date_with_a_time_of_day_keeps_it() {
        let text = |micros, offset| date_text(micros, offset).expect("the date is representable");
        // 2026-10-11T13:45:30, its midnight, and 0.456 s past it, as microseconds since the epoch.
        assert_eq!(text(1_791_726_330_000_000, None), "2026-10-11T13:45:30");
        assert_eq!(text(1_791_676_800_000_000, None), "2026-10-11");
        assert_eq!(text(1_791_676_800_456_000, None), "2026-10-11T00:00:00.456");
        // The driver's own reading of `2025-10-31 11:52:15.456 -06:00`: the UTC instant beside the offset.
        assert_eq!(text(1_761_933_135_456_000, Some(-360)), "2025-10-31T11:52:15.456-06:00");
        // 2026-10-11T23:30:00Z at +05:30.
        assert_eq!(text(1_791_761_400_000_000, Some(330)), "2026-10-12T05:00:00+05:30");
        // 2026-01-01T02:00:00Z at -03:30, a negative offset with minutes.
        assert_eq!(text(1_767_232_800_000_000, Some(-210)), "2025-12-31T22:30:00-03:30");
        assert_eq!(text(1_791_676_800_000_000, Some(0)), "2026-10-11T00:00:00+00:00");
    }
}
