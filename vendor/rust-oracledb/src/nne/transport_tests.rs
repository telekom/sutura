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
fn unprotected_control_and_marker_packets_close_a_native_transport() {
    for kind in [constants::PACKET_TYPE_CONTROL, constants::PACKET_TYPE_MARKER, constants::PACKET_TYPE_ACCEPT] {
        let (mut server, mut transport) = pair(256);
        transport.protection = Some(nne::protection_for_test());
        let mut injected = packet(&[0; 8]);
        injected[4] = kind;
        server.write_all(&injected).unwrap();
        let error = transport.receive_packet().err().unwrap();
        assert_eq!(error.to_string(), "native encryption refused: unprotected native control packet");
        assert!(transport.low_level_transport.is_none());
        assert!(transport.protection.is_none());
    }
}
