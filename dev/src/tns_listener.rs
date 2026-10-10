//! Fake Oracle listeners on loopback that answer a driver's CONNECT: with a TNS REDIRECT, with
//! bytes a test chooses, or with an ACCEPT and then an authentication response the driver cannot
//! use.
//!
//! Here rather than in a test module because two crates dial Oracle - the warehouse adapter through
//! `sutura-cli`, and the RDBMS catalog's Oracle reader - and both prove the same refusal. `cargo
//! xtask check-jscpd` refuses a clone under `crates/`, so the fake is written once.
//!
//! Limit: [`RedirectingListener`], [`answering`] and [`sending`] speak only the pre-negotiation
//! framing the driver reads first, so a driver that follows the redirect reaches
//! [`RedirectingListener::target`] and is closed there, before any authentication.
//! [`authenticating`] and [`marking`] go one step further and no more: neither completes a login.
//!
//! Each speaks over the accepted TCP stream itself, or, through the `_over` forms, over a session a
//! test opens on it - TLS, for a driver that sends a token only over `tcps`. This crate
//! links no TLS library, so the test brings the session.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// A listener that redirects every client to a second loopback listener, and records whether that
/// second listener was dialled.
pub struct RedirectingListener {
    port: u16,
    target_port: u16,
    dialled: mpsc::Receiver<()>,
}

impl RedirectingListener {
    /// Binds both listeners on `127.0.0.1` and answers ONE client.
    ///
    /// # Errors
    ///
    /// A listener that cannot bind or has no local address.
    pub fn start() -> std::io::Result<Self> {
        Self::start_over(Some)
    }

    /// [`Self::start`], with the declared listener speaking over the session `open` opens on the
    /// accepted connection. The redirect still names a plaintext `TCP` address.
    ///
    /// # Errors
    ///
    /// A listener that cannot bind or has no local address.
    pub fn start_over<S>(open: impl FnOnce(TcpStream) -> Option<S> + Send + 'static) -> std::io::Result<Self>
    where
        S: Read + Write,
    {
        let target = TcpListener::bind("127.0.0.1:0")?;
        let target_port = target.local_addr()?.port();
        let declared = TcpListener::bind("127.0.0.1:0")?;
        let port = declared.local_addr()?.port();
        let (told, dialled) = mpsc::channel();
        drop(std::thread::spawn(move || {
            // Accepted and closed at once: that the driver arrived is the whole measurement.
            if let Ok((stream, _)) = target.accept() {
                drop(stream);
                let _ignored = told.send(());
            }
        }));
        drop(std::thread::spawn(move || {
            let Ok((stream, _)) = declared.accept() else { return };
            let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let Some(mut stream) = open(stream) else { return };
            let mut connect = [0_u8; 4096];
            let _ignored = stream.read(&mut connect);
            let address = format!("(ADDRESS=(PROTOCOL=TCP)(HOST=127.0.0.1)(PORT={target_port}))");
            let data = format!("{address}\u{0}(DESCRIPTION=(CONNECT_DATA=(SERVICE_NAME=FREEPDB1)))");
            let _ignored = stream.write_all(&redirect(data.as_bytes()));
            let _ignored = stream.flush();
        }));
        Ok(Self {
            port,
            target_port,
            dialled,
        })
    }

    /// The port a source declares: the listener that redirects.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The port the redirect names, which no source declares.
    #[must_use]
    pub const fn target(&self) -> u16 {
        self.target_port
    }

    /// Whether a client reached [`Self::target`], waiting up to `wait` for the accept to land.
    ///
    /// Call it after the client returned: a dial it made is then already in the target's backlog.
    #[must_use]
    pub fn target_was_dialled(&self, wait: Duration) -> bool {
        self.dialled.recv_timeout(wait).is_ok()
    }
}

/// Binds a listener on `127.0.0.1` that answers ONE client's CONNECT with a single packet of
/// `packet_type` carrying `body`, then closes, and returns its port.
///
/// # Errors
///
/// A listener that cannot bind or has no local address.
pub fn answering(packet_type: u8, body: Vec<u8>) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut connect = [0_u8; 4096];
        let _ignored = stream.read(&mut connect);
        let _ignored = stream.write_all(&packet(packet_type, &body));
    }));
    Ok(port)
}

/// Binds a listener on `127.0.0.1` that answers ONE client's CONNECT with `bytes` as they are,
/// framed or not, then closes, and returns its port.
///
/// # Errors
///
/// A listener that cannot bind or has no local address.
pub fn sending(bytes: Vec<u8>) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let mut connect = [0_u8; 4096];
        let _ignored = stream.read(&mut connect);
        let _ignored = stream.write_all(&bytes);
    }));
    Ok(port)
}

/// Binds a listener on `127.0.0.1` that accepts ONE client's CONNECT, and returns its port.
///
/// It offers fast authentication, then answers the first authentication message with `session`,
/// the key and value pairs of the session data the driver reads its verifier fields from.
///
/// # Errors
///
/// A listener that cannot bind or has no local address.
pub fn authenticating(session: &[(&str, &str)]) -> std::io::Result<u16> {
    authenticating_over(Some, session).map(|(port, _sent)| port)
}

/// A listener's port, and what the client sent it after the ACCEPT: its first authentication
/// message, and the message after the answer if it sent one before it closed.
pub type Listening = (u16, mpsc::Receiver<Vec<u8>>);

/// [`authenticating`], over the session `open` opens on the accepted connection.
///
/// # Errors
///
/// A listener that cannot bind or has no local address.
pub fn authenticating_over<S>(
    open: impl FnOnce(TcpStream) -> Option<S> + Send + 'static,
    session: &[(&str, &str)],
) -> std::io::Result<Listening>
where
    S: Read + Write,
{
    after_accept(open, negotiated(6, &session_data(session)))
}

/// Binds a listener on `127.0.0.1` that accepts ONE client's CONNECT, and returns its port.
///
/// It offers fast authentication, then answers the first authentication message with a BREAK
/// marker, which makes the driver reset the connection, and then with a marker whose body ends
/// before its type.
///
/// # Errors
///
/// A listener that cannot bind or has no local address.
pub fn marking() -> std::io::Result<u16> {
    let mut answer = negotiated(12, &[1, 0, 1]);
    answer.extend(negotiated(12, &[1]));
    after_accept(Some, answer).map(|(port, _sent)| port)
}

/// Binds a listener on `127.0.0.1` that ACCEPTs ONE client's CONNECT over the session `open`
/// opens, answers the client's next message with `answer`, closes after the message after that,
/// and returns its port and what the client sent after the ACCEPT.
fn after_accept<S>(open: impl FnOnce(TcpStream) -> Option<S> + Send + 'static, answer: Vec<u8>) -> std::io::Result<Listening>
where
    S: Read + Write,
{
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let (told, sent) = mpsc::channel();
    drop(std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else { return };
        let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let Some(mut stream) = open(stream) else { return };
        let mut read = [0_u8; 8192];
        let _ignored = stream.read(&mut read);
        let _ignored = stream.write_all(&packet(2, &accept()));
        let _ignored = stream.flush();
        let mut after = Vec::new();
        let mut keep = |stream: &mut S| {
            let count = stream.read(&mut read).unwrap_or(0);
            after.extend_from_slice(read.get(..count).unwrap_or_default());
        };
        keep(&mut stream);
        let _ignored = stream.write_all(&answer);
        let _ignored = stream.flush();
        // One more read - the client's next message, or its close - so the answer is not cut off by
        // a reset, and a client that waits on the listener after its next message reads a close.
        keep(&mut stream);
        let _ignored = told.send(after);
    }));
    Ok((port, sent))
}

/// An ACCEPT body: protocol version 318, no native network encryption, an 8 KiB SDU, and the flag
/// that offers fast authentication.
fn accept() -> Vec<u8> {
    let mut body = be(318).to_vec();
    body.extend([0; 13]);
    body.extend([0; 9]);
    body.extend(be32(8192));
    body.extend([0; 5]);
    body.extend(be32(0x1000_0000));
    body
}

/// A TTC parameter message with the session data pairs, each with zero flags, then a status
/// message that ends the answer.
fn session_data(pairs: &[(&str, &str)]) -> Vec<u8> {
    let count = u8::try_from(pairs.len()).unwrap_or(u8::MAX);
    let mut ttc = vec![8, 1, count];
    for (key, value) in pairs {
        for text in [key, value] {
            let length = u8::try_from(text.len()).unwrap_or(u8::MAX);
            ttc.extend([1, length, length]);
            ttc.extend(text.as_bytes());
        }
        ttc.push(0);
    }
    ttc.extend([9, 0, 0]);
    ttc
}

/// One packet in the framing after an ACCEPT: a 32-bit length, the type, a zero flags byte, two
/// zero bytes - and, for a DATA packet, two zero data-flag bytes - then the body.
fn negotiated(packet_type: u8, body: &[u8]) -> Vec<u8> {
    let header = if packet_type == 6 { 10 } else { 8 };
    let length = u32::try_from(header + body.len()).unwrap_or(u32::MAX);
    let mut packet = be32(length).to_vec();
    packet.extend([packet_type, 0, 0, 0]);
    if packet_type == 6 {
        packet.extend([0, 0]);
    }
    packet.extend(body);
    packet
}

/// A REDIRECT packet carrying `data`, then the DATA packet the driver reads it from.
fn redirect(data: &[u8]) -> Vec<u8> {
    let length = u16::try_from(data.len()).unwrap_or(u16::MAX);
    let mut reply = packet(5, &be(length));
    reply.extend(packet(6, data));
    reply
}

/// One TNS packet in the pre-negotiation framing: a 16-bit length, two zero bytes, the type, a zero
/// flags byte, two zero bytes - and, for a DATA packet, two zero data-flag bytes - then the body.
fn packet(packet_type: u8, body: &[u8]) -> Vec<u8> {
    let header = if packet_type == 6 { 10 } else { 8 };
    let length = u16::try_from(header + body.len()).unwrap_or(u16::MAX);
    let mut packet = be(length).to_vec();
    packet.extend([0, 0, packet_type, 0, 0, 0]);
    if packet_type == 6 {
        packet.extend([0, 0]);
    }
    packet.extend(body);
    packet
}

#[expect(clippy::big_endian_bytes, reason = "a TNS header is in network byte order")]
const fn be(value: u16) -> [u8; 2] {
    value.to_be_bytes()
}

#[expect(clippy::big_endian_bytes, reason = "a TNS header is in network byte order")]
const fn be32(value: u32) -> [u8; 4] {
    value.to_be_bytes()
}
