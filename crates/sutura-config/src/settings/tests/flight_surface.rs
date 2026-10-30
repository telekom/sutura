//! `server.flight_surface.enabled` with no `direct` inbound identity: the one boot refusal
//! `Settings::flight_surface_refusals` returns, and the deployment that serves it.
//!
//! A file of its own because `settings/tests.rs` is at the thousand-line limit `cargo xtask
//! max-lines` enforces. Same reason as `agent_surface` beside it: leg 1 verifies the Flight
//! surface's own caller, the exchanged token's subject, so the predicate is `direct` inbound
//! identity rather than `security.identity`.

use super::inbound::DIRECT_INBOUND;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

const FLIGHT: &str = "server:\n  flight_surface:\n    enabled: true\n";

/// The settings an overlay loads to.
fn serves(overlay: &str) -> Settings {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay)).expect("this deployment is fit to serve")
}

/// The posture refusals an overlay is refused with.
fn refused(overlay: &str) -> Vec<NotFitToServe> {
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect_err("this deployment is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    refusals.clone()
}

#[test]
fn a_flight_surface_with_no_direct_inbound_identity_is_not_fit_to_serve() {
    let refusals = refused(FLIGHT);
    assert_eq!(refusals, vec![NotFitToServe::FlightSurfaceWithoutDirectInbound]);
    let rendered = refusals.first().expect("one refusal").to_string();
    for key in ["server.flight_surface.enabled", "direct"] {
        assert!(rendered.contains(key), "{key} missing from: {rendered}");
    }
    // Leg 1 declared in `direct` mode, the same deployment serves.
    serves(&format!("{FLIGHT}security:\n{DIRECT_INBOUND}"));
}

#[test]
fn a_flight_surface_off_by_default_needs_no_inbound_identity() {
    let settings = serves("");
    assert!(!settings.server().flight_surface_enabled());
    // `Settings::refusals` gates the predicate on the key; asked directly, it still refuses.
    assert_eq!(
        settings.flight_surface_refusals(),
        vec![NotFitToServe::FlightSurfaceWithoutDirectInbound]
    );
}
