//! The linked `BigQuery` driver's Go auth library debug log, which no `BigQuery` dial in this
//! workspace runs under.
//!
//! `cloud.google.com/go/auth`'s own HTTP transport logs each request and response - headers and
//! body, unredacted - whenever the variable below is set, to stderr and outside `tracing`. That
//! would write the loopback subject-token request `super::subject` builds, with its secret
//! header and the caller's own identity assertion, straight past the [`sutura_domain::identity::Secret`]
//! bound `super::subject` states. The composition root asks [`refuse`] before any command,
//! mirroring `sutura_oracle_connect::oracle_trace`'s refusal of the Oracle driver's equivalent.

/// The variable that switches the driver's debug log on, at any value.
const VARIABLE: &str = "GOOGLE_SDK_GO_LOGGING_LEVEL";

/// The driver's debug log is switched on.
#[derive(Debug, thiserror::Error)]
#[error("`{variable}` is set, which switches the BigQuery driver's debug log on")]
pub struct DebugLogOn {
    variable: &'static str,
}

/// Refuses while the `BigQuery` driver's debug log is switched on.
///
/// # Errors
///
/// [`DebugLogOn`] while the variable is set, at any value.
pub fn refuse() -> Result<(), DebugLogOn> {
    std::env::var_os(VARIABLE).map_or(Ok(()), |_set| Err(DebugLogOn { variable: VARIABLE }))
}
