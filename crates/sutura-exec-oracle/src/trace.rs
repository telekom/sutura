//! The vendored driver's packet trace, which this adapter does not run under.

use crate::OracleError;

/// The variable that switches the driver's packet trace on, at any value.
const PACKET_TRACE: &str = "RSO_DEBUG_PACKETS";

/// Refuses while the driver's packet trace is switched on. Every dial this adapter makes asks here
/// first.
///
/// # Errors
///
/// [`OracleError::PacketTraceOn`] while the variable is set.
pub fn refuse_packet_trace() -> Result<(), OracleError> {
    std::env::var_os(PACKET_TRACE).map_or(Ok(()), |_set| Err(OracleError::PacketTraceOn { variable: PACKET_TRACE }))
}
