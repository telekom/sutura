//! Where the service listens, and the two bounds every request is held to.
//!
//! The interesting type here is [`BindAddress`], and it is interesting for a security reason
//! rather than an ergonomic one. This service has no per-caller identity - see the module
//! documentation on [`crate`] - so the interface it listens on is the whole of its perimeter.
//! Making the bind an IP address rather than a string is what lets
//! [`BindAddress::is_loopback`] be a fact the startup refusals can read, instead of a substring
//! test somebody has to remember to write.

use std::net::{IpAddr, SocketAddr};
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

/// Everything about the socket and the two per-request bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSettings {
    bind: BindAddress,
    request_timeout: RequestTimeout,
    max_body: BodyLimit,
}

impl ServerSettings {
    /// Assembles the group from parts that have each already been parsed.
    ///
    /// Infallible, and that is the shape the newtypes buy: there is no cross-field rule inside
    /// this group, so once every part exists the group exists. The cross-field rules - the ones
    /// that pair a bind address with an environment and a token - live in
    /// [`crate::Settings::parse`], because they need the other groups to decide.
    #[inline]
    pub const fn new(bind: BindAddress, request_timeout: RequestTimeout, max_body: BodyLimit) -> Self {
        Self {
            bind,
            request_timeout,
            max_body,
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
}

#[cfg(test)]
mod tests {
    use super::{BindAddress, BodyLimit, InvalidBindAddress, InvalidBound, RequestTimeout};

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
}
