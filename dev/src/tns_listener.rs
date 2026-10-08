//! A fake Oracle listener on loopback that answers a driver's CONNECT with a TNS REDIRECT.
//!
//! Here rather than in a test module because two crates dial Oracle - the warehouse adapter through
//! `sutura-cli`, and the RDBMS catalog's Oracle reader - and both prove the same refusal. `cargo
//! xtask check-jscpd` refuses a clone under `crates/`, so the fake is written once.
//!
//! Limit: it speaks only the pre-negotiation framing the driver reads first. It cannot accept a
//! connection, so a driver that follows the redirect reaches [`RedirectingListener::target`] and is
//! closed there, before any authentication.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
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
            let Ok((mut stream, _)) = declared.accept() else { return };
            let _ignored = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let mut connect = [0_u8; 4096];
            let _ignored = stream.read(&mut connect);
            let address = format!("(ADDRESS=(PROTOCOL=TCP)(HOST=127.0.0.1)(PORT={target_port}))");
            let data = format!("{address}\u{0}(DESCRIPTION=(CONNECT_DATA=(SERVICE_NAME=FREEPDB1)))");
            let _ignored = stream.write_all(&redirect(data.as_bytes()));
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
