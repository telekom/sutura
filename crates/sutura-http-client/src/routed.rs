//! The connector every [`crate::agent()`] dials through: `ureq`'s own chain, except that a proxied
//! request to a host NAME is resolved first and, when any address it resolves to is loopback or
//! unspecified, dialled directly to exactly those local addresses - an address of another host in
//! the same answer is never dialled without the proxy.
//!
//! **One resolution.** The addresses that decide the route are the addresses dialled, so nothing
//! between the check and the connect can resolve the name to something else. A lookup that fails
//! leaves the request to the proxy, which may resolve what this host cannot.

use std::net::IpAddr;
use std::sync::Arc;

use ureq::unversioned::resolver::ResolvedSocketAddrs;
use ureq::unversioned::transport::{ConnectionDetails, Connector, DefaultConnector, Transport};

/// Whether `address` reaches this host itself.
pub(crate) const fn local(address: IpAddr) -> bool {
    let address = address.to_canonical();
    address.is_loopback() || address.is_unspecified()
}

/// The addresses of `resolved` that reach this host, in answer order, or `None` when none does.
/// Taken by value so the dial can only be handed this subset, never the whole answer.
fn local_only(mut resolved: ResolvedSocketAddrs) -> Option<ResolvedSocketAddrs> {
    let mut kept = 0;
    for index in 0..resolved.len() {
        if resolved.get(index).is_some_and(|address| local(address.ip())) {
            resolved.swap(kept, index);
            kept += 1;
        }
    }
    resolved.truncate(kept);
    (kept > 0).then_some(resolved)
}

/// `ureq`'s default chain for every connection, and `direct` - a TCP dial wrapped in TLS - for a
/// proxied name that resolved to this host.
#[derive(Debug)]
pub(crate) struct Routed<D> {
    default: DefaultConnector,
    direct: D,
}

impl<D> Routed<D> {
    pub(crate) fn new(direct: D) -> Self {
        Self {
            default: DefaultConnector::default(),
            direct,
        }
    }
}

impl<D: Connector> Connector for Routed<D> {
    type Out = Box<dyn Transport>;

    fn connect(&self, details: &ConnectionDetails, chained: Option<()>) -> Result<Option<Self::Out>, ureq::Error> {
        let proxied = details.config.proxy().is_some_and(|proxy| !proxy.is_no_proxy(details.uri));
        let named = details
            .uri
            .host()
            .is_some_and(|host| !host.starts_with('[') && host.parse::<IpAddr>().is_err());
        if proxied
            && named
            && let Ok(resolved) = details.resolver.resolve(details.uri, details.config, details.timeout)
            && let Some(addrs) = local_only(resolved)
        {
            let pinned = ConnectionDetails {
                uri: details.uri,
                addrs,
                config: details.config,
                request_level: details.request_level,
                resolver: details.resolver,
                now: details.now,
                timeout: details.timeout,
                current_time: Arc::clone(&details.current_time),
                run_connector: Arc::clone(&details.run_connector),
            };
            let dialled = self.direct.connect(&pinned, chained)?;
            return Ok(dialled.map(|transport| -> Box<dyn Transport> { Box::new(transport) }));
        }
        self.default.connect(details, chained)
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::{ResolvedSocketAddrs, local_only};

    fn answer(addresses: &[&str]) -> ResolvedSocketAddrs {
        let mut answer = ResolvedSocketAddrs::from_fn(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
        for address in addresses {
            answer.push(address.parse().expect("a test address parses"));
        }
        answer
    }

    #[test]
    fn only_the_local_addresses_of_a_mixed_answer_are_dialled_directly() {
        let kept = local_only(answer(&["192.0.2.1:443", "127.0.0.1:443", "[2001:db8::1]:443", "[::1]:443"]))
            .expect("an answer with a local address routes directly");
        let kept: Vec<String> = kept.iter().map(ToString::to_string).collect();
        assert_eq!(
            kept,
            ["127.0.0.1:443", "[::1]:443"],
            "a non-local address was kept for a direct dial"
        );
    }

    #[test]
    fn an_answer_with_no_local_address_keeps_the_proxy() {
        assert!(local_only(answer(&["192.0.2.1:443", "[2001:db8::1]:443"])).is_none());
    }
}
