//! Which IP family a node declared by a host is dialled over: `server.nodes.address_family`.

use core::fmt;
use core::net::IpAddr;

/// The IP family a host in `server.nodes` is resolved to.
///
/// **A rule over the addresses a name resolves to, and nothing more.** [`Self::Auto`] keeps one
/// family per host, so a dual-stack pod is not two nodes: its IPv4 addresses when it has any, else
/// its IPv6 ones. [`Self::Ipv4`] and [`Self::Ipv6`] keep that family alone, which a name with none
/// of it loses entirely.
///
/// **Limit:** it chooses among the addresses a resolver returns. It does not make a node reachable
/// over a family the network between the nodes does not carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressFamily {
    #[default]
    Auto,
    Ipv4,
    Ipv6,
}

impl AddressFamily {
    /// The spelling the setting takes.
    #[inline]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
        }
    }

    /// Whether `address`, written as a literal, is one this family dials.
    #[inline]
    pub const fn admits(self, address: IpAddr) -> bool {
        match self {
            Self::Auto => true,
            Self::Ipv4 => address.is_ipv4(),
            Self::Ipv6 => address.is_ipv6(),
        }
    }

    /// The addresses of `resolved` this family dials, in the order they came.
    pub fn keep(self, resolved: impl IntoIterator<Item = IpAddr>) -> Vec<IpAddr> {
        let resolved: Vec<IpAddr> = resolved.into_iter().collect();
        let ipv4 = match self {
            Self::Ipv4 => true,
            Self::Ipv6 => false,
            Self::Auto => resolved.iter().any(IpAddr::is_ipv4),
        };
        resolved.into_iter().filter(|address| address.is_ipv4() == ipv4).collect()
    }
}

impl fmt::Display for AddressFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
