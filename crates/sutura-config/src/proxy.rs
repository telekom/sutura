//! Which address a request is counted against, and who is allowed to say what it is.
//!
//! # The problem this module exists for
//!
//! A rate limiter needs a key, and on a surface with no per-caller identity - see
//! [`crate::security`] - the only key available is a network address. There are two ways to learn
//! one and they fail in opposite directions.
//!
//! The **peer address** is the far end of the TCP connection. It cannot be forged by a caller,
//! and behind a reverse proxy or an ingress controller it is the *proxy's* address for every
//! request that has ever arrived - so every caller on the internet shares one bucket. Either one
//! abusive caller limits everybody, or the quota is set high enough to be no limit at all.
//!
//! A **forwarded header** carries the address the proxy saw. It is the right answer behind a
//! proxy and it is a value any caller can write, so on a service that is *not* behind one it
//! makes every bucket the caller's to choose - which is worse than one shared bucket, because it
//! is a limiter that reports a configured limit and bounds nothing.
//!
//! Neither is correct on its own. What is correct is a header read **only from a peer the
//! operator named**, which is what [`TrustedProxies`] is: an explicit list, empty by default, and
//! [`ClientAddressSource::Forwarded`] refuses to start without one rather than trusting a
//! spoofable header because a key was left at its default.
//!
//! # Which entry of the header is the caller
//!
//! `X-Forwarded-For` is appended to, hop by hop, so it reads left to right as oldest to newest.
//! Everything to the left of what our own trusted proxy wrote is a value the caller supplied, and
//! a caller who writes `X-Forwarded-For: 10.0.0.1` gets that value read as their address by
//! anything that takes the leftmost entry.
//!
//! So the walk is from the **right**: skip the entries that are addresses of trusted proxies, and
//! the first entry that is not one is the client. If every entry is a trusted proxy, or the list
//! runs out, or an entry is not an address at all, the peer address is used - it is the one value
//! in the request nobody but the network can choose.

use std::net::IpAddr;

/// An address or a block of them, as an operator writes it.
///
/// `10.0.0.0/8` or a bare `10.0.0.7`, in either address family. A bare address is a block whose
/// prefix covers every bit, so there is one shape to match against rather than two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

/// Why a string is not an address block.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTrustedProxy {
    /// The part before the slash was not an `IPv4` or `IPv6` literal.
    #[error("`{found}` is not an address or an address block - a trusted proxy is an interface, not a name to resolve")]
    NotAnAddress { found: String },
    /// The part after the slash was not a number.
    #[error("`{found}` has a prefix length that is not a number")]
    PrefixNotANumber { found: String },
    /// The prefix is longer than the address family has bits.
    #[error("`{found}` has a prefix length of {prefix} and this address family has {maximum} bits")]
    PrefixTooLong { found: String, prefix: u8, maximum: u8 },
}

impl Cidr {
    /// Reads one entry.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidTrustedProxy> {
        let raw = raw.as_ref().trim();
        let (address, prefix) = match raw.split_once('/') {
            None => (raw, None),
            Some((address, prefix)) => (address, Some(prefix)),
        };
        // A v6 address is bracketed in a URL, and an operator who copied one out of one should not
        // be told it is not an address. The same affordance as `crate::server::BindAddress`.
        let bare = address
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .unwrap_or(address);
        let network = bare
            .parse::<IpAddr>()
            .map_err(|_ignored| InvalidTrustedProxy::NotAnAddress {
                found: String::from(raw),
            })?
            // An `IPv4`-mapped `IPv6` address is the same host written another way, and a
            // dual-stack listener reports every v4 peer in that form. Canonicalising both sides is
            // what makes `10.0.0.0/8` match a peer that arrived as `::ffff:10.0.0.1`.
            .to_canonical();
        let maximum = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => maximum,
            Some(digits) => digits
                .trim()
                .parse::<u8>()
                .map_err(|_ignored| InvalidTrustedProxy::PrefixNotANumber {
                    found: String::from(raw),
                })?,
        };
        if prefix > maximum {
            return Err(InvalidTrustedProxy::PrefixTooLong {
                found: String::from(raw),
                prefix,
                maximum,
            });
        }
        Ok(Self { network, prefix })
    }

    /// Is `address` inside this block?
    ///
    /// Mixed families never match: a v4 block does not contain a v6 address, and the canonical
    /// form applied on both sides is what keeps a v4 peer arriving over a dual-stack socket from
    /// being one.
    #[must_use]
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.network, address.to_canonical()) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                let mask = u32::MAX
                    .checked_shl(u32::from(32_u8.saturating_sub(self.prefix)))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                let mask = u128::MAX
                    .checked_shl(u32::from(128_u8.saturating_sub(self.prefix)))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

impl core::fmt::Display for Cidr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

/// The peers whose forwarded header this service will believe.
///
/// **Empty by default, and the emptiness is the safe posture rather than an unset value.** With
/// nothing in the list the peer address is the key, which is correct for a service with no proxy
/// in front. It becomes non-empty only when an operator names the hop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<Cidr>);

impl TrustedProxies {
    /// Reads the configured list.
    pub fn parse<S>(entries: &[S]) -> Result<Self, InvalidTrustedProxy>
    where
        S: AsRef<str>,
    {
        entries.iter().map(Cidr::parse).collect::<Result<Vec<Cidr>, _>>().map(Self)
    }

    /// Did the operator name any hop at all?
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many blocks are trusted, for the startup log.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Is `address` one of the hops the operator named?
    #[must_use]
    pub fn trusts(&self, address: IpAddr) -> bool {
        self.0.iter().any(|block| block.contains(address))
    }
}

/// Where the address a request is counted against comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClientAddressSource {
    /// The far end of the connection. Unforgeable, and one bucket for everything behind a proxy.
    #[default]
    Peer,
    /// `X-Forwarded-For`, read only from a peer in [`TrustedProxies`], falling back to the peer
    /// address for anything else.
    Forwarded,
}

/// The configured value did not name a source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not a client address source - one of: {}", ClientAddressSource::NAMES.join(", "))]
pub struct UnknownClientAddressSource {
    found: String,
}

impl ClientAddressSource {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["peer", "forwarded"];

    /// Reads the configured value.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownClientAddressSource> {
        match raw.as_ref().trim() {
            "peer" => Ok(Self::Peer),
            "forwarded" => Ok(Self::Forwarded),
            other => Err(UnknownClientAddressSource {
                found: String::from(other),
            }),
        }
    }

    /// The spelling, for the startup log.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Peer => "peer",
            Self::Forwarded => "forwarded",
        }
    }

    /// Does this source read a caller-supplied header?
    #[inline]
    pub const fn reads_a_header(self) -> bool {
        matches!(self, Self::Forwarded)
    }
}

impl core::fmt::Display for ClientAddressSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::{Cidr, ClientAddressSource, InvalidTrustedProxy, TrustedProxies};

    fn address(raw: &str) -> IpAddr {
        raw.parse().expect("a test address is an address")
    }

    #[test]
    fn a_bare_address_is_a_block_of_one() {
        let block = Cidr::parse("10.0.0.7").expect("a bare address is a block");
        assert!(block.contains(address("10.0.0.7")));
        assert!(!block.contains(address("10.0.0.8")));
    }

    #[test]
    fn a_prefix_matches_the_whole_block_and_nothing_outside_it() {
        let block = Cidr::parse("10.42.0.0/16").expect("a block is a block");
        assert!(block.contains(address("10.42.0.1")));
        assert!(block.contains(address("10.42.255.254")));
        assert!(!block.contains(address("10.43.0.1")));
        // The boundary a shift off by one gets wrong.
        let single = Cidr::parse("10.42.0.1/32").expect("a full prefix is a block");
        assert!(single.contains(address("10.42.0.1")));
        assert!(!single.contains(address("10.42.0.2")));
    }

    #[test]
    fn a_zero_prefix_matches_everything_in_its_family_and_nothing_in_the_other() {
        // The arithmetic edge: a shift by the full width is what `checked_shl` returns `None` for,
        // and reading that as "no mask" rather than panicking is what makes `/0` expressible.
        let all = Cidr::parse("0.0.0.0/0").expect("the whole v4 space is a block");
        assert!(all.contains(address("1.2.3.4")));
        assert!(all.contains(address("255.255.255.255")));
        assert!(!all.contains(address("2001:db8::1")));
    }

    #[test]
    fn a_v4_mapped_v6_peer_matches_the_v4_block_it_is() {
        // The case a dual-stack listener produces, and the reason both sides are canonicalised. A
        // pod behind an ingress reports its peer this way, so without this every v4 block would
        // silently trust nothing.
        let block = Cidr::parse("10.0.0.0/8").expect("a block is a block");
        assert!(block.contains(address("::ffff:10.0.0.1")));
        assert!(!block.contains(address("::ffff:11.0.0.1")));
    }

    #[test]
    fn a_bracketed_v6_block_is_read_the_way_a_url_writes_it() {
        let block = Cidr::parse("[2001:db8::]/32").expect("a bracketed block is a block");
        assert!(block.contains(address("2001:db8::1")));
        assert!(!block.contains(address("2001:db9::1")));
    }

    #[test]
    fn a_name_a_bad_prefix_and_an_oversized_prefix_are_each_refused() {
        assert_eq!(
            Cidr::parse("proxy.example"),
            Err(InvalidTrustedProxy::NotAnAddress {
                found: String::from("proxy.example")
            })
        );
        assert_eq!(
            Cidr::parse("10.0.0.0/eight"),
            Err(InvalidTrustedProxy::PrefixNotANumber {
                found: String::from("10.0.0.0/eight")
            })
        );
        assert_eq!(
            Cidr::parse("10.0.0.0/33"),
            Err(InvalidTrustedProxy::PrefixTooLong {
                found: String::from("10.0.0.0/33"),
                prefix: 33,
                maximum: 32
            })
        );
        // 33 is legal for v6, so the maximum is per family rather than a constant.
        Cidr::parse("2001:db8::/33").expect("33 is a legal v6 prefix length");
    }

    #[test]
    fn an_empty_list_trusts_nothing() {
        // The default, and the whole reason the default is safe: with nothing named, no header is
        // believed and the peer address is the key.
        let proxies = TrustedProxies::default();
        assert!(proxies.is_empty());
        assert_eq!(proxies.len(), 0);
        assert!(!proxies.trusts(address("10.0.0.1")));
    }

    #[test]
    fn a_configured_list_trusts_exactly_what_it_names() {
        let proxies = TrustedProxies::parse(&["10.0.0.0/8", "192.168.1.10"]).expect("two blocks parse");
        assert!(!proxies.is_empty());
        assert_eq!(proxies.len(), 2);
        assert!(proxies.trusts(address("10.1.2.3")));
        assert!(proxies.trusts(address("192.168.1.10")));
        assert!(!proxies.trusts(address("192.168.1.11")));
        assert!(!proxies.trusts(address("203.0.113.7")));
    }

    #[test]
    fn one_bad_entry_refuses_the_whole_list() {
        // Rather than dropping it: a list that silently lost an entry is a proxy nobody trusts and
        // a limiter keyed on the wrong thing, with nothing anywhere saying so.
        TrustedProxies::parse(&["10.0.0.0/8", "not-an-address"]).unwrap_err();
    }

    #[test]
    fn the_source_round_trips_and_an_unknown_one_is_refused() {
        for name in ClientAddressSource::NAMES {
            let parsed = ClientAddressSource::parse(name).expect("a listed name parses");
            assert_eq!(parsed.as_str(), *name);
        }
        assert_eq!(ClientAddressSource::default(), ClientAddressSource::Peer);
        assert!(!ClientAddressSource::Peer.reads_a_header());
        assert!(ClientAddressSource::Forwarded.reads_a_header());
        let error = ClientAddressSource::parse("x-real-ip").expect_err("an unlisted name is not a source");
        assert!(error.to_string().contains("forwarded"), "{error}");
    }
}
