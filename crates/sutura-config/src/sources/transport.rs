//! How sutura secures the channel to one source, per source and never globally.
//!
//! **This module is `docs/adr/0010`'s configuration half, and it exists because a source
//! connection is the first thing in this repository that must VERIFY a peer's chain.** The serving
//! side presents a chain and verifies none, so `sutura-http` deliberately pulls in neither
//! `webpki-roots` nor `rustls-native-certs`; a source verifies, so which anchors are trusted
//! becomes a decision with a name. Everything here is about making that decision a TYPE rather than
//! a habit, because this is the one decision whose wrong answer is a password and a whole result set
//! sent to an impostor.
//!
//! # The shape is closed, and why
//!
//! ```text
//! Plaintext                                      - no TLS. A named choice an operator wrote.
//! Verified { anchors }                           - TLS, verified. No unverified variant exists.
//! Mutual    { anchors, identity }                - TLS, verified, and sutura presents a certificate.
//! ```
//!
//! There is deliberately no `Verified`-without-anchors shape: a source asking for TLS and naming no
//! trust store is a refusal at load, naming the source (ADR 0010 rule 2). `TrustAnchors` has no
//! default, so there is no value the loader could have filled in on the operator's behalf.
//!
//! And there is deliberately no way to ask for TLS without verification. Every library in this
//! space offers the escape hatch - `danger_accept_invalid_certs`, `sslmode=require` - and each one
//! is an encrypted channel with an unknown peer. A `bool` named `verify` would make that reachable
//! from a configuration file, and a `bool` defaulted to `true` would make it reachable from a typo.
//!
//! # What "no transport" means
//!
//! `Plaintext` is a named choice, not the absence of a setting. A source that names no TLS
//! `transport_mode` is refused at load; an operator who wants no TLS writes
//! `transport_mode: plaintext`, and the startup log prints it. That ordering is what lets
//! `sutura-serve` keep #124's fail-closed refusal for a **non-loopback host with no TLS** while the
//! unix-socket tier keeps working: a socket or loopback host may declare `plaintext`, and any host a
//! network can reach it from must not.
//!
//! **The keys an operator writes are FLAT, not a nested `transport:` block.** The schema holds
//! `transport_mode`, `transport_anchors`, `client_certificate` and `client_key` as top-level source
//! keys (see `crate::raw`), and every refusal below names one of those flat keys - never a
//! `transport.*` spelling that the tree would refuse as an unknown field. The word `transport` here
//! is the CONCEPT (what the channel is made of), and the flat spelling is what an operator edits.

use std::net::IpAddr;
use std::path::PathBuf;

use super::SourceName;

/// The trust anchors a source chain may be verified against.
///
/// **No `Default`, and the field is named here rather than filled in.** Rule 2 of `docs/adr/0010`
/// is that the trust store is stated, not inherited - defaulting to whatever the host happens to
/// trust is how a source is silently accepted from the wrong issuer. So there is no value this type
/// could hold on the operator's behalf, and a `Default` impl would be a value that never passed a
/// constructor.
///
/// The system store remains reachable, but only by an operator WRITING it - a source that wants the
/// host's own anchors says so - and the startup line then prints that it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustAnchors {
    /// A PEM bundle at this absolute path. The file is read by the adapter, at boot, once.
    File(PathBuf),
    /// The host's own trust store. An explicit choice rather than a default.
    System,
}

impl TrustAnchors {
    /// The declared anchors, refusing a TLS mode that names none.
    ///
    /// Written `system` names the host's own store; any other non-empty spelling is an absolute
    /// path to a PEM bundle. An absent or empty value is the refusal [`InvalidTransport::TlsWithoutAnchors`]
    /// - the case `TrustAnchors`'s lack of a `Default` exists for.
    fn parse(alias: &SourceName, anchors: Option<&str>) -> Result<Self, InvalidTransport> {
        match anchors.map(str::trim).filter(|text| !text.is_empty()) {
            // Written `system`: the host's own store, chosen by name.
            Some("system") => Ok(Self::System),
            // Written as a path: the file must be absolute.
            Some(path) => {
                let path = PathBuf::from(path);
                if path.is_relative() {
                    return Err(InvalidTransport::RelativePath {
                        alias: alias.clone(),
                        key: "transport_anchors",
                        path,
                    });
                }
                Ok(Self::File(path))
            }
            None => Err(InvalidTransport::TlsWithoutAnchors { alias: alias.clone() }),
        }
    }
}

/// The client certificate and key this deployment presents to a source.
///
/// A pair - a certificate with no key, or a key with no certificate, is refused at load naming the
/// missing half. The paths are read by the adapter at boot; only the paths live in configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    /// The certificate (and any chain) this deployment presents. Absolute.
    certificate: PathBuf,
    /// The private key for that certificate. Absolute. A secret; loaded, never inlined.
    key: PathBuf,
}

impl ClientIdentity {
    #[inline]
    #[must_use]
    pub const fn certificate(&self) -> &PathBuf {
        &self.certificate
    }

    #[inline]
    #[must_use]
    pub const fn key(&self) -> &PathBuf {
        &self.key
    }
}

/// How a source connection is secured.
///
/// Three states, and the middle one is the one that gets forgotten - which is why it has a test of
/// its own. There is no unverified TLS variant, and no `Default`: what a source's channel is made
/// of is a decision the deployment makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceTransport {
    /// No transport security. For a local file or a process-local unix socket. A named choice.
    Plaintext,
    /// TLS, verified against the declared `anchors`.
    Verified { anchors: TrustAnchors },
    /// TLS, verified, and sutura presents a `ClientIdentity`.
    Mutual {
        anchors: TrustAnchors,
        identity: ClientIdentity,
    },
}

impl SourceTransport {
    /// A phrase for the startup log, per source.
    #[must_use]
    pub const fn describe(&self) -> &'static str {
        match *self {
            Self::Plaintext => "plaintext",
            Self::Verified { .. } => "verified",
            Self::Mutual { .. } => "mutual",
        }
    }

    /// The declared anchors, if this channel verifies anything.
    ///
    /// `None` for `Plaintext` - nothing to verify against - and `Some` for both TLS variants,
    /// because a TLS channel always names its store. This is what lets a caller say "no transport
    /// security" without matching the variant, and it is what the load-time refusal of a remote
    /// `plaintext` source reads: a source with no anchors and a host a network can reach is refused
    /// rather than connected to in the clear.
    #[inline]
    #[must_use]
    pub const fn anchors(&self) -> Option<&TrustAnchors> {
        match *self {
            Self::Plaintext => None,
            Self::Verified { ref anchors } | Self::Mutual { ref anchors, .. } => Some(anchors),
        }
    }
}

/// Why a declared transport was not usable.
///
/// Every variant names the source (`alias`) whose entry is refused, because a refusal that does not
/// say which entry to change is a support request. No `Clone`: the cause is a `PathBuf` and the
/// value that would be cloned is a path, which is fine to own here.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTransport {
    #[error("`sources.{alias}.transport_mode` does not name how the channel to this source is secured - one of: {known}")]
    UnknownTransport { alias: SourceName, known: &'static str },
    /// A `plaintext` channel also named anchors or a client identity, which nothing would read.
    #[error(
        "`sources.{alias}` is `transport_mode: plaintext` and also names `transport_anchors` or a client certificate - a channel that encrypts nothing reads neither. Remove them, or write a TLS mode"
    )]
    PlaintextWithMaterial { alias: SourceName },
    /// A source declared TLS and named no trust anchors.
    ///
    /// Rule 2 of `docs/adr/0010`: the trust store is stated, not inherited, so there is no value to
    /// fall back to. `TrustAnchors` has no `Default` for exactly this refusal's sake.
    #[error(
        "`sources.{alias}` asks for TLS and names no `transport_anchors` - say which authority signs the source's chain, or write `transport_mode: plaintext`"
    )]
    TlsWithoutAnchors { alias: SourceName },
    /// A client certificate was written without its key, or the reverse.
    #[error(
        "`sources.{alias}.{given}` was written and `sources.{alias}.{missing}` was not - a client certificate is one pair, and a partial declaration would start with mTLS quietly disabled"
    )]
    MissingHalf {
        alias: SourceName,
        given: &'static str,
        missing: &'static str,
    },
    /// A `mutual` channel declared no client identity at all.
    ///
    /// Distinct from [`Self::MissingHalf`], which names the half a written pair left out: here
    /// nothing was written, so a message that said one half was present would be false. `mutual`
    /// promises this deployment presents a certificate, so the refusal says which keys to write.
    #[error(
        "`sources.{alias}.transport_mode` is `mutual` and no client identity was written - mutual TLS presents a certificate, so write `client_certificate` and `client_key`, or use `transport_mode: verified`"
    )]
    MutualWithoutIdentity { alias: SourceName },
    /// A path a transport declares is relative.
    #[error(
        "`sources.{alias}.{key}` is `{path}`, which is relative and resolves against this process's \
         working directory - a different directory on every host. Write an absolute path"
    )]
    RelativePath {
        alias: SourceName,
        key: &'static str,
        path: PathBuf,
    },
}

/// The three accepted `transport_mode` spellings, for the refusal's "one of" sentence.
const TRANSPORT_KNOWN: &str = "plaintext, verified, mutual";

/// Reads one source's transport from its written fields, refusing the combinations ADR 0010 says
/// a closed type must refuse.
///
/// `mode` is the `transport_mode` word. `anchors` is the written `transport_anchors` value (a path or
/// the word `system`). `client_certificate`/`client_key` are the optional identity pair. `plaintext`
/// is the one way to declare no TLS; a `plaintext` declaration that also names material is refused.
pub fn parse(
    alias: &SourceName,
    mode: &str,
    anchors: Option<&str>,
    client_certificate: Option<&str>,
    client_key: Option<&str>,
) -> Result<SourceTransport, InvalidTransport> {
    let mode = mode.trim();
    // The three words are closed: `plaintext`, or one of the two TLS modes. Anything else is unknown.
    match mode {
        // A `plaintext` channel that also names anchors or a client identity is a configuration
        // nobody can see. Refused rather than ignored.
        "plaintext" if anchors.is_some() || client_certificate.is_some() || client_key.is_some() => {
            Err(InvalidTransport::PlaintextWithMaterial { alias: alias.clone() })
        }
        "plaintext" => Ok(SourceTransport::Plaintext),
        "verified" => {
            let anchors = TrustAnchors::parse(alias, anchors)?;
            Ok(SourceTransport::Verified { anchors })
        }
        "mutual" => {
            let anchors = TrustAnchors::parse(alias, anchors)?;
            let identity = parse_client_identity(alias, client_certificate, client_key)?;
            Ok(SourceTransport::Mutual { anchors, identity })
        }
        _ => Err(InvalidTransport::UnknownTransport {
            alias: alias.clone(),
            known: TRANSPORT_KNOWN,
        }),
    }
}
/// Reads a client identity that may be absent, refusing a partial one.
fn parse_client_identity(
    alias: &SourceName,
    certificate: Option<&str>,
    key: Option<&str>,
) -> Result<ClientIdentity, InvalidTransport> {
    let certificate = certificate.map(str::trim).filter(|text| !text.is_empty());
    let key = key.map(str::trim).filter(|text| !text.is_empty());
    match (certificate, key) {
        (Some(certificate), Some(key)) => {
            let certificate = absolute(alias, "client_certificate", certificate)?;
            let key = absolute(alias, "client_key", key)?;
            Ok(ClientIdentity { certificate, key })
        }
        (Some(_), None) => Err(InvalidTransport::MissingHalf {
            alias: alias.clone(),
            given: "client_certificate",
            missing: "client_key",
        }),
        (None, Some(_)) => Err(InvalidTransport::MissingHalf {
            alias: alias.clone(),
            given: "client_key",
            missing: "client_certificate",
        }),
        (None, None) => Err(InvalidTransport::MutualWithoutIdentity { alias: alias.clone() }),
    }
}

/// Reads a transport path that has to be absolute, naming the key it refuses.
fn absolute(alias: &SourceName, key: &'static str, written: &str) -> Result<PathBuf, InvalidTransport> {
    let path = PathBuf::from(written);
    if path.is_relative() {
        return Err(InvalidTransport::RelativePath {
            alias: alias.clone(),
            key,
            path,
        });
    }
    Ok(path)
}

/// Whether a declared source host can only be reached from this machine.
///
/// **A name is not an address**, which is the rule `crate::server::BindAddress` already applies to
/// the serving bind read the other way round: `localhost` resolves to whatever the resolver says
/// today, so it cannot carry a claim about what a network can reach. Only an `IpAddr` literal
/// answers `true`, and only a loopback one - so a `plaintext` declaration is refused for a hostname
/// however it happens to resolve. That is the fail-closed direction issue 124 asks for: an operator
/// who means a loopback TCP dial writes `127.0.0.1` or `::1`.
#[must_use]
pub fn host_is_loopback(host: &str) -> bool {
    host.trim().parse::<IpAddr>().is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test source is a source")
    }

    #[test]
    fn a_partial_client_certificate_names_the_missing_half() {
        assert_eq!(
            parse(&source("pg"), "mutual", Some("/certs/root.pem"), Some("/tls/c.pem"), None),
            Err(InvalidTransport::MissingHalf {
                alias: source("pg"),
                given: "client_certificate",
                missing: "client_key",
            })
        );
        assert_eq!(
            parse(&source("pg"), "mutual", Some("/certs/root.pem"), None, Some("/tls/k.pem")),
            Err(InvalidTransport::MissingHalf {
                alias: source("pg"),
                given: "client_key",
                missing: "client_certificate",
            })
        );
        // And neither half written is its own refusal, not a `MissingHalf` claiming one was there.
        assert_eq!(
            parse(&source("pg"), "mutual", Some("/certs/root.pem"), None, None),
            Err(InvalidTransport::MutualWithoutIdentity { alias: source("pg") })
        );
    }

    #[test]
    fn only_a_loopback_ip_literal_is_loopback() {
        // The whole of issue 124's fail-closed rule: a name is not an address, so it cannot carry a
        // claim about what a network can reach. `127.0.0.53` is in `127.0.0.0/8` and is loopback.
        assert!(host_is_loopback("127.0.0.1"));
        assert!(host_is_loopback("::1"));
        assert!(host_is_loopback("127.0.0.53"));
        assert!(!host_is_loopback("localhost"));
        assert!(!host_is_loopback("0.0.0.0"));
        assert!(!host_is_loopback("10.0.0.7"));
        assert!(!host_is_loopback("db.example.com"));
    }

    #[test]
    fn a_client_identity_is_one_pair_or_nothing() {
        let parsed = parse(
            &source("pg"),
            "mutual",
            Some("system"),
            Some("/tls/c.pem"),
            Some("/tls/k.pem"),
        )
        .expect("a pair parses");
        match parsed {
            SourceTransport::Mutual {
                anchors: TrustAnchors::System,
                identity,
            } => {
                assert_eq!(identity.certificate(), &PathBuf::from("/tls/c.pem"));
                assert_eq!(identity.key(), &PathBuf::from("/tls/k.pem"));
            }
            other => panic!("expected mutual, got {other:?}"),
        }
        // A relative client key is refused naming the key.
        assert!(matches!(
            parse(&source("pg"), "mutual", Some("system"), Some("/tls/c.pem"), Some("k.pem")),
            Err(InvalidTransport::RelativePath {
                alias,
                key: "client_key",
                ..
            }) if alias == source("pg")
        ));
    }

    #[test]
    fn tls_without_anchors_refuses_naming_the_source() {
        assert_eq!(
            parse(&source("warehouse"), "verified", None, None, None),
            Err(InvalidTransport::TlsWithoutAnchors {
                alias: source("warehouse")
            })
        );
        assert_eq!(
            parse(&source("warehouse"), "mutual", None, Some("/tls/c.pem"), Some("/tls/k.pem")),
            Err(InvalidTransport::TlsWithoutAnchors {
                alias: source("warehouse")
            })
        );
    }

    #[test]
    fn an_unknown_transport_word_is_refused() {
        assert!(matches!(
            parse(&source("pg"), "require", None, None, None),
            Err(InvalidTransport::UnknownTransport { alias, known }) if alias == source("pg") && known.contains("plaintext")
        ));
    }

    #[test]
    fn a_plaintext_channel_names_itself_and_refuses_orphans() {
        assert_eq!(
            parse(&source("pg"), "plaintext", None, None, None).unwrap(),
            SourceTransport::Plaintext
        );
        // A plaintext channel that also names anchors is a configuration nobody can see.
        assert!(matches!(
            parse(&source("pg"), "plaintext", Some("system"), None, None),
            Err(InvalidTransport::PlaintextWithMaterial { alias }) if alias == source("pg")
        ));
    }

    #[test]
    fn the_transport_describes_itself_for_the_log() {
        assert_eq!(SourceTransport::Plaintext.describe(), "plaintext");
        assert_eq!(
            SourceTransport::Verified {
                anchors: TrustAnchors::System
            }
            .describe(),
            "verified"
        );
        assert_eq!(
            SourceTransport::Mutual {
                anchors: TrustAnchors::System,
                identity: ClientIdentity {
                    certificate: PathBuf::from("/tls/c.pem"),
                    key: PathBuf::from("/tls/k.pem"),
                }
            }
            .describe(),
            "mutual"
        );
    }

    #[test]
    fn only_the_tls_variants_carry_anchors() {
        assert!(SourceTransport::Plaintext.anchors().is_none());
        assert!(
            SourceTransport::Verified {
                anchors: TrustAnchors::System
            }
            .anchors()
            .is_some()
        );
        assert!(
            SourceTransport::Mutual {
                anchors: TrustAnchors::System,
                identity: ClientIdentity {
                    certificate: PathBuf::from("/tls/c.pem"),
                    key: PathBuf::from("/tls/k.pem"),
                }
            }
            .anchors()
            .is_some()
        );
    }
}
