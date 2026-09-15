use super::{Endpoint, InvalidEndpoint};

/// **The reviewer's own probe shape, held as a cell rather than a scratch file.** A non-loopback
/// `http://` endpoint is refused HERE, at construction - `HttpAspectReader::new` cannot be
/// called with a `String` at all, so there is no later point where this endpoint could be
/// dialled with the bearer prepared.
#[test]
fn a_plaintext_endpoint_beyond_loopback_is_refused_by_name() {
    assert_eq!(
        Endpoint::parse("http://datahub.example.internal"),
        Err(InvalidEndpoint::PlaintextBeyondLoopback {
            host: String::from("datahub.example.internal")
        })
    );
}

/// A hostname that HAPPENS to be `localhost` is still refused - `host_is_loopback` parses an
/// `IpAddr` literal or nothing, the same rule `sutura_config::sources::transport` holds.
#[test]
fn localhost_by_name_is_not_a_loopback_literal() {
    assert_eq!(
        Endpoint::parse("http://localhost:8080"),
        Err(InvalidEndpoint::PlaintextBeyondLoopback {
            host: String::from("localhost")
        })
    );
}

#[test]
fn a_plaintext_endpoint_to_an_ip_loopback_literal_is_accepted() {
    assert_eq!(
        Endpoint::parse("http://127.0.0.1:1").map(|e| e.as_str().to_owned()),
        Ok(String::from("http://127.0.0.1:1"))
    );
    assert_eq!(
        Endpoint::parse("http://[::1]:9002").map(|e| e.as_str().to_owned()),
        Ok(String::from("http://[::1]:9002"))
    );
}

#[test]
fn an_https_endpoint_is_accepted_for_any_host() {
    assert_eq!(
        Endpoint::parse("https://datahub.example.internal").map(|e| e.as_str().to_owned()),
        Ok(String::from("https://datahub.example.internal"))
    );
}

#[test]
fn a_trailing_slash_is_normalised_away() {
    assert_eq!(
        Endpoint::parse("https://datahub.example/").map(|e| e.as_str().to_owned()),
        Ok(String::from("https://datahub.example"))
    );
}

#[test]
fn a_url_naming_neither_scheme_is_refused() {
    assert_eq!(
        Endpoint::parse("ftp://datahub.example"),
        Err(InvalidEndpoint::NotAnHttpUrl {
            given: String::from("ftp://datahub.example")
        })
    );
}

/// **The round-2 review's exact bypass shape, held as a cell.** A hand-rolled host extraction
/// took the text before the LAST `:` in the authority, which for `127.0.0.1:1@evil.example`
/// gave `127.0.0.1` (loopback, wrongly accepted) instead of the real target, `evil.example`
/// (everything after the userinfo's `@`). [`Uri`]'s own `Authority::host` resolves this
/// correctly, and this crate additionally refuses the `user[:pass]@` shape outright rather than
/// trusting that resolution alone.
#[test]
fn a_userinfo_prefix_naming_a_loopback_ip_is_refused_as_credentials_not_silently_accepted() {
    assert_eq!(
        Endpoint::parse("http://127.0.0.1:1@evil.example"),
        Err(InvalidEndpoint::CredentialsInUrl {
            given: String::from("http://127.0.0.1:1@evil.example")
        })
    );
}

/// The bracketed-IPv6 spelling of the same bypass shape.
#[test]
fn a_bracketed_ipv6_userinfo_prefix_is_refused_as_credentials_too() {
    assert_eq!(
        Endpoint::parse("http://[::1]:1@evil.example"),
        Err(InvalidEndpoint::CredentialsInUrl {
            given: String::from("http://[::1]:1@evil.example")
        })
    );
}

/// A query string is refused rather than silently carried into the request path - see
/// [`InvalidEndpoint::PathBeyondRoot`]. This particular shape has no `@` in the AUTHORITY (the
/// `@` is inside the query, after the `?`), so it is `PathBeyondRoot` rather than
/// `CredentialsInUrl` - the two refusals name different reasons for a reason.
#[test]
fn a_query_string_is_refused_rather_than_silently_kept() {
    assert_eq!(
        Endpoint::parse("http://127.0.0.1:1?@evil.example"),
        Err(InvalidEndpoint::PathBeyondRoot {
            given: String::from("http://127.0.0.1:1?@evil.example")
        })
    );
}

/// A fragment is refused, not silently dropped by `http::Uri`'s own parse.
#[test]
fn a_fragment_is_refused_rather_than_silently_dropped() {
    assert_eq!(
        Endpoint::parse("http://127.0.0.1:1#x"),
        Err(InvalidEndpoint::PathBeyondRoot {
            given: String::from("http://127.0.0.1:1#x")
        })
    );
    assert_eq!(
        Endpoint::parse("https://datahub.example#"),
        Err(InvalidEndpoint::PathBeyondRoot {
            given: String::from("https://datahub.example#")
        })
    );
}

/// A missing, zero or out-of-range port is refused at construction, not at the first read.
#[test]
fn a_malformed_or_out_of_range_port_is_refused() {
    for bad in ["http://127.0.0.1:", "http://127.0.0.1:0", "http://127.0.0.1:65536"] {
        assert_eq!(
            Endpoint::parse(bad),
            Err(InvalidEndpoint::NotAnHttpUrl {
                given: String::from(bad)
            })
        );
    }
}

/// A bare `/path` is refused - the same `PathBeyondRoot` branch as the query cell above.
#[test]
fn a_path_is_refused_rather_than_silently_dropped() {
    assert_eq!(
        Endpoint::parse("http://127.0.0.1:1/path"),
        Err(InvalidEndpoint::PathBeyondRoot {
            given: String::from("http://127.0.0.1:1/path")
        })
    );
}

/// Credentials in the URL are refused over `https://` too, not only over plaintext - there is
/// no legitimate use for them in a declared `DataHub` endpoint either way.
#[test]
fn credentials_over_https_are_refused_too() {
    assert_eq!(
        Endpoint::parse("https://user:pw@datahub.example"),
        Err(InvalidEndpoint::CredentialsInUrl {
            given: String::from("https://user:pw@datahub.example")
        })
    );
}
