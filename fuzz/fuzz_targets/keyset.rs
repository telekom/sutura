//! `KeySet::parse` over arbitrary bytes.
//!
//! **The boundary.** A JWK set document is not written by this deployment: it is fetched from an
//! authorization server, or read from a file a platform put there. `KeySet::parse` runs
//! `serde_json`, then `jsonwebtoken`'s JWK decode, then base64 over key material, then
//! `KeyId::parse` - four parsers, none of them first-party, none of them reached by the
//! workspace's panic lint table.
//!
//! **What is asserted beyond "did not abort".** `KeySet::parse`'s own documentation says every
//! refusal is a refusal to start rather than a key that gets skipped, so a set that parses holds at
//! least one key and reports as many ids as it counts. A parse that returned an empty set would
//! authenticate nobody while reporting success, which is the failure that reads as an outage.
//!
//! **The limit.** Bytes that are not UTF-8 are converted lossily, because the function takes
//! `&str`; the caller in `keys.rs` reads a file to a `String` and so refuses non-UTF-8 one layer
//! out. This target therefore does not cover that refusal.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sutura_http::inbound::keys::KeySet;

fuzz_target!(|data: &[u8]| {
    let document = String::from_utf8_lossy(data);
    if let Ok(set) = KeySet::parse(&document) {
        assert!(set.count() > 0, "a parsed key set with no key would authenticate nobody");
        assert_eq!(set.ids().len(), set.count(), "the ids and the count are the same set");
    }
});
