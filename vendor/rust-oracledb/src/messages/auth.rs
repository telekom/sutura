//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// auth.rs
//
// Defines the structure used for sending and receiving the auth message.
// This is the fourth (and fifth) messages sent to the database while
// establishing a connection and can be combined with the protocol and data
// types messages when fast authentication is available.
//
// The 10G password verifier (des_verifier) and the 16 byte combo key of a
// 10G login are a port, not a copy, of getKeyFromUserNameAndPassword and
// generatePasswordEncKey in v2/auth_object.go of
// github.com/sijms/go-ora at commit
// 62f8a42fe821414cec8fcacb1b877dfef836f798, which carries this notice:
//
// MIT License
//
// Copyright (c) 2020 Samy Sultan
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.
// -----------------------------------------------------------------------------

use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyIvInit};
use rand::RngExt;
use sha2::Digest;
use std::collections::HashMap;

use crate::client::Client;
use crate::constants;
use crate::error::Error;
use crate::messages::Message;
use crate::ora_version::OracleVersion;
use crate::response::Response;
use crate::write_buffer::WriteBuffer;

// The most PBKDF2 iterations the driver runs for a count the database sends
// in AUTH_PBKDF2_VGEN_COUNT or AUTH_PBKDF2_SDER_COUNT. Oracle does not document
// a range for these counts. The OWASP Password Storage Cheat Sheet
// (https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html)
// recommends 220,000 iterations for PBKDF2-HMAC-SHA512; the cap is nearly five
// times that, and bounds the work one login can ask the client to do.
const MAX_PBKDF2_ITERATIONS: u32 = 1 << 20;

// The DES key of the first pass of the 10G password verifier.
const DES_10G_KEY: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];

struct Pair {
    key: String,
    value: String,
    flags: u32,
}

impl Pair {
    fn new(key: &str, value: &str, flags: u32) -> Pair {
        Pair { key: String::from(key), value: String::from(value), flags }
    }
}

/// The password verifier the database asks a login to use, named by the
/// flags of the AUTH_VFR_DATA pair it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verifier {
    /// 0x939: DES over the user name and the password.
    Legacy10g,
    /// 0xb152 or 0x1b25: SHA-1 over the password and a salt.
    Legacy11g,
    /// 0x4815: PBKDF2 with SHA-512 over the password and a salt.
    Pbkdf2,
}

impl Verifier {
    /// Returns the verifier the flags name. A type the driver does not know,
    /// or a 10G or 11G type when legacy verifiers are not allowed, is
    /// ErrorKind::PasswordVerifierRefused naming the type.
    fn from_flags(flags: u32, allow_legacy: bool) -> Result<Verifier, Error> {
        let verifier = match flags {
            constants::VERIFIER_TYPE_12C => return Ok(Verifier::Pbkdf2),
            constants::VERIFIER_TYPE_11G_1 | constants::VERIFIER_TYPE_11G_2 => {
                Verifier::Legacy11g
            }
            constants::VERIFIER_TYPE_10G => Verifier::Legacy10g,
            _ => return Err(Error::password_verifier_refused(flags)),
        };
        if allow_legacy {
            Ok(verifier)
        } else {
            Err(Error::password_verifier_refused(flags))
        }
    }

    /// Returns how the combo key is derived, chosen by the negotiated logon
    /// mode and the verifier. When the server negotiated custom hashing - it
    /// sends AUTH_PBKDF2_CSK_SALT and AUTH_PBKDF2_SDER_COUNT - every verifier
    /// derives the combo key with PBKDF2 (10G 16 bytes, 11G 24 bytes, 12C
    /// 32 bytes). Without custom hashing a 10G login MD5s bytes 16 to 32 of
    /// the two session keys and an 11G login with 48 byte session keys MD5s
    /// bytes 16 to 40. A 12C verifier is always a PBKDF2 verifier, so its
    /// login derives the combo key with PBKDF2 whatever the custom-hash
    /// signal; this lets the per-field validation name which PBKDF2 field of
    /// a malformed session is missing. Any other session key length is an
    /// error.
    fn combo_key_derivation(
        self,
        session_key_len: usize,
        custom_hash: bool,
    ) -> Result<ComboKeyDerivation, Error> {
        match (self, custom_hash) {
            // A 12C verifier is always PBKDF2, independently of the
            // custom-hash signal, so a malformed 12C session names the
            // per-field error. The session key length is validated here so a
            // malformed AUTH_SESSKEY names itself before the PBKDF2 reads.
            (Verifier::Pbkdf2, _) => match session_key_len {
                32 | 48 => Ok(ComboKeyDerivation::Pbkdf2),
                _ => Err(Error::invalid_auth_response("AUTH_SESSKEY")),
            },
            // Custom-hash mode (the server sent AUTH_PBKDF2_CSK_SALT and
            // AUTH_PBKDF2_SDER_COUNT): a legacy 10G or 11G login derives the
            // combo key with PBKDF2 too. The session key length is validated
            // here so a malformed AUTH_SESSKEY names itself.
            (_, true) => match session_key_len {
                32 | 48 => Ok(ComboKeyDerivation::Pbkdf2),
                _ => Err(Error::invalid_auth_response("AUTH_SESSKEY")),
            },
            // Without custom hashing a 10G login MD5s bytes 16 to 32 of the
            // two session keys and an 11G login with 48 byte session keys MD5s
            // bytes 16 to 40. Any other session key length is an error.
            (Verifier::Legacy10g, false) => match session_key_len {
                32 | 48 => Ok(ComboKeyDerivation::Md5),
                _ => Err(Error::invalid_auth_response("AUTH_SESSKEY")),
            },
            (Verifier::Legacy11g, false) => match session_key_len {
                48 => Ok(ComboKeyDerivation::Md5),
                _ => Err(Error::invalid_auth_response("AUTH_SESSKEY")),
            },
        }
    }
}

/// How the combo key of a login is derived from its two session keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComboKeyDerivation {
    Pbkdf2,
    Md5,
}

/// An AES key of a login: the password hash the session keys are encrypted
/// with, or the combo key the passwords are encrypted with. Its length
/// follows the verifier: 16 bytes for 10G, 24 bytes for 11G and 32 bytes for
/// 12C. Both encrypt in CBC mode with a zero IV and no padding.
#[derive(Clone, Copy)]
pub(crate) enum LoginKey {
    Aes128([u8; 16]),
    Aes192([u8; 24]),
    Aes256([u8; 32]),
}

impl LoginKey {
    /// Returns a key of zeros of the length the verifier uses.
    fn zeroed(verifier: Verifier) -> LoginKey {
        match verifier {
            Verifier::Legacy10g => LoginKey::Aes128([0; 16]),
            Verifier::Legacy11g => LoginKey::Aes192([0; 24]),
            Verifier::Pbkdf2 => LoginKey::Aes256([0; 32]),
        }
    }

    fn as_mut_bytes(&mut self) -> &mut [u8] {
        match self {
            LoginKey::Aes128(key) => key,
            LoginKey::Aes192(key) => key,
            LoginKey::Aes256(key) => key,
        }
    }

    /// Decrypts a value the database sent. A value that is not whole blocks,
    /// or longer than the output, is an error naming AUTH_SESSKEY, the one
    /// value decrypted.
    fn decrypt(
        &self,
        encrypted_text: &[u8],
        plain_text: &mut [u8],
    ) -> Result<(), Error> {
        let iv = [0u8; 16];
        let result = match self {
            LoginKey::Aes128(key) => {
                cbc::Decryptor::<aes::Aes128>::new(key.into(), &iv.into())
                    .decrypt_padded_b2b::<NoPadding>(encrypted_text, plain_text)
                    .map(|_| ())
            }
            LoginKey::Aes192(key) => {
                cbc::Decryptor::<aes::Aes192>::new(key.into(), &iv.into())
                    .decrypt_padded_b2b::<NoPadding>(encrypted_text, plain_text)
                    .map(|_| ())
            }
            LoginKey::Aes256(key) => {
                cbc::Decryptor::<aes::Aes256>::new(key.into(), &iv.into())
                    .decrypt_padded_b2b::<NoPadding>(encrypted_text, plain_text)
                    .map(|_| ())
            }
        };
        result.map_err(|_| Error::invalid_auth_response("AUTH_SESSKEY"))
    }

    /// Encrypts a value the driver built in whole blocks into an output of
    /// the same length.
    fn encrypt(&self, plain_text: &[u8], encrypted_text: &mut [u8]) {
        let iv = [0u8; 16];
        match self {
            LoginKey::Aes128(key) => {
                cbc::Encryptor::<aes::Aes128>::new(key.into(), &iv.into())
                    .encrypt_padded_b2b::<NoPadding>(plain_text, encrypted_text)
                    .unwrap();
            }
            LoginKey::Aes192(key) => {
                cbc::Encryptor::<aes::Aes192>::new(key.into(), &iv.into())
                    .encrypt_padded_b2b::<NoPadding>(plain_text, encrypted_text)
                    .unwrap();
            }
            LoginKey::Aes256(key) => {
                cbc::Encryptor::<aes::Aes256>::new(key.into(), &iv.into())
                    .encrypt_padded_b2b::<NoPadding>(plain_text, encrypted_text)
                    .unwrap();
            }
        }
    }
}

fn get_derived_key(
    key: &[u8],
    salt: &[u8],
    iterations: u32,
    derived_key: &mut [u8],
) {
    pbkdf2::pbkdf2_hmac::<sha2::Sha512>(key, salt, iterations, derived_key);
}

/// Returns the last block of DES in CBC mode, with a zero IV, over text of
/// whole blocks.
fn des_cbc_last_block(key: &[u8; 8], text: &[u8]) -> [u8; 8] {
    let iv = [0u8; 8];
    let mut encrypted_text = vec![0u8; text.len()];
    cbc::Encryptor::<des::Des>::new(key.into(), &iv.into())
        .encrypt_padded_b2b::<NoPadding>(text, &mut encrypted_text)
        .unwrap();
    let mut last_block = [0u8; 8];
    if let Some(block) = encrypted_text.rchunks_exact(8).next() {
        last_block.copy_from_slice(block);
    }
    last_block
}

/// Returns the 10G password verifier of a user name and a password: their
/// upper-cased text in UTF-16 (big-endian), padded with zeros to whole DES
/// blocks, run through DES in CBC mode under a fixed key, then again under
/// the last block of that first pass. The last block of the second pass is
/// the verifier.
///
/// The UTF-16 (big-endian) encoding is verified only by the ASCII known-answer
/// vectors below; non-ASCII legacy-credential encoding is unverified against a
/// real server.
fn des_verifier(user: &str, password: &str) -> [u8; 8] {
    let mut text: Vec<u8> = user
        .to_uppercase()
        .encode_utf16()
        .chain(password.to_uppercase().encode_utf16())
        .flat_map(u16::to_be_bytes)
        .collect();
    text.resize(text.len().div_ceil(8) * 8, 0);
    let first_pass = des_cbc_last_block(&DES_10G_KEY, &text);
    des_cbc_last_block(&first_pass, &text)
}

/// Returns the AES-128 password hash of a 10G login: the 10G verifier with
/// eight zero bytes appended.
fn des_password_hash(user: &str, password: &str) -> LoginKey {
    let mut key = [0u8; 16];
    key[..8].copy_from_slice(&des_verifier(user, password));
    LoginKey::Aes128(key)
}

/// Returns the AES-192 password hash of an 11G login: SHA-1 over the
/// password and the verifier data, with four zero bytes appended.
fn sha1_password_hash(password: &[u8], verifier_data: &[u8]) -> LoginKey {
    let mut hasher = sha1::Sha1::new();
    hasher.update(password);
    hasher.update(verifier_data);
    let mut key = [0u8; 24];
    key[..20].copy_from_slice(&hasher.finalize());
    LoginKey::Aes192(key)
}

/// Returns the PBKDF2 password key of a 12C login, which the speedy key
/// carries, and its AES-256 password hash: the first half of SHA-512 over
/// that password key and the verifier data.
fn pbkdf2_password_hash(
    password: &[u8],
    verifier_data: &[u8],
    iterations: u32,
) -> ([u8; 64], LoginKey) {
    let mut salt = verifier_data.to_vec();
    salt.extend(b"AUTH_PBKDF2_SPEEDY_KEY");
    let mut password_key = [0u8; 64];
    get_derived_key(password, &salt, iterations, &mut password_key);
    let mut hasher = sha2::Sha512::new();
    hasher.update(password_key);
    hasher.update(verifier_data);
    let mut key = [0u8; 32];
    key.copy_from_slice(&hasher.finalize()[..32]);
    (password_key, LoginKey::Aes256(key))
}

/// Returns the combo key of a login in custom-hash mode: PBKDF2 with SHA-512
/// over the upper-case hex text of the client's and then the server's
/// session key bytes, cut to the verifier's key length. The window follows
/// go-ora generatePasswordEncKey: for a 10G login it is half the session key
/// (client[:len/2] + server[:len/2]), for an 11G login bytes 0..24 of each,
/// and for a 12C login the whole 32 byte session keys.
fn pbkdf2_combo_key(
    verifier: Verifier,
    server_key: &[u8],
    client_key: &[u8],
    salt: &[u8],
    iterations: u32,
) -> LoginKey {
    let mut combo_key = LoginKey::zeroed(verifier);
    let derived_key = combo_key.as_mut_bytes();
    let window = match verifier {
        Verifier::Legacy10g => server_key.len() / 2,
        Verifier::Legacy11g => 24,
        // A 12C login uses the whole session keys (go-ora case 18453).
        Verifier::Pbkdf2 => server_key.len(),
    };
    let mut temp_key = client_key[..window].to_vec();
    temp_key.extend_from_slice(&server_key[..window]);
    let temp_key_str = base16ct::upper::encode_string(&temp_key);
    get_derived_key(temp_key_str.as_bytes(), salt, iterations, derived_key);
    combo_key
}

/// Returns the combo key of a legacy login, from the two session keys it
/// exchanged. A 10G login XORs bytes 16 to 32 and MD5s them into a 16 byte
/// AES-128 key, whatever the session key length; an 11G login with 48 byte
/// session keys XORs bytes 16 to 40, MD5s bytes 16 to 32 and bytes 32 to 40
/// into a 24 byte AES-192 key. A 12C login derives its combo key with PBKDF2
/// and never reaches this.
fn md5_combo_key(
    verifier: Verifier,
    server_key: &[u8],
    client_key: &[u8],
) -> LoginKey {
    match verifier {
        Verifier::Legacy10g => {
            let mut mixed = [0u8; 16];
            for (i, byte) in mixed.iter_mut().enumerate() {
                *byte = server_key[i + 16] ^ client_key[i + 16];
            }
            let mut key = [0u8; 16];
            key.copy_from_slice(&md5::Md5::digest(&mixed));
            LoginKey::Aes128(key)
        }
        Verifier::Legacy11g => {
            let mut mixed = [0u8; 24];
            for (i, byte) in mixed.iter_mut().enumerate() {
                *byte = server_key[i + 16] ^ client_key[i + 16];
            }
            let mut key = [0u8; 24];
            key[..16].copy_from_slice(&md5::Md5::digest(&mixed[..16]));
            key[16..].copy_from_slice(&md5::Md5::digest(&mixed[16..])[..8]);
            LoginKey::Aes192(key)
        }
        Verifier::Pbkdf2 => unreachable!(
            "a 12C login derives its combo key with PBKDF2, never MD5"
        ),
    }
}

pub struct AuthMessage {
    pub session_data: HashMap<String, String>,
    pairs: Vec<Pair>,
    combo_key: Option<LoginKey>,
    external_auth: bool,
    resend_needed: bool,
    verifier_type: Option<u32>,
}

impl AuthMessage {
    /// Adds a key/value pair to the list of key/value pairs being sent to the
    /// database during authentication.
    fn add_pair(&mut self, key: &str, value: &str, flags: u32) {
        self.pairs.push(Pair::new(key, value, flags));
    }

    /// Adds a key/value pair to the list of key/value pairs being sent to the
    /// database during authentication, but hex encodes the binary value first.
    fn add_pair_binary(&mut self, key: &str, value: &[u8], flags: u32) {
        let str_value = base16ct::upper::encode_string(value);
        self.add_pair(key, &str_value, flags);
    }

    /// Returns the value the database sent for the key. A missing key is an
    /// error.
    fn session_value(&self, key: &str) -> Result<&str, Error> {
        self.session_data
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| Error::invalid_auth_response(key))
    }

    /// Returns the value the database sent for the key as a number.
    fn session_number<T: std::str::FromStr>(
        &self,
        key: &str,
    ) -> Result<T, Error> {
        self.session_value(key)?
            .parse()
            .map_err(|_| Error::invalid_auth_response(key))
    }

    /// Returns the PBKDF2 iteration count the database sent for the key. A
    /// count above MAX_PBKDF2_ITERATIONS is an error.
    fn session_iterations(&self, key: &str) -> Result<u32, Error> {
        let count = self.session_number(key)?;
        if count > MAX_PBKDF2_ITERATIONS {
            return Err(Error::iteration_count_too_large(
                key,
                count,
                MAX_PBKDF2_ITERATIONS,
            ));
        }
        Ok(count)
    }

    /// Returns the value the database sent for the key, decoded from hex.
    fn session_bytes(&self, key: &str) -> Result<Vec<u8>, Error> {
        base16ct::upper::decode_vec(self.session_value(key)?)
            .map_err(|_| Error::invalid_auth_response(key))
    }

    /// Encrypts a password and adds a key/value pair containing the encrypted
    /// password.
    fn encrypt_password(
        &mut self,
        key: &str,
        password: &[u8],
        combo_key: &LoginKey,
    ) {
        let pad_amount = 16 - password.len() % 16;
        let mut padded_input = vec![0u8; password.len() + pad_amount + 16];
        rand::rng().fill(&mut padded_input[..16]);
        padded_input[16..16 + password.len()].copy_from_slice(password);
        let offset = password.len() + 16;
        let pad_byte: u8 = pad_amount.try_into().unwrap();
        for i in 0..pad_amount {
            padded_input[offset + i] = pad_byte;
        }
        let mut output_val = vec![0u8; padded_input.len()];
        combo_key.encrypt(&padded_input, &mut output_val);
        self.add_pair_binary(key, &output_val[..], 0);
    }

    /// Encrypts the main password (and the new password, if applicable).
    fn encrypt_passwords(&mut self, client: &mut Client, combo_key: &LoginKey) {
        self.encrypt_password(
            "AUTH_PASSWORD",
            &client.config().get_password_bytes(),
            combo_key,
        );
        if let Some(new_password) = client.config().get_new_password_bytes() {
            self.encrypt_password(
                "AUTH_NEWPASSWORD",
                &new_password,
                combo_key,
            );
        }
    }

    /// Decrypts the server's half of the session key, generates the client's
    /// half and adds it, encrypted, to the pairs sent to the database.
    fn exchange_session_keys<const N: usize>(
        &mut self,
        password_hash: &LoginKey,
        encoded_server_key: &[u8],
    ) -> Result<([u8; N], [u8; N]), Error> {
        let mut server_key = [0u8; N];
        password_hash.decrypt(encoded_server_key, &mut server_key)?;
        let mut client_key = [0u8; N];
        rand::rng().fill(&mut client_key[..]);
        let mut encoded_client_key = [0u8; N];
        password_hash.encrypt(&client_key, &mut encoded_client_key);
        self.add_pair_binary("AUTH_SESSKEY", &encoded_client_key, 1);
        Ok((server_key, client_key))
    }

    /// Generates the "speedy" key used by the server for validation without
    /// requiring time consuming calculations. Only a 12C login sends one.
    fn generate_speedy_key(
        &mut self,
        password_key: &[u8; 64],
        combo_key: &LoginKey,
    ) {
        let mut input_val = [0u8; 80];
        rand::rng().fill(&mut input_val[..16]);
        input_val[16..].copy_from_slice(password_key);
        let mut speedy_key = [0u8; 80];
        combo_key.encrypt(&input_val, &mut speedy_key);
        self.add_pair_binary("AUTH_PBKDF2_SPEEDY_KEY", &speedy_key, 0);
    }

    /// Generates the password verifier and the various keys required by the
    /// server for validation. The verifier type the database asked for is
    /// checked first, so a refused one fails before any value derived from
    /// the password is computed or sent.
    fn generate_verifier(&mut self, client: &mut Client) -> Result<(), Error> {
        let verifier_type = self
            .verifier_type
            .ok_or_else(|| Error::invalid_auth_response("AUTH_VFR_DATA"))?;
        let verifier = Verifier::from_flags(
            verifier_type,
            client.config().allow_legacy_verifiers(),
        )?;

        // create password hash
        let password = client.config().get_password_bytes();
        let mut password_key = None;
        let password_hash = match verifier {
            Verifier::Pbkdf2 => {
                let iterations =
                    self.session_iterations("AUTH_PBKDF2_VGEN_COUNT")?;
                let verifier_data = self.session_bytes("AUTH_VFR_DATA")?;
                let (key, hash) =
                    pbkdf2_password_hash(&password, &verifier_data, iterations);
                password_key = Some(key);
                hash
            }
            Verifier::Legacy11g => sha1_password_hash(
                &password,
                &self.session_bytes("AUTH_VFR_DATA")?,
            ),
            Verifier::Legacy10g => des_password_hash(
                client.config().user().ok_or_else(Error::no_credentials)?,
                std::str::from_utf8(&password)?,
            ),
        };

        // exchange the session keys and calculate the combo key from them
        let encoded_server_key = self.session_bytes("AUTH_SESSKEY")?;
        // The negotiated logon mode: a server that sends either PBKDF2 combo
        // field (the salt or the iteration count) has attempted custom hashing,
        // whatever verifier it serves (go-ora's logonCompatibility & 32 custom-
        // hash branch). Only when the server sends BOTH do we read them; when it
        // sends EXACTLY ONE the challenge is partial and the Pbkdf2 arm reads
        // both in order and names the missing field rather than falling to the
        // legacy MD5 path.
        let custom_hash = self
            .session_data
            .contains_key("AUTH_PBKDF2_CSK_SALT")
            || self
                .session_data
                .contains_key("AUTH_PBKDF2_SDER_COUNT");
        let combo_key = match verifier.combo_key_derivation(
            encoded_server_key.len(),
            custom_hash,
        )? {
            ComboKeyDerivation::Pbkdf2 => {
                let iterations =
                    self.session_iterations("AUTH_PBKDF2_SDER_COUNT")?;
                let salt = self.session_bytes("AUTH_PBKDF2_CSK_SALT")?;
                // The session keys a server sends are 32 or 48 bytes; the
                // PBKDF2 window and the derived key length follow the verifier.
                match encoded_server_key.len() {
                    32 => {
                        let (server_key, client_key) = self
                            .exchange_session_keys::<32>(
                                &password_hash,
                                &encoded_server_key,
                            )?;
                        pbkdf2_combo_key(
                            verifier,
                            &server_key,
                            &client_key,
                            &salt,
                            iterations,
                        )
                    }
                    48 => {
                        let (server_key, client_key) = self
                            .exchange_session_keys::<48>(
                                &password_hash,
                                &encoded_server_key,
                            )?;
                        pbkdf2_combo_key(
                            verifier,
                            &server_key,
                            &client_key,
                            &salt,
                            iterations,
                        )
                    }
                    _ => {
                        return Err(Error::invalid_auth_response("AUTH_SESSKEY"))
                    }
                }
            }
            ComboKeyDerivation::Md5 => {
                // The session keys a server sends are 32 or 48 bytes; the
                // verifier picks the MD5 variant (16 or 24 byte combo key).
                match encoded_server_key.len() {
                    32 => {
                        let (server_key, client_key) = self
                            .exchange_session_keys::<32>(
                                &password_hash,
                                &encoded_server_key,
                            )?;
                        md5_combo_key(verifier, &server_key, &client_key)
                    }
                    48 => {
                        let (server_key, client_key) = self
                            .exchange_session_keys::<48>(
                                &password_hash,
                                &encoded_server_key,
                            )?;
                        md5_combo_key(verifier, &server_key, &client_key)
                    }
                    _ => {
                        return Err(Error::invalid_auth_response("AUTH_SESSKEY"))
                    }
                }
            }
        };

        // generate speedy key
        if let Some(password_key) = password_key {
            self.generate_speedy_key(&password_key, &combo_key);
        }

        // encrypt password(s)
        self.encrypt_passwords(client, &combo_key);
        self.combo_key = Some(combo_key);
        Ok(())
    }

    /// Returns the alter session statement that is sent during the initial
    /// connection to the database. This includes the directive to change the
    /// session time zone to match the time zone in use by the system.
    fn get_alter_session_statement(&self) -> String {
        let tz_repr = if let Ok(value) = std::env::var("ORA_SDTZ")
            && !value.is_empty()
        {
            match value.to_uppercase().as_str() {
                "LOCAL" | "DBTIMEZONE" => value,
                _ => crate::enquote_literal(&value),
            }
        } else {
            let offset_seconds =
                chrono::Local::now().offset().local_minus_utc();
            let tz_hour = offset_seconds / 3600;
            let tz_minute = (offset_seconds - (tz_hour * 3600)) / 60;
            if tz_hour < 0 || tz_minute < 0 {
                format!("'-{:02}:{:02}'", tz_hour.abs(), tz_minute.abs())
            } else {
                format!("'+{:02}:{:02}'", tz_hour, tz_minute)
            }
        };
        format!("ALTER SESSION SET TIME_ZONE={}\0", tz_repr)
    }

    /// Returns the authorization mode to use when performing authentication.
    fn get_auth_mode(&self, client: &Client) -> u32 {
        let mut auth_mode: u32 = 0;
        if self.get_is_phase_one() {
            auth_mode |= constants::TTC_AUTH_MODE_LOGON;
        } else if self.external_auth {
            auth_mode |= constants::TTC_AUTH_MODE_LOGON;
            if client.iam_auth().is_some() {
                auth_mode |= constants::TTC_AUTH_MODE_IAM_TOKEN;
            }
        } else {
            auth_mode |= constants::TTC_AUTH_MODE_WITH_PASSWORD;
            if client.config().get_new_password_bytes().is_some() {
                auth_mode |= constants::TTC_AUTH_MODE_CHANGE_PASSWORD;
            } else {
                auth_mode |= constants::TTC_AUTH_MODE_LOGON;
            }
        }
        let user_auth_mode = client.config().auth_mode();
        if user_auth_mode & constants::AUTH_MODE_SYSASM != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSASM;
        }
        if user_auth_mode & constants::AUTH_MODE_SYSBKP != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSBKP;
        }
        if user_auth_mode & constants::AUTH_MODE_SYSDBA != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSDBA;
        }
        if user_auth_mode & constants::AUTH_MODE_SYSDGD != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSDGD;
        }
        if user_auth_mode & constants::AUTH_MODE_SYSKMT != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSKMT;
        }
        if user_auth_mode & constants::AUTH_MODE_SYSOPER != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSOPER;
        }
        if user_auth_mode & constants::AUTH_MODE_SYSRAC != 0 {
            auth_mode |= constants::TTC_AUTH_MODE_SYSRAC;
        }
        auth_mode
    }

    /// Returns whether phase one authorization is taking place or phase two
    /// authorization. Phase one is only used on logon and is skipped entirely
    /// when external authentication is in use, since the database has no
    /// password verifier to return.
    fn get_is_phase_one(&self) -> bool {
        self.combo_key.is_none()
            && self.session_data.is_empty()
            && !self.external_auth
    }

    /// Returns the TTC meessage type to use when sending the request to the
    /// database.
    fn get_ttc_message_type(&self) -> u8 {
        if self.get_is_phase_one() {
            constants::TTC_RPC_AUTH_PHASE_ONE
        } else {
            constants::TTC_RPC_AUTH_PHASE_TWO
        }
    }

    /// Writes a key/value pair to the network buffers.
    fn write_pair(&self, buf: &mut WriteBuffer, pair: &Pair) {
        let key_bytes = pair.key.as_bytes();
        let value_bytes = pair.value.as_bytes();
        buf.write_ub4(key_bytes.len().try_into().unwrap());
        buf.write_bytes_with_length(key_bytes);
        buf.write_ub4(value_bytes.len().try_into().unwrap());
        if !value_bytes.is_empty() {
            buf.write_bytes_with_length(value_bytes);
        }
        buf.write_ub4(pair.flags);
    }

    /// Returns the domain of the database.
    pub(crate) fn get_db_domain(&self) -> String {
        if let Some(value) = self.session_data.get("AUTH_SC_DB_DOMAIN") {
            value.to_string()
        } else {
            String::new()
        }
    }

    /// Returns the name of the database.
    pub(crate) fn get_db_name(&self) -> String {
        if let Some(value) = self.session_data.get("AUTH_SC_DBUNIQUE_NAME") {
            value.to_string()
        } else {
            String::new()
        }
    }

    /// Returns the instance name used to connect to the database.
    pub(crate) fn get_instance_name(&self) -> String {
        if let Some(value) = self.session_data.get("AUTH_INSTANCENAME") {
            value.to_string()
        } else {
            String::new()
        }
    }

    /// Returns the maximum number of bytes allowed to be used in identifiers.
    pub(crate) fn get_max_identifier_length(&self) -> Result<usize, Error> {
        if self.session_data.contains_key("AUTH_MAX_IDEN_LENGTH") {
            self.session_number("AUTH_MAX_IDEN_LENGTH")
        } else {
            Ok(30)
        }
    }

    /// Returns the maximum number of open cursors allowed by the database.
    pub(crate) fn get_max_open_cursors(&self) -> Result<usize, Error> {
        if self.session_data.contains_key("AUTH_MAX_OPEN_CURSORS") {
            self.session_number("AUTH_MAX_OPEN_CURSORS")
        } else {
            Ok(0)
        }
    }

    /// Returns the serial number associated with the connection to the
    /// database.
    pub(crate) fn get_serial_num(&self) -> Result<usize, Error> {
        self.session_number("AUTH_SERIAL_NUM")
    }

    /// Returns the server version.
    pub(crate) fn get_server_version(
        &self,
        client: &Client,
    ) -> Result<OracleVersion, Error> {
        let full_version_num: usize = self.session_number("AUTH_VERSION_NO")?;
        Ok(if client.supports_ttc_field_version(
            constants::TTC_FIELD_VERSION_18_1_EXT_1,
        ) {
            OracleVersion(
                (full_version_num >> 24) & 0xff,
                (full_version_num >> 16) & 0xff,
                (full_version_num >> 12) & 0x0f,
                (full_version_num >> 4) & 0xff,
                full_version_num & 0x0f,
            )
        } else {
            OracleVersion(
                (full_version_num >> 24) & 0xff,
                (full_version_num >> 20) & 0x0f,
                (full_version_num >> 12) & 0x0f,
                (full_version_num >> 8) & 0x0f,
                full_version_num & 0x0f,
            )
        })
    }

    /// Returns the service name used to connect to the database.
    pub(crate) fn get_service_name(&self) -> String {
        if let Some(value) = self.session_data.get("AUTH_SC_SERVICE_NAME") {
            value.to_string()
        } else {
            String::new()
        }
    }

    /// Returns the session id associated with the connection to the database.
    pub(crate) fn get_session_id(&self) -> Result<usize, Error> {
        self.session_number("AUTH_SESSION_ID")
    }

    /// Creates a new auth message.
    pub(crate) fn new() -> AuthMessage {
        AuthMessage {
            session_data: HashMap::new(),
            pairs: Vec::<Pair>::new(),
            combo_key: None,
            external_auth: false,
            resend_needed: false,
            verifier_type: None,
        }
    }

    /// Sets the combo key to use for performing authentication.
    pub(crate) fn set_combo_key(&mut self, combo_key: &LoginKey) {
        self.combo_key = Some(*combo_key);
    }

    /// Takes the combo key from the auth message so it can be stored
    /// elsewhere.
    pub(crate) fn take_combo_key(&mut self) -> Option<LoginKey> {
        self.combo_key.take()
    }
}

impl Message for AuthMessage {
    fn deserialize_return_parameters(
        &mut self,
        _client: &Client,
        resp: &mut Response,
    ) -> Result<(), Error> {
        let num_params = resp.read_ub2()?;
        for _ in 0..num_params {
            let key = resp.read_utf8_with_double_length()?.to_string();
            let value = resp.read_utf8_with_double_length()?.to_string();
            let flags = resp.read_ub4()?;
            // the flags of AUTH_VFR_DATA name the password verifier type
            if key == "AUTH_VFR_DATA" {
                self.verifier_type = Some(flags);
            }
            self.session_data.insert(key, value);
        }
        Ok(())
    }

    fn pre_process(&mut self, client: &mut Client) -> Result<(), Error> {
        self.pairs.clear();
        self.external_auth = client.config().uses_external_auth();
        if self.get_is_phase_one() {
            self.add_pair("AUTH_TERMINAL", client.config().terminal(), 0);
            self.add_pair("AUTH_PROGRAM_NM", client.config().program(), 0);
            self.add_pair("AUTH_MACHINE", client.config().machine(), 0);
            self.add_pair("AUTH_PID", &std::process::id().to_string(), 0);
            self.add_pair("AUTH_SID", client.config().osuser(), 0);
            self.resend_needed = true;
        } else if let Some(combo_key) = self.combo_key {
            self.encrypt_passwords(client, &combo_key);
        } else {
            if let Some(token) = client.config().get_token() {
                self.add_pair("AUTH_TOKEN", &token, 0);
            } else {
                self.generate_verifier(client)?;
            }
            self.add_pair(
                "SESSION_CLIENT_CHARSET",
                &constants::CHARSET_ID_UTF8.to_string(),
                0,
            );
            self.add_pair(
                "SESSION_CLIENT_DRIVER_NAME",
                client.config().driver_name(),
                0,
            );
            let major =
                env!("CARGO_PKG_VERSION_MAJOR").parse::<usize>().unwrap();
            let minor =
                env!("CARGO_PKG_VERSION_MINOR").parse::<usize>().unwrap();
            let patch =
                env!("CARGO_PKG_VERSION_PATCH").parse::<usize>().unwrap();
            let full_version_num = major << 24 | minor << 20 | patch << 12;
            self.add_pair(
                "SESSION_CLIENT_VERSION",
                &full_version_num.to_string(),
                0,
            );
            self.add_pair(
                "AUTH_ALTER_SESSION",
                &self.get_alter_session_statement(),
                1,
            );
            if let Some(cclass) = client.config().cclass() {
                self.add_pair("AUTH_KPPL_CONN_CLASS", cclass, 0);
            }
            if let Some((header, signature)) = client.iam_auth() {
                self.add_pair("AUTH_HEADER", header, 0);
                self.add_pair("AUTH_SIGNATURE", signature, 0);
            }
            self.resend_needed = false;
        }
        Ok(())
    }

    fn resend_needed(&self) -> bool {
        self.resend_needed
    }

    fn serialize(&self, client: &Client, buf: &mut WriteBuffer) {
        buf.write_function_header(client, self.get_ttc_message_type());
        let user_bytes = client.config().user().unwrap_or_default().as_bytes();
        let has_user = !user_bytes.is_empty();
        buf.write_u8(has_user.into()); // pointer (user)
        buf.write_ub4(user_bytes.len().try_into().unwrap());
        buf.write_ub4(self.get_auth_mode(client));
        buf.write_u8(1); // pointer (authivl)
        buf.write_ub4(self.pairs.len().try_into().unwrap());
        buf.write_u8(1); // pointer (authovl)
        buf.write_u8(1); // pointer (authovln)
        if has_user {
            buf.write_bytes_with_length(user_bytes);
        }
        for pair in self.pairs.iter() {
            self.write_pair(buf, pair);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;

    fn bytes(hex: &str) -> Vec<u8> {
        base16ct::upper::decode_vec(hex).unwrap()
    }

    fn key_bytes(key: &LoginKey) -> Vec<u8> {
        let mut key = *key;
        key.as_mut_bytes().to_vec()
    }

    fn refused_type(result: Result<Verifier, Error>) -> u32 {
        match result.unwrap_err().kind() {
            ErrorKind::PasswordVerifierRefused(verifier_type) => *verifier_type,
            kind => panic!("expected PasswordVerifierRefused, got {kind:?}"),
        }
    }

    // Oracle's 10G hashes of default accounts, as published in default
    // password lists: SCOTT/TIGER is F894844C34402B67 and SYSTEM/MANAGER is
    // D4DF7931AB130E37. Both recomputed on the side with LibreSSL 3.3.6:
    // `openssl enc -des-cbc -nopad -iv 0000000000000000`, under the key
    // 0123456789ABCDEF and then under the last block of that pass, over the
    // UTF-16BE text of SCOTTTIGER (or SYSTEMMANAGER) padded with zeros.
    #[test]
    fn the_10g_verifier_is_the_published_des_hash() {
        assert_eq!(
            des_verifier("SCOTT", "TIGER").to_vec(),
            bytes("F894844C34402B67")
        );
        assert_eq!(
            des_verifier("SYSTEM", "MANAGER").to_vec(),
            bytes("D4DF7931AB130E37")
        );
    }

    #[test]
    fn the_10g_verifier_ignores_the_case_of_user_and_password() {
        assert_eq!(des_verifier("scott", "tiger"), des_verifier("SCOTT", "TIGER"));
        assert_eq!(
            key_bytes(&des_password_hash("Scott", "Tiger")),
            bytes("F894844C34402B670000000000000000")
        );
    }

    // hashcat's published example hash for mode 112, "Oracle S: Type
    // (Oracle 11+)", which is SHA-1 over the password and the salt:
    // ac5f1e62d21fd0529428b84d42e8955b04966703 with the salt
    // 38445748184477378130 for the password "hashcat". Recomputed on the
    // side with Python 3 `hashlib.sha1(b"hashcat" + salt).hexdigest()`.
    #[test]
    fn the_11g_password_hash_is_sha1_of_password_and_salt() {
        let hash =
            sha1_password_hash(b"hashcat", &bytes("38445748184477378130"));
        assert_eq!(
            key_bytes(&hash),
            bytes("AC5F1E62D21FD0529428B84D42E8955B0496670300000000")
        );
    }

    // Computed with Python 3 hashlib:
    // pk = hashlib.pbkdf2_hmac("sha512", b"tiger",
    //     vfr + b"AUTH_PBKDF2_SPEEDY_KEY", 4096, 64)
    // hashlib.sha512(pk + vfr).digest()[:32]
    // with vfr = A1B2C3D4E5F60718293A4B5C6D7E8F90.
    #[test]
    fn the_12c_password_hash_is_pbkdf2_then_sha512() {
        let (password_key, hash) = pbkdf2_password_hash(
            b"tiger",
            &bytes("A1B2C3D4E5F60718293A4B5C6D7E8F90"),
            4096,
        );
        assert_eq!(
            password_key.to_vec(),
            bytes(
                "FC38EE411C0D55C36219B018DBED798B5B70515699E9B724053FE02E24AC0720\
                 4DE7FD2E4120BDD8F14655C9DE851F334C9FF24E70D37F1DC30545F4FE03E2C9"
            )
        );
        assert_eq!(
            key_bytes(&hash),
            bytes(
                "56F513B0BD502AF6A056AF6A7427A7FB247FB28BA83F861205E5B6F1E5E701DF"
            )
        );
    }

    // Computed with Python 3 hashlib, for n in 16, 24, 32:
    // temp = (client[:n] + server[:n]).hex().upper().encode()
    // hashlib.pbkdf2_hmac("sha512", temp, salt, 3, n)
    // with server = bytes(range(0x00, 0x20)),
    // client = bytes(range(0x20, 0x40)) and
    // salt = 0F1E2D3C4B5A69788796A5B4C3D2E1F0.
    // For a 10G login the window is client[:len/2] + server[:len/2] with a 32
    // byte session key, so a custom-hash 10G login reaches this too.
    #[test]
    fn the_pbkdf2_combo_key_has_the_verifier_s_key_length() {
        let server_key: [u8; 32] = std::array::from_fn(|i| i as u8);
        let client_key: [u8; 32] = std::array::from_fn(|i| 0x20 + i as u8);
        let salt = bytes("0F1E2D3C4B5A69788796A5B4C3D2E1F0");
        let combo_key = |verifier| {
            key_bytes(&pbkdf2_combo_key(
                verifier,
                &server_key,
                &client_key,
                &salt,
                3,
            ))
        };
        assert_eq!(
            combo_key(Verifier::Legacy10g),
            bytes("7354FACE7838731D624750690CE14EA0")
        );
        assert_eq!(
            combo_key(Verifier::Legacy11g),
            bytes("0C465732B9E15E6C78FF48D99318ACFC18D8E73234D6C8D1")
        );
        assert_eq!(
            combo_key(Verifier::Pbkdf2),
            bytes(
                "3F225E6EE15DE8C7DBD2544177587CB8A477531D449AC7BDD47CA9B2E99666E6"
            )
        );
    }

    // Computed with Python 3 hashlib:
    // x = bytes(a ^ b for a, b in zip(server[16:40], client[16:40]))
    // (hashlib.md5(x[:16]).digest() + hashlib.md5(x[16:]).digest())[:24]
    // with server = bytes(range(0x00, 0x30)) and
    // client = bytes(range(0x80, 0xb0)).
    #[test]
    fn the_md5_combo_key_of_48_byte_session_keys() {
        let server_key: [u8; 48] = std::array::from_fn(|i| i as u8);
        let client_key: [u8; 48] = std::array::from_fn(|i| 0x80 + i as u8);
        assert_eq!(
            key_bytes(&md5_combo_key(Verifier::Legacy11g, &server_key, &client_key)),
            bytes("324C5183D4096C99A3D737B4522F21B2D0B32013C743BC99")
        );
    }

    // Computed with Python 3 hashlib:
    // x = bytes(a ^ b for a, b in zip(server[16:32], client[16:32]))
    // hashlib.md5(x).digest()
    // with server = bytes(range(0x00, 0x20)) and
    // client = bytes([0x80 - i for i in range(0x20)]).
    #[test]
    fn the_10g_combo_key_is_md5_of_the_xor_of_session_key_bytes() {
        let server_key: [u8; 32] = std::array::from_fn(|i| i as u8);
        let client_key: [u8; 32] = std::array::from_fn(|i| 0x80u8 - i as u8);
        assert_eq!(
            key_bytes(&md5_combo_key(
                Verifier::Legacy10g,
                &server_key,
                &client_key,
            )),
            bytes("3FDDC6C5B58046D477165B04ADF45474")
        );
    }

    // Regresses the PR 1856 P1: the combo-key derivation follows the
    // negotiated logon mode, not the verifier alone. A custom-hash 10G
    // challenge derives the PBKDF2 16 byte key (go-ora custom-hash case
    // 2361), a legacy-MD5 10G challenge the MD5 16 byte key (go-ora
    // non-customHash case 2361). Each vector is independently derived.
    #[test]
    fn a_10g_login_derives_the_combo_key_by_the_negotiated_mode() {
        let salt = bytes("0F1E2D3C4B5A69788796A5B4C3D2E1F0");
        // Custom-hash PBKDF2: server 0x00..0x1f, client 0x20..0x3f.
        let server_key_p: [u8; 32] = std::array::from_fn(|i| i as u8);
        let client_key_p: [u8; 32] =
            std::array::from_fn(|i| 0x20 + i as u8);
        assert_eq!(
            key_bytes(&pbkdf2_combo_key(
                Verifier::Legacy10g,
                &server_key_p,
                &client_key_p,
                &salt,
                3,
            )),
            bytes("7354FACE7838731D624750690CE14EA0")
        );
        // Legacy MD5: server 0x00..0x1f, client 0x80..0x61.
        let server_key_m: [u8; 32] = std::array::from_fn(|i| i as u8);
        let client_key_m: [u8; 32] =
            std::array::from_fn(|i| 0x80u8 - i as u8);
        assert_eq!(
            key_bytes(&md5_combo_key(
                Verifier::Legacy10g,
                &server_key_m,
                &client_key_m,
            )),
            bytes("3FDDC6C5B58046D477165B04ADF45474")
        );
        // The dispatch picks the derivation by the negotiated mode.
        assert_eq!(
            Verifier::Legacy10g.combo_key_derivation(32, true).unwrap(),
            ComboKeyDerivation::Pbkdf2
        );
        assert_eq!(
            Verifier::Legacy10g.combo_key_derivation(32, false).unwrap(),
            ComboKeyDerivation::Md5
        );
    }

    // FIPS 197 appendix C.1, C.2 and C.3: the plaintext
    // 00112233445566778899AABBCCDDEEFF under the keys 000102... of 16, 24
    // and 32 bytes. One block in CBC mode with a zero IV is that block in
    // ECB mode; recomputed on the side with LibreSSL 3.3.6
    // `openssl enc -aes-{128,192,256}-cbc -nopad -iv 0...0`.
    #[test]
    fn a_login_key_encrypts_with_aes_of_its_length() {
        let plain_text = bytes("00112233445566778899AABBCCDDEEFF");
        let cases = [
            (
                LoginKey::Aes128(std::array::from_fn(|i| i as u8)),
                "69C4E0D86A7B0430D8CDB78070B4C55A",
            ),
            (
                LoginKey::Aes192(std::array::from_fn(|i| i as u8)),
                "DDA97CA4864CDFE06EAF70A0EC0D7191",
            ),
            (
                LoginKey::Aes256(std::array::from_fn(|i| i as u8)),
                "8EA2B7CA516745BFEAFC49904B496089",
            ),
        ];
        for (key, expected) in cases {
            let mut encrypted_text = [0u8; 16];
            key.encrypt(&plain_text, &mut encrypted_text);
            assert_eq!(encrypted_text.to_vec(), bytes(expected));
            let mut decrypted_text = [0u8; 16];
            key.decrypt(&encrypted_text, &mut decrypted_text).unwrap();
            assert_eq!(decrypted_text.to_vec(), plain_text);
        }
    }

    #[test]
    fn a_session_key_that_is_not_whole_blocks_is_an_invalid_response() {
        let key = LoginKey::Aes256([0; 32]);
        let mut plain_text = [0u8; 32];
        let err = key.decrypt(&[0u8; 31], &mut plain_text).unwrap_err();
        assert_eq!(
            err.kind(),
            &ErrorKind::InvalidAuthResponse("AUTH_SESSKEY".to_string())
        );
    }

    #[test]
    fn a_legacy_verifier_is_refused_unless_allowed() {
        for flags in [
            constants::VERIFIER_TYPE_10G,
            constants::VERIFIER_TYPE_11G_1,
            constants::VERIFIER_TYPE_11G_2,
        ] {
            assert_eq!(refused_type(Verifier::from_flags(flags, false)), flags);
        }
        assert_eq!(
            Verifier::from_flags(constants::VERIFIER_TYPE_10G, true).unwrap(),
            Verifier::Legacy10g
        );
        assert_eq!(
            Verifier::from_flags(constants::VERIFIER_TYPE_11G_1, true).unwrap(),
            Verifier::Legacy11g
        );
        assert_eq!(
            Verifier::from_flags(constants::VERIFIER_TYPE_11G_2, true).unwrap(),
            Verifier::Legacy11g
        );
    }

    #[test]
    fn the_12c_verifier_needs_no_opt_in() {
        assert_eq!(
            Verifier::from_flags(constants::VERIFIER_TYPE_12C, false).unwrap(),
            Verifier::Pbkdf2
        );
    }

    #[test]
    fn an_unknown_verifier_is_refused_with_or_without_the_opt_in() {
        assert_eq!(refused_type(Verifier::from_flags(0x1234, false)), 0x1234);
        assert_eq!(refused_type(Verifier::from_flags(0x1234, true)), 0x1234);
    }

    #[test]
    fn the_refusal_names_the_verifier_type() {
        let message = |flags| {
            Verifier::from_flags(flags, false).unwrap_err().to_string()
        };
        assert_eq!(
            message(constants::VERIFIER_TYPE_10G),
            "password verifier type 0x939 (10G) is refused: legacy password \
             verifiers are not allowed"
        );
        assert_eq!(
            message(constants::VERIFIER_TYPE_11G_2),
            "password verifier type 0x1b25 (11G) is refused: legacy password \
             verifiers are not allowed"
        );
        assert_eq!(
            message(0x1234),
            "password verifier type 0x1234 is refused: the driver does not \
             support it"
        );
    }

    #[test]
    fn a_legacy_login_takes_the_derivation_of_its_negotiated_mode() {
        // Custom-hash mode (the server sent AUTH_PBKDF2_CSK_SALT and
        // AUTH_PBKDF2_SDER_COUNT): every verifier uses PBKDF2.
        for verifier in
            [Verifier::Legacy10g, Verifier::Legacy11g, Verifier::Pbkdf2]
        {
            assert_eq!(
                verifier.combo_key_derivation(32, true).unwrap(),
                ComboKeyDerivation::Pbkdf2
            );
            assert_eq!(
                verifier.combo_key_derivation(48, true).unwrap(),
                ComboKeyDerivation::Pbkdf2
            );
        }
        // Legacy mode (no custom hash): a 10G login MD5s 32 or 48 byte session
        // keys; an 11G login MD5s only 48 byte ones; a 12C login still uses
        // PBKDF2.
        assert_eq!(
            Verifier::Legacy10g.combo_key_derivation(32, false).unwrap(),
            ComboKeyDerivation::Md5
        );
        assert_eq!(
            Verifier::Legacy10g.combo_key_derivation(48, false).unwrap(),
            ComboKeyDerivation::Md5
        );
        assert_eq!(
            Verifier::Legacy11g.combo_key_derivation(48, false).unwrap(),
            ComboKeyDerivation::Md5
        );
        assert!(Verifier::Legacy11g.combo_key_derivation(32, false).is_err());
        // A 12C verifier is always a PBKDF2 verifier, so it derives the combo
        // key with PBKDF2 even without the custom-hash signal; this lets a
        // malformed 12C session name the missing PBKDF2 field.
        assert_eq!(
            Verifier::Pbkdf2.combo_key_derivation(32, false).unwrap(),
            ComboKeyDerivation::Pbkdf2
        );
        assert_eq!(
            Verifier::Pbkdf2.combo_key_derivation(48, false).unwrap(),
            ComboKeyDerivation::Pbkdf2
        );
        // Every verifier validates the session key length in the dispatch, so
        // a malformed AUTH_SESSKEY names itself before any per-field read.
        for verifier in
            [Verifier::Legacy10g, Verifier::Legacy11g, Verifier::Pbkdf2]
        {
            for len in [0, 16, 64] {
                assert!(verifier.combo_key_derivation(len, false).is_err());
            }
        }
    }

}
