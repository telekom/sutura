//! A question refused because no other node was ready in time, as the wire carries it.

use axum::http::StatusCode;
use sutura_domain::query::RefusalReason;

use super::{refused, retry_after};

#[test]
fn no_other_node_ready_in_time_is_a_503_worth_a_retry_with_no_invented_header() {
    // 503, not a 4xx: nothing in the question is wrong, and the same question can succeed once a
    // node is ready. No `Retry-After`, because when one will be ready is not something this
    // deployment knows.
    let reason = RefusalReason::WorkersNotReady { wanted: 3, ready: 0 };
    let (status, body) = refused(&reason);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body.code(), "workers_not_ready");
    assert_eq!(body.status(), StatusCode::SERVICE_UNAVAILABLE.as_u16());
    let detail = body.detail();
    assert!(
        detail.contains("needed 3 other nodes") && detail.contains("0 were ready"),
        "the sentence does not name both counts: {detail}"
    );
    assert_eq!(retry_after(&reason), None);
}
