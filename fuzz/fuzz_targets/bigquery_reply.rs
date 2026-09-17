//! A BigQuery job reply: arbitrary bytes through the real wire-reply parser.
//!
//! **The boundary.** This is item 1 of #146, the one target class the original five did not
//! cover: a data SYSTEM's reply, not a file an operator placed or a caller's request body. The
//! endpoint is `bigquery.googleapis.com`, not this deployment, and its response is deserialized on
//! the way to an agent - `BigQueryWire::submit` (`crates/sutura-exec-bigquery/src/wire.rs`) reads
//! the HTTP body into a `String` and hands it to `wire::document::parse`, mapping a failure to
//! `WireError::NotADocument`. This target drives `wire::document::deserializes`, a `pub` wrapper
//! one call deep over that same `parse` - see its own doc comment for why the wrapper exists rather
//! than widening `QueryAnswer` itself.
//!
//! **The real parser and not a copy of it.** `parse` is one line, `serde_json::from_str::<QueryAnswer>`,
//! and `deserializes` calls that function rather than re-typing the shape - the same reason
//! `catalog_document.rs` drives `LocalCatalog::load` and `token.rs` drives `validator.verify`
//! instead of a local reimplementation.
//!
//! **What is asserted beyond "did not abort".** Nothing else: `QueryAnswer` stays `pub(super)` to
//! this crate, so there is no accessor on the fuzzed value for this target to assert a property
//! over the way `key_set_document.rs` counts keys. The finding this target exists to make is a
//! panic reachable through `serde_json`'s own deserializer or `serde`'s derived `Deserialize` impl
//! for this shape - `panic = "abort"` on every shipped profile, so a panic here is the endpoint's
//! own reply text taking down the process that asked it a question.
//!
//! **The limit.** Deserialization only. The HTTP layer that produced this body (status handling,
//! `MAX_ANSWER_BYTES`, transport errors), the token-exchange/credential path that authorized the
//! request, and every other adapter's own reply shape are out of scope - a fuzzed `QueryAnswer`
//! never reaches `document::complete`, `estimated_bytes` or `cells`, all of which run only after a
//! caller has already decided the job is complete, which this target's input never claims to be
//! from outside the type's own private fields.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sutura_exec_bigquery::wire::document::deserializes;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = deserializes(&text);
});
