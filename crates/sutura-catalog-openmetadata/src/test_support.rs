//! The happy-path `OpenMetadata` wire pages this crate's own tests need.
//!
//! Over the loopback fake `sutura-http-client::test_support` now hosts (issue #970's review: this
//! file's own `FakeServer`/`Scripted`/plumbing was byte-for-byte identical to
//! `sutura-catalog-datahub`'s copy, `cargo xtask check-jscpd` measured). Re-exported here so
//! `sutura-cli`'s served-binary suite (`crates/sutura-cli/tests/served/openmetadata.rs`) keeps
//! building `test_support::FakeServer` off THIS crate's public API - it takes
//! `sutura-catalog-openmetadata` as a dependency, not `sutura-http-client` directly.
//!
//! `#[cfg(feature = "http")]`, not `#[cfg(test)]`, for the reason `sutura_http_client::test_support`'s
//! own header gives: an integration test binary cannot see another crate's `tests/` directory, so
//! the only way to share a fake across crates is through a library, `pub`, reachable at compile
//! time from whichever feature both a reader and its composition root's tests turn on.

#![expect(
    clippy::expect_used,
    clippy::too_long_first_doc_paragraph,
    reason = "test: the page builders below carry the same long-form fixture prose this crate's \
              other modules do, and read the crate's own recorded corpus - a test invariant \
              rather than a production path."
)]

pub use sutura_http_client::test_support::{CapturedAuthorizations, FakeServer, Scripted};

use crate::SnapshotReader as _;

/// The `sources.<alias>` the happy-path tables sit on, matching the recorded fixture corpus's own
/// `service: "warehouse"` so the two transports serve one content.
///
/// A fixed test constant, independent of a deployment's own choice, the way `sutura-catalog-datahub`
///'s `DEPLOYMENT_PROPERTY` is: a fake carrying the adapter's own constant would pass whether the
/// service were the deployment's choice or a value this crate required.
pub const SERVICE: &str = "warehouse";

/// One `tables` page, over the two models the crate's recorded fixture carries (`orders` and
/// `customers`), each with its columns, data types, and descriptions. Served in the shape a REAL
/// `OpenMetadata` list endpoint answers: a `data` array with a `paging` block whose `total` matches
/// what was returned (so the reader does not refuse it as truncated).
#[must_use]
pub fn tables_page() -> serde_json::Value {
    serde_json::json!({
        "data": [
            {
                "name": "orders",
                "fullyQualifiedName": "warehouse.sales.orders",
                "description": "Net revenue orders, in minor units.",
                "columns": [
                    {"name": "order_id", "dataType": "STRING", "constraint": "PRIMARY_KEY", "description": "The order's own identifier."},
                    {"name": "customer_id", "dataType": "STRING"},
                    {"name": "amount_cents", "dataType": "BIGINT", "description": "The order total, in minor units."},
                    {"name": "order_date", "dataType": "DATE"},
                    {"name": "status", "dataType": "STRING"},
                ],
                "tableConstraints": [
                    {
                        "constraintType": "FOREIGN_KEY",
                        "columns": ["customer_id"],
                        "referredColumns": ["warehouse.sales.customers.customer_id"],
                        "relationshipType": "ONE_TO_MANY",
                    }
                ],
            },
            {
                "name": "customers",
                "fullyQualifiedName": "warehouse.sales.customers",
                "description": "The customer dimension.",
                "columns": [
                    {"name": "customer_id", "dataType": "STRING"},
                    {"name": "segment", "dataType": "STRING"},
                ],
            },
        ],
        "paging": {"total": 2, "after": null},
    })
}

/// One `metrics` page carrying the recorded fixture's OWN reported-not-defined metric, so the two
/// transports cannot drift - it is read through the crate's public fixture rather than restated.
#[must_use]
pub fn metrics_page() -> serde_json::Value {
    let recorded = crate::fixture::FixtureReader.read().expect("the recorded fixture reads");
    let metric = recorded.metrics().first().expect("the fixture carries one metric");
    serde_json::json!({
        "data": [
            {
                "name": metric.name(),
                "metricType": metric.aggregation(),
                "granularity": metric.granularity(),
                "metricExpression": { "language": "SQL", "code": metric.expression() },
            }
        ],
        "paging": {"total": 1, "after": null},
    })
}

/// The two pages a `read()` call makes, in order, all answering `200` - what a real `OpenMetadata`
/// carrying exactly the recorded fixture's content would serve.
#[must_use]
pub fn happy_path_answers() -> Vec<Scripted> {
    vec![Scripted::ok(&tables_page()), Scripted::ok(&metrics_page())]
}
