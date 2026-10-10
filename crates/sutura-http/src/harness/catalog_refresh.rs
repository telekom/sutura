//! `GET /v1/catalog` over a service whose bundle is swapped by a refresh.
//!
//! `crate::wire`'s tests pin what a bundle renders as; this pins WHICH bundle the route reads. A
//! handler that read the bundle once at construction, or a state that cached it, passes every other
//! cell here and serves the boot-time catalog forever.

use std::sync::Arc;

use axum::body::Body;
use axum::http::StatusCode;
use sutura_app::surface::Adopted;
use sutura_config::Environment;

use super::settings;
use crate::surface::{LocalService, Surface};
use crate::testing::{broker, bundle, call, catalog_of, described_bundle, fake_warehouse, request, sink, state_over};

#[tokio::test]
async fn the_catalog_route_reads_the_bundle_being_served_not_the_one_it_started_with() {
    let service = Arc::new(
        LocalService::start(
            &catalog_of(bundle()),
            fake_warehouse(),
            sink(),
            broker(),
            sutura_domain::plan::RefusingCombiner,
            1 << 30,
        )
        .expect("the test bundle validates"),
    );
    let state = state_over(
        Arc::clone(&service) as Arc<dyn Surface>,
        settings(Environment::Development, ""),
    );
    let app = crate::router(&state).expect("the test router assembles");

    let (status, before) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(before.contains("Revenue, in minor units."), "{before}");

    let adopted = service
        .adopt(described_bundle("Revenue, restated."))
        .expect("a bundle with the same anchor validates");
    assert!(matches!(adopted, Adopted::Rotated { .. }), "{adopted:?}");

    let (status, after) = call(&app, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(after.contains("Revenue, restated."), "{after}");
    assert!(!after.contains("Revenue, in minor units."), "{after}");
}
