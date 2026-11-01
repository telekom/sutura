//! A question refused because a node it was planned across was lost, as the wire carries it.

use axum::http::StatusCode;
use sutura_domain::query::RefusalReason;

use super::{refused, retry_after};

#[test]
fn a_lost_node_is_a_503_worth_a_retry_with_no_invented_header() {
    let reason = RefusalReason::NodeLost;
    let (status, body) = refused(&reason);
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body.code(), "node_lost");
    assert_eq!(body.status(), StatusCode::SERVICE_UNAVAILABLE.as_u16());
    assert_eq!(retry_after(&reason), None);
}
