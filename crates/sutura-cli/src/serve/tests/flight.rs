//! The Flight surface with the `flight` feature OFF: the refusal `agent.rs` beside it holds for
//! `/mcp`, for the Flight transport. Only `check-default-feature-tests` runs this file.

#[test]
fn an_enabled_flight_surface_is_refused_by_a_build_without_the_feature() {
    // `direct` inbound, so `NotFitToServe::FlightSurfaceWithoutDirectInbound` lets these settings
    // load and the only refusal left is the build's own.
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(
            "server:\n  flight_surface:\n    enabled: true\nsecurity:\n  inbound:\n    mode: \"direct\"\n    \
         resource: \"https://sutura.example.com\"\n    authorization_server: \"https://idp.example.com\"\n    \
         key_set_file: \"/nonexistent/keys.json\"\n    algorithms: [\"RS256\"]\n",
        ),
    )
    .expect("an enabled Flight surface behind a direct inbound identity is fit to serve");
    let refused = super::super::not_linked::flight_refused_if_enabled(&settings)
        .expect_err("an enabled Flight surface on a build without the feature is a startup refusal");
    assert!(
        refused.contains("--features flight"),
        "the refusal must name the feature that would mount it: {refused}"
    );
}
