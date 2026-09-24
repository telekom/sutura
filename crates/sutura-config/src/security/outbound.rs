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
/// (`crate::sources::refuse_foreign_keys` on `files`/`bigquery`). A deployment-wide declaration is
/// the shape a fixed-host client has something to attach to. See `docs/adr/0010`'s amendment.
///
/// **What reads it today is the `DataHub` reader, and NOT the `BigQuery` transport.** The HTTP wire
/// and its token exchange that did read it are deleted; the ADBC driver that replaced them dials with
/// its own trust store and presents no client certificate. So `sutura-cli` refuses a declared bundle
/// or client identity beside a `bigquery` source at startup rather than serving that source under a
/// declaration it would not honour.
///
/// **Anchors are required whenever the block is written; a client identity beside them is
/// optional** - `github.com/telekom/sutura#911`. Every fixed-host client this covers speaks a
/// bearer token, so a certificate buys nothing against the ENDPOINT; what it is for is a peer
/// in front of it (a gateway, a proxy) that a deployment wants to authenticate the connection
/// itself, hence [`OutboundIdentity`] rather than a field on this type - see that type's own doc.
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

/// The client certificate and key every fixed-host outbound client presents -
/// `security.outbound.client_certificate`/`client_key`, `github.com/telekom/sutura#911`.
///
/// **Its own type, not `crate::sources::transport::ClientIdentity` reused** - the same reason
/// [`OutboundAnchors`] is its own type beside `crate::sources::transport::TrustAnchors`: this
/// module is a self-contained declaration, and the two identity pairs are declared under
/// different keys with different refusal wording even though the SHAPE (a certificate, a key,
/// both absolute) is identical.
///
/// **Absence is not a refusal - it is today's behaviour, unchanged.** No shipped source is
/// configured to demand a client certificate from this deployment, so an absent pair means no
/// certificate is presented, exactly as before this declaration existed. A WRITTEN half with no
/// partner IS a refusal ([`InvalidOutbound::IdentityMissingHalf`]) - the same argument a source's
/// own `mutual` identity makes: a partial pair would start with the certificate quietly
/// unpresented, which is worse than a deployment that never asked for one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundIdentity {
    certificate: std::path::PathBuf,
    key: std::path::PathBuf,
}

impl OutboundIdentity {
    /// The certificate (and any chain) this deployment presents. Absolute.
    #[inline]
    #[must_use]
    pub const fn certificate(&self) -> &std::path::PathBuf {
        &self.certificate
    }

    /// The private key for that certificate. Absolute. A secret; loaded, never inlined.
    #[inline]
    #[must_use]
    pub const fn key(&self) -> &std::path::PathBuf {
        &self.key
    }
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
    /// A `security.outbound.transport_anchors`/`client_certificate`/`client_key` path that is
    /// relative.
    #[error(
        "`security.outbound.{key}` is `{path}`, which is relative and resolves against this \
         process's working directory - a different directory on every host. Write an absolute path"
    )]
    RelativePath { key: &'static str, path: std::path::PathBuf },
    /// A `client_certificate` was written without its `client_key`, or the reverse.
    #[error(
        "`security.outbound.{given}` was written and `security.outbound.{missing}` was not - a \
         client certificate is one pair, and a partial declaration would start with it quietly \
         unpresented"
    )]
    IdentityMissingHalf { given: &'static str, missing: &'static str },
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
                return Err(InvalidOutbound::RelativePath {
                    key: "transport_anchors",
                    path,
                });
            }
            Ok(OutboundAnchors::Bundle(path))
        }
        None => Err(InvalidOutbound::NoAnchors),
    }
}

/// Reads the deployment-wide client identity, refusing a written half with no partner.
///
/// `Ok(None)` for the ordinary deployment (neither key written) - see [`OutboundIdentity`]'s own
/// doc for why that is not a gap.
pub(crate) fn parse_outbound_identity(
    certificate: Option<&str>,
    key: Option<&str>,
) -> Result<Option<OutboundIdentity>, InvalidOutbound> {
    let certificate = certificate.map(str::trim).filter(|text| !text.is_empty());
    let key = key.map(str::trim).filter(|text| !text.is_empty());
    match (certificate, key) {
        (None, None) => Ok(None),
        (Some(certificate), Some(key)) => Ok(Some(OutboundIdentity {
            certificate: absolute("client_certificate", certificate)?,
            key: absolute("client_key", key)?,
        })),
        (Some(_), None) => Err(InvalidOutbound::IdentityMissingHalf {
            given: "client_certificate",
            missing: "client_key",
        }),
        (None, Some(_)) => Err(InvalidOutbound::IdentityMissingHalf {
            given: "client_key",
            missing: "client_certificate",
        }),
    }
}

/// One identity path that has to be absolute, naming the key it refuses.
fn absolute(key: &'static str, written: &str) -> Result<std::path::PathBuf, InvalidOutbound> {
    let path = std::path::PathBuf::from(written);
    if path.is_relative() {
        return Err(InvalidOutbound::RelativePath { key, path });
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_identity_written_is_not_a_refusal() {
        assert_eq!(parse_outbound_identity(None, None), Ok(None));
        assert_eq!(parse_outbound_identity(Some(""), Some("   ")), Ok(None));
    }

    #[test]
    fn a_full_identity_pair_parses() {
        let identity = parse_outbound_identity(Some("/etc/sutura/outbound.crt"), Some("/etc/sutura/outbound.key"))
            .unwrap()
            .expect("both halves written");
        assert_eq!(identity.certificate(), &std::path::PathBuf::from("/etc/sutura/outbound.crt"));
        assert_eq!(identity.key(), &std::path::PathBuf::from("/etc/sutura/outbound.key"));
    }

    #[test]
    fn a_partial_identity_pair_is_refused() {
        assert_eq!(
            parse_outbound_identity(Some("/etc/sutura/outbound.crt"), None),
            Err(InvalidOutbound::IdentityMissingHalf {
                given: "client_certificate",
                missing: "client_key",
            })
        );
        assert_eq!(
            parse_outbound_identity(None, Some("/etc/sutura/outbound.key")),
            Err(InvalidOutbound::IdentityMissingHalf {
                given: "client_key",
                missing: "client_certificate",
            })
        );
    }

    #[test]
    fn a_relative_identity_path_is_refused_naming_the_key() {
        assert_eq!(
            parse_outbound_identity(Some("outbound.crt"), Some("/etc/sutura/outbound.key")),
            Err(InvalidOutbound::RelativePath {
                key: "client_certificate",
                path: std::path::PathBuf::from("outbound.crt"),
            })
        );
    }
}
