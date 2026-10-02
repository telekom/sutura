//! A fake Postgres server that completes only the handshake and one boot `SET` statement, so
//! refused-before-SQL cells can provoke a refusal against a connection that at least exists.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Reads one length-prefixed message body; `tagged` says whether a type byte precedes it.
#[expect(clippy::big_endian_bytes, reason = "the Postgres wire protocol frames lengths big-endian")]
fn read_message(stream: &mut TcpStream, tagged: bool) {
    if tagged {
        let mut tag = [0_u8; 1];
        stream.read_exact(&mut tag).expect("the driver sends a message type");
    }
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length).expect("the driver sends a message length");
    let body = usize::try_from(u32::from_be_bytes(length)).expect("a u32 fits a usize") - 4;
    let mut discarded = vec![0_u8; body];
    stream.read_exact(&mut discarded).expect("the driver sends the message body");
}

/// A loopback listener that completes one connection's handshake - `AuthenticationOk`,
/// `ReadyForQuery`, then `CommandComplete` for the boot `SET` - and closes it.
pub(crate) fn fake_postgres() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener binds");
    let port = listener.local_addr().expect("a bound listener has an address").port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the driver dials this listener");
        read_message(&mut stream, false);
        stream
            .write_all(b"R\x00\x00\x00\x08\x00\x00\x00\x00Z\x00\x00\x00\x05I")
            .expect("the handshake is answered");
        read_message(&mut stream, true);
        stream
            .write_all(b"C\x00\x00\x00\x08SET\x00Z\x00\x00\x00\x05I")
            .expect("the boot statement is answered");
    });
    port
}

/// A connection config for the fake on `port`, failing fast rather than waiting on a dead one.
pub(crate) fn config(port: u16) -> tokio_postgres::Config {
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(port)
        .dbname("sutura")
        .user("sutura")
        .connect_timeout(Duration::from_secs(5));
    config
}
