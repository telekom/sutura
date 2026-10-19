//! A key set fetched from an `https` URL - an issuer's `jwks_uri`.
//!
//! The second [`KeySetSource`], and nothing above it changed for it: the cache's triggers, rate limit
//! and revocation bound are the file source's. What is its own is the request, and every bound on it
//! is set here or by the shared agent: `https` by the [`KeySetUrl`] parse, no redirect and the proxy
//! pinning by `sutura_http_client`'s agent, [`FETCH_TIMEOUT_SECONDS`], and [`MAX_KEY_SET_BYTES`]
//! enforced by the body reader as it reads.
//!
//! **No cache header is read.** The cache re-reads within [`super::MAX_KEY_SET_AGE`] whatever the
//! issuer says, so an issuer's `max-age` can neither lengthen that bound nor shorten it.

use sutura_config::KeySetUrl;
use sutura_http_client::{InvalidReadBounds, ReadBounds};

use super::source::{KeySetSource, KeySetUnavailable, MAX_KEY_SET_BYTES};

/// How long one fetch may take, from connect to the last byte.
///
/// A look runs on the blocking pool and one at a time, so this bounds how long a hung issuer holds
/// that one pool thread and the refresh behind it.
const FETCH_TIMEOUT_SECONDS: u64 = 10;

/// Why one fetch returned no document.
#[derive(Debug, thiserror::Error)]
pub enum FetchFailed {
    #[error("the request did not complete")]
    Unreachable(#[source] Box<ureq::Error>),
    /// Anything but `200`, a redirect included: the agent follows none, so a `3xx` ends here.
    #[error("the answer was HTTP {status}, not 200")]
    Status { status: u16 },
    #[error("the answer is larger than the {limit} bytes a key set may be")]
    TooLarge { limit: usize },
}

/// The client for a key set URL could not be built.
#[derive(Debug, thiserror::Error)]
pub enum KeySetClientUnusable {
    #[error(transparent)]
    Bounds(#[from] InvalidReadBounds),
    #[error("`security.outbound.transport_anchors` could not be loaded")]
    Anchors(#[source] sutura_tls::LoadError),
}

/// A JWK set at an `https` URL.
#[derive(Debug)]
pub struct UrlKeySet {
    url: KeySetUrl,
    agent: sutura_tls::Rotating<ureq::Agent>,
    /// The whole fetch's budget. Set per request, because the shared agent's own timeout adds a
    /// connect margin on top of the budget it is given.
    timeout: core::time::Duration,
}

impl UrlKeySet {
    /// Names the URL, trusting `anchors` or, with none declared, the compiled-in web roots.
    ///
    /// The anchors are read once, here: a rotated bundle reaches this fetch on the next start.
    pub fn trusting(url: &KeySetUrl, anchors: Option<&sutura_tls::Anchors>) -> Result<Self, KeySetClientUnusable> {
        Self::trusting_within(url, anchors, FETCH_TIMEOUT_SECONDS)
    }

    /// The same, with the fetch budget a cell can shorten.
    pub(crate) fn trusting_within(
        url: &KeySetUrl,
        anchors: Option<&sutura_tls::Anchors>,
        timeout_seconds: u64,
    ) -> Result<Self, KeySetClientUnusable> {
        let bounds = ReadBounds::parse(timeout_seconds, MAX_KEY_SET_BYTES as u64)?;
        let anchors = anchors
            .map(sutura_tls::load_anchors)
            .transpose()
            .map_err(KeySetClientUnusable::Anchors)?;
        Ok(Self {
            url: url.clone(),
            agent: sutura_http_client::fixed(bounds, anchors),
            timeout: bounds.timeout(),
        })
    }

    /// The URL, for a startup log line.
    #[inline]
    #[must_use]
    pub const fn url(&self) -> &KeySetUrl {
        &self.url
    }
}

impl KeySetSource for UrlKeySet {
    /// One `GET`, refusing anything but a `200` with a body of at most [`MAX_KEY_SET_BYTES`].
    ///
    /// The reader's limit is one byte past the cap, because `ureq` refuses the read *after* the
    /// limit is reached: at the cap itself a document exactly that size would be refused.
    fn read(&self) -> Result<String, KeySetUnavailable> {
        let failed = |cause| KeySetUnavailable::Fetch {
            url: self.url.to_string(),
            cause,
        };
        let mut response = self
            .agent
            .current()
            .get(self.url.as_str())
            .config()
            .timeout_global(Some(self.timeout))
            .build()
            .call()
            .map_err(|cause| failed(FetchFailed::Unreachable(Box::new(cause))))?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(failed(FetchFailed::Status { status }));
        }
        response
            .body_mut()
            .with_config()
            .limit(MAX_KEY_SET_BYTES as u64 + 1)
            .read_to_string()
            .map_err(|cause| {
                failed(match cause {
                    ureq::Error::BodyExceedsLimit(_) => FetchFailed::TooLarge {
                        limit: MAX_KEY_SET_BYTES,
                    },
                    other => FetchFailed::Unreachable(Box::new(other)),
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use sutura_http_client::test_support::Scripted;
    use sutura_http_client::tls_test_support::{Scratch, TlsFakeServer, declared_bundle, issue};

    use super::*;

    /// One fetch from a loopback TLS server answering `answer`, trusting that server's certificate.
    ///
    /// The server thread owns its listener, so it keeps answering after its handle is dropped; the
    /// bundle is loaded by `trusting`, so its scratch directory may go too.
    fn fetched(name: &str, answer: Scripted) -> Result<String, KeySetUnavailable> {
        source_answering(name, vec![answer]).read()
    }

    /// A source whose server answers each fetch with the next of `answers`, and then no more.
    fn source_answering(name: &str, answers: Vec<Scripted>) -> UrlKeySet {
        let issued = issue();
        let scratch = Scratch::new(name);
        let anchors = sutura_tls::Anchors::Bundle(declared_bundle(&scratch, name, &issued));
        let server = TlsFakeServer::start(&issued, answers);
        let url = KeySetUrl::parse(format!("{}/keys", server.endpoint())).expect("a loopback https URL parses");
        UrlKeySet::trusting(&url, Some(&anchors)).expect("a written bundle loads")
    }

    #[test]
    fn a_key_set_is_read_from_an_https_url() {
        let document = serde_json::json!({"keys": []});
        let read = fetched("read", Scripted::ok(&document)).expect("a 200 is the document");
        assert_eq!(read, document.to_string());
    }

    #[test]
    fn a_fetch_that_never_answers_gives_up_within_its_budget() {
        // A listener the kernel accepts for and nobody serves: the TLS handshake never completes.
        let silent = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let address = silent.local_addr().expect("a bound listener has an address");
        let url = KeySetUrl::parse(format!("https://{address}/keys")).expect("a loopback https URL parses");
        let started = std::time::Instant::now();
        let refused = UrlKeySet::trusting_within(&url, None, 1)
            .expect("the web roots load")
            .read()
            .expect_err("a silent issuer is no key set");
        let waited = started.elapsed();
        assert!(waited < std::time::Duration::from_secs(5), "the fetch waited {waited:?}");
        assert!(
            matches!(
                refused,
                KeySetUnavailable::Fetch {
                    cause: FetchFailed::Unreachable(_),
                    ..
                }
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn anything_but_a_200_is_refused_a_redirect_included() {
        let published = crate::testing::an_issuer().key_set().into_bytes();
        for (name, sent, answer) in [
            ("redirect", 302, Scripted::status(302, "")),
            ("non-200-2xx", 203, Scripted::raw(203, published)),
        ] {
            let refused = fetched(name, answer).expect_err("only a 200 is a key set");
            assert!(
                matches!(
                    refused,
                    KeySetUnavailable::Fetch {
                        cause: FetchFailed::Status { status },
                        ..
                    } if status == sent
                ),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_key_set_over_the_cap_is_refused() {
        let at_cap = fetched("at-cap", Scripted::raw(200, vec![b' '; MAX_KEY_SET_BYTES])).expect("the cap itself is read");
        assert_eq!(at_cap.len(), MAX_KEY_SET_BYTES);
        let refused = fetched("over-cap", Scripted::raw(200, vec![b' '; MAX_KEY_SET_BYTES + 1])).expect_err("past the cap");
        assert!(
            matches!(
                refused,
                KeySetUnavailable::Fetch {
                    cause: FetchFailed::TooLarge { .. },
                    ..
                }
            ),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn an_unknown_key_id_while_the_url_does_not_answer_is_503_identity_unavailable() {
        // The set is read once at start; the look an unknown key id triggers is answered `500`.
        let issuer = crate::testing::an_issuer()
            .also_holding("a-key-not-yet-read", sutura_dev::issuer::Curve::P256)
            .expect("a mock issuer generates a second key");
        let published = issuer.key_set_without("a-key-not-yet-read").into_bytes();
        let source = source_answering("outage", vec![Scripted::raw(200, published), Scripted::status(500, "")]);
        let gate = crate::inbound::tests::gate_around(Box::new(source));
        let mint = |token| issuer.mint(&token).expect("the mock issuer signs");
        let unknown = mint(sutura_dev::issuer::Token::for_subject("someone@example.com").signed_by("a-key-not-yet-read"));
        let (status, ref body, challenged) = crate::inbound::tests::guarded_answer(&gate, &unknown).await;
        assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE, "{body}");
        assert_eq!(body, crate::inbound::tests::IDENTITY_UNAVAILABLE);
        assert!(!challenged, "an outage carries no challenge to present a different token");
        let known = mint(sutura_dev::issuer::Token::for_subject("someone@example.com"));
        let (held, ref body, _) = crate::inbound::tests::guarded_answer(&gate, &known).await;
        assert_eq!(held, axum::http::StatusCode::NO_CONTENT, "{body}");
    }
}
