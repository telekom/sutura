use crypto_bigint::{Odd, U256, U2048, modular::{FixedMontyForm, FixedMontyParams}};
use zeroize::{Zeroize, Zeroizing};

use super::{Negotiated, field, finish, header, refused, service};
use super::record::Protection;
use crate::error::Error;

// RFC 3526 group 14: a fixed, independently published safe prime. Unknown groups
// are refused before modular arithmetic rather than trusting server-selected primes.
const GROUP14: U2048 = U2048::from_be_hex(concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
    "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3D",
    "C2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F",
    "83655D23DCA3AD961C62F356208552BB9ED529077096966D67",
    "0C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39",
    "E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2",
    "BCBF6955817183995497CEA956AE515D2261898FA051015728",
    "E5A8AACAA68FFFFFFFFFFFFFFFF"
));

fn number(bytes: &[u8]) -> Result<U2048, Error> {
    if bytes.len() > 256 {
        return Err(refused("unsupported Diffie-Hellman group"));
    }
    let mut padded = [0u8; 256];
    padded[256 - bytes.len()..].copy_from_slice(bytes);
    Ok(U2048::from_be_slice(&padded))
}

pub(super) fn exchange(negotiated: &Negotiated<'_>) -> Result<(Vec<u8>, Protection), Error> {
    if negotiated.group_bits != 2048 || number(negotiated.modulus)? != GROUP14
        || number(negotiated.generator)? != U2048::from_u8(2)
    {
        return Err(refused("unsupported Diffie-Hellman group"));
    }
    let peer = number(negotiated.peer)?;
    if peer <= U2048::ONE || peer >= GROUP14.wrapping_sub(&U2048::ONE) {
        return Err(refused("invalid Diffie-Hellman public value"));
    }
    let odd = Odd::new(GROUP14).into_option().ok_or_else(|| refused("invalid Diffie-Hellman group"))?;
    let parameters = FixedMontyParams::new(odd);
    let peer_form = FixedMontyForm::new(&peer, &parameters);
    let order = GROUP14.wrapping_sub(&U2048::ONE).shr(1);
    if peer_form.pow(&order).retrieve() != U2048::ONE {
        return Err(refused("invalid Diffie-Hellman subgroup"));
    }
    let mut private_bytes = Zeroizing::new([0u8; 32]);
    rand::fill(private_bytes.as_mut_slice());
    private_bytes[0] |= 0x80;
    let private = Zeroizing::new(U256::from_be_slice(private_bytes.as_slice()));
    let public = FixedMontyForm::new(&U2048::from_u8(2), &parameters).pow(&*private).retrieve();
    let shared_form = Zeroizing::new(peer_form.pow(&*private));
    let shared = Zeroizing::new(shared_form.retrieve());
    if *shared <= U2048::ONE || *shared >= GROUP14.wrapping_sub(&U2048::ONE) {
        return Err(refused("invalid Diffie-Hellman shared value"));
    }
    let mut shared_bytes = shared.to_be_bytes();
    let protection = Protection::new(shared_bytes.to_vec(), negotiated.encryption, negotiated.integrity);
    shared_bytes.zeroize();
    let protection = protection?;
    let mut response = Vec::with_capacity(281);
    header(&mut response, 1);
    service(&mut response, 3, 1);
    field(&mut response, 1, &public.to_be_bytes());
    finish(&mut response);
    Ok((response, protection))
}

#[cfg(test)]
#[path = "dh_tests.rs"]
mod tests;
