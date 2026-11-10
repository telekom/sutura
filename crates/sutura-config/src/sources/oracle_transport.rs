//! Oracle's encrypted channels: verified TLS or explicitly acknowledged, unauthenticated NNE.

use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;
use sutura_domain::source::{AcknowledgementReason, InvalidOperatorText};

use super::placement::{HostName, InvalidTlsServerName, TlsServerName};
use super::transport::{InvalidTransport, SourceTransport, TrustAnchors};

const NATIVE_REASON: &str = "native_encryption_acknowledged_because";
const LEGACY_REASON: &str = "legacy_password_verifier_acknowledged_because";

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RawOracleOptions<'raw> {
    pub(crate) native_reason: Option<&'raw str>,
    pub(crate) legacy: Option<bool>,
    pub(crate) legacy_reason: Option<&'raw str>,
}

impl RawOracleOptions<'_> {
    pub(crate) const fn keys(self) -> [(&'static str, bool); 3] {
        [
            (NATIVE_REASON, self.native_reason.is_some()),
            ("legacy_password_verifier", self.legacy.is_some()),
            (LEGACY_REASON, self.legacy_reason.is_some()),
        ]
    }
}

pub(crate) struct RawOracleTransport<'raw> {
    pub(crate) mode: &'raw str,
    pub(crate) anchors: Option<&'raw str>,
    pub(crate) client_certificate: Option<&'raw str>,
    pub(crate) client_key: Option<&'raw str>,
    pub(crate) options: RawOracleOptions<'raw>,
}

/// Native encryption protects records but its unauthenticated DH does not verify the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleTransport {
    /// TLS against the declared PEM bundle, with the declared host as the verified name.
    Verified { anchors: PathBuf, server_name: TlsServerName },
    /// AES encryption and SHA-2 integrity over TCP, without authenticated peer identity.
    NativeEncryption { acknowledged_because: AcknowledgementReason },
}

/// An encrypted Oracle channel and the independent acknowledgement of legacy password logins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleChannel {
    transport: OracleTransport,
    legacy_password_verifier: Option<AcknowledgementReason>,
}

/// Why an Oracle channel declaration is refused. No operator reason is included in an error.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum NoOracleChannel {
    #[error("Oracle cannot deliver transport `{0}`")]
    Undeliverable(&'static str),
    #[error("the host is not a TLS server name: {0}")]
    ServerName(InvalidTlsServerName),
    #[error("Oracle requires verified TLS or acknowledged native-encryption")]
    Plaintext,
    #[error(transparent)]
    Transport(InvalidTransport),
    #[error("`{key}` is not read by this transport or password mode")]
    Unread { key: &'static str },
    #[error("`{key}` must state the reason for this opt-in: {cause}")]
    Reason { key: &'static str, cause: InvalidOperatorText },
    #[error("`legacy_password_verifier` requires verified TLS or acknowledged native-encryption")]
    LegacyWithoutEncryption,
}

impl OracleChannel {
    pub(crate) fn parse(alias: &SourceName, host: &HostName, raw: RawOracleTransport<'_>) -> Result<Self, NoOracleChannel> {
        let native = raw.mode.trim() == "native-encryption";
        if !native && raw.options.native_reason.is_some() {
            return Err(NoOracleChannel::Unread { key: NATIVE_REASON });
        }
        if raw.options.legacy != Some(true) && raw.options.legacy_reason.is_some() {
            return Err(NoOracleChannel::Unread { key: LEGACY_REASON });
        }
        if raw.options.legacy == Some(true) && raw.mode.trim() == "plaintext" {
            return Err(NoOracleChannel::LegacyWithoutEncryption);
        }
        let transport = if native {
            for (key, present) in [
                ("transport_anchors", raw.anchors.is_some()),
                ("client_certificate", raw.client_certificate.is_some()),
                ("client_key", raw.client_key.is_some()),
            ] {
                if present {
                    return Err(NoOracleChannel::Unread { key });
                }
            }
            OracleTransport::NativeEncryption { acknowledged_because: reason(NATIVE_REASON, raw.options.native_reason)? }
        } else {
            let transport = super::transport::parse(alias, raw.mode, raw.anchors, raw.client_certificate, raw.client_key)
                .map_err(NoOracleChannel::Transport)?;
            // Preserve the remote-plaintext refusal for existing declarations.
            match transport {
                SourceTransport::Plaintext => return Err(NoOracleChannel::Plaintext),
                SourceTransport::Verified { anchors: TrustAnchors::File(anchors) } => OracleTransport::Verified {
                    anchors,
                    server_name: TlsServerName::parse(host).map_err(NoOracleChannel::ServerName)?,
                },
                SourceTransport::Verified { anchors: TrustAnchors::System } => return Err(NoOracleChannel::Undeliverable("verified with `transport_anchors: system`")),
                SourceTransport::Mutual { .. } => return Err(NoOracleChannel::Undeliverable("mutual")),
            }
        };
        let legacy_password_verifier = if raw.options.legacy == Some(true) {
            Some(reason(LEGACY_REASON, raw.options.legacy_reason)?)
        } else {
            None
        };
        Ok(Self { transport, legacy_password_verifier })
    }

    #[must_use]
    pub const fn transport(&self) -> &OracleTransport { &self.transport }

    #[must_use]
    pub const fn legacy_password_verifier(&self) -> Option<&AcknowledgementReason> { self.legacy_password_verifier.as_ref() }

    /// The PEM bundle, present only for verified TLS.
    #[must_use]
    pub fn anchors(&self) -> Option<&Path> {
        match &self.transport {
            OracleTransport::Verified { anchors, .. } => Some(anchors),
            OracleTransport::NativeEncryption { .. } => None,
        }
    }

    /// The authenticated server name, absent for native encryption.
    #[must_use]
    pub const fn server_name(&self) -> Option<&TlsServerName> {
        match &self.transport {
            OracleTransport::Verified { server_name, .. } => Some(server_name),
            OracleTransport::NativeEncryption { .. } => None,
        }
    }
}

fn reason(key: &'static str, raw: Option<&str>) -> Result<AcknowledgementReason, NoOracleChannel> {
    AcknowledgementReason::written_under(key, raw.unwrap_or_default()).map_err(|cause| NoOracleChannel::Reason { key, cause })
}
