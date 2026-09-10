//! A caller's question body: arbitrary bytes as the JSON both transports accept.
//!
//! **The boundary.** [`QuestionBody`] is what an authenticated caller sends on the query path, and
//! `sutura_domain::query::Query::try_from` is where its strings become the domain's parsed types -
//! `MetricName`, `DimensionName`, `DimensionValue`, `Date`, `TimeRange` and the grain table.
//! `serde_json` runs first, over bytes, with `deny_unknown_fields`. Authentication bounds *who*
//! reaches this, not *what* they send, so every byte here is still caller-controlled: the
//! transport parses caller JSON before or after verifying the caller, and a panic inside `serde`
//! or a domain newtype is the process dying under `panic = "abort"`.
//!
//! **What is asserted beyond "did not abort".** That a body which becomes a `Query` produces a
//! half-open range whose start is not after its end - the invariant every SQL render assumes, and
//! the one an inverted range would push into generated SQL. This is a semantic property no
//! example-based test states, and it is exactly the class of finding fuzzing exists for.
//!
//! **The limit.** This is the body shape, not the HTTP layer: axum's extractor, the body-size cap
//! and the rate limiter are all in front of it and are not exercised here. A panic found by this
//! target is reachable through both transports; a panic in the extractor would not be found by it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sutura_domain::query::Query;
use sutura_http::wire::QuestionBody;

fuzz_target!(|data: &[u8]| {
    let Ok(body) = serde_json::from_slice::<QuestionBody>(data) else {
        return;
    };
    if let Ok(query) = Query::try_from(body) {
        let range = query.range();
        assert!(
            range.start() <= range.end(),
            "a question that parsed carried an inverted half-open range"
        );
    }
});
