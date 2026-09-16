//! The HTTP half of [`crate::sts::ImpersonateAsAccount`]: a federated access token in, a
//! service-account access token out - telekom/sutura#376's second hop.
//!
//! **Behind the same default-off `wire` feature [`crate::wire::StsOverHttp`] is**, and for the same
//! reason: an outbound TLS stack is a dependency decision a composition root makes in a manifest
//! line, not something the adapter inherits by being linked. It shares the crate's one
//! [`crate::wire::WireAgent`] - the outbound anchors `security.outbound.transport_anchors` resolves
//! reach this endpoint identically to `sts.googleapis.com`, no new anchor plumbing.
//!
//! The request is `iamcredentials.generateAccessToken`: the federated access token the first hop
//! produced, presented as this request's own bearer, asking for a short-lived access token scoped to
//! the account [`crate::sts::WorkloadIdentity::target_for`] declares. The exchanged
//! [`crate::sts::StsCredential`] carries that access token and the instant it stops being usable.
//!
//! **The deadline comes from the endpoint's own `expireTime`, bounded by the lifetime this adapter
//! requested - never asserted from the request alone.** An organization policy (a real Google
//! control) can cap token lifetime below the requested ceiling, and a broker and cache that reason
//! from a deadline LATER than the truth can serve a dead token. So the expiry is
//! `min(now + requested seconds, <the response's expireTime>)`, and an `expireTime` that would
//! grant MORE than was requested, or is absent, is refused rather than trusted - the same posture
//! `wire::StsOverHttp` holds for a missing `expires_in`.
//!
//! **The refusal is bounded but not to the second.** `iamcredentials` answers an `expireTime`
//! stamped by its own clock, which reads a fraction past `now + requested` against this adapter's
//! own second-rounded `now` (the vendor's clock vs ours), so a strict `<= now + requested` bound
//! refuses Google's own answer to the requested ceiling. The bound therefore carries an explicit,
//! documented [`IMPERSONATED_LIFETIME_SKEW_SECONDS`] allowance on top of the requested lifetime;
//! an `expireTime` up to `now + requested + skew` is accepted (the granted deadline still READS the
//! response's `expireTime`, never the bound), anything beyond it is still refused, and an absent
//! one is still refused.

use sutura_domain::identity::{Expiry, Secret};

use crate::sts::{ImpersonateAsAccount, StsCredential};
use crate::wire::{CallDeadline, WireAgent};

/// The API this module speaks to.
const HOST: &str = "https://iamcredentials.googleapis.com/v1";

/// The clock-skew allowance added to the requested lifetime before the `expireTime` bound bites.
///
/// `iamcredentials` stamps `expireTime` from its own clock, up to a second-rounding fraction past
/// this adapter's `now + requested`; the bound stays a mechanism (an `expireTime` beyond
/// `now + requested + SKEW` is refused, an absent one is refused), but this explicit window keeps
/// the vendor's own answer to the requested ceiling from tripping it. The granted deadline is still
/// the response's `expireTime`, never this bound.
const IMPERSONATED_LIFETIME_SKEW_SECONDS: u64 = 60;

/// The request body, as `generateAccessToken`'s own document describes it.
#[derive(serde::Serialize)]
struct Request<'a> {
    scope: [&'a str; 1],
    lifetime: String,
}

/// The answer, with the two fields this adapter reads.
///
/// `expireTime` is REQUIRED (no `#[serde(default)]`) so an answer without it deserializes to
/// [`IamCredentialsError::NotADocument`] rather than being answered with an invented lifetime - the
/// same refusal `wire::StsOverHttp` gives a missing `expires_in`. It is read as text and parsed by
/// [`expire_time_unix`], since `serde` has no RFC 3339 type of its own.
#[derive(serde::Deserialize)]
struct Response {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expireTime")]
    expire_time: String,
}

/// Parses Google's `expireTime` - RFC 3339 in UTC (`YYYY-MM-DDTHH:MM:SS[.sss]Z`) - to whole unix
/// seconds, truncating any sub-second precision.
///
/// `None` for anything that is not that exact shape (an offset timezone, a date without a time), so
/// an unexpected schema is refused as [`IamCredentialsError::NoLifetime`] rather than guessed at.
///
/// Reads the fixed-format fields through [`unsigned`] on the raw BYTES - never a `str` slice
/// (`clippy::string_slice` is denied) and never a bare `bytes[i]` index (`clippy::indexing_slicing`
/// is denied) - and converts the civil date with days-since-epoch arithmetic, so no truncating `as`
/// cast exists here. The date half is validated by range checks on the parsed fields; a
/// calendar-impossible combination (say, February 30) is a genuine Google response this adapter
/// would rather refuse with `NoLifetime` than trust, which the range checks below already admit.
fn expire_time_unix(raw: &str) -> Option<u64> {
    let bytes = raw.as_bytes();
    // "YYYY-MM-DDTHH:MM:SSZ" is 20 bytes; allow an optional ".sss" fraction before the Z.
    if bytes.len() < 20 {
        return None;
    }
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let year = unsigned(bytes, 0, 4)?;
    let month = unsigned(bytes, 5, 2)?;
    let day = unsigned(bytes, 8, 2)?;
    let hour = unsigned(bytes, 11, 2)?;
    let minute = unsigned(bytes, 14, 2)?;
    let second = unsigned(bytes, 17, 2)?;
    if month > 12 || day > 31 || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    // A fraction, if present: ".ccc" between the seconds and the Z. Its VALUE is truncated away -
    // whole unix seconds are all [`Expiry`] keeps - but it must be well-formed to be consumed.
    let mut at = 19;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let mut saw_digit = false;
        while let Some(&byte) = bytes.get(at) {
            if byte.is_ascii_digit() {
                saw_digit = true;
                at += 1;
            } else {
                break;
            }
        }
        if !saw_digit {
            return None;
        }
    }
    // The timestamp must end here, and it must be UTC ("Z"), never an offset.
    if bytes.get(at) != Some(&b'Z') || at + 1 != bytes.len() {
        return None;
    }
    // Reuse the domain's `Date`, which counts days since epoch by walking years rather than by
    // integer division (banned in this workspace's lint table) - the same calendar a query's own
    // dates validate against, so a month/day combination it rejects becomes `NoLifetime` here too.
    // `Day`/`month`/`year` are already range-checked above, so the `TryFrom` casts are lossless.
    let year = i16::try_from(year).ok()?;
    let month = u8::try_from(month).ok()?;
    let day = u8::try_from(day).ok()?;
    let date = sutura_domain::calendar::Date::new(year, month, day).ok()?;
    let seconds =
        i64::from(date.days_since_epoch()) * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    u64::try_from(seconds).ok()
}

/// Reads `count` decimal digits of a fixed-position ASCII field (`bytes[at..at+count]`) as an
/// integer, or `None` if any byte there is not a digit or the field runs off the slice.
///
/// The one safe way to read a positional ASCII byte under this workspace's `indexing_slicing` ban:
/// `bytes.get(..)` returns `Option`, and every byte is bounds-checked whether it is a digit or not.
fn unsigned(bytes: &[u8], at: usize, count: usize) -> Option<u32> {
    let mut value: u32 = 0;
    for offset in 0..count {
        let byte = *bytes.get(at + offset)?;
        let digit = byte.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u32::from(digit))?;
    }
    Some(value)
}

/// The account this hop names in a refusal - carried, and never rendered raw.
///
/// **The same discipline `crate::wire::EndpointMessage` holds, applied to a value that is ours rather
/// than the endpoint's own free text.** The target is a declared configuration value, not a secret,
/// but this hop's whole point is that "the pool subject may not impersonate this account" reaches a
/// caller as a class of refusal and never as an account identifier - see
/// `docs/where-identity-is-proven.md` and this crate's own `wire::EndpointMessage` header for the
/// measured cost of a formatter that walked a struct instead of asking it.
#[derive(Clone, PartialEq, Eq)]
pub struct RedactedSa(String);

impl RedactedSa {
    fn of(raw: &str) -> Self {
        Self(String::from(raw))
    }
}

impl core::fmt::Debug for RedactedSa {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "<the declared impersonation target, {} char(s), redacted>", self.0.len())
    }
}

/// The account this hop asks the endpoint to impersonate, as this adapter holds it.
///
/// **Re-validated here for the reason `crate::transport::ProjectId` gives**:
/// `sutura_config::sources::workload_identity::WorkloadIdentitySa` is checked once where it is
/// declared, and this crate may not depend on that settings tree - so the format is checked again
/// where the value is interpolated into a request path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImpersonatedAccount(String);

/// Why a declared impersonation target cannot be interpolated into this hop's request path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableAccount {
    /// Nothing was written, or only whitespace was.
    #[error("the impersonation target is empty")]
    Empty,
    /// A character that could leave the URL path segment this value is written into.
    #[error("the character at position {at} is not allowed in an impersonation target")]
    Character { at: usize },
}

impl ImpersonatedAccount {
    /// Parses a target account.
    ///
    /// The accepted set is `[A-Za-z0-9._-@]` - a service-account email's own alphabet - so a value
    /// that would escape the URL path segment this is interpolated into cannot exist here.
    fn parse(raw: &str) -> Result<Self, UnusableAccount> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(UnusableAccount::Empty);
        }
        if let Some(at) = trimmed
            .char_indices()
            .find_map(|(at, c)| (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '@')).then_some(at))
        {
            return Err(UnusableAccount::Character { at });
        }
        Ok(Self(String::from(trimmed)))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why the hop could not happen.
#[derive(Debug, thiserror::Error)]
pub enum IamCredentialsError {
    /// The declared impersonation target could not be interpolated into the request path.
    #[error("the declared impersonation target is not usable")]
    Account {
        #[source]
        cause: UnusableAccount,
    },
    /// This process could not read a wall clock, so no deadline could be computed.
    #[error("this process could not read the time, so no impersonated-token deadline could be computed")]
    NoClock {
        #[source]
        cause: std::time::SystemTimeError,
    },
    /// The request could not be serialized.
    #[error("the request body could not be serialized, which is a defect in this adapter")]
    RequestNotSerializable {
        #[source]
        cause: serde_json::Error,
    },
    /// This call's budget was gone before the hop could be submitted.
    #[error("this call's budget was spent before the impersonation request could be submitted")]
    DeadlineSpent,
    /// The endpoint was not reached.
    #[error("the impersonation endpoint was not reached")]
    Unreachable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The endpoint's answer could not be read.
    #[error("the impersonation endpoint's answer could not be read")]
    Unreadable {
        #[source]
        cause: Box<ureq::Error>,
    },
    /// The pool subject may not impersonate the declared account - `403`, named rather than the
    /// endpoint's own free-text body.
    #[error("the pool subject may not impersonate the declared account")]
    ImpersonationRefused {
        /// Carried for a caller that has decided it needs to know which target - never rendered by
        /// this variant's own `Display`, and redacted under `Debug`.
        target: RedactedSa,
    },
    /// The endpoint refused for a reason other than `403`.
    #[error("the impersonation endpoint refused with {status}")]
    Refused { status: u16 },
    /// The answer was not the access-token document this adapter reads.
    #[error("the impersonation endpoint's answer was not an access-token document")]
    NotADocument {
        #[source]
        cause: serde_json::Error,
    },
    /// No access token came back.
    #[error("the impersonation endpoint answered without an access token")]
    NoAccessToken,
    /// The `expireTime` was absent (refused by deserialization) or not the RFC 3339 UTC shape this
    /// adapter reads - never an invented deadline, the same posture `StsError::NoLifetime` holds.
    #[error("the impersonation endpoint's answer carried no usable expiry (expireTime)")]
    NoLifetime,
    /// The `expireTime` grants MORE life than this request asked for, beyond the documented
    /// clock-skew allowance ([`IMPERSONATED_LIFETIME_SKEW_SECONDS`]) - a vendor answering outside
    /// what was asked, refused rather than trusted with a longer-than-requested credential. An
    /// `expireTime` inside the allowance is accepted; the granted deadline still reads it.
    #[error("the impersonation endpoint granted a longer lifetime than was requested")]
    ExceedsRequestedLifetime,
}

/// An [`ImpersonateAsAccount`] that talks to Google's `iamcredentials` API over HTTP.
#[derive(Debug, Clone)]
pub struct IamCredentialsOverHttp {
    agent: WireAgent,
}

impl IamCredentialsOverHttp {
    /// Opens the transport, reusing the pinned [`WireAgent`] so a composition root shares one client,
    /// one connection pool and one set of pins with [`crate::wire::StsOverHttp`] and `BigQueryWire`.
    #[must_use]
    pub const fn new(agent: WireAgent) -> Self {
        Self { agent }
    }
}

impl ImpersonateAsAccount for IamCredentialsOverHttp {
    type Error = IamCredentialsError;

    #[expect(
        clippy::disallowed_methods,
        reason = "the federated access token is this hop's own bearer, so it is exposed once as a header value and nothing else"
    )]
    fn impersonate(
        &self,
        federated: &Secret,
        target_sa: &str,
        scope: &str,
        lifetime: core::time::Duration,
    ) -> Result<StsCredential, Self::Error> {
        let account = ImpersonatedAccount::parse(target_sa).map_err(|cause| IamCredentialsError::Account { cause })?;
        let call = CallDeadline::opened(self.agent.bounds().deadline());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|cause| IamCredentialsError::NoClock { cause })?;
        let seconds = lifetime.as_secs();
        let document = serde_json::to_vec(&Request {
            scope: [scope],
            lifetime: format!("{seconds}s"),
        })
        .map_err(|cause| IamCredentialsError::RequestNotSerializable { cause })?;
        let left = call.remaining().ok_or(IamCredentialsError::DeadlineSpent)?;
        let url = format!("{HOST}/projects/-/serviceAccounts/{}:generateAccessToken", account.as_str());
        let mut answer = self
            .agent
            .agent()
            .post(&url)
            .config()
            .timeout_global(Some(CallDeadline::socket(left)))
            .build()
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", federated.expose_secret()))
            .send(&document)
            .map_err(|cause| IamCredentialsError::Unreachable { cause: Box::new(cause) })?;
        let status = answer.status();
        let text = answer
            .body_mut()
            .with_config()
            .limit(1 << 20)
            .read_to_string()
            .map_err(|cause| IamCredentialsError::Unreadable { cause: Box::new(cause) })?;
        let status_u16: u16 = status.into();
        parse_answer(status_u16, &text, now, seconds, target_sa)
    }
}

/// Turns a raw `iamcredentials` HTTP answer (its status and body) into a granted credential or a
/// typed refusal.
///
/// **Split out of [`IamCredentialsOverHttp::impersonate`] so the redaction and the expiry contract
/// are held by CODE the adapter itself runs, not by a fake port's fixture.** The marketing claim -
/// a `403` surfaces as a named [`IamCredentialsError::ImpersonationRefused`] whose target is a
/// [`RedactedSa`], and the deadline comes from the endpoint's `expireTime` bounded by the requested
/// lifetime - is exercised by feeding a real-shaped body through THIS function in the suite. The
/// HTTP method's only job is to produce `(status, text)` and hand it over.
fn parse_answer(
    status: u16,
    text: &str,
    now: u64,
    requested_seconds: u64,
    target_sa: &str,
) -> Result<StsCredential, IamCredentialsError> {
    if status == 403 {
        return Err(IamCredentialsError::ImpersonationRefused {
            target: RedactedSa::of(target_sa),
        });
    }
    if !(200..300).contains(&status) {
        return Err(IamCredentialsError::Refused { status });
    }
    let response: Response = serde_json::from_str(text).map_err(|cause| IamCredentialsError::NotADocument { cause })?;
    if response.access_token.is_empty() {
        return Err(IamCredentialsError::NoAccessToken);
    }
    let requested_until = now.saturating_add(requested_seconds);
    // The requested ceiling plus the documented clock-skew window: `iamcredentials` stamps
    // `expireTime` from its own clock a fraction past this adapter's second-rounded `now + requested`,
    // so a strict `expireTime > requested_until` bound refuses the vendor's own answer. Anything
    // beyond the allowance is still refused, and an absent `expireTime` still is (`NoLifetime`).
    let bound = requested_until.saturating_add(IMPERSONATED_LIFETIME_SKEW_SECONDS);
    let expire_time = expire_time_unix(&response.expire_time).ok_or(IamCredentialsError::NoLifetime)?;
    if expire_time > bound {
        return Err(IamCredentialsError::ExceedsRequestedLifetime);
    }
    Ok(StsCredential::of(
        Secret::new(response.access_token),
        Expiry::At {
            unix_seconds: expire_time,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::{ImpersonatedAccount, Request, UnusableAccount, expire_time_unix, parse_answer};
    use sutura_domain::identity::Expiry;

    #[test]
    fn the_request_body_is_the_scope_and_lifetime_shape() {
        let request = Request {
            scope: ["https://www.googleapis.com/auth/bigquery.readonly"],
            lifetime: String::from("3600s"),
        };
        let json = serde_json::to_value(&request).expect("serializes");
        assert_eq!(json["scope"][0], "https://www.googleapis.com/auth/bigquery.readonly");
        assert_eq!(json["lifetime"], "3600s");
    }

    #[test]
    fn an_account_with_no_at_sign_still_parses_here_the_config_boundary_owns_that_check() {
        // This adapter's OWN check is narrower than the settings tree's: it refuses only what would
        // escape a URL path segment, not "is this a plausible email" - that discrimination belongs
        // to `sutura_config::sources::workload_identity::WorkloadIdentitySa`, which this crate may
        // not depend on. A value with no `@` but no illegal character still parses here.
        drop(ImpersonatedAccount::parse("not-an-account").expect("no illegal character is present"));
    }

    #[test]
    fn a_character_that_could_escape_the_path_is_refused() {
        let err = ImpersonatedAccount::parse("sa@x/../y.iam.gserviceaccount.com").expect_err("a slash is refused");
        assert!(matches!(err, UnusableAccount::Character { .. }));
    }

    #[test]
    fn an_empty_account_is_refused() {
        let err = ImpersonatedAccount::parse("   ").expect_err("whitespace only is empty");
        assert!(matches!(err, UnusableAccount::Empty));
    }

    #[test]
    fn a_redacted_sa_never_renders_the_account_under_debug() {
        let redacted = super::RedactedSa::of("principal-a@acme-analytics.iam.gserviceaccount.com");
        let rendered = format!("{redacted:?}");
        assert!(!rendered.contains("principal-a"), "{rendered}");
        assert!(!rendered.contains("acme-analytics"), "{rendered}");
    }

    #[test]
    fn a_real_shaped_403_body_surfaces_as_a_named_refusal_never_the_free_text() {
        // The reviewer's F5: this feeds a REAL-shaped 403 document (the free text an operator would
        // actually see, naming an account) through the ADAPTER's own response parser
        // (`parse_answer` - what `IamCredentialsOverHttp::impersonate` runs on every answer), not
        // through a fake port whose `Debug` a fixture redacted. The claim - a 403 is a named
        // `IamCredentialsError::ImpersonationRefused` whose target is a `RedactedSa` - is held by
        // the shipped code: the body is discarded on the 403 arm, so neither rendering can carry
        // the free text or the target account name.
        let body = r#"{"error":{"code":403,"message":"Permission 'iam.serviceAccounts.actAs' denied on resource ... for principal alice@corp.example","status":"PERMISSION_DENIED"}}"#;
        let failure = parse_answer(
            403,
            body,
            4_000_000_000,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect_err("a 403 is a refusal");
        assert!(matches!(failure, super::IamCredentialsError::ImpersonationRefused { .. }));
        for rendered in [format!("{failure}"), format!("{failure:?}")] {
            assert!(!rendered.contains("PERMISSION_DENIED"), "{rendered}");
            assert!(!rendered.contains("actAs"), "{rendered}");
            assert!(!rendered.contains("alice"), "{rendered}");
            assert!(!rendered.contains("acme-analytics"), "{rendered}");
            assert!(!rendered.contains("target-sa"), "{rendered}");
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "reading the granted token IS the assertion that the deadline came from expireTime, and that the access token itself was carried"
    )]
    fn a_granted_deadline_is_the_endpoints_expire_time_not_the_requested_lifetime() {
        // The reviewer's F3: the impersonated credential's expiry READS the response's `expireTime`,
        // never now + requested. A token marked to expire in 30 seconds is answered with that
        // deadline even though 3600 seconds were requested - the cache and the broker floor then
        // reason from the true (earlier) boundary, not a later one they invented.
        let ts = "2096-10-01T00:00:30.000Z";
        let parsed = expire_time_unix(ts).expect("a well-formed expireTime parses");
        let now = parsed - 30; // requested_until = now + 3600, comfortably after parsed
        let credential = parse_answer(
            200,
            &format!(r#"{{"accessToken":"the-sa-token","expireTime":"{ts}"}}"#),
            now,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect("a 200 with a valid expireTime grants");
        assert_eq!(credential.access_token().expose_secret(), "the-sa-token");
        assert_eq!(credential.not_after(), Expiry::At { unix_seconds: parsed });
        assert_ne!(
            credential.not_after(),
            Expiry::At {
                unix_seconds: now + 3_600
            },
            "the deadline must be the endpoint's expireTime, not the requested lifetime"
        );
    }

    #[test]
    fn an_expire_time_wiser_than_requested_is_refused_not_trusted() {
        // A vendor granting MORE life than was asked is refused outright - a longer-than-requested
        // credential is the one shape this adapter must not answer with.
        let ts = "2096-10-01T00:00:30.000Z";
        let parsed = expire_time_unix(ts).expect("a well-formed expireTime parses");
        let now = parsed - 100_000; // requested_until = now + 3600 sits before parsed
        let failure = parse_answer(
            200,
            &format!(r#"{{"accessToken":"the-sa-token","expireTime":"{ts}"}}"#),
            now,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect_err("an expireTime beyond the requested lifetime is refused");
        assert!(matches!(failure, super::IamCredentialsError::ExceedsRequestedLifetime));
    }

    #[test]
    fn an_expire_time_inside_the_clock_skew_allowance_is_accepted_beyond_it_refused() {
        // The vendor's `expireTime` (its own clock) can sit a fraction past this adapter's
        // second-rounded `now + requested`, so a strict `expireTime > requested_until` bound refuses
        // Google's own answer to the requested ceiling. The bound stays a mechanism with a documented
        // skew allowance: an `expireTime` inside `now + requested + SKEW` is accepted (the granted
        // deadline still READS that `expireTime`), one beyond it is refused, an absent one is refused
        // elsewhere (`NoLifetime`). This cell pins both edges of that window.
        const SKEW: u64 = super::IMPERSONATED_LIFETIME_SKEW_SECONDS;
        let now = 3_999_884_400u64;
        // inside: now + 3600 + 30 - past the strict second-rounding bound, inside the 60s window.
        let inside_ts = "2096-10-01T00:00:30.000Z";
        let inside = expire_time_unix(inside_ts).expect("a well-formed expireTime parses");
        // beyond: inside + 40 - past even the allowance (now + 3600 + 100).
        let beyond_ts = "2096-10-01T00:01:10.000Z";
        let beyond = expire_time_unix(beyond_ts).expect("a well-formed expireTime parses");
        assert!(
            inside - (now + 3_600) < SKEW && beyond - (now + 3_600) > SKEW,
            "the fixture must straddle the allowance: inside by {inside}, beyond by {beyond}, now {now}"
        );

        let granted = parse_answer(
            200,
            &format!(r#"{{"accessToken":"the-sa-token","expireTime":"{inside_ts}"}}"#),
            now,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect("an expireTime inside the clock-skew allowance is granted");
        assert_eq!(
            granted.not_after(),
            Expiry::At { unix_seconds: inside },
            "even inside the allowance, the granted deadline is the response's expireTime, not the bound"
        );

        let failure = parse_answer(
            200,
            &format!(r#"{{"accessToken":"the-sa-token","expireTime":"{beyond_ts}"}}"#),
            now,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect_err("an expireTime beyond the skew allowance is still refused");
        assert!(matches!(failure, super::IamCredentialsError::ExceedsRequestedLifetime));
    }

    #[test]
    fn an_answer_without_expire_time_is_not_a_document() {
        // Absent `expireTime` deserializes as a missing required field -> `NotADocument`, never a
        // guessed deadline.
        let failure = parse_answer(
            200,
            r#"{"accessToken":"the-sa-token"}"#,
            4_000_000_000,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect_err("no expireTime is refused");
        assert!(matches!(failure, super::IamCredentialsError::NotADocument { .. }));
    }

    #[test]
    fn a_malformed_expire_time_is_a_no_lifetime_refusal() {
        let failure = parse_answer(
            200,
            r#"{"accessToken":"the-sa-token","expireTime":"not-a-timestamp"}"#,
            4_000_000_000,
            3_600,
            "target-sa@acme-analytics.iam.gserviceaccount.com",
        )
        .expect_err("a malformed expireTime is refused");
        assert!(matches!(failure, super::IamCredentialsError::NoLifetime));
    }

    #[test]
    fn an_offset_expire_time_is_refused_only_utc_is_read() {
        assert_eq!(expire_time_unix("2096-10-01T00:00:30+02:00"), None);
        assert_eq!(expire_time_unix("2096-10-01T00:00:30.000Z"), Some(3_999_888_030));
    }
}
