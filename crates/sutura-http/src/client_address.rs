//! The address a rate-limit bucket is counted against, and why it is not simply the peer.
//!
//! # The bug this module exists to fix
//!
//! `tower_governor` ships two extractors and both are wrong for one of the two deployments this
//! service has. `PeerIpKeyExtractor` keys on the connection's far end, which cannot be forged and
//! which behind an ingress controller is *the ingress* for every request that has ever arrived - so
//! every caller on the internet shares one bucket, and the limiter either takes everybody down with
//! one abusive caller or is set high enough to bound nothing. `SmartIpKeyExtractor` reads
//! `X-Forwarded-For` unconditionally, which is correct behind a proxy and is a value any caller can
//! write, so with no proxy in front every caller picks their own bucket.
//!
//! [`ClientAddress`] is the third option: read the header **only from a peer the operator named**,
//! and use the peer address for everything else. Whether it reads a header at all is
//! [`sutura_config::ClientAddressSource`], the named hops are
//! [`sutura_config::TrustedProxies`], and the combination that would trust a header with nobody
//! named is a startup refusal rather than a default - see `sutura_config::NotFitToServe`.
//!
//! # Reading the header from the right
//!
//! `X-Forwarded-For` is appended to hop by hop, so it reads oldest to newest left to right. The
//! *rightmost* entry is the one our own trusted proxy wrote; everything left of it arrived from the
//! caller. Taking the leftmost entry - which is what every `maybe_x_forwarded_for` helper does,
//! upstream's included - hands the key straight to whoever sent the request.
//!
//! So the walk is from the right, skipping entries that are themselves trusted hops, and the first
//! entry that is not one is the client. Four things fall back to the peer address, which is the one
//! value in a request that nobody but the network chooses:
//!
//! * the peer is not a trusted proxy - the header is then a caller's own text;
//! * there is no header;
//! * an entry is not an address, so nothing further left can be trusted either;
//! * every entry is a trusted hop, so the header names no client.
//!
//! Multiple `X-Forwarded-For` header *lines* are walked from the last line backwards as well.
//! `HeaderMap::get` returns the first, and a caller who sends their own line before the proxy
//! appends a second one would otherwise have theirs read.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request};
use sutura_config::{ClientAddressSource, TrustedProxies};
use tower_governor::GovernorError;
use tower_governor::key_extractor::KeyExtractor;

/// The header a proxy records the caller's address in.
const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Where a bucket's key comes from, as a value the limiter layer is built with.
///
/// `Clone` because [`KeyExtractor`] requires it and the layer clones it per connection; the trusted
/// list is behind an `Arc` so that clone is a pointer bump rather than a copy of every block.
#[derive(Debug, Clone)]
pub struct ClientAddress {
    source: ClientAddressSource,
    trusted: Arc<TrustedProxies>,
}

impl ClientAddress {
    /// Builds the extractor the configuration describes.
    #[must_use]
    pub const fn new(source: ClientAddressSource, trusted: Arc<TrustedProxies>) -> Self {
        Self { source, trusted }
    }

    /// Reads the configuration's own two values, so a caller cannot pair a source with the wrong
    /// list.
    #[must_use]
    pub fn from_settings(limits: &sutura_config::RateLimitSettings) -> Self {
        Self::new(limits.client_address(), Arc::new(limits.trusted_proxies().clone()))
    }

    /// The address this request is attributed to.
    ///
    /// Public and taking the parts rather than a request, so the whole decision is testable without
    /// building one - the interesting cases are all about which entry of a header is believed.
    #[must_use]
    pub fn attribute(&self, headers: &HeaderMap, peer: IpAddr) -> IpAddr {
        let peer = peer.to_canonical();
        match self.source {
            ClientAddressSource::Peer => peer,
            // The peer is what decides whether the header is read at all. A caller who is not a
            // named hop is a caller whose header is their own prose.
            ClientAddressSource::Forwarded if !self.trusted.trusts(peer) => peer,
            ClientAddressSource::Forwarded => self.rightmost_untrusted(headers).unwrap_or(peer),
        }
    }

    /// The first entry from the right that is not a trusted hop.
    ///
    /// `None` for every case the module documentation lists as a fallback, so the caller decides
    /// what to do with it once rather than this function returning the peer it was not given.
    fn rightmost_untrusted(&self, headers: &HeaderMap) -> Option<IpAddr> {
        for value in headers.get_all(X_FORWARDED_FOR).iter().rev() {
            // A header value that is not visible ASCII is not an address list. Stop rather than
            // skip: a value we cannot read is a hop we cannot account for.
            let text = value.to_str().ok()?;
            for entry in text.rsplit(',') {
                let address = entry.trim().parse::<IpAddr>().ok()?.to_canonical();
                if !self.trusted.trusts(address) {
                    return Some(address);
                }
            }
        }
        None
    }
}

impl KeyExtractor for ClientAddress {
    type Key = IpAddr;

    /// The peer address is required even when the header is what is read, because it is what
    /// decides whether the header may be read - so a request with no connection information is a
    /// request this cannot key, and `tower_governor` turns that into its own error rather than a
    /// silently unlimited path.
    ///
    /// The parameter is `req` and not `request` because that is the name [`KeyExtractor`] declares.
    /// A renamed parameter in an implementation renames it for a reader of this file only, and the
    /// name in the documentation an implementor reads stays the other one.
    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|connect| connect.0.ip())
            .ok_or(GovernorError::UnableToExtractKey)?;
        Ok(self.attribute(req.headers(), peer))
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::Arc;

    use axum::http::{HeaderMap, HeaderValue};
    use sutura_config::{ClientAddressSource, TrustedProxies};

    use super::{ClientAddress, X_FORWARDED_FOR};

    /// The ingress, in the deployment these tests describe.
    const PROXY: &str = "10.0.0.1";
    /// A caller, out on the internet.
    const CALLER: &str = "203.0.113.7";

    fn address(raw: &str) -> IpAddr {
        raw.parse().expect("a test address is an address")
    }

    fn headers(values: &[&str]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for value in values {
            map.append(
                X_FORWARDED_FOR,
                HeaderValue::from_str(value).expect("a test header value is a header value"),
            );
        }
        map
    }

    fn peer_keyed() -> ClientAddress {
        ClientAddress::new(ClientAddressSource::Peer, Arc::new(TrustedProxies::default()))
    }

    fn forwarded_behind(blocks: &[&str]) -> ClientAddress {
        ClientAddress::new(
            ClientAddressSource::Forwarded,
            Arc::new(TrustedProxies::parse(blocks).expect("test blocks parse")),
        )
    }

    #[test]
    fn with_no_proxy_configured_the_peer_is_the_key_and_a_header_is_ignored() {
        // The default posture. A caller who writes the header gets nothing for it, which is why the
        // default is safe with no proxy in front.
        let extractor = peer_keyed();
        assert_eq!(extractor.attribute(&headers(&["1.2.3.4"]), address(CALLER)), address(CALLER));
        assert_eq!(extractor.attribute(&HeaderMap::new(), address(CALLER)), address(CALLER));
    }

    #[test]
    fn behind_a_trusted_proxy_the_header_is_the_key() {
        // The bug this module exists for. Without it, every request through the ingress keys on the
        // ingress and the whole internet shares one bucket.
        let extractor = forwarded_behind(&["10.0.0.0/8"]);
        assert_eq!(
            extractor.attribute(&headers(&[CALLER]), address(PROXY)),
            address(CALLER),
            "the header a trusted hop wrote is the caller"
        );
        // Two callers through the same ingress are two keys, which is the whole point.
        assert_ne!(
            extractor.attribute(&headers(&["203.0.113.8"]), address(PROXY)),
            extractor.attribute(&headers(&[CALLER]), address(PROXY))
        );
    }

    #[test]
    fn a_header_from_an_untrusted_peer_is_ignored() {
        // The other half, and the one that matters: a caller who reaches this service directly and
        // sets the header must not get to choose their bucket.
        let extractor = forwarded_behind(&["10.0.0.0/8"]);
        assert_eq!(
            extractor.attribute(&headers(&["198.51.100.1"]), address(CALLER)),
            address(CALLER),
            "a spoofed header from an untrusted peer chose the bucket"
        );
    }

    #[test]
    fn the_entry_taken_is_the_rightmost_one_no_trusted_hop_wrote() {
        // A caller who prepends their own value gets it skipped: the ingress appended the address it
        // saw, and that is the rightmost entry. Taking the LEFTMOST - which is what every helper in
        // the ecosystem does - would return `1.2.3.4` here and hand the key to the caller.
        let extractor = forwarded_behind(&["10.0.0.0/8"]);
        assert_eq!(
            extractor.attribute(&headers(&[&format!("1.2.3.4, {CALLER}")]), address(PROXY)),
            address(CALLER)
        );
        // And a chain of trusted hops on the right is walked past, not stopped at.
        assert_eq!(
            extractor.attribute(&headers(&[&format!("1.2.3.4, {CALLER}, 10.0.0.9, 10.0.0.8")]), address(PROXY)),
            address(CALLER)
        );
    }

    #[test]
    fn a_second_header_line_the_proxy_appended_wins_over_the_callers_own() {
        // `HeaderMap::get` returns the FIRST value, so a caller who sends their own line before the
        // proxy appends a second one would have theirs read. The walk starts at the last line.
        let extractor = forwarded_behind(&["10.0.0.0/8"]);
        assert_eq!(
            extractor.attribute(&headers(&["1.2.3.4", CALLER]), address(PROXY)),
            address(CALLER)
        );
    }

    #[test]
    fn a_malformed_or_exhausted_header_falls_back_to_the_peer() {
        let extractor = forwarded_behind(&["10.0.0.0/8"]);
        // Not an address: nothing further left can be accounted for, so the peer it is.
        assert_eq!(
            extractor.attribute(&headers(&["1.2.3.4, not-an-address"]), address(PROXY)),
            address(PROXY)
        );
        // Every entry is a trusted hop, so the header names no client at all.
        assert_eq!(
            extractor.attribute(&headers(&["10.0.0.9, 10.0.0.8"]), address(PROXY)),
            address(PROXY)
        );
        // No header at all - a request that reached the ingress and lost it, or a health probe
        // inside the cluster.
        assert_eq!(extractor.attribute(&HeaderMap::new(), address(PROXY)), address(PROXY));
    }

    #[test]
    fn a_v4_mapped_peer_and_entry_key_the_same_bucket_as_the_plain_form() {
        // A dual-stack listener reports every v4 peer as `::ffff:a.b.c.d`. Without canonicalising,
        // the same caller arriving over the two families would be two buckets, and a v4 trusted
        // block would trust nothing at all.
        let extractor = forwarded_behind(&["10.0.0.0/8"]);
        assert_eq!(
            extractor.attribute(&headers(&["::ffff:203.0.113.7"]), address("::ffff:10.0.0.1")),
            address(CALLER)
        );
        assert_eq!(
            peer_keyed().attribute(&HeaderMap::new(), address("::ffff:203.0.113.7")),
            address(CALLER)
        );
    }
}
