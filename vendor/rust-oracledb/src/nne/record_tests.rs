use super::*;

fn bytes(hex: &str) -> Vec<u8> {
    base16ct::lower::decode_vec(hex).unwrap()
}

fn protection(encryption: u8, integrity: u8) -> Protection {
    Protection::new((0..=255).collect(), encryption, integrity).unwrap()
}

fn server(mut protection: Protection) -> Protection {
    std::mem::swap(&mut protection.hash.tx, &mut protection.hash.rx);
    std::mem::swap(&mut protection.hash.tx_mask, &mut protection.hash.rx_mask);
    protection
}

#[test]
fn cbc_matches_independent_nist_sp800_38a_vectors() {
    let iv = bytes("000102030405060708090a0b0c0d0e0f");
    let plaintext = bytes("6bc1bee22e409f96e93d7e117393172a");
    for (encryption, key, expected) in [
        (15, "2b7e151628aed2a6abf7158809cf4f3c", "7649abac8119b246cee98e9b12e9197d"),
        (16, "8e73b0f7da0e6452c810f32b809079e562f8ead2522c6b7b", "4f021db243bc633d7178183a9fa071e8"),
        (17, "603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4", "f58c4c04d6e5f1ba779eabfb5f7bfbd6"),
    ] {
        let key = bytes(key);
        let mut secret = vec![0; 256];
        secret[..key.len()].copy_from_slice(&key);
        secret[32..48].copy_from_slice(&iv);
        let protection = Protection::new(secret, encryption, 5).unwrap();
        let mut encrypted = plaintext.clone();
        protection.encrypt(&mut encrypted).unwrap();
        assert_eq!(encrypted, bytes(expected));
        protection.decrypt(&mut encrypted).unwrap();
        assert_eq!(encrypted, plaintext);
    }
}

#[test]
fn every_algorithm_pair_protects_directional_multirecord_and_padding_boundaries() {
    for encryption in [15, 16, 17] {
        for integrity in [4, 5, 6] {
            let mut client = protection(encryption, integrity);
            let mut server = server(protection(encryption, integrity));
            for size in [0, 1, 15, 16, 17, 31, 32, 255, 8192] {
                let plaintext = vec![0xab; size];
                let sealed = client.seal(&plaintext).unwrap();
                assert!(sealed.len() <= plaintext.len() + client.overhead());
                assert_eq!(server.open(&sealed).unwrap(), plaintext);
                let reply = server.seal(&plaintext).unwrap();
                assert_eq!(client.open(&reply).unwrap(), plaintext);
            }
        }
    }
}

#[test]
fn every_single_byte_record_corruption_and_truncation_is_refused_uniformly() {
    let record = protection(17, 5).seal(b"authenticated payload").unwrap();
    for index in 0..record.len() {
        let mut tampered = record.clone();
        tampered[index] ^= 1;
        let error = server(protection(17, 5)).open(&tampered).unwrap_err();
        assert_eq!(error.to_string(), "native encryption refused: invalid protected record");
    }
    for end in 0..record.len() {
        let error = server(protection(17, 5)).open(&record[..end]).unwrap_err();
        assert_eq!(error.to_string(), "native encryption refused: invalid protected record");
    }
    let mut extended = record.clone();
    extended.push(0);
    assert!(server(protection(17, 5)).open(&extended).is_err());
}

#[test]
fn replay_and_reordering_cannot_reuse_a_directional_integrity_mask() {
    let mut client = protection(17, 5);
    let first = client.seal(b"first").unwrap();
    let second = client.seal(b"second").unwrap();
    let mut receiving = server(protection(17, 5));
    assert_eq!(receiving.open(&first).unwrap(), b"first");
    assert!(receiving.open(&first).is_err());
    assert!(server(protection(17, 5)).open(&second).is_err());
    // The opposite direction has a different mask even for its first record.
    assert!(protection(17, 5).open(&first).is_err());
}

#[test]
fn authentication_key_folding_is_a_one_way_phase_change() {
    for key_length in [16, 24, 32] {
        let authentication_key = vec![0xa5; key_length];
        let mut client = protection(17, 5);
        let before = client.seal(b"same payload").unwrap();
        client.fold(&authentication_key).unwrap();
        let after = client.seal(b"same payload").unwrap();
        assert_ne!(before, after);
        assert_eq!(before.last(), Some(&0));
        assert_eq!(after.last(), Some(&1));
        let mut peer = protection(17, 5);
        peer.fold(&authentication_key).unwrap();
        assert_eq!(server(peer).open(&after).unwrap(), b"same payload");
        assert!(server(protection(17, 5)).open(&after).is_err());
        let mut peer = protection(17, 5);
        peer.fold(&authentication_key).unwrap();
        assert!(server(peer).open(&before).is_err());
        assert!(client.fold(&authentication_key).is_err());
    }
    assert!(protection(17, 5).fold(&[]).is_err());
}

#[test]
fn protected_records_match_independent_go_ora_v3_vectors() {
    // reference_vectors.go, pinned go-ora 62f8a42fe821414cec8fcacb1b877dfef836f798.
    // Hosted generator SHA256: d0ed9ddd0c2ad63921949bfb02024d2f2d9b6d9545c8ab2e19c501375fbf38bc.
    // These cover the client direction and 32-byte folding; live peers must prove interoperability.
    let lines: Vec<_> = include_str!("reference_vectors.txt").lines().collect();
    assert_eq!(lines.len(), 54);
    for group in lines.chunks_exact(3) {
        let mut fields = group[0].split_whitespace();
        let encryption = fields.next().unwrap().parse().unwrap();
        let integrity = fields.next().unwrap().parse().unwrap();
        let folded: bool = fields.next().unwrap().parse().unwrap();
        let mut client = protection(encryption, integrity);
        let mut peer = protection(encryption, integrity);
        if folded {
            client.fold(&[0xa5; 32]).unwrap();
            peer.fold(&[0xa5; 32]).unwrap();
        }
        let mut peer = server(peer);
        for (index, (line, plaintext)) in group.iter().zip([b"first".as_slice(), b"second", b""]).enumerate() {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 5);
            assert_eq!(fields[0].parse::<u8>().unwrap(), encryption);
            assert_eq!(fields[1].parse::<u8>().unwrap(), integrity);
            assert_eq!(fields[2].parse::<bool>().unwrap(), folded);
            assert_eq!(fields[3].parse::<usize>().unwrap(), index);
            let expected = bytes(fields[4]);
            assert_eq!(client.seal(plaintext).unwrap(), expected);
            assert_eq!(peer.open(&expected).unwrap(), plaintext);
        }
    }
}
