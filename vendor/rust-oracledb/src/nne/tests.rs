use super::*;
use crate::{Config, ErrorKind};

fn response(encryption: u8, integrity: u8, version: u32) -> Vec<u8> {
    let mut output = Vec::new();
    header(&mut output, 4);
    service(&mut output, 4, 3);
    field(&mut output, 5, &VERSION.to_be_bytes());
    field(&mut output, 6, &31u16.to_be_bytes());
    field(&mut output, 1, &[0xde, 0xad, 0xbe, 0xef, 0, 3, 0, 0, 0, 2, 0, 4, 0, 1]);
    service(&mut output, 1, 2);
    field(&mut output, 5, &VERSION.to_be_bytes());
    field(&mut output, 6, &0xfbffu16.to_be_bytes());
    service(&mut output, 2, 2);
    field(&mut output, 5, &version.to_be_bytes());
    field(&mut output, 2, &[encryption]);
    service(&mut output, 3, 8);
    field(&mut output, 5, &version.to_be_bytes());
    field(&mut output, 2, &[integrity]);
    field(&mut output, 3, &2048u16.to_be_bytes());
    field(&mut output, 3, &256u16.to_be_bytes());
    field(&mut output, 1, &[2]);
    field(&mut output, 1, &[0xff; 256]);
    field(&mut output, 1, &[4; 256]);
    field(&mut output, 1, &[0; 16]);
    finish(&mut output);
    output
}

#[test]
fn native_is_explicit_and_defaults_off() {
    assert!(!Config::default().native_encryption());
    assert!(Config::default().set_native_encryption(true).native_encryption());
    assert!(!Config::default().set_native_encryption(true).set_native_encryption(false).native_encryption());
}

#[test]
fn required_services_accept_only_compiled_algorithms_and_modern_versions() {
    for encryption in [15, 16, 17] {
        for integrity in [4, 5, 6] {
            let frame = response(encryption, integrity, VERSION);
            let value = parse(&frame).unwrap();
            assert_eq!(value.encryption, encryption);
            assert_eq!(value.integrity, integrity);
        }
    }
    for encryption in [0, 1, 2, 3, 6, 8, 10, 11, 12, 255] {
        let frame = response(encryption, 5, VERSION);
        assert!(matches!(parse(&frame).err().unwrap().kind(), ErrorKind::NativeAlgorithmRefused("encryption", id) if *id == encryption));
    }
    for integrity in [0, 1, 2, 3, 7, 255] {
        let frame = response(17, integrity, VERSION);
        assert!(matches!(parse(&frame).err().unwrap().kind(), ErrorKind::NativeAlgorithmRefused("integrity", id) if *id == integrity));
    }
    assert!(parse(&response(17, 5, 0x0b200200)).is_err());
    assert!(parse(&response(17, 5, 0x13001000)).is_ok());
}

#[test]
fn malformed_bounded_negotiation_never_becomes_a_session() {
    let frame = response(17, 5, VERSION);
    for end in 0..frame.len() {
        assert!(parse(&frame[..end]).is_err(), "accepted truncated frame {end}");
    }
    let mut bad = frame.clone();
    bad.push(0);
    assert!(parse(&bad).is_err());
    for size in [0u16, 12, 4097, u16::MAX] {
        let mut bad = frame.clone();
        bad[4..6].copy_from_slice(&size.to_be_bytes());
        assert!(parse(&bad).is_err());
    }
    for count in [0u16, 1, 3, 5, u16::MAX] {
        let mut bad = frame.clone();
        bad[10..12].copy_from_slice(&count.to_be_bytes());
        assert!(parse(&bad).is_err());
    }
    // A syntactically valid but unrecognised modulus never reaches exponentiation.
    assert!(establish(&frame).is_err());
}

#[test]
fn duplicate_missing_and_unknown_services_are_refused() {
    let frame = response(17, 5, VERSION);
    for kind in [0u16, 1, 2, 3, 5, u16::MAX] {
        let mut bad = frame.clone();
        bad[13..15].copy_from_slice(&kind.to_be_bytes());
        assert!(parse(&bad).is_err());
    }
    let mut bad = frame;
    bad[17..21].copy_from_slice(&1u32.to_be_bytes());
    assert!(parse(&bad).is_err());
}
