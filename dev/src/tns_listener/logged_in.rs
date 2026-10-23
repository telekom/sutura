//! A fake listener that completes a password login.
//!
//! So a test can read what the driver sends on a session it believes is open.
//!
//! Limit: the login is the driver's half only. The fake checks no password and sends what the driver
//! needs to finish: a protocol message, the verifier fields of the first phase and the session fields
//! of the second. It answers nothing after the login: the first message after it is recorded and the
//! connection is closed, so every statement fails at the client.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use super::{accept, field, negotiated, packet, session_data};

/// What one connection sent, one entry per packet.
///
/// The CONNECT, the fast authentication message, the second login phase, and the first message after
/// the login.
pub type Sent = Vec<Vec<u8>>;

/// A listener's port, and what each connection sent, in the order the connections ended.
pub type Serving = (u16, mpsc::Receiver<Sent>);

/// Binds a listener on `127.0.0.1` that logs in every client the same way.
///
/// Each client is served over the session `open` opens on its accepted connection; this returns the
/// port and what each connection sent.
///
/// The protocol message it answers with offers end-user security contexts when
/// `end_user_contexts` is set - the capability bit a driver reads before it attaches one.
///
/// # Errors
///
/// A listener that cannot bind or has no local address.
pub fn logging_in<S, O>(open: O, end_user_contexts: bool) -> std::io::Result<Serving>
where
    S: Read + Write,
    O: Fn(TcpStream) -> Option<S> + Clone + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let (told, sent) = mpsc::channel();
    drop(std::thread::spawn(move || {
        for accepted in listener.incoming() {
            let Ok(stream) = accepted else { return };
            let (open, told) = (open.clone(), told.clone());
            drop(std::thread::spawn(move || {
                let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let Some(mut session) = open(stream) else { return };
                let _ignored = told.send(serve(&mut session, end_user_contexts));
            }));
        }
    }));
    Ok((port, sent))
}

/// One connection: the CONNECT, the login, and the one message after it.
fn serve<S>(session: &mut S, end_user_contexts: bool) -> Sent
where
    S: Read + Write,
{
    let mut sent = Vec::new();
    let mut read = [0_u8; 8192];
    let count = session.read(&mut read).unwrap_or(0);
    let mut connect = read.get(..count).unwrap_or_default().to_vec();
    // A CONNECT whose connect data overflows it sends that data as the next packet.
    if read.get(4) == Some(&1) && field(&read, 24) + field(&read, 26) > field(&read, 0) && count <= field(&read, 0) {
        let more = session.read(&mut read).unwrap_or(0);
        connect.extend_from_slice(read.get(..more).unwrap_or_default());
    }
    sent.push(connect);
    let phase_one = [
        ("AUTH_PBKDF2_VGEN_COUNT", "1"),
        ("AUTH_VFR_DATA", "00"),
        ("AUTH_SESSKEY", SESSION_KEY),
        ("AUTH_PBKDF2_SDER_COUNT", "1"),
        ("AUTH_PBKDF2_CSK_SALT", "00"),
    ];
    let phase_two = [
        ("AUTH_SESSION_ID", "1"),
        ("AUTH_SERIAL_NUM", "1"),
        ("AUTH_VERSION_NO", VERSION),
    ];
    let mut fast_auth = protocol(end_user_contexts);
    fast_auth.extend(session_data(&phase_one));
    for answer in [
        packet(2, &accept()),
        negotiated(6, &fast_auth),
        negotiated(6, &session_data(&phase_two)),
    ] {
        if session.write_all(&answer).and_then(|()| session.flush()).is_err() {
            return sent;
        }
        match next_packet(session) {
            Some(packet) => sent.push(packet),
            None => return sent,
        }
    }
    sent
}

/// A session key of two AES blocks, hex - the only thing the driver checks of it.
const SESSION_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Release 23.26, in the encoding a field version of 18.1 or later reads.
const VERSION: &str = "387579904";

/// One packet in the framing after an ACCEPT: its 32-bit length, then the rest of it.
fn next_packet<S>(session: &mut S) -> Option<Vec<u8>>
where
    S: Read,
{
    let mut header = [0_u8; 8];
    session.read_exact(&mut header).ok()?;
    let length = usize::try_from(length_of(header)).ok()?;
    let mut packet = vec![0_u8; length.checked_sub(header.len())?];
    session.read_exact(&mut packet).ok()?;
    let mut whole = header.to_vec();
    whole.extend(packet);
    Some(whole)
}

/// The length field of a packet header in the framing after an ACCEPT.
#[expect(clippy::big_endian_bytes, reason = "a TNS header is in network byte order")]
const fn length_of(header: [u8; 8]) -> u32 {
    let [a, b, c, d, ..] = header;
    u32::from_be_bytes([a, b, c, d])
}

/// The protocol message: its version, a banner, `AL32UTF8` and `AL16UTF16`, and compile
/// capabilities that lower no field version - with the end-user security context bit at index 45
/// when `end_user_contexts` is set.
fn protocol(end_user_contexts: bool) -> Vec<u8> {
    let mut message = vec![1, 6, 0];
    message.extend(b"fake");
    message.push(0);
    message.extend(AL32UTF8);
    // The server flags, no character set graph, and an FDO of 11 bytes that ends in `AL16UTF16`.
    message.extend([0, 0, 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x07, 0xD0]);
    let mut compile = [0_u8; 56];
    if let Some(version) = compile.get_mut(7) {
        *version = u8::MAX;
    }
    if let Some(backport) = compile.get_mut(45)
        && end_user_contexts
    {
        *backport = 0x02;
    }
    message.push(56);
    message.extend(compile);
    // No runtime capabilities.
    message.push(0);
    message
}

/// Character set 873, `AL32UTF8`, as the protocol message carries it: least significant byte first.
const AL32UTF8: [u8; 2] = [0x69, 0x03];
