//! Oracle's `DATE` and `TIMESTAMP` values, to and from the domain's.

use sutura_domain::calendar::Date;
use sutura_domain::warehouse::Value;

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

/// A `DATE`, `TIMESTAMP` or `TIMESTAMP WITH TIME ZONE` value, back to a domain [`Value::Text`].
///
/// Midnight with no fraction answers the ISO day - the reverse of [`oracle_date`], and how the
/// domain's Arrow reader renders a Postgres `DATE`. Any other time answers the ISO-8601 date-time
/// `YYYY-MM-DDTHH:MM:SS`, then `.` and the fraction with its trailing zeros trimmed when the
/// nanoseconds are not zero. For `TIMESTAMP WITH TIME ZONE` the driver hands the fields in UTC
/// beside the value's own offset, so the text is the wall-clock time in that offset, then
/// `+HH:MM` or `-HH:MM`, and is never shortened to a day.
pub(crate) fn date_cell(ts: &oracledb::OracleTimestamp, db_type: &oracledb::DbType) -> Result<Value, OracleError> {
    let zoned = db_type == oracledb::DB_TYPE_TIMESTAMP_TZ;
    let offset = if zoned {
        i32::from(ts.tz_hour_offset()) * 60 + i32::from(ts.tz_minute_offset())
    } else {
        0
    };
    let minutes = i32::from(ts.hour()) * 60 + i32::from(ts.minute()) + offset;
    let shift = minutes.div_euclid(1440);
    let day = Date::new(ts.year(), ts.month(), ts.day())
        .and_then(|day| Date::from_days_since_epoch(day.days_since_epoch() + shift))
        .map_err(|_cause| OracleError::UnsupportedType {
            column: String::from("<date>"),
            oracle_type: "a date this build cannot represent",
        })?;
    let (minutes, second, nanos) = (minutes.rem_euclid(1440), ts.second(), ts.nanoseconds());
    if !zoned && minutes == 0 && second == 0 && nanos == 0 {
        return Ok(Value::Text(day.to_iso()));
    }
    let fraction = if nanos == 0 {
        String::new()
    } else {
        String::from(format!(".{nanos:09}").trim_end_matches('0'))
    };
    let zone = if zoned {
        let sign = if offset < 0 { '-' } else { '+' };
        let offset = offset.unsigned_abs();
        format!("{sign}{:02}:{:02}", offset.div_euclid(60), offset.rem_euclid(60))
    } else {
        String::new()
    };
    let (hour, minute) = (minutes.div_euclid(60), minutes.rem_euclid(60));
    Ok(Value::Text(format!(
        "{day}T{hour:02}:{minute:02}:{second:02}{fraction}{zone}"
    )))
}

#[cfg(test)]
mod tests {
    use sutura_domain::warehouse::Value;

    use super::date_cell;

    #[test]
    fn a_date_with_a_time_of_day_keeps_it() {
        use oracledb::{DB_TYPE_DATE, DB_TYPE_TIMESTAMP, DB_TYPE_TIMESTAMP_TZ, OracleTimestamp};
        let text = |ts: OracleTimestamp, db_type| date_cell(&ts, db_type).expect("the date is representable");
        let at = |raw: &str| Value::Text(String::from(raw));
        assert_eq!(
            text(OracleTimestamp::new_timestamp(2026, 10, 11, 13, 45, 30, 0), DB_TYPE_DATE),
            at("2026-10-11T13:45:30")
        );
        assert_eq!(text(OracleTimestamp::new_date(2026, 10, 11), DB_TYPE_DATE), at("2026-10-11"));
        assert_eq!(
            text(
                OracleTimestamp::new_timestamp(2026, 10, 11, 0, 0, 0, 456_000_000),
                DB_TYPE_TIMESTAMP
            ),
            at("2026-10-11T00:00:00.456")
        );
        // The driver's own reading of `2025-10-31 11:52:15.456 -06:00`: UTC fields beside the offset.
        assert_eq!(
            text(
                OracleTimestamp::new_timestamp_tz(2025, 10, 31, 17, 52, 15, 456_000_000, -6, 0),
                DB_TYPE_TIMESTAMP_TZ
            ),
            at("2025-10-31T11:52:15.456-06:00")
        );
        assert_eq!(
            text(
                OracleTimestamp::new_timestamp_tz(2026, 10, 11, 23, 30, 0, 0, 5, 30),
                DB_TYPE_TIMESTAMP_TZ
            ),
            at("2026-10-12T05:00:00+05:30")
        );
        assert_eq!(
            text(
                OracleTimestamp::new_timestamp_tz(2026, 1, 1, 2, 0, 0, 0, -3, -30),
                DB_TYPE_TIMESTAMP_TZ
            ),
            at("2025-12-31T22:30:00-03:30")
        );
        assert_eq!(
            text(OracleTimestamp::new_date(2026, 10, 11), DB_TYPE_TIMESTAMP_TZ),
            at("2026-10-11T00:00:00+00:00")
        );
    }
}
