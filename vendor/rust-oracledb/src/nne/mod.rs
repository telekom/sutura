// ANO framing adapted from go-ora 62f8a42fe821414cec8fcacb1b877dfef836f798
// (MIT; see LICENSE-go-ora). Cryptographic primitives are supplied by RustCrypto.
// Native encryption does not authenticate the peer.

use crate::error::Error;

mod record;
mod dh;
pub(crate) use record::Protection;

const MAGIC: u32 = 0xdeadbeef;
const VERSION: u32 = 0x17000000;
pub(crate) const MAX_NEGOTIATION: usize = 4096;
const MAX_FIELD: usize = 256;

fn refused(reason: &'static str) -> Error {
    Error::native_encryption_refused(reason)
}

/// A cursor never advances until its complete field has been bounded.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], Error> {
        let value = self.0.get(..size).ok_or_else(|| refused("malformed negotiation"))?;
        self.0 = &self.0[size..];
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let value = self.take(2)?;
        Ok(u16::from_be_bytes([value[0], value[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let value = self.take(4)?;
        Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
    }

    fn field(&mut self, kind: u16, size: Option<usize>) -> Result<&'a [u8], Error> {
        let length = self.u16()? as usize;
        if self.u16()? != kind || length > MAX_FIELD || size.is_some_and(|n| length != n) {
            return Err(refused("invalid negotiation field"));
        }
        self.take(length)
    }

    fn version(&mut self) -> Result<u32, Error> {
        let value = self.field(5, Some(4))?;
        Ok(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
    }

    fn scalar16(&mut self, kind: u16) -> Result<u16, Error> {
        let value = self.field(kind, Some(2))?;
        Ok(u16::from_be_bytes([value[0], value[1]]))
    }
}

fn field(output: &mut Vec<u8>, kind: u16, value: &[u8]) {
    // Every caller supplies a compile-time-bounded field, never peer input.
    output.extend_from_slice(&(value.len() as u16).to_be_bytes());
    output.extend_from_slice(&kind.to_be_bytes());
    output.extend_from_slice(value);
}

fn service(output: &mut Vec<u8>, kind: u16, fields: u16) {
    output.extend_from_slice(&kind.to_be_bytes());
    output.extend_from_slice(&fields.to_be_bytes());
    output.extend_from_slice(&0u32.to_be_bytes());
}

fn header(output: &mut Vec<u8>, services: u16) {
    output.extend_from_slice(&MAGIC.to_be_bytes());
    output.extend_from_slice(&0u16.to_be_bytes());
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.extend_from_slice(&services.to_be_bytes());
    output.push(0);
}

fn finish(output: &mut [u8]) {
    let length = (output.len() as u16).to_be_bytes();
    output[4..6].copy_from_slice(&length);
}

pub(crate) fn offer() -> Vec<u8> {
    let mut output = Vec::with_capacity(160);
    header(&mut output, 4);
    service(&mut output, 4, 3);
    field(&mut output, 5, &VERSION.to_be_bytes());
    field(&mut output, 1, &[0, 0, 16, 28, 102, 236, 40, 234]);
    field(&mut output, 1, &[
        0xde, 0xad, 0xbe, 0xef, 0, 3, 0, 0, 0, 4, 0, 4, 0, 1, 0, 2, 0, 3,
    ]);
    service(&mut output, 1, 3);
    field(&mut output, 5, &VERSION.to_be_bytes());
    field(&mut output, 3, &0xe0e1u16.to_be_bytes());
    field(&mut output, 6, &0xfcffu16.to_be_bytes());
    service(&mut output, 2, 3);
    field(&mut output, 5, &VERSION.to_be_bytes());
    field(&mut output, 1, &[17, 16, 15]);
    field(&mut output, 2, &[1]);
    service(&mut output, 3, 2);
    field(&mut output, 5, &VERSION.to_be_bytes());
    field(&mut output, 1, &[4, 6, 5]);
    finish(&mut output);
    output
}

pub(crate) struct Negotiated<'a> {
    pub(crate) encryption: u8,
    pub(crate) integrity: u8,

    pub(crate) group_bits: u16,
    pub(crate) generator: &'a [u8],
    pub(crate) modulus: &'a [u8],
    pub(crate) peer: &'a [u8],
}

/// Reads only the advertised length; callers bound the accumulated frame before extending it.
pub(crate) fn frame_length(input: &[u8]) -> Result<usize, Error> {
    let mut reader = Reader(input);
    if reader.u32()? != MAGIC {
        return Err(refused("invalid negotiation header"));
    }
    let length = reader.u16()? as usize;
    if !(13..=MAX_NEGOTIATION).contains(&length) {
        return Err(refused("negotiation exceeds bound"));
    }
    Ok(length)
}

pub(crate) fn parse(input: &[u8]) -> Result<Negotiated<'_>, Error> {
    if frame_length(input)? != input.len() {
        return Err(refused("negotiation length mismatch"));
    }
    let mut reader = Reader(&input[6..]);
    let _version = reader.u32()?;
    if reader.u16()? != 4 || reader.take(1)? != [0] {
        return Err(refused("required negotiation services missing"));
    }
    let mut seen = [false; 4];
    let mut encryption = 0;
    let mut integrity = 0;
    let mut dh = None;
    for _ in 0..4 {
        let kind = reader.u16()?;
        let count = reader.u16()?;
        if !(1..=4).contains(&kind) || seen[kind as usize - 1] {
            return Err(refused("unknown or duplicate negotiation service"));
        }
        seen[kind as usize - 1] = true;
        if reader.u32()? != 0 {
            return Err(refused("peer refused required negotiation service"));
        }
        let service_version = reader.version()?;
        match (kind, count) {
            (4, 3) => {
                if reader.scalar16(6)? != 31 {
                    return Err(refused("supervisor refused negotiation"));
                }
                let services = reader.field(1, None)?;
                let mut services = Reader(services);
                if services.u32()? != MAGIC || services.u16()? != 3 {
                    return Err(refused("invalid supervisor services"));
                }
                let count = services.u32()?;
                if !(1..=4).contains(&count) || services.0.len() != count as usize * 2 {
                    return Err(refused("invalid supervisor services"));
                }
                let mut announced = [false; 4];
                for _ in 0..count {
                    let id = services.u16()?;
                    if !(1..=4).contains(&id) || announced[id as usize - 1] {
                        return Err(refused("invalid supervisor services"));
                    }
                    announced[id as usize - 1] = true;
                }
            }
            (1, 2) => {
                if reader.scalar16(6)? != 0xfbff {
                    return Err(refused("unexpected native authentication service"));
                }
            }
            (2, 2) => {
                require_modern(service_version)?;
                encryption = reader.field(2, Some(1))?[0];
                if !matches!(encryption, 15..=17) {
                    return Err(Error::native_algorithm_refused("encryption", encryption));
                }
            }
            (3, 8) => {
                require_modern(service_version)?;
                integrity = reader.field(2, Some(1))?[0];
                if !matches!(integrity, 4..=6) {
                    return Err(Error::native_algorithm_refused("integrity", integrity));
                }
                let group_bits = reader.scalar16(3)?;
                let private_bits = reader.scalar16(3)?;
                if !(512..=2048).contains(&group_bits) || !(1..=2048).contains(&private_bits) {
                    return Err(refused("unsupported Diffie-Hellman size"));
                }
                let generator = reader.field(1, None)?;
                let modulus = reader.field(1, Some((group_bits as usize).div_ceil(8)))?;
                let peer = reader.field(1, Some(modulus.len()))?;
                let iv = reader.field(1, None)?;
                if generator.is_empty() || generator.len() > modulus.len() || !(16..=64).contains(&iv.len()) {
                    return Err(refused("invalid Diffie-Hellman parameters"));
                }
                dh = Some((group_bits, generator, modulus, peer));
            }
            _ => return Err(refused("invalid negotiation service fields")),
        }
    }
    if !reader.0.is_empty() || encryption == 0 || integrity == 0 {
        return Err(refused("required negotiation services missing"));
    }
    let (group_bits, generator, modulus, peer) =
        dh.ok_or_else(|| refused("missing Diffie-Hellman exchange"))?;
    Ok(Negotiated {
        encryption,
        integrity,
        group_bits,
        generator,
        modulus,
        peer,
    })
}

fn require_modern(version: u32) -> Result<(), Error> {
    if version >> 24 >= 23 || (version >> 12) & 0xff == 1 {
        Ok(())
    } else {
        Err(refused("legacy native cryptography is not supported"))
    }
}

pub(crate) fn establish(input: &[u8]) -> Result<(Vec<u8>, Protection), Error> {
    dh::exchange(&parse(input)?)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn protection_for_test() -> Protection {
    Protection::new((0..=255).collect(), 17, 5).unwrap()
}
