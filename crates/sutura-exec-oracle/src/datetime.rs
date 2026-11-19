//! Oracle's `DATE` and `TIMESTAMP` values, to and from the domain's.

use sutura_domain::calendar::Date;

use crate::{Dial, OracleError, connect_err};
use sutura_domain::identity::Secret;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_oracle_connect::{DialError, PrepareError};

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

/// A naive `DATE` or `TIMESTAMP` value as microseconds since the epoch, rendered as domain text.
///
/// Midnight with no fraction answers the ISO day - the reverse of [`oracle_date`], and how the
/// domain's Arrow reader renders a Postgres `DATE`. Any other time answers the ISO-8601 date-time
/// `YYYY-MM-DDTHH:MM:SS`, then `.` and the fraction with its trailing zeros trimmed when it is not
/// zero.
pub(crate) fn date_text(micros: i64) -> Result<String, OracleError> {
    let day = i32::try_from(micros.div_euclid(MICROS_PER_DAY))
        .ok()
        .and_then(|days| Date::from_days_since_epoch(days).ok())
        .ok_or_else(|| OracleError::UnsupportedType {
            column: String::from("<date>"),
            oracle_type: "a date this build cannot represent",
        })?;
    let into_day = micros.rem_euclid(MICROS_PER_DAY);
    if into_day == 0 {
        return Ok(day.to_iso());
    }
    let (seconds, fraction) = (into_day.div_euclid(MICROS_PER_SECOND), into_day.rem_euclid(MICROS_PER_SECOND));
    let fraction = if fraction == 0 {
        String::new()
    } else {
        String::from(format!(".{fraction:06}").trim_end_matches('0'))
    };
    let (hour, minute, second) = (
        seconds.div_euclid(3600),
        seconds.div_euclid(60).rem_euclid(60),
        seconds.rem_euclid(60),
    );
    Ok(format!("{day}T{hour:02}:{minute:02}:{second:02}{fraction}"))
}

/// A zoned instant always includes a time and UTC suffix, including midnight.
pub(crate) fn utc_text(micros: i64) -> Result<String, OracleError> {
    let text = date_text(micros)?;
    Ok(if micros.rem_euclid(MICROS_PER_DAY) == 0 {
        format!("{text}T00:00:00Z")
    } else {
        format!("{text}Z")
    })
}

impl crate::TokenSessions {
    /// The driver's configuration for `dial`: its address, its channel and anchors, the dial bound, and a refused redirect.
    ///
    /// # Errors
    ///
    /// [`OracleError::PacketTraceOn`] while the driver's packet trace is switched on,
    /// [`OracleError::TrustAnchors`] for anchors that are not usable certificates, and
    /// [`OracleError::Connect`] for an address the driver cannot parse.
    pub fn new(dial: Dial<'_>) -> Result<Self, OracleError> {
        dial.config().map(Self).map_err(|refused| match refused {
            DialError::PacketTraceOn(on) => OracleError::PacketTraceOn(on),
            DialError::Prepare(PrepareError::TrustAnchors(cause)) => OracleError::TrustAnchors { cause: cause.into() },
            DialError::Prepare(PrepareError::Address(cause)) => connect_err(cause),
        })
    }

    /// Opens one session that authenticates with `token`. Dropping it closes the session.
    ///
    /// # Errors
    ///
    /// [`OracleError::RedirectRefused`] for a listener that redirects, and [`OracleError::Connect`]
    /// for every other refused dial or login. Neither carries the token.
    pub fn open(&self, token: &Secret) -> Result<oracledb::Connection, OracleError> {
        self.open_before(token, None)
    }

    pub(crate) fn open_before(&self, token: &Secret, deadline: Option<Deadline>) -> Result<oracledb::Connection, OracleError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the asker's token is what this session authenticates with, so it is handed to the \
                      driver once, here; nothing on this path logs or formats it"
        )]
        let token = String::from(token.expose_secret());
        #[expect(clippy::disallowed_methods, reason = "`TokenSessions::new` asked the packet-trace refusal")]
        let session = oracledb::connect(self.0.clone().set_external_auth(oracledb::ExternalAuth::AccessToken(token)));
        let session = session.map_err(connect_err)?;
        pin_utc(&session, deadline)?;
        Ok(session)
    }
}

/// Every execution dial overrides the driver's host or `ORA_SDTZ` default and confirms UTC before use.
pub(crate) fn pin_utc(connection: &oracledb::Connection, deadline: Option<Deadline>) -> Result<(), OracleError> {
    let timed = || {
        let timeout = deadline.map_or(Ok(crate::DIAL_DEADLINE), crate::refuse_if_spent)?;
        connection
            .set_call_timeout(Some(timeout))
            .map_err(|cause| OracleError::CallTimeout { cause: cause.into() })
    };
    timed()?;
    connection
        .execute("ALTER SESSION SET TIME_ZONE = '+00:00'", &[])
        .map_err(crate::execute_err_mapped)?;
    timed()?;
    let row = connection
        .query_row("SELECT SESSIONTIMEZONE FROM DUAL", &[])
        .map_err(crate::execute_err_mapped)?;
    let zone: String = row.get(0).map_err(crate::execute_err_mapped)?;
    if zone != "+00:00" {
        return Err(OracleError::UnsupportedType {
            column: String::from("<session>"),
            oracle_type: "UTC session time zone was not confirmed",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::date_text;

    #[test]
    fn a_date_with_a_time_of_day_keeps_it() {
        let text = |micros| date_text(micros).expect("the date is representable");
        // 2026-10-11T13:45:30, its midnight, and 0.456 s past it, as microseconds since the epoch.
        assert_eq!(text(1_791_726_330_000_000), "2026-10-11T13:45:30");
        assert_eq!(text(1_791_676_800_000_000), "2026-10-11");
        assert_eq!(text(1_791_676_800_456_000), "2026-10-11T00:00:00.456");
        // The driver's own reading of a `TIMESTAMP WITH TIME ZONE`: its UTC instant, no offset kept.
        assert_eq!(text(1_761_933_135_456_000), "2025-10-31T17:52:15.456");
    }
}
