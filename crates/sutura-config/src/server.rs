//! Where the service listens, and the two bounds every request is held to.
//!
//! The interesting type here is [`BindAddress`], and it is interesting for a security reason
//! rather than an ergonomic one. This service has no per-caller identity - see the module
//! documentation on [`crate`] - so the interface it listens on is the whole of its perimeter.
//! Making the bind an IP address rather than a string is what lets
//! [`BindAddress::is_loopback`] be a fact the startup refusals can read, instead of a substring
//! test somebody has to remember to write.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The socket the service listens on.
///
/// An `IpAddr` and a port, never a hostname. A hostname is refused rather than resolved: a name
/// resolves to whatever the resolver says today, which may be a public interface tomorrow, and
/// a perimeter that moves when DNS moves is not a perimeter. The operator writes the interface
/// they mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindAddress(SocketAddr);

/// Why a host and port are not an address to listen on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBindAddress {
    /// The host was not an `IPv4` or `IPv6` literal.
    #[error("`{found}` is not an IP address - a bind host is an interface, not a name to resolve")]
    NotAnIpAddress { found: String },
}

impl BindAddress {
    /// Reads a host and port.
    pub fn parse(host: impl AsRef<str>, port: u16) -> Result<Self, InvalidBindAddress> {
        let host = host.as_ref().trim();
        // A v6 address is bracketed in a socket string, and an operator who copied one out of a
        // URL should not be told it is not an address.
        let bare = host.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')).unwrap_or(host);
        let address: IpAddr = bare.parse().map_err(|_ignored| InvalidBindAddress::NotAnIpAddress {
            found: String::from(host),
        })?;
        Ok(Self(SocketAddr::new(address, port)))
    }

    /// The address, for whatever binds the socket.
    #[inline]
    pub const fn socket(self) -> SocketAddr {
        self.0
    }

    /// Is this address reachable only from this host?
    ///
    /// The question the production refusals are keyed on. The two wildcard addresses answer
    /// `false`, which is the answer that matters: the wildcard is the bind that reaches every
    /// interface, and it is the one an operator reaches for without meaning to publish anything.
    #[inline]
    pub const fn is_loopback(self) -> bool {
        self.0.ip().is_loopback()
    }

    /// The port, so a caller can report the one it actually got.
    #[inline]
    pub const fn port(self) -> u16 {
        self.0.port()
    }
}

impl core::fmt::Display for BindAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How long one request may take before the service gives up on it.
///
/// Bounded at both ends. Zero is a service that answers nothing, and an hour is a connection
/// held open long enough that a handful of them are the outage: a question here is one
/// aggregate over a bounded range, so a minute is already generous and five is the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestTimeout(Duration);

/// The largest request body the service will read.
///
/// A modelled question is a metric name, a grain, two dates and at most four dimensions, which
/// is a few hundred bytes. The bound exists because a body limit is the cheapest availability
/// control there is, and because the default in most stacks is whatever arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyLimit(usize);

/// How long stopping may take, once stopping has been asked for.
///
/// Chosen against the deadline on the other side rather than as a round number. An orchestrator
/// sends a termination signal and starts a kill timer - the usual window is thirty seconds - and a
/// process still running when that expires is killed mid-answer, so whatever it would have done on
/// the way out does not happen. Fifteen seconds leaves room for the exit itself.
///
/// It bounds the *whole* of stopping and not the connection drain alone: the drain gets the budget
/// first, and what is left of it is what the runtime will wait for a blocking task it cannot
/// cancel. See `sutura_runtime::Shutdown::remaining_grace`.
///
/// Zero is refused for the reason every bound in this module is. A zero grace period means "drop
/// every in-flight answer immediately", which is a decision somebody would write differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownGrace(Duration);

/// Why a bound is not a bound.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBound {
    /// Nothing here may be zero: a zero timeout answers nothing and a zero body limit accepts
    /// nothing, and both read as no limit at all to somebody writing the file.
    #[error("{name} may not be zero - a zero bound reads as `no limit` and is the opposite")]
    Zero { name: &'static str },
    /// Above the ceiling this type declares.
    #[error("{name} is {found} and the maximum is {limit}")]
    TooLarge { name: &'static str, found: u64, limit: u64 },
}

impl RequestTimeout {
    /// The ceiling, in seconds. Five minutes: long enough for a cold cache on a large scan,
    /// short enough that a stuck request is not a leaked connection for the rest of the day.
    pub const MAX_SECONDS: u64 = 300;

    /// Reads a timeout in whole seconds.
    ///
    /// Seconds and not a duration string: sub-second precision is meaningless for a bound this
    /// coarse, and a parser for a suffixed number is a second grammar for a single value.
    pub const fn parse(seconds: u64) -> Result<Self, InvalidBound> {
        if seconds == 0 {
            return Err(InvalidBound::Zero {
                name: "server.request_timeout_seconds",
            });
        }
        if seconds > Self::MAX_SECONDS {
            return Err(InvalidBound::TooLarge {
                name: "server.request_timeout_seconds",
                found: seconds,
                limit: Self::MAX_SECONDS,
            });
        }
        Ok(Self(Duration::from_secs(seconds)))
    }

    #[inline]
    pub const fn duration(self) -> Duration {
        self.0
    }

    #[inline]
    pub const fn seconds(self) -> u64 {
        self.0.as_secs()
    }
}

impl BodyLimit {
    /// The ceiling, in bytes. One mebibyte, which is roughly four orders of magnitude more than
    /// the largest question this surface can represent.
    pub const MAX_BYTES: usize = 1024 * 1024;

    /// Reads a body limit in bytes.
    pub const fn parse(bytes: usize) -> Result<Self, InvalidBound> {
        if bytes == 0 {
            return Err(InvalidBound::Zero {
                name: "server.max_body_bytes",
            });
        }
        if bytes > Self::MAX_BYTES {
            return Err(InvalidBound::TooLarge {
                name: "server.max_body_bytes",
                found: bytes as u64,
                limit: Self::MAX_BYTES as u64,
            });
        }
        Ok(Self(bytes))
    }

    #[inline]
    pub const fn bytes(self) -> usize {
        self.0
    }
}

/// A certificate chain and the private key that goes with it, as paths.
///
/// **Paths and nothing more, and the split is deliberate.** This crate holds no framework and
/// reads no files: it parses the *pair* - both halves or neither - and stops there. Whether the
/// files are readable, whether they are PEM at all, and whether the key matches the certificate
/// are questions only the TLS implementation can answer, so they are answered once, in
/// `sutura_http::tls`, before the socket is bound. Two checks in two crates would be two messages
/// for one mistake, and the weaker one would be the reassuring one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsMaterial {
    certificate: PathBuf,
    key: PathBuf,
}

/// Why a pair of paths is not usable TLS material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTlsMaterial {
    /// One half was given and the other was not.
    ///
    /// Refused rather than half-configured: a certificate with no key cannot start a listener, and
    /// the silent alternative is a service that either falls back to plaintext or fails at the
    /// first handshake.
    #[error("{given} is set and {missing} is not - TLS needs both a certificate chain and its private key")]
    OnlyOneHalf { given: &'static str, missing: &'static str },
    /// A path was given as an empty string, which is not a path.
    #[error("{name} is empty - remove the key to disable TLS rather than setting it to nothing")]
    EmptyPath { name: &'static str },
}

impl TlsMaterial {
    /// Reads the pair, or nothing.
    ///
    /// `None` for both is "no in-process TLS", which is the default and is not an error.
    pub fn parse(certificate: Option<&str>, key: Option<&str>) -> Result<Option<Self>, InvalidTlsMaterial> {
        match (trimmed(certificate), trimmed(key)) {
            (None, None) => Ok(None),
            (Some(certificate), None) => {
                if certificate.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath {
                        name: "server.tls_certificate",
                    });
                }
                Err(InvalidTlsMaterial::OnlyOneHalf {
                    given: "server.tls_certificate",
                    missing: "server.tls_key",
                })
            }
            (None, Some(key)) => {
                if key.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath { name: "server.tls_key" });
                }
                Err(InvalidTlsMaterial::OnlyOneHalf {
                    given: "server.tls_key",
                    missing: "server.tls_certificate",
                })
            }
            (Some(certificate), Some(key)) => {
                if certificate.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath {
                        name: "server.tls_certificate",
                    });
                }
                if key.is_empty() {
                    return Err(InvalidTlsMaterial::EmptyPath { name: "server.tls_key" });
                }
                Ok(Some(Self {
                    certificate: PathBuf::from(certificate),
                    key: PathBuf::from(key),
                }))
            }
        }
    }

    /// The certificate chain, in PEM.
    #[inline]
    pub fn certificate(&self) -> &Path {
        &self.certificate
    }

    /// The private key, in PEM.
    #[inline]
    pub fn key(&self) -> &Path {
        &self.key
    }
}

/// The trimmed value, treating an absent key and an unset variable the same way.
///
/// An empty string is the shape an unset variable takes in a shell, and it reaches here as
/// `Some("")` - kept distinct from `None` on purpose, so setting a path to nothing is an error
/// naming the key rather than TLS silently switching itself off.
fn trimmed(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim)
}

/// Everything about the socket, the two per-request bounds, and the TLS material if there is any.
///
/// **Not `Copy`, and that is the TLS paths.** Every accessor borrows or returns a `Copy` value, and
/// the group itself is read once, at assembly time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSettings {
    bind: BindAddress,
    request_timeout: RequestTimeout,
    max_body: BodyLimit,
    tls: Option<TlsMaterial>,
}

impl ServerSettings {
    /// Assembles the group from parts that have each already been parsed.
    ///
    /// Infallible, and that is the shape the newtypes buy: there is no cross-field rule inside
    /// this group, so once every part exists the group exists. The cross-field rules - the ones
    /// that pair a bind address with an environment, a token and a TLS declaration - live in
    /// [`crate::Settings::parse`], because they need the other groups to decide.
    #[inline]
    pub const fn new(bind: BindAddress, request_timeout: RequestTimeout, max_body: BodyLimit, tls: Option<TlsMaterial>) -> Self {
        Self {
            bind,
            request_timeout,
            max_body,
            tls,
        }
    }

    #[inline]
    pub const fn bind(&self) -> BindAddress {
        self.bind
    }

    #[inline]
    pub const fn request_timeout(&self) -> RequestTimeout {
        self.request_timeout
    }

    #[inline]
    pub const fn max_body(&self) -> BodyLimit {
        self.max_body
    }

    /// The certificate and key this process would terminate TLS with, if any were configured.
    #[inline]
    pub const fn tls(&self) -> Option<&TlsMaterial> {
        self.tls.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{BindAddress, BodyLimit, InvalidBindAddress, InvalidBound, InvalidTlsMaterial, RequestTimeout, TlsMaterial};

    #[test]
    fn loopback_is_recognised_in_both_address_families() {
        assert!(BindAddress::parse("127.0.0.1", 8080).expect("v4 loopback").is_loopback());
        assert!(BindAddress::parse("::1", 8080).expect("v6 loopback").is_loopback());
        // Written the way a URL writes it, which is how it will be copied in.
        assert!(
            BindAddress::parse("[::1]", 8080)
                .expect("bracketed v6 loopback")
                .is_loopback()
        );
    }

    #[test]
    fn the_wildcard_is_not_loopback() {
        // The whole reason `is_loopback` exists. The v4 wildcard is the bind an operator reaches
        // for to make a container reachable, and it is every interface - so it must answer
        // `false` and take the explicit acknowledgement path.
        assert!(
            !BindAddress::parse("0.0.0.0", 8080)
                .expect("the v4 wildcard is an address")
                .is_loopback()
        );
        assert!(
            !BindAddress::parse("::", 8080)
                .expect("the v6 wildcard is an address")
                .is_loopback()
        );
        assert!(
            !BindAddress::parse("10.0.0.7", 8080)
                .expect("a private address is an address")
                .is_loopback()
        );
    }

    #[test]
    fn a_hostname_is_not_a_bind_address() {
        // Refused rather than resolved: what a name points at is not this process's decision,
        // and a perimeter that moves when a DNS record moves is not one.
        let error = BindAddress::parse("localhost", 8080).expect_err("a name is not an interface");
        assert_eq!(
            error,
            InvalidBindAddress::NotAnIpAddress {
                found: String::from("localhost")
            }
        );
    }

    #[test]
    fn a_zero_bound_is_refused_rather_than_read_as_unlimited() {
        assert_eq!(
            RequestTimeout::parse(0),
            Err(InvalidBound::Zero {
                name: "server.request_timeout_seconds"
            })
        );
        assert_eq!(
            BodyLimit::parse(0),
            Err(InvalidBound::Zero {
                name: "server.max_body_bytes"
            })
        );
    }

    #[test]
    fn a_bound_above_the_ceiling_is_refused_and_names_both_numbers() {
        let error =
            RequestTimeout::parse(RequestTimeout::MAX_SECONDS.saturating_add(1)).expect_err("above the ceiling is not a timeout");
        assert!(
            matches!(
                error,
                InvalidBound::TooLarge {
                    found: 301,
                    limit: 300,
                    ..
                }
            ),
            "{error:?}"
        );
        let error = BodyLimit::parse(BodyLimit::MAX_BYTES.saturating_add(1)).expect_err("above the ceiling is not a body limit");
        let InvalidBound::TooLarge { limit, .. } = error else {
            panic!("above the ceiling is a too-large bound, not {error:?}");
        };
        assert_eq!(limit, BodyLimit::MAX_BYTES as u64);
    }

    #[test]
    fn the_boundary_values_themselves_are_accepted() {
        // The other side of the two assertions above, a step away, so a `>=` written where `>`
        // belongs fails one of them.
        assert_eq!(
            RequestTimeout::parse(RequestTimeout::MAX_SECONDS)
                .expect("the ceiling is a timeout")
                .seconds(),
            RequestTimeout::MAX_SECONDS
        );
        assert_eq!(RequestTimeout::parse(1).expect("one second is a timeout").seconds(), 1);
        assert_eq!(
            BodyLimit::parse(BodyLimit::MAX_BYTES)
                .expect("the ceiling is a body limit")
                .bytes(),
            BodyLimit::MAX_BYTES
        );
        assert_eq!(BodyLimit::parse(1).expect("one byte is a body limit").bytes(), 1);
    }

    #[test]
    fn tls_material_is_both_halves_or_neither() {
        assert_eq!(TlsMaterial::parse(None, None), Ok(None));
        let pair = TlsMaterial::parse(Some("/tls/chain.pem"), Some("/tls/key.pem"))
            .expect("a pair parses")
            .expect("a pair is material");
        assert_eq!(pair.certificate(), std::path::Path::new("/tls/chain.pem"));
        assert_eq!(pair.key(), std::path::Path::new("/tls/key.pem"));
    }

    #[test]
    fn half_configured_tls_is_refused_rather_than_ignored() {
        // The failure this refuses is the quiet one: a certificate with no key either falls back to
        // plaintext on a port somebody believes is encrypted, or dies at the first handshake.
        assert_eq!(
            TlsMaterial::parse(Some("/tls/chain.pem"), None),
            Err(InvalidTlsMaterial::OnlyOneHalf {
                given: "server.tls_certificate",
                missing: "server.tls_key"
            })
        );
        assert_eq!(
            TlsMaterial::parse(None, Some("/tls/key.pem")),
            Err(InvalidTlsMaterial::OnlyOneHalf {
                given: "server.tls_key",
                missing: "server.tls_certificate"
            })
        );
    }

    #[test]
    fn a_path_set_to_nothing_is_an_error_and_not_a_switch() {
        // An empty string is what an unset variable looks like in a shell, and reading it as "TLS
        // off" is how a deployment that meant to serve TLS serves plaintext instead.
        assert_eq!(
            TlsMaterial::parse(Some(""), Some("/tls/key.pem")),
            Err(InvalidTlsMaterial::EmptyPath {
                name: "server.tls_certificate"
            })
        );
        assert_eq!(
            TlsMaterial::parse(Some("/tls/chain.pem"), Some("  ")),
            Err(InvalidTlsMaterial::EmptyPath { name: "server.tls_key" })
        );
    }
}
