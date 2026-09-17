//! A data system's reply: arbitrary text as the JSON `sutura-exec-bigquery` decodes a `jobs.query`
//! answer.
//!
//! **The boundary.** A reply is foreign text on its way to an agent: `BigQuery` owns the document,
//! and `sutura_exec_bigquery::wire::document::decode_answer` is where its bytes become this
//! repository's typed [`JobRows`]. `serde_json` runs first over the transport text, then the shape checks refuse
//! an incomplete job, a page of a larger result, a missing or non-numeric total, a column type
//! outside [`FieldType`]'s closed vocabulary, and a cell that is neither text nor null. There is no
//! `deny_unknown_fields`, because the endpoint's document will grow fields and an answer that grew
//! one is a correct answer - the one deliberate exception to this repository's rule over documents
//! it owns. Every byte here is service-controlled, and a panic in `serde` or a domain parse is the
//! process dying under `panic = "abort"`.
//!
//! **What is asserted beyond "did not abort".** Nothing further, and that is the point rather than
//! an omission: the whole decode is the finding. A reply is foreign text the adapter reads once and
//! maps to a certified row set, so a panic anywhere in that mapping - `serde_json`, a domain
//! newtype, the reason-code translation - is the same process death the other five targets measure.
//!
//! **The limit.** This is the answer document, not the HTTP layer: the transport's body-size cap,
//! header cap and status handling are all in front of it and are not exercised here. A REFUSAL
//! (`status` non-`2xx`) never reaches this decode in the adapter - `refusal` short-circuits before
//! `serde_json` - so the refusal envelope is not this boundary either. The non-UTF-8 rejection is
//! `read_to_string`'s, one layer out, and matches how the harness discards invalid bytes below.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sutura_exec_bigquery::wire::document::decode_answer;

fuzz_target!(|data: &[u8]| {
    // The reply arrives as transport text read with `read_to_string`, which refuses invalid UTF-8
    // one layer before the decode - so the adapter's `serde_json` sees only valid UTF-8, and this
    // harness reflects that: invalid bytes never reach the parser under fuzz either.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // A parse refusal or a shape refusal is an answer this adapter rejects - both are fine, and the
    // refusal path itself is part of what is being driven. The finding is a panic anywhere in the
    // decode.
    drop(decode_answer(text));
});
