//! The connector every [`crate::agent()`] dials through: `ureq`'s own chain, except that a proxied
//! request to a host NAME is resolved first and, when any address it resolves to is loopback or
//! unspecified, dialled directly to exactly those addresses.
//!
//! **One resolution.** The addresses that decide the route are the addresses dialled, so nothing
//! between the check and the connect can resolve the name to something else. A lookup that fails
//! leaves the request to the proxy, which may resolve what this host cannot.

use std::net::IpAddr;
use std::sync::Arc;

use ureq::unversioned::transport::{ConnectionDetails, Connector, DefaultConnector, Transport};

/// Whether `address` reaches this host itself.
pub(crate) const fn local(address: IpAddr) -> bool {
    let address = address.to_canonical();
    address.is_loopback() || address.is_unspecified()
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
            && let Ok(addrs) = details.resolver.resolve(details.uri, details.config, details.timeout)
            && addrs.iter().any(|address| local(address.ip()))
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
