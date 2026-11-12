use super::{RouterNotBuilt, every_route_governed, governed_routes};

/// The check passes for the routes this crate actually mounts.
#[test]
fn every_route_this_crate_mounts_names_a_capability() {
    assert!(governed_routes().is_ok(), "{:?}", governed_routes().err());
}

/// A document carrying one route under one method, for the two tests below.
fn document_with(route: &str, method: utoipa::openapi::HttpMethod) -> utoipa::openapi::OpenApi {
    let mut document = utoipa::openapi::OpenApi::new(
        utoipa::openapi::InfoBuilder::new().title("test").version("0").build(),
        utoipa::openapi::Paths::new(),
    );
    drop(document.paths.paths.insert(
        String::from(route),
        utoipa::openapi::PathItem::new(method, utoipa::openapi::path::OperationBuilder::new().build()),
    ));
    document
}

/// **And the check is not vacuous.** A route nobody mapped is refused, and the error names it.
///
/// This is the half that matters: the test above passes for a check that returns `Ok` without
/// looking at anything. Here a document carrying a route the table does not name has to fail, and
/// the failure has to name what was not governed - because the error is what tells whoever added
/// the route what to do about it.
#[test]
fn a_route_that_names_no_capability_assembles_no_router() {
    let refused = every_route_governed(&document_with("/v1/rogue", utoipa::openapi::HttpMethod::Post))
        .expect_err("an ungoverned route is refused");
    let RouterNotBuilt::RouteNotGoverned { method, route } = refused else {
        panic!("expected RouteNotGoverned, got {refused:?}");
    };
    assert_eq!(method, "POST");
    assert_eq!(route, "/v1/rogue");
}

/// A method nobody expected on a route that IS governed is still refused.
///
/// The fail-closed direction, and the reason the method list in [`every_route_governed`] is
/// written out in full: `crate::capability::governed` keys on method as well as path, so a
/// `DELETE` mounted by accident on the question route does not inherit its capability.
#[test]
fn a_governed_route_under_an_unexpected_method_is_refused_too() {
    let refused = every_route_governed(&document_with("/v1/query", utoipa::openapi::HttpMethod::Delete))
        .expect_err("an unexpected method is refused");
    assert!(
        matches!(refused, RouterNotBuilt::RouteNotGoverned { ref method, .. } if method == "DELETE"),
        "{refused:?}"
    );
}

/// A mounted agent surface is refused at assembly by the same predicate `Settings::refusals`
/// asks, even where `server.agent_surface.enabled` was never set - and assembles on a
/// single-user loopback deployment with no inbound identity.
///
/// The fake service below never runs: this asserts on the assembly decision, ahead of any request.
#[cfg(feature = "agent")]
#[test]
fn a_mounted_agent_surface_is_refused_where_the_deployment_may_not_serve_it_without_leg_one() {
    let mounted = |overlay: &str| {
        let service = crate::surface::LocalService::start(
            &crate::testing::catalog_of(crate::testing::bundle()),
            crate::testing::fake_warehouse(),
            crate::testing::sink(),
            crate::testing::broker(),
            sutura_domain::plan::RefusingCombiner,
            1 << 30,
        )
        .expect("the test bundle validates");
        let state = crate::testing::state_over(std::sync::Arc::new(service), crate::testing::settings_with(overlay));
        // No spend ceiling is configured, so `ServiceState::new` registered no
        // `sutura_spend_headroom_bytes` and this declaration is the true one.
        let mount = crate::state::AgentMount::new(
            tower::service_fn(|request: axum::http::Request<axum::body::Body>| {
                let _request = request;
                async { Ok::<_, std::convert::Infallible>(axum::response::Response::new(axum::body::Body::empty())) }
            }),
            crate::state::SpendHeadroomPush::NoCeilingConfigured,
        );
        let state = state.with_agent_surface(mount);
        super::agent_subtree(
            &state,
            state.agent_surface().first().expect("the attached mount"),
            None,
            None,
            None,
        )
        .map(|_mount| true)
    };
    let single_user = "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator\"\n";
    // The switch never set, so `Settings::load` had nothing to refuse: no mode declared, then
    // `multi-user`.
    for overlay in ["", "security:\n  identity: \"multi-user\"\n"] {
        let refused = mounted(overlay).expect_err("this mount assembles nothing");
        assert!(
            refused
                .to_string()
                .contains(&sutura_config::NotFitToServe::AgentSurfaceWithoutInboundIdentity.to_string()),
            "the operator reads which refusal stopped the mount: {refused}"
        );
        let RouterNotBuilt::AgentSurfaceNotFitToServe { refusals } = refused else {
            panic!("expected the agent-surface refusal, got {refused:?}");
        };
        assert_eq!(
            refusals,
            vec![sutura_config::NotFitToServe::AgentSurfaceWithoutInboundIdentity],
            "{overlay}"
        );
    }
    let assembled = mounted(single_user).expect("a single-user loopback deployment serves /mcp without leg 1");
    assert!(assembled, "the mount is handed back for merging");
}

/// The ungoverned-route allowlist is not vacuous: a merged route with no `ungoverned_routes()`
/// row is refused by name, and the legitimate `/mcp` row passes.
#[cfg(feature = "agent")]
#[test]
fn a_second_ungoverned_route_with_no_allowlist_entry_is_refused() {
    assert!(
        super::check_ungoverned(&[crate::constants::AGENT_MOUNT_PATH], &[]).is_ok(),
        "the allowlisted agent mount passes"
    );
    let refused = super::check_ungoverned(&[crate::constants::AGENT_MOUNT_PATH, "/v1/rogue"], &[])
        .expect_err("a route neither governed nor allowlisted is refused");
    let RouterNotBuilt::UngovernedRouteNotAllowlisted { path } = refused else {
        panic!("expected UngovernedRouteNotAllowlisted, got {refused:?}");
    };
    assert_eq!(path, "/v1/rogue");
}

#[cfg(feature = "agent")]
#[test]
fn duplicate_or_undeclared_agent_mounts_are_refused_before_merging() {
    fn mount(path: &str) -> crate::AgentMount {
        crate::AgentMount::at(
            tower::service_fn(|_request: axum::http::Request<axum::body::Body>| async {
                Ok::<_, std::convert::Infallible>(axum::response::Response::new(axum::body::Body::empty()))
            }),
            crate::SpendHeadroomPush::NoCeilingConfigured,
            sutura_config::McpPath::parse(path).expect("a literal fixture path"),
        )
    }
    let state = crate::testing::state_over(
        std::sync::Arc::new(
            crate::surface::LocalService::start(
                &crate::testing::catalog_of(crate::testing::bundle()),
                crate::testing::fake_warehouse(),
                crate::testing::sink(),
                crate::testing::broker(),
                sutura_domain::plan::RefusingCombiner,
                1 << 30,
            )
            .expect("the fixture service starts"),
        ),
        crate::testing::settings_with("security:\n  identity: single-user\n  single_user_because: fixture\n"),
    );
    let duplicate = state
        .clone()
        .with_agent_surface(mount("/mcp"))
        .with_agent_surface(mount("/mcp"));
    let assembled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::assemble(&duplicate)));
    assert!(matches!(assembled, Ok(Err(RouterNotBuilt::DuplicateAgentMount { path })) if path.as_str() == "/mcp"));
    for path in ["/rogue", "/arrow.flight.protocol.FlightService"] {
        let undeclared = state.clone().with_agent_surface(mount(path));
        assert!(
            matches!(super::assemble(&undeclared), Err(RouterNotBuilt::UngovernedRouteNotAllowlisted { path: refused }) if refused == path)
        );
    }
}
