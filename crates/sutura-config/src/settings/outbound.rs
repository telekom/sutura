//! Turning the written `security.outbound` declaration into resolved anchors, or refusing it.
//!
//! A file of its own for the same mechanical reason its neighbour `inbound.rs` is: `settings.rs`
//! sat at the thousand-line limit `cargo xtask max-lines` enforces, and the outbound declaration is
//! the newest separable group in it. Everything here is private to [`crate::settings`]; the type
//! produced lives in [`crate::security`], and the `system`-vs-bundle decision itself is
//! [`crate::security::parse_outbound`]. This file is the thin boundary that reads a
//! `RawOutbound` and maps the refusal onto [`SettingsError`].

use crate::raw::RawOutbound;
use crate::security::{OutboundAnchors, parse_outbound as parse_anchors};
use crate::settings::SettingsError;

/// The outbound trust declaration, `None` for the deployment that names none.
///
/// Absence is not a refusal - see [`crate::security::OutboundAnchors`] - so only a PRESENT, empty
/// block reaches the parse's own refusal ([`crate::security::InvalidOutbound::NoAnchors`]).
pub(super) fn parse_outbound(written: Option<&RawOutbound>) -> Result<Option<OutboundAnchors>, SettingsError> {
    let Some(written) = written else {
        return Ok(None);
    };
    parse_anchors(written.transport_anchors.as_deref())
        .map(Some)
        .map_err(|cause| SettingsError::Outbound { cause })
}
