// Modern NNE record state adapted from go-ora v3 at the revision in ../mod.rs.
// MIT; see LICENSE-go-ora. No legacy five-byte integrity key mode is implemented.

use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit, block_padding::NoPadding};
use sha2::{Digest, Sha256, Sha384, Sha512};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use super::refused;
use crate::error::Error;

type MaskCipher = cbc::Encryptor<aes::Aes128>;

#[derive(Clone, Copy)]
pub(super) enum Integrity {
    Sha256,
    Sha384,
    Sha512,
}

impl Integrity {
    pub(super) fn from_id(id: u8) -> Result<Self, Error> {
        match id {
            4 => Ok(Self::Sha512),
            5 => Ok(Self::Sha256),
            6 => Ok(Self::Sha384),
            _ => Err(Error::native_algorithm_refused("integrity", id)),
        }
    }

    fn size(self) -> usize {
        match self {
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }

    fn digest(self, data: &[u8], mask: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha256 => Sha256::new().chain_update(data).chain_update(mask).finalize().to_vec(),
            Self::Sha384 => Sha384::new().chain_update(data).chain_update(mask).finalize().to_vec(),
            Self::Sha512 => Sha512::new().chain_update(data).chain_update(mask).finalize().to_vec(),
        }
    }
}

fn cipher(key: &[u8], iv: &[u8]) -> Result<MaskCipher, Error> {
    MaskCipher::new_from_slices(key, iv).map_err(|_| refused("invalid native key state"))
}

fn step(cipher: &mut MaskCipher, buffer: &mut [u8]) {
    for chunk in buffer.chunks_exact_mut(16) {
        let mut block = aes::cipher::Block::<aes::Aes128>::default();
        block.copy_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        chunk.copy_from_slice(&block);
        block.zeroize();
    }
}

struct HashState {
    tx: MaskCipher,
    rx: MaskCipher,
    tx_mask: Zeroizing<Vec<u8>>,
    rx_mask: Zeroizing<Vec<u8>>,
    algorithm: Integrity,
}

impl HashState {
    fn new(key: &[u8], iv: &[u8], algorithm: Integrity) -> Result<Self, Error> {
        let mut seed = Zeroizing::new([0u8; 16]);
        seed[..15].copy_from_slice(&key[..15]);
        seed[15] = 0xff;
        let mut generator = cipher(seed.as_slice(), iv)?;
        let mut generated = Zeroizing::new([0u8; 32]);
        step(&mut generator, generated.as_mut_slice());
        seed.copy_from_slice(&generated[..16]);
        seed[15] = 0x5a;
        let tx = cipher(seed.as_slice(), &generated[16..])?;
        seed[15] = 0xb4;
        let rx = cipher(seed.as_slice(), &generated[16..])?;
        Ok(Self {
            tx,
            rx,
            tx_mask: Zeroizing::new(vec![0; algorithm.size()]),
            rx_mask: Zeroizing::new(vec![0; algorithm.size()]),
            algorithm,
        })
    }

    fn compute(&mut self, data: &[u8]) -> Vec<u8> {
        step(&mut self.tx, &mut self.tx_mask);
        self.algorithm.digest(data, &self.tx_mask)
    }

    fn verify(&mut self, data: &[u8], digest: &[u8]) -> bool {
        step(&mut self.rx, &mut self.rx_mask);
        let expected = Zeroizing::new(self.algorithm.digest(data, &self.rx_mask));
        bool::from(expected.as_slice().ct_eq(digest))
    }
}

pub(crate) struct Protection {
    session_key: Zeroizing<Vec<u8>>,
    iv: Zeroizing<[u8; 32]>,
    key_length: usize,
    hash: HashState,
    folded: bool,
}

impl Protection {
    pub(super) fn new(session_key: Vec<u8>, encryption: u8, integrity: u8) -> Result<Self, Error> {
        let session_key = Zeroizing::new(session_key);
        if session_key.len() < 64 {
            return Err(refused("short native key material"));
        }
        let key_length = match encryption {
            15 => 16,
            16 => 24,
            17 => 32,
            _ => return Err(Error::native_algorithm_refused("encryption", encryption)),
        };
        let mut iv = Zeroizing::new([0; 32]);
        iv.copy_from_slice(&session_key[32..64]);
        let hash = HashState::new(&session_key, &iv[..16], Integrity::from_id(integrity)?)?;
        Ok(Self { session_key, iv, key_length, hash, folded: false })
    }

    pub(crate) fn overhead(&self) -> usize {
        self.hash.algorithm.size() + 17
    }

    pub(crate) fn fold(&mut self, authentication_key: &[u8]) -> Result<(), Error> {
        if self.folded || !matches!(authentication_key.len(), 16 | 24 | 32) {
            return Err(refused("missing or repeated authentication key folding"));
        }
        for (target, byte) in self.session_key.iter_mut().zip(authentication_key) {
            *target ^= byte;
        }
        for (target, byte) in self.iv.iter_mut().zip(authentication_key) {
            *target ^= byte;
        }
        self.hash = HashState::new(&self.session_key, &self.iv[..16], self.hash.algorithm)?;
        self.folded = true;
        Ok(())
    }

    /// CBC is reset to the negotiated session IV for each record, as specified by NNE.
    fn encrypt(&self, data: &mut [u8]) -> Result<(), Error> {
        let key = &self.session_key[..self.key_length];
        let iv = &self.iv[..16];
        let length = data.len();
        let result = match self.key_length {
            16 => cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
                .map_err(|_| refused("invalid native key state"))?
                .encrypt_padded::<NoPadding>(data, length).map(|_| ()),
            24 => cbc::Encryptor::<aes::Aes192>::new_from_slices(key, iv)
                .map_err(|_| refused("invalid native key state"))?
                .encrypt_padded::<NoPadding>(data, length).map(|_| ()),
            32 => cbc::Encryptor::<aes::Aes256>::new_from_slices(key, iv)
                .map_err(|_| refused("invalid native key state"))?
                .encrypt_padded::<NoPadding>(data, length).map(|_| ()),
            _ => return Err(refused("invalid native key state")),
        };
        result.map_err(|_| refused("invalid protected record"))
    }

    fn decrypt(&self, data: &mut [u8]) -> Result<(), Error> {
        let key = &self.session_key[..self.key_length];
        let iv = &self.iv[..16];
        let result = match self.key_length {
            16 => cbc::Decryptor::<aes::Aes128>::new_from_slices(key, iv)
                .map_err(|_| refused("invalid native key state"))?
                .decrypt_padded::<NoPadding>(data).map(|_| ()),
            24 => cbc::Decryptor::<aes::Aes192>::new_from_slices(key, iv)
                .map_err(|_| refused("invalid native key state"))?
                .decrypt_padded::<NoPadding>(data).map(|_| ()),
            32 => cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv)
                .map_err(|_| refused("invalid native key state"))?
                .decrypt_padded::<NoPadding>(data).map(|_| ()),
            _ => return Err(refused("invalid native key state")),
        };
        result.map_err(|_| refused("invalid protected record"))
    }

    pub(crate) fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, Error> {
        let mut record = Zeroizing::new(Vec::with_capacity(plaintext.len() + self.overhead()));
        record.extend_from_slice(plaintext);
        record.extend_from_slice(&self.hash.compute(plaintext));
        let padding = (16 - record.len() % 16) % 16;
        record.resize(record.len() + padding, 0);
        self.encrypt(&mut record)?;
        record.push((padding + 1) as u8);
        record.push(u8::from(self.folded));
        Ok(std::mem::take(&mut *record))
    }

    pub(crate) fn open(&mut self, record: &[u8]) -> Result<Vec<u8>, Error> {
        if record.len() < self.hash.algorithm.size() + 2
            || (record.len() - 2) % 16 != 0
            || record.last() != Some(&u8::from(self.folded))
        {
            return Err(refused("invalid protected record"));
        }
        let padding = record[record.len() - 2] as usize;
        if !(1..=16).contains(&padding) {
            return Err(refused("invalid protected record"));
        }
        let mut plaintext = Zeroizing::new(record[..record.len() - 2].to_vec());
        self.decrypt(&mut plaintext)?;
        let end = plaintext.len().checked_sub(padding - 1)
            .filter(|&n| n >= self.hash.algorithm.size())
            .ok_or_else(|| refused("invalid protected record"))?;
        let data_end = end - self.hash.algorithm.size();
        // No unauthenticated plaintext leaves this method. All corrupt-record errors are uniform.
        let valid = self.hash.verify(&plaintext[..data_end], &plaintext[data_end..end]);
        let zero_padding = plaintext[end..].iter().fold(0, |acc, byte| acc | byte) == 0;
        if !valid || !zero_padding {
            return Err(refused("invalid protected record"));
        }
        plaintext[data_end..].zeroize();
        plaintext.truncate(data_end);
        Ok(std::mem::take(&mut *plaintext))
    }
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
