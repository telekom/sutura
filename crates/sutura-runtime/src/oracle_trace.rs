//! The vendored Oracle driver's packet trace, which no Oracle dial in this workspace runs under.
//!
//! The variable is read from the process environment, so the refusal lives with the other process
//! globals: `sutura-exec-oracle` and `sutura-catalog-rdbms`'s Oracle reader both ask [`refuse`]
//! before they dial, and the composition root asks it before any command.

/// The variable that switches the driver's packet trace on, at any value.
const VARIABLE: &str = "RSO_DEBUG_PACKETS";

/// The driver's packet trace is switched on.
#[derive(Debug, thiserror::Error)]
#[error("`{variable}` is set, which switches the Oracle driver's packet trace on")]
pub struct PacketTraceOn {
    variable: &'static str,
}

/// Refuses while the Oracle driver's packet trace is switched on.
///
/// # Errors
///
/// [`PacketTraceOn`] while the variable is set, at any value.
pub fn refuse() -> Result<(), PacketTraceOn> {
    std::env::var_os(VARIABLE).map_or(Ok(()), |_set| Err(PacketTraceOn { variable: VARIABLE }))
}
