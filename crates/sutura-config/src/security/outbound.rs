//! The deployment-wide outbound trust declaration - `security.outbound`.
//!
//! A file of its own for the same mechanical reason `settings/outbound.rs` is: `security.rs`
//! is at the 1000-line cap `cargo xtask max-lines` enforces. The declaration types live here
//! and are re-exported from [`crate::security`] so every existing `crate::security::*` path
//! keeps resolving.

/// The deployment-wide outbound trust declaration - `security.outbound`.
///
/// **Distinct from a per-source `transport_anchors`** (`crate::sources::transport::TrustAnchors`),
/// and deliberately a second, smaller type rather than the same one reused: a per-source declaration
/// is refused when the source's own kind has no dial target the anchor could attach to
/// (`crate::sources::refuse_foreign_keys` on `files`/`bigquery`), and the outbound clients this
/// settles - the `BigQuery` wire, the STS token exchange - dial a HOST THAT IS A COMPILE-TIME CONSTANT.
/// There is no source entry a per-entry `transport_anchors` on a `bigquery` kind could mean anything
/// on, which is exactly why #125 keeps that refusal rather than lifting it: a deployment-wide
/// declaration is the shape that has something to attach to. See `docs/adr/0010`'s amendment.
///
/// **Anchors only - no client identity.** Every fixed-host client this covers takes a bearer token,
/// not a certificate, so a `ClientIdentity` field here would be a shape nothing exercises.
///
/// Absent `security.outbound` is not a refusal, unlike a source that asks for TLS and names no
/// anchors: these clients always speak TLS regardless of configuration, and an absent block means
/// "verify against `ureq`'s own compiled-in roots", which is today's (and every prior release's)
/// behaviour. A PRESENT block with no `transport_anchors` IS a refusal - see
/// [`InvalidOutbound::NoAnchors`] - because a block that names nothing declares nothing, the same
/// argument `security.inbound` with no `mode` already makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundAnchors {
    /// A PEM bundle at this absolute path.
    Bundle(std::path::PathBuf),
    /// The host's own trust store, chosen by name.
    System,
}

/// Why a `security.outbound` declaration was not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidOutbound {
    /// `security.outbound` was written with no `transport_anchors`.
    #[error(
        "`security.outbound` is declared and names no `transport_anchors` - a block that names no \
         trust store declares nothing. Say which authority signs the fixed-host endpoints this \
         deployment reaches (a PEM bundle path, or `system`), or remove the block"
    )]
    NoAnchors,
    /// A `security.outbound.transport_anchors` path that is relative.
    #[error(
        "`security.outbound.transport_anchors` is `{path}`, which is relative and resolves against \
         this process's working directory - a different directory on every host. Write an absolute path"
    )]
    RelativePath { path: std::path::PathBuf },
}

/// Reads the outbound trust declaration from its one written field.
///
/// `None` means `security.outbound` was absent, which is not a refusal - see
/// [`OutboundAnchors`]'s own doc for why. `Some(None)` (a present block, no field written) is what
/// reaches this function as `Some("")`/`Some(None)` from an empty or unset `transport_anchors`, and
/// it is [`InvalidOutbound::NoAnchors`].
pub(crate) fn parse_outbound(anchors: Option<&str>) -> Result<OutboundAnchors, InvalidOutbound> {
    match anchors.map(str::trim).filter(|text| !text.is_empty()) {
        Some("system") => Ok(OutboundAnchors::System),
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            if path.is_relative() {
                return Err(InvalidOutbound::RelativePath { path });
            }
            Ok(OutboundAnchors::Bundle(path))
        }
        None => Err(InvalidOutbound::NoAnchors),
    }
}
