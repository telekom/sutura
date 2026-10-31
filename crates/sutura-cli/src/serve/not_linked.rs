//! A build missing a feature, with the key that needs it set anyway: every such refusal, one file.
//!
//! Moved out of `serve.rs` by the thousand-line limit `cargo xtask max-lines` enforces. Each
//! function here only compiles for the build it refuses - `check-default-feature-tests` is the
//! cell that runs it, since `just test`'s `--all-features` run never sees any body here.

#[cfg(any(not(feature = "agent"), not(feature = "flight")))]
use sutura_config::Settings;

/// A build without the `agent` feature: an enabled switch is a startup refusal.
///
/// The feature-off twin of `super::agent::mount` and of `super::serve_as_configured`'s no-`tls`
/// body - the "configured for a build it does not have" refusal. Clearing this function (or its
/// call in `run`) is exactly the regression
/// `crate::tests::agent::an_enabled_agent_surface_is_refused_by_a_build_without_the_feature`
/// exists to hold.
#[cfg(not(feature = "agent"))]
pub(super) fn agent_refused_if_enabled(settings: &Settings) -> Result<(), String> {
    if settings.server().agent_surface_enabled() {
        return Err(String::from(
            "server.agent_surface.enabled is set and this binary was built without the `agent` \
             feature, so it has no agent surface to mount. Rebuild with `--features agent`, or \
             remove the key.",
        ));
    }
    Ok(())
}

/// A build without the `flight` feature: an enabled switch is a startup refusal.
///
/// The feature-off twin of `sutura_http::flight::mount` and of `agent_refused_if_enabled` above it.
/// `crate::serve::tests::flight::an_enabled_flight_surface_is_refused_by_a_build_without_the_feature`
/// holds it.
#[cfg(not(feature = "flight"))]
pub(super) fn flight_refused_if_enabled(settings: &Settings) -> Result<(), String> {
    if settings.server().flight_surface_enabled() {
        return Err(String::from(
            "server.flight_surface.enabled is set and this binary was built without the `flight` \
             feature, so it has no Flight transport to mount. Rebuild with `--features flight`, or \
             remove the key.",
        ));
    }
    Ok(())
}

/// A build without the `distributed` feature: declared nodes are a startup refusal, not a combine
/// that quietly runs on one node. `crate::serve::tests::nodes` holds it.
#[cfg(not(feature = "distributed"))]
pub(super) fn nodes_refused_if_declared(tls: Option<&sutura_config::TlsMaterial>) -> Result<(), String> {
    if tls.and_then(sutura_config::TlsMaterial::nodes).is_some() {
        return Err(String::from(
            "server.nodes is set and this binary was built without the `distributed` feature, so \
             it cannot plan a combine across nodes. Rebuild with `--features distributed`, or \
             remove the key.",
        ));
    }
    Ok(())
}
