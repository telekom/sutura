//! The receiving node's address, `https://host[:port]` and nothing else (Decision 6).

/// A node's Flight endpoint.
///
/// **`https` is the only scheme this parses**, so a dial in the clear is not a value this crate can
/// hold: every leg carries a caller's exchanged token, and the configuration refusal for an
/// `http://` endpoint is the operator-facing half of the same rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEndpoint {
    uri: http::Uri,
    host: String,
    port: u16,
}

/// Why a declared node endpoint is not one this adapter dials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidEndpoint {
    #[error("the node endpoint is not a URI")]
    NotAUri,
    #[error("the node endpoint is not `https://`, and every leg carries a caller's token")]
    NotHttps,
    #[error("the node endpoint names no host")]
    NoHost,
    #[error("the node endpoint carries credentials, which belong in no URI")]
    Credentials,
    #[error("the node endpoint carries a path or a query; it is an origin")]
    NotAnOrigin,
}

impl NodeEndpoint {
    /// Parses `scheme://host[:port]`, defaulting the port to 443.
    ///
    /// # Errors
    ///
    /// Not a URI, not `https`, no host, a user-info part, or a path or query beyond `/`.
    pub fn parse(raw: &str) -> Result<Self, InvalidEndpoint> {
        let uri: http::Uri = raw.trim().parse().map_err(|_cause| InvalidEndpoint::NotAUri)?;
        if uri.scheme_str() != Some("https") {
            return Err(InvalidEndpoint::NotHttps);
        }
        let authority = uri.authority().ok_or(InvalidEndpoint::NoHost)?;
        if authority.as_str().contains('@') {
            return Err(InvalidEndpoint::Credentials);
        }
        if !matches!(uri.path(), "" | "/") || uri.query().is_some() {
            return Err(InvalidEndpoint::NotAnOrigin);
        }
        let host = authority.host().trim_start_matches('[').trim_end_matches(']');
        if host.is_empty() {
            return Err(InvalidEndpoint::NoHost);
        }
        let port = authority.port_u16().unwrap_or(443);
        Ok(Self {
            host: String::from(host),
            port,
            uri,
        })
    }

    /// The whole URI, which the gRPC client derives `:authority` from.
    #[must_use]
    pub const fn uri(&self) -> &http::Uri {
        &self.uri
    }

    /// The host, without IPv6 brackets: what the dial connects to and the certificate must name.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
mod tests {
    use super::{InvalidEndpoint, NodeEndpoint};

    #[test]
    fn a_node_endpoint_is_an_https_origin_only() {
        for (raw, refused) in [
            ("http://node-b.example.com", InvalidEndpoint::NotHttps),
            ("grpc://node-b.example.com", InvalidEndpoint::NotHttps),
            ("node-b.example.com", InvalidEndpoint::NotHttps),
            ("https://user:secret@node-b.example.com", InvalidEndpoint::Credentials),
            ("https://node-b.example.com/flight", InvalidEndpoint::NotAnOrigin),
            ("https://node-b.example.com/?q=1", InvalidEndpoint::NotAnOrigin),
            ("not a uri", InvalidEndpoint::NotAUri),
        ] {
            assert_eq!(NodeEndpoint::parse(raw), Err(refused), "{raw}");
        }
        let parsed = NodeEndpoint::parse("https://node-b.example.com:8443").expect("an https origin");
        assert_eq!((parsed.host(), parsed.port()), ("node-b.example.com", 8443));
        let defaulted = NodeEndpoint::parse("https://[::1]").expect("an IPv6 origin");
        assert_eq!((defaulted.host(), defaulted.port()), ("::1", 443));
    }
}
