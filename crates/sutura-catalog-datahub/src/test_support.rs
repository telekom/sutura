//! A real local HTTP server - "ports get fakes, not mocked HTTP" - and the happy-path DataHub wire
//! pages this crate's own tests need, made `pub` so a DIFFERENT crate's integration test can build
//! the same fake rather than a second one.
//!
//! **Moved out of `tests/http_reader.rs` by issue #202's second PR, not written fresh.** That file's
//! own `FakeServer`/`Scripted`/page builders were `mod tests`-private, which is exactly right for a
//! `#[cfg(test)]`-only fake used by one crate - until a SECOND crate needed one too:
//! `sutura-cli`'s own served-binary suite (`crates/sutura-cli/tests/served/datahub.rs`) wants a
//! real loopback DataHub server to boot a composed `catalog.kind: datahub` deployment against, and
//! an integration test binary cannot see another crate's `tests/` directory at all - Rust does not
//! expose one. The only way to share this fake is through the LIBRARY, which is what this module is.
//!
//! **`#[cfg(feature = "http")]`, not `#[cfg(test)]`.** A downstream crate's OWN test compilation is
//! what needs to see this, and `#[cfg(test)]` on an item is private to the crate that sets it - it
//! never crosses the dependency edge the way a feature does. That means this module compiles into
//! any NON-test build with `--features http` too (`sutura-cli --features datahub`, in particular)
//! - dead code there, never called by production composition, but real object code in a shipped
//! binary. **Stated rather than hidden:** `.agents/skills/sutura/crate-map/SKILL.md`'s "why a
//! networked adapter hides behind a default-off feature" argument is about the DEPENDENCY EDGE the
//! four cross builds' `crane.buildDepsOnly` derivation carries - `std::net::TcpListener` adds none,
//! so this module does not reopen that measurement. A dedicated `test-support`-only feature is the
//! natural follow-up if the object-code cost itself becomes the concern instead of the edge.

#![expect(
    clippy::doc_lazy_continuation,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_long_first_doc_paragraph,
    reason = "test-support: a fake server and scripted wire pages shared by this crate's own tests \
              and sutura-cli's served-binary suite. The module is NOT #[cfg(test)] - integration \
              tests build the library with cfg(test)=false, so it must compile like production code - \
              but its panics, expects and slices are how a test asserts invariants, so the \
              production-code restriction lints it triggers are scoped out here and nowhere else."
)]

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::AspectReader as _;

/// One scripted answer: a status, a body, and how long to wait before sending it.
pub struct Scripted {
    status: u16,
    body: Vec<u8>,
    delay: Duration,
}

impl Scripted {
    pub fn ok(body: &serde_json::Value) -> Self {
        Self {
            status: 200,
            body: body.to_string().into_bytes(),
            delay: Duration::ZERO,
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    pub fn delayed(body: &serde_json::Value, delay: Duration) -> Self {
        Self {
            status: 200,
            body: body.to_string().into_bytes(),
            delay,
        }
    }

    /// The status this scripted answer is served with - public so a TLS loopback variant of the
    /// fake (`tests/http_reader.rs::tls_anchors`, a served-binary boot-line cell) can serve the SAME
    /// answers this server does, instead of a second worth of page-building. Named `status_code`
    /// rather than `status` because [`Self::status`] is already the constructor's name.
    #[must_use]
    pub const fn status_code(&self) -> u16 {
        self.status
    }

    /// The body this scripted answer is served with - see [`Self::status_code`] for why it is public.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// A response whose body is opaque bytes, as a size-cap test needs: a body that must be too
    /// big to be legal JSON (the length check runs before decode), so it is not served through
    /// the JSON-typed constructors above.
    #[must_use]
    pub const fn raw(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            body,
            delay: Duration::ZERO,
        }
    }
}

/// Every request this fake server has answered, in order: `authorization` header or `None`.
pub type CapturedAuthorizations = Vec<Option<String>>;

/// A real local HTTP/1.1 server answering one [`Scripted`] response per connection, in order, then
/// closing. Captures each request's `authorization` header so a test can assert the bearer was sent.
pub struct FakeServer {
    addr: SocketAddr,
    handle: Option<thread::JoinHandle<CapturedAuthorizations>>,
}

impl FakeServer {
    pub fn start(answers: Vec<Scripted>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let addr = listener.local_addr().expect("a bound listener has a local address");
        // **`answers.into_iter()` bounds the loop, not `listener.incoming()`** - see the crate's
        // `tests/http_reader.rs` history for the measurement this shape came from: looping over
        // `answers` and calling `accept()` exactly that many times is what lets this thread exit
        // once the last scripted answer has been served, with no further accept ever attempted.
        let handle = thread::spawn(move || {
            let mut authorizations = Vec::new();
            for answer in answers {
                let Ok((mut stream, _)) = listener.accept() else { break };
                authorizations.push(read_authorization(&mut stream));
                if !answer.delay.is_zero() {
                    thread::sleep(answer.delay);
                }
                write_response(&mut stream, answer.status, &answer.body);
            }
            authorizations
        });
        Self {
            addr,
            handle: Some(handle),
        }
    }

    #[must_use]
    pub fn endpoint(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The bound loopback address, for a case that builds its own (malformed) endpoint string
    /// around it rather than using [`Self::endpoint`] as-is.
    #[must_use]
    pub const fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Joins the server thread and returns every request's `authorization` header, in order.
    ///
    /// Only called by a test that knows exactly how many connections it will make - a test that
    /// deliberately stops short drops the server instead, and the abandoned thread exits with the
    /// process.
    pub fn finish(mut self) -> CapturedAuthorizations {
        self.handle
            .take()
            .expect("a server is finished at most once")
            .join()
            .expect("the fake server thread did not panic")
    }
}

/// Reads one HTTP request up to its blank line and returns its `authorization` header, if any. This
/// reader is never asked to read a GET body, so it does not look for one.
fn read_authorization(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
        if buf.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8_lossy(&buf)
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
        .map(|line| line.split_once(':').map_or("", |(_, value)| value).trim().to_owned())
}

fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    drop(stream.write_all(head.as_bytes()));
    drop(stream.write_all(body));
    drop(stream.flush());
}

/// The structured property name the happy-path pages below register the certified metric's content
/// under - a fixed test constant, deliberately independent of the deployment's OWN choice, the same
/// way `tests/provisioned.rs`'s `DEPLOYMENT_PROPERTY` is: a fake registering the adapter's own field
/// name would pass equally whether the name were the deployment's choice or a constant this crate
/// requires.
pub const DEPLOYMENT_PROPERTY: &str = "deployment_metric_document";

/// One `dataset` page, over the two models the certified fixture metric needs: `orders` (carrying
/// every column the metric's measure, time column and required filter name) and `customers`
/// (carrying the dimension's column). Model name and table are the same string, because
/// `HttpAspectReader::read_datasets`'s own doc names that as a real limit rather than hiding it.
#[must_use]
pub fn dataset_page() -> serde_json::Value {
    serde_json::json!({
        "entities": [
            {
                "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                "schemaMetadata": { "value": { "fields": [
                    {"fieldPath": "order_id"},
                    {"fieldPath": "customer_id"},
                    {"fieldPath": "amount_cents"},
                    {"fieldPath": "order_date"},
                    {"fieldPath": "status"},
                ] } },
                "datasetProperties": { "value": { "description": "Net revenue orders, in minor units." } },
            },
            {
                "urn": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                "schemaMetadata": { "value": { "fields": [
                    {"fieldPath": "customer_id"},
                    {"fieldPath": "segment"},
                ] } },
                "datasetProperties": { "value": { "description": "The customer dimension." } },
            },
        ]
    })
}

/// One `semanticModel` page, over the one relationship the certified fixture metric's dimension
/// reaches `customers` through. Served in the shape a REAL `DataHub` carries - the relationship
/// nested inside the `semanticModel` entity's own `semanticModelInfo.value.relationships[]`, not as
/// a top-level aspect (GMS drops that on write; measured against the docker tier, 2026-09-16) - so
/// the fake and the live tier serve one content over two transports, and
/// `HttpAspectReader::read_relationships` walks both identically.
#[must_use]
pub fn relationship_page() -> serde_json::Value {
    serde_json::json!({
        "entities": [
            {
                "urn": "urn:li:semanticModel:(urn:li:dataPlatform:bigquery,PROD,orders_to_customer)",
                "semanticModelInfo": { "value": {
                    "name": "orders_to_customer",
                    "relationships": [
                        {
                            "name": "orders_to_customer",
                            "from": "urn:li:dataset:(urn:li:dataPlatform:bigquery,orders,PROD)",
                            "fromColumns": ["customer_id"],
                            "to": "urn:li:dataset:(urn:li:dataPlatform:bigquery,customers,PROD)",
                            "toColumns": ["customer_id"],
                            "cardinality": "N_ONE",
                        },
                    ],
                } },
            },
        ]
    })
}

/// One `metric` page carrying the recorded fixture's OWN certified metric, read through the crate's
/// public port rather than restated here - the recorded corpus and this page cannot drift.
#[must_use]
pub fn metric_page() -> serde_json::Value {
    let recorded = crate::fixture::FixtureReader.read().expect("the recorded fixture reads");
    let certified = recorded
        .metrics()
        .iter()
        .find(|metric| metric.sutura().is_some())
        .expect("the fixture carries one certified metric");
    let property = certified.sutura().expect("the metric just found carries the property");
    serde_json::json!({
        "entities": [
            {
                "urn": "urn:li:metric:(urn:li:dataPlatform:bigquery,orders,revenue)",
                "metricInfo": { "value": {
                    "name": certified.name(),
                    "expression": { "dialects": [{ "dialect": certified.dialect(), "expression": certified.expression() }] },
                } },
                "structuredProperties": { "value": { "properties": [
                    { "propertyUrn": format!("urn:li:structuredProperty:{DEPLOYMENT_PROPERTY}"), "values": [{ "string": property.string_value() }] },
                ] } },
            },
        ]
    })
}

/// The three pages a `read()` call makes, in order, all answering `200` - what a real `DataHub`
/// carrying exactly the recorded fixture's content would serve.
#[must_use]
pub fn happy_path_answers() -> Vec<Scripted> {
    vec![
        Scripted::ok(&dataset_page()),
        Scripted::ok(&relationship_page()),
        Scripted::ok(&metric_page()),
    ]
}
