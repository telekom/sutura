//! `KeySet::parse` over arbitrary bytes.
//!
//! **The boundary.** A JWK set document is not written by this deployment: it is read from a
//! local file an operator placed (`keys/source.rs`'s own header states there is no HTTPS fetcher
//! shipped), so the document is only operator-adjacent, not caller-reachable - but it is one of the
//! two places in this codebase where nobody proves who supplied the bytes before they are parsed.
//! `KeySet::parse` runs `serde_json`, then `jsonwebtoken`'s JWK decode, then base64 over key
//! material, then `KeyId::parse` - four parsers, none of them first-party, none of them reached by
//! the workspace's panic lint table (`unwrap_used`/`expect_used`/`panic` are `deny` for first-party
//! code only).
//!
//! **What is asserted beyond "did not abort".** `KeySet::parse`'s own documentation says every
//! refusal is a refusal to start rather than a key that gets skipped, so a set that parses holds at
//! least one key and reports as many ids as it counts. A parse that returned an empty `Ok` set
//! would authenticate nobody while reporting success - the availability failure this target is
//! built to catch, mirroring the framing the merged `token.rs` target (#648) used for a successful
//! verification without a private key.
//!
//! **The limit.** Bytes that are not UTF-8 are converted lossily, because the function takes
//! `&str`; the caller in `keys.rs` reads a file to a `String` and so refuses non-UTF-8 one layer
//! out (`FileKeySet::read`'s own `read_to_string`). This target therefore does not cover that
//! refusal.
//!
//! **Provenance.** Ported from `origin/test/fuzz-the-untrusted-parsers:fuzz/fuzz_targets/keyset.rs`
//! (119 commits behind `main`, no open PR) as a content port rather than a rebase. `KeySet::parse`,
//! `.count()` and `.ids()` are unchanged on current `main`.

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
