use super::*;
use std::net::TcpListener;

fn pair(size: usize) -> (TcpStream, Transport) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    let mut transport = Transport::new();
    transport.low_level_transport = Some(LowLevelTransport::Tcp(client));
    transport.native_requested = true;
    transport.set_max_packet_size(size);
    transport.set_full_packet_size();
    transport.set_timeout(Some(Duration::from_secs(2))).unwrap();
    (server, transport)
}

fn packet(payload: &[u8]) -> Vec<u8> {
    let mut output = ((payload.len() + 10) as u32).to_be_bytes().to_vec();
    output.extend_from_slice(&[6, 0, 0, 0, 0, 0]);
    output.extend_from_slice(payload);
    output
}

#[test]
fn corrupt_record_poisoning_prevents_any_later_plaintext_write() {
    let (mut server, mut transport) = pair(256);
    transport.protection = Some(nne::protection_for_test());
    server.write_all(&packet(&[0; 50])).unwrap();
    let error = transport.receive_packet().err().unwrap();
    assert_eq!(error.to_string(), "native encryption refused: invalid protected record");
    assert!(transport.low_level_transport.is_none());
    assert!(transport.protection.is_none());
    assert!(transport.send_packets(6, 0, 0, b"must never be sent").is_err());
    server.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let mut received = [0; 64];
    assert_eq!(server.read(&mut received).unwrap(), 0);
}

#[test]
fn protected_fragmentation_keeps_every_wire_packet_inside_its_sdu() {
    let (mut server, mut transport) = pair(128);
    transport.protection = Some(nne::protection_for_test());
    transport.send_packets(6, 0, 0, &[0xab; 300]).unwrap();
    let mut packet_count = 0;
    let mut total = 0;
    while total < 300 {
        let mut header = [0; 10];
        server.read_exact(&mut header).unwrap();
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        assert!(size <= 128);
        assert_eq!(&header[4..], &[6, 0, 0, 0, 0, 0]);
        let mut data = vec![0; size - 10];
        server.read_exact(&mut data).unwrap();
        assert_eq!(data.last(), Some(&0));
        packet_count += 1;
        // max plaintext is 128 - header10 - SHA25632 - framing/padding17.
        total += (128usize - 10 - 49).min(300 - total);
    }
    assert_eq!(packet_count, 5);
}

#[test]
fn partial_packet_trickle_cannot_renew_the_absolute_native_deadline() {
    let (mut server, mut transport) = pair(256);
    transport.native_deadline = Some(Instant::now() + Duration::from_millis(100));
    let sender = std::thread::spawn(move || {
        let mut header = packet(&[0; 190]);
        header.truncate(10);
        server.write_all(&header).unwrap();
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(20));
            if server.write_all(&[1]).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    assert!(transport.receive_packet().is_err());
    assert!(start.elapsed() < Duration::from_millis(400));
    assert!(transport.low_level_transport.is_none());
    sender.join().unwrap();
}

#[test]
fn unprotected_non_marker_control_packets_close_a_native_transport() {
    for kind in [constants::PACKET_TYPE_CONTROL, constants::PACKET_TYPE_ACCEPT] {
        let (mut server, mut transport) = pair(256);
        transport.protection = Some(nne::protection_for_test());
        let mut injected = packet(&[0; 8]);
        injected[4] = kind;
        server.write_all(&injected).unwrap();
        let error = transport.receive_packet().err().unwrap();
        let message = error.to_string();
        assert!(
            message.starts_with("native encryption refused: expected a protected DATA or an unprotected MARKER, got"),
            "names what was expected and what arrived: {message}"
        );
        assert!(message.ends_with(&format!("got an unprotected {}", packet_type_label(kind))), "{message}");
        assert!(transport.low_level_transport.is_none());
        assert!(transport.protection.is_none());
    }
}

#[test]
fn unprotected_marker_packet_is_admitted_then_the_next_data_packet_is_still_authenticated() {
    let (mut server, mut transport) = pair(256);
    transport.protection = Some(nne::protection_for_test());

    // The server's own break marker: never encrypted, admitted as-is - this
    // is the packet type this patch widens the admitted set to cover.
    let mut marker = packet(&[0; 8]);
    marker[4] = constants::PACKET_TYPE_MARKER;
    server.write_all(&marker).unwrap();
    let received = transport.receive_packet().unwrap();
    assert_eq!(received.packet_type, constants::PACKET_TYPE_MARKER);
    assert!(transport.protection.is_some(), "admitting the marker does not drop protection");

    // Admitting the marker does not admit unauthenticated data: the next
    // packet, which carries the server's actual (here: unencrypted, so
    // undecodable) error reply, is still run through the authenticated
    // protected-record path and refused if it fails to decrypt.
    server.write_all(&packet(b"ORA-00942: table or view does not exist")).unwrap();
    let error = transport.receive_packet().err().unwrap();
    assert_eq!(error.to_string(), "native encryption refused: invalid protected record");
}
