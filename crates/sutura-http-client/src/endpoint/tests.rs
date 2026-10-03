//! Moved here verbatim from `sutura-catalog-datahub`'s own `src/http/tests.rs` when [`Endpoint`]
//! itself moved (issue #970's review) - `sutura-catalog-openmetadata` carried a byte-for-byte copy
//! of the same cells (different example hostnames only), which is exactly the duplication this
//! crate exists to end. One copy now proves the grammar for both readers.

use super::{Endpoint, InvalidEndpoint};

/// **The reviewer's own probe shape, held as a cell rather than a scratch file.** A non-loopback
/// `http://` endpoint is refused HERE, at construction - a reader cannot be built from a `String`
/// at all, so there is no later point where this endpoint could be dialled with a bearer prepared.
#[test]
fn a_plaintext_endpoint_beyond_loopback_is_refused_by_name() {
    assert_eq!(
        Endpoint::parse("http://catalog.example.internal"),
        Err(InvalidEndpoint::PlaintextBeyondLoopback {
            host: String::from("catalog.example.internal")
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
    // "any host" is the universal property this cell names, so it is witnessed across the
    // host shapes a regression could split on: a dotted name, a non-loopback IPv4 literal and a
    // non-loopback bracketed IPv6 literal, each with and without an explicit port. A guard that
    // applied the plaintext loopback rule to any of them over https would refuse it here.
    for (given, stored) in [
        ("https://catalog.example.internal", "https://catalog.example.internal"),
        (
            "https://catalog.example.internal:8443",
            "https://catalog.example.internal:8443",
        ),
        ("https://10.0.0.5", "https://10.0.0.5"),
        ("https://10.0.0.5:8443", "https://10.0.0.5:8443"),
        ("https://[2001:db8::1]", "https://[2001:db8::1]"),
        ("https://[2001:db8::1]:8443", "https://[2001:db8::1]:8443"),
    ] {
        assert_eq!(
            Endpoint::parse(given).map(|e| e.as_str().to_owned()),
            Ok(String::from(stored)),
            "https endpoint for {given:?} should be accepted and stored as {stored:?}"
        );
    }
}

#[test]
fn a_trailing_slash_is_normalised_away() {
    assert_eq!(
        Endpoint::parse("https://catalog.example/").map(|e| e.as_str().to_owned()),
        Ok(String::from("https://catalog.example"))
    );
}

#[test]
fn a_url_naming_neither_scheme_is_refused() {
    let refused = Endpoint::parse("ftp://catalog.example");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::NotAnHttpUrl { given }) if given.to_string() == "ftp://catalog.example"),
        "{refused:?}"
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
    let refused = Endpoint::parse("http://127.0.0.1:1@evil.example");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::CredentialsInUrl { given }) if given.to_string() == "http://evil.example"),
        "{refused:?}"
    );
}

/// The bracketed-IPv6 spelling of the same bypass shape.
#[test]
fn a_bracketed_ipv6_userinfo_prefix_is_refused_as_credentials_too() {
    let refused = Endpoint::parse("http://[::1]:1@evil.example");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::CredentialsInUrl { given }) if given.to_string() == "http://evil.example"),
        "{refused:?}"
    );
}

/// A query string is refused rather than silently carried into the request path - see
/// [`InvalidEndpoint::PathBeyondRoot`]. This particular shape has no `@` in the AUTHORITY (the
/// `@` is inside the query, after the `?`), so it is `PathBeyondRoot` rather than
/// `CredentialsInUrl` - the two refusals name different reasons for a reason. An `@` outside the
/// parsed authority shows nothing: it may be the rest of a password.
#[test]
fn a_query_string_is_refused_rather_than_silently_kept() {
    let refused = Endpoint::parse("http://127.0.0.1:1?@evil.example");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::PathBeyondRoot { given }) if given.to_string() == "the declared endpoint"),
        "{refused:?}"
    );
}

/// A fragment is refused, not silently dropped by `http::Uri`'s own parse.
#[test]
fn a_fragment_is_refused_rather_than_silently_dropped() {
    let refused = Endpoint::parse("http://127.0.0.1:1#x");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::PathBeyondRoot { given }) if given.to_string() == "http://127.0.0.1:1"),
        "{refused:?}"
    );
    let refused = Endpoint::parse("https://catalog.example#");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::PathBeyondRoot { given }) if given.to_string() == "https://catalog.example"),
        "{refused:?}"
    );
}

/// A missing, zero or out-of-range port is refused at construction, not at the first read.
#[test]
fn a_malformed_or_out_of_range_port_is_refused() {
    for bad in ["http://127.0.0.1:", "http://127.0.0.1:0", "http://127.0.0.1:65536"] {
        let refused = Endpoint::parse(bad);
        assert!(
            matches!(&refused, Err(InvalidEndpoint::NotAnHttpUrl { given }) if given.to_string() == bad),
            "{refused:?}"
        );
    }
}

/// A bare `/path` is refused - the same `PathBeyondRoot` branch as the query cell above.
#[test]
fn a_path_is_refused_rather_than_silently_dropped() {
    let refused = Endpoint::parse("http://127.0.0.1:1/path");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::PathBeyondRoot { given }) if given.to_string() == "http://127.0.0.1:1/path"),
        "{refused:?}"
    );
}

/// Credentials in the URL are refused over `https://` too, not only over plaintext - there is
/// no legitimate use for them in a declared catalog endpoint either way.
#[test]
fn credentials_over_https_are_refused_too() {
    let refused = Endpoint::parse("https://user:pw@catalog.example");
    assert!(
        matches!(&refused, Err(InvalidEndpoint::CredentialsInUrl { given }) if given.to_string() == "https://catalog.example"),
        "{refused:?}"
    );
}

/// Diagnostics show endpoint URLs without userinfo: the credentials refusal names the host it
/// refused and not the `user:secret@` it refused it for.
#[test]
fn a_credentials_refusal_shows_the_host_without_the_userinfo() {
    let refused = Endpoint::parse("https://user:s3cret@catalog.example").expect_err("userinfo is refused");
    let shown = refused.to_string();
    assert!(matches!(refused, InvalidEndpoint::CredentialsInUrl { .. }), "{refused:?}");
    assert!(shown.contains("catalog.example") && !shown.contains("s3cret"), "{shown}");
}

/// The fragment refusal runs on the raw text before anything is parsed - and still shows it parsed.
#[test]
fn a_fragment_refusal_shows_the_host_without_the_userinfo() {
    let refused = Endpoint::parse("https://user:s3cret@catalog.example#f").expect_err("a fragment is refused");
    let shown = refused.to_string();
    assert!(matches!(refused, InvalidEndpoint::PathBeyondRoot { .. }), "{refused:?}");
    assert!(shown.contains("catalog.example") && !shown.contains("s3cret"), "{shown}");
}

/// The scheme refusal is reached before the userinfo check, so it is the one most likely to echo it.
#[test]
fn a_scheme_refusal_shows_the_host_without_the_userinfo() {
    let refused = Endpoint::parse("ftp://user:s3cret@catalog.example").expect_err("ftp is refused");
    let shown = refused.to_string();
    assert!(matches!(refused, InvalidEndpoint::NotAnHttpUrl { .. }), "{refused:?}");
    assert!(shown.contains("catalog.example") && !shown.contains("s3cret"), "{shown}");
}

/// What the dial parser cannot read is not shown at all, rather than shown through a second parser.
#[test]
fn an_endpoint_the_parser_refuses_is_not_shown_at_all() {
    let refused = Endpoint::parse("https://user:s3 cret@catalog.example").expect_err("a space is refused");
    let shown = refused.to_string();
    assert!(matches!(refused, InvalidEndpoint::NotAnHttpUrl { .. }), "{refused:?}");
    assert!(
        shown.starts_with("the declared endpoint ") && !shown.contains("cret"),
        "{shown}"
    );
}

/// An unencoded `/`, `?` or `#` in a password ends the parsed authority early, so the rest of the
/// credential reads as port, path or query: any `@` outside the parsed authority shows nothing.
#[test]
fn an_at_sign_outside_the_parsed_authority_is_not_shown_at_all() {
    for raw in [
        "https://svc:Ab?cd@catalog.example",
        "https://svc:Ab#cd@catalog.example",
        "https://svc:Ab/cd@catalog.example",
        "https://svc/x:Ab@catalog.example",
        "https://svc:1234/x@catalog.example",
    ] {
        let shown = Endpoint::parse(raw).expect_err("each shape is refused").to_string();
        assert!(shown.starts_with("the declared endpoint "), "{raw}: {shown}");
    }
}
