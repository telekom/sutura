//! `prompt.catalog_prose`, through the assembled router.
//!
//! `crate::wire`'s own tests pin what `CatalogBody::of` renders for each setting. This one pins the
//! WIRING, which is the half `#128` actually left broken: the body was built by a
//! `From<&PinnedDefinitions>` that could not see the settings, so an operator's
//! `catalog_prose: omitted` was honoured by the prompt and by the MCP tool and silently ignored
//! here. A test over the body alone passes with the handler still calling a conversion that
//! defaults the setting, which is why this one goes through the real router.

use axum::body::Body;
use axum::http::StatusCode;
use sutura_config::Environment;

use super::{app, settings};
use crate::testing::{call, request};

#[tokio::test]
async fn the_catalog_route_honours_the_prose_setting_it_was_started_with() {
    // Both directions, in one test, because either alone is satisfiable by a handler that ignores
    // the setting: quoted-and-absent would pass if it always omitted, omitted-and-present if it
    // always shipped.
    let omitted = app(settings(Environment::Development, "prompt:\n  catalog_prose: omitted\n"));
    let (status, body) = call(&omitted, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    // Neither description reaches the caller. Two different strings, so the dimension's half of this
    // cannot pass on the metric's - see `crate::testing`'s default fixture.
    assert!(!body.contains("Revenue, in minor units."), "{body}");
    assert!(!body.contains("Sales region."), "{body}");
    assert!(body.contains(r#""catalog_prose":"omitted""#), "{body}");
    // And the caller can still form a valid question, which is what makes the omission a narrowing
    // of what an agent is TOLD rather than of what it may ask.
    assert!(body.contains("revenue"), "{body}");
    assert!(body.contains("north"), "{body}");

    let quoted = app(settings(Environment::Development, ""));
    let (status, body) = call(&quoted, request("GET", "/v1/catalog", None, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Revenue, in minor units."), "{body}");
    assert!(body.contains("Sales region."), "{body}");
    assert!(body.contains(r#""catalog_prose":"quoted""#), "{body}");
}
