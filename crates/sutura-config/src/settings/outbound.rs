//! Turning the written `security.outbound` declaration into resolved anchors, or refusing it.
//!
//! A file of its own for the same mechanical reason its neighbour `inbound.rs` is: `settings.rs`
//! sat at the thousand-line limit `cargo xtask max-lines` enforces, and the outbound declaration is
//! the newest separable group in it. Everything here is private to [`crate::settings`]; the type
//! produced lives in [`crate::security`], and the `system`-vs-bundle decision itself is
//! [`crate::security::parse_outbound`]. This file is the thin boundary that reads a
//! `RawOutbound` and maps the refusal onto [`SettingsError`].

use crate::raw::RawOutbound;
use crate::security::{OutboundAnchors, OutboundIdentity, parse_outbound as parse_anchors, parse_outbound_identity};
use crate::settings::SettingsError;

/// The anchors and the optional client identity beside them - named for `clippy::type_complexity`,
/// the same reason a three-argument generic elsewhere in this workspace gets its own alias.
type OutboundDeclaration = (Option<OutboundAnchors>, Option<OutboundIdentity>);

/// The outbound trust declaration, `None` for the deployment that names none, alongside the
/// optional client identity beside it (`github.com/telekom/sutura#911`) - `Ok(None)` for the
/// identity half whenever the block itself is absent, since there is no way to write
/// `client_certificate`/`client_key` without the `security.outbound:` block that nests them.
///
/// Absence is not a refusal - see [`crate::security::OutboundAnchors`] - so only a PRESENT, empty
/// block reaches the parse's own refusal ([`crate::security::InvalidOutbound::NoAnchors`]).
pub(super) fn parse_outbound(written: Option<&RawOutbound>) -> Result<OutboundDeclaration, SettingsError> {
    let Some(written) = written else {
        return Ok((None, None));
    };
    let anchors = parse_anchors(written.transport_anchors.as_deref()).map_err(|cause| SettingsError::Outbound { cause })?;
    let identity = parse_outbound_identity(written.client_certificate.as_deref(), written.client_key.as_deref())
        .map_err(|cause| SettingsError::Outbound { cause })?;
    Ok((Some(anchors), identity))
}
