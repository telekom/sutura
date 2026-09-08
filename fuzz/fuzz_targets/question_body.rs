//! A caller's question body: arbitrary bytes as the JSON both transports accept.
//!
//! **The boundary.** `QuestionBody` is what an authenticated caller PUTs on the query path, and
//! `Query::try_from` is where its strings become the domain's parsed types - `MetricName`,
//! `DimensionName`, `DimensionValue`, `Date`, and the grain table. `serde_json` runs first, over
//! bytes, with `deny_unknown_fields`. Authentication bounds *who* reaches this, not *what* they
//! send, so every byte here is still caller-controlled.
//!
//! **What is asserted beyond "did not abort".** That a body which becomes a `Query` produces a
//! half-open range whose start is not after its end - the invariant every dialect's rendering
//! assumes, and the one an inverted range would push into generated SQL.
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
