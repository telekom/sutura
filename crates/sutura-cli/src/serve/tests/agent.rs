//! The agent surface behind leg 1, with the `agent` feature OFF: the one switch on the default
//! build whose "configured for a build it does not have" refusal must fire at startup. No transport
//! is named here - the feature that links it is compiled out, so the only reachable surface is the
//! assembly-time refusal in `super::super::agent_refused_if_enabled`.

#[test]
fn an_enabled_agent_surface_is_refused_by_a_build_without_the_feature() {
    // **The agent mirror of the bigquery no-feature cell in `tests.rs`**: the one switch on the
    // default build that must refuse at startup rather than silently serve no `/mcp` - the same
    // "configured for a build it does not have" refusal `serve_as_configured` gives for TLS.
    // Clearing `agent_refused_if_enabled`'s refusal body (or its call in `run`) reddens this cell.
    let settings = sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development)
            .with_overlay("server:\n  agent_surface:\n    enabled: true\n"),
    )
    .expect("an enabled agent surface parses");
    let refused = super::super::agent_refused_if_enabled(&settings)
        .expect_err("an enabled agent surface on a build without the feature is a startup refusal");
    assert!(
        refused.contains("--features agent"),
        "the refusal must name the feature that would mount it: {refused}"
    );
}
