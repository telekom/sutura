//! A validated catalog HTTP endpoint - loopback plaintext or any-host TLS, never anything else.
//!
//! This has been wrong twice on the way to this shape, and both corrections are worth keeping
//! visible (moved here verbatim from `sutura-catalog-datahub`'s own module header, which measured
//! them). The first draft dialled a bearer in clear text to any host a deployment typed, refused
//! only by a connection timeout. The fix was a hand-rolled parse that split the authority by hand -
//! and for `http://[::1]:1@localhost:<port>` took the text before the LAST `:`, so the endpoint
//! read as loopback while the REAL host (`localhost`, after the userinfo's `@`) is exactly the name
//! this module is supposed to refuse in plaintext. Parsing with [`Uri`] - the SAME parser `ureq`
//! itself dials with - closes this the way it should have been closed the first time:
//! `Authority::host` already resolves past userinfo correctly, and [`Endpoint::parse`]
//! additionally refuses any `user[:pass]@` prefix outright rather than trusting that resolution to
//! stay correct forever.

use ureq::http::Uri;

/// Why a declared endpoint is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidEndpoint {
    /// Not a parseable URL, or a parseable URL naming neither `http` nor `https`, or one naming no
    /// authority at all.
    #[error("{given} is not an http:// or https:// URL")]
    NotAnHttpUrl { given: String },
    /// The authority carries `user[:pass]@` - refused outright. **This is not merely defence in
    /// depth against a spoofed host**: the round-2 review measured a reader built from
    /// `http://[::1]:1@localhost:<port>` dialling `localhost` in clear text with the bearer
    /// prepared, because a hand-rolled host extraction split on the wrong delimiter. Parsing with
    /// [`Uri`] closes that specific bypass on its own - `Authority::host` resolves to the text
    /// AFTER the last `@`, which is `localhost` here, so the loopback check below already sees the
    /// real target - but a declared endpoint has no legitimate use for embedded credentials, so
    /// this refuses the shape by name rather than relying on that resolution being correct forever.
    #[error("{given} carries credentials in the URL (a user[:pass]@ prefix), which is refused")]
    CredentialsInUrl { given: String },
    /// A path, a query or a fragment beyond the bare root - a reverse-proxy path prefix is a real
    /// shape, not yet supported, a stated limit. The fragment is checked on RAW text in
    /// [`Endpoint::parse`]: `http::Uri` silently discards a `#`.
    #[error("{given} carries a path, query or fragment beyond the root, which this reader does not support")]
    PathBeyondRoot { given: String },
    /// `http://` to a host that is not an IP loopback literal - see
    /// [`sutura_domain::source::host_is_loopback`].
    #[error(
        "http:// is refused for {host} - only an IP loopback literal (127.0.0.1, ::1) may carry a \
         bearer in clear text; write https:// or a loopback address"
    )]
    PlaintextBeyondLoopback { host: String },
}

/// A validated catalog HTTP endpoint, obtainable only through [`Self::parse`].
///
/// What it accepts, exactly: `scheme://host[:port]`, scheme `http` or `https` (case-folded), on a
/// [`Uri`] (`ureq`'s own re-export of the `http` crate's parser, the SAME type `ureq` itself parses
/// a request URL into before dialling), an OPTIONAL nonzero valid `:port`, an OPTIONAL trailing
/// `/`, and NOTHING else. `https://` is accepted for any host; `http://` only for an IP loopback
/// LITERAL - a hostname is not an address, so `localhost` does not count either, and only something
/// that parses as `IpAddr` and answers `is_loopback()` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint(String);

impl Endpoint {
    /// Parses and validates against [`Uri`] - the SAME parser `ureq` itself dials with, rather
    /// than a hand-rolled split, which is what let the round-2 review's userinfo form
    /// (`http://[::1]:1@localhost`) reach `host_is_loopback` with the wrong string. The stored
    /// form is rebuilt from the parsed `scheme`/`authority`, so any root spelling normalises alike.
    pub fn parse(raw: &str) -> Result<Self, InvalidEndpoint> {
        let not_an_http_url = || InvalidEndpoint::NotAnHttpUrl { given: raw.to_owned() };
        // `http::Uri` silently discards a fragment at parse, so a `#` is refused HERE, on the raw text.
        if raw.contains('#') {
            return Err(InvalidEndpoint::PathBeyondRoot { given: raw.to_owned() });
        }
        let uri: Uri = raw
            .trim()
            .parse()
            .map_err(|_cause: ureq::http::uri::InvalidUri| not_an_http_url())?;
        let scheme = uri.scheme_str().unwrap_or_default();
        if scheme != "http" && scheme != "https" {
            return Err(not_an_http_url());
        }
        let authority = uri.authority().ok_or_else(not_an_http_url)?;
        if authority.as_str().contains('@') {
            return Err(InvalidEndpoint::CredentialsInUrl { given: raw.to_owned() });
        }
        // A declared port must be a valid nonzero `u16`; bytes (not `&str`) so a bracketed IPv6
        // host's own colons are never mistaken for the port's.
        let after_host = authority
            .as_str()
            .as_bytes()
            .get(authority.host().len()..)
            .unwrap_or_default();
        if after_host.starts_with(b":") && !matches!(authority.port_u16(), Some(port) if port > 0) {
            return Err(not_an_http_url());
        }
        let root_only = uri
            .path_and_query()
            .is_none_or(|path_and_query| matches!(path_and_query.as_str(), "" | "/"));
        if !root_only {
            return Err(InvalidEndpoint::PathBeyondRoot { given: raw.to_owned() });
        }
        if scheme == "http" {
            // `Authority::host` already resolves past any userinfo (to the text after the LAST
            // `@`), and keeps IPv6 brackets - stripped here because `host_is_loopback` parses an
            // `IpAddr`, which does not accept them.
            let host = authority.host().trim_start_matches('[').trim_end_matches(']');
            if !sutura_domain::source::host_is_loopback(host) {
                return Err(InvalidEndpoint::PlaintextBeyondLoopback { host: host.to_owned() });
            }
        }
        Ok(Self(format!("{scheme}://{}", authority.as_str())))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests;
