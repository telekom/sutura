//! A real local HTTP server - "ports get fakes, not mocked HTTP".
//!
//! `pub` behind this crate's `test` feature so a reader's own tests AND a composition root's
//! served-binary suite can build the same fake rather than each crate carrying a second copy.
//!
//! **Moved here (issue #970's review) from `sutura-catalog-datahub::test_support`**, which issue
//! #202's second PR had already made `pub` behind that crate's own `http` feature for the identical
//! reason: `sutura-cli`'s served suite needs a real loopback server to boot a composed deployment
//! against, and an integration test binary cannot see another crate's `tests/` directory at all -
//! Rust exposes no such thing, only the library does. Once `sutura-catalog-openmetadata` needed the
//! SAME fake, the two copies were byte-for-byte identical wire plumbing (`cargo xtask check-jscpd`
//! measured it), which is what moved it one crate further out rather than leaving a second copy.
//!
//! **`#[cfg(feature = "test")]`, not `#[cfg(test)]`.** A downstream crate's OWN test
//! compilation is what needs to see this, and `#[cfg(test)]` on an item never crosses a dependency
//! edge. A catalog crate folds `sutura-http-client/test` into its own `http` feature, so
//! this module compiles into any NON-test `--features http` build too (`sutura-cli --features
//! datahub`, in particular) - dead code there, never called by production composition, but real
//! object code in a shipped binary. **Stated rather than hidden**, the same trade
//! `.agents/skills/sutura/crate-map/SKILL.md`'s "why a networked adapter hides behind a default-off
//! feature" already accepted for `sutura-catalog-datahub::test_support`: `std::net::TcpListener`
//! adds no new DEPENDENCY edge to the four cross builds' `crane.buildDepsOnly` derivation, which is
//! what that argument is about.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test: a fake server shared by both catalog HTTP readers' own tests and \
              sutura-cli's served-binary suite. The module is NOT #[cfg(test)] - integration tests \
              build the library with cfg(test)=false, so it must compile like production code - but \
              its panics and slices are how a test asserts invariants, so the production-code \
              restriction lints it triggers are scoped out here and nowhere else."
)]

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// One scripted answer: a status, a body, and how long to wait before sending it.
pub struct Scripted {
    status: u16,
    body: Vec<u8>,
    delay: Duration,
}

impl Scripted {
    #[must_use]
    pub fn ok(body: &serde_json::Value) -> Self {
        Self {
            status: 200,
            body: body.to_string().into_bytes(),
            delay: Duration::ZERO,
        }
    }

    #[must_use]
    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    #[must_use]
    pub fn delayed(body: &serde_json::Value, delay: Duration) -> Self {
        Self {
            status: 200,
            body: body.to_string().into_bytes(),
            delay,
        }
    }

    /// The status this scripted answer is served with - public so a TLS loopback variant of the
    /// fake ([`crate::tls_test_support`]) can serve the SAME answers this server does, instead of a
    /// second worth of page-building. Named `status_code` rather than `status` because
    /// [`Self::status`] is already the constructor's name.
    #[must_use]
    pub const fn status_code(&self) -> u16 {
        self.status
    }

    /// The body this scripted answer is served with - see [`Self::status_code`] for why it is public.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// A response whose body is opaque bytes, as a size-cap test needs: a body that must be too big
    /// to be legal JSON (the length check runs before decode), so it is not served through the
    /// JSON-typed constructors above.
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
        // **`answers.into_iter()` bounds the loop, not `listener.incoming()`**: looping over
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
