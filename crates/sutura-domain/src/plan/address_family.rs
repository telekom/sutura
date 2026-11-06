//! Which IP family a node declared by a host is dialled over: `server.nodes.address_family`.

use core::fmt;
use core::net::IpAddr;
use core::str::FromStr;

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
#[serde(try_from = "String")]
pub enum AddressFamily {
    #[default]
    Auto,
    Ipv4,
    Ipv6,
}

/// The written value named no family.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` is not an address family - one of: {}", AddressFamily::NAMES.join(", "))]
pub struct UnknownAddressFamily {
    found: String,
}

impl AddressFamily {
    /// Every accepted spelling, so a refusal and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["auto", "ipv4", "ipv6"];

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

impl FromStr for AddressFamily {
    type Err = UnknownAddressFamily;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "auto" => Ok(Self::Auto),
            "ipv4" => Ok(Self::Ipv4),
            "ipv6" => Ok(Self::Ipv6),
            other => Err(UnknownAddressFamily { found: other.to_owned() }),
        }
    }
}

/// Delegates to [`FromStr`], so `serde(try_from)` above and a direct parse are one code path.
impl TryFrom<String> for AddressFamily {
    type Error = UnknownAddressFamily;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        raw.parse()
    }
}
