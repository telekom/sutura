//! Settings `/v1` may be served under, and the one check that says so.
//!
//! [`crate::ServiceState::new`] takes a [`ServableSettings`] rather than bare settings in a build
//! without this crate's `fixtures` feature, so an embedder cannot assemble [`crate::router()`] over a
//! deployment with no `security.inbound` that `Settings::query_surface_refusals` refuses: the type
//! is the guard, and there is no assembly-time refusal to forget.

use std::sync::Arc;

use sutura_config::{NotFitToServe, Settings};

/// Settings that passed [`ServableSettings::check`].
///
/// **Only the check can build one.** The field is private, so construction from outside this crate
/// is `E0423`:
///
/// ```compile_fail,E0423
/// fn _unchecked(settings: std::sync::Arc<sutura_config::Settings>) -> sutura_http::ServableSettings {
///     sutura_http::ServableSettings(settings)
/// }
/// ```
///
/// The compiling twin, so the failure above is the private field and not an unresolved path:
///
/// ```
/// fn _reachable(_: sutura_http::ServableSettings) {}
/// ```
///
/// The limit: a deployment that declares `security.inbound` passes without the rule being asked,
/// because the rule is about the deployment that does not. That deployment's gate is held at
/// assembly instead, by `RouterNotBuilt::InboundIdentityNotAttached`.
#[derive(Clone, Debug)]
pub struct ServableSettings(Arc<Settings>);

impl ServableSettings {
    /// Accepts `settings` that declare `security.inbound`, or whose deployment may serve `/v1`
    /// without it by `Settings::query_surface_refusals`; refuses every other one with each reason.
    ///
    /// # Errors
    ///
    /// [`NotServable`] carrying every refusal, not just the first.
    pub fn check(settings: Settings) -> Result<Self, NotServable> {
        if settings.security().inbound().is_none() {
            let refusals = settings.query_surface_refusals();
            if !refusals.is_empty() {
                return Err(NotServable { refusals });
            }
        }
        Ok(Self(Arc::new(settings)))
    }

    pub(crate) fn into_shared(self) -> Arc<Settings> {
        self.0
    }
}

/// Bare settings no check was asked of, only in this crate's own tests or a build with its
/// `fixtures` feature, so a fixture can drive the router over a deployment shape the rule refuses.
#[cfg(any(test, feature = "fixtures"))]
impl From<Arc<Settings>> for ServableSettings {
    fn from(settings: Arc<Settings>) -> Self {
        Self(settings)
    }
}

/// Why [`ServableSettings::check`] refused.
#[derive(Debug, thiserror::Error)]
#[error(
    "this configuration is not fit to serve /v1:\n  - {}",
    refusals.iter().map(ToString::to_string).collect::<Vec<String>>().join("\n  - ")
)]
pub struct NotServable {
    refusals: Vec<NotFitToServe>,
}

impl NotServable {
    /// Every reason, in the order `Settings::query_surface_refusals` gave them.
    #[must_use]
    pub fn refusals(&self) -> &[NotFitToServe] {
        &self.refusals
    }
}

#[cfg(test)]
mod tests {
    use sutura_config::NotFitToServe;

    use super::ServableSettings;
    use crate::testing::settings_with;

    #[test]
    fn a_deployment_with_no_inbound_identity_and_no_impersonating_source_is_servable_only_when_single_user() {
        for overlay in ["security:\n  identity: \"multi-user\"\n", ""] {
            let refused = ServableSettings::check(settings_with(overlay)).expect_err("no inbound identity, not single-user");
            assert!(
                matches!(refused.refusals(), [NotFitToServe::QuerySurfaceWithoutInboundIdentity]),
                "{refused}"
            );
        }
        let single_user = "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator\"\n";
        ServableSettings::check(settings_with(single_user)).expect("a single-user deployment is servable");
    }

    #[test]
    fn a_deployment_that_declares_inbound_identity_is_servable_without_the_rule() {
        let declared = "security:\n  identity: \"multi-user\"\n  inbound:\n    mode: \"behind-gateway\"\n    \
                        transit_header: \"X-Transit-Proof\"\n    transit_issuer: \"https://issuer.example.com\"\n    \
                        transit_audience: \"https://sutura.example.com\"\n    key_set_file: \"/unread.json\"\n    \
                        algorithms: [\"ES256\"]\n    transit_token_type: \"at+jwt\"\n";
        ServableSettings::check(settings_with(declared)).expect("an inbound declaration is servable");
    }

    /// A source that runs each question as the asking subject has no subject to run as without an
    /// inbound identity, so single-user does not make the deployment servable, and a multi-user one
    /// carries both reasons.
    #[test]
    fn a_deployment_with_no_inbound_identity_over_an_impersonating_source_is_not_servable() {
        let source = "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
                      posture: \"impersonation-at-source\"\n    workload_identity:\n      audience: \
                      \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n";
        let single_user = format!("security:\n  identity: \"single-user\"\n  single_user_because: \"one operator\"\n{source}");
        let refused =
            ServableSettings::check(settings_with(&single_user)).expect_err("single-user does not serve an impersonating source");
        assert!(
            matches!(
                refused.refusals(),
                [NotFitToServe::QuerySurfaceOverAnImpersonatingSource { .. }]
            ),
            "{refused}"
        );
        let multi_user = format!("security:\n  identity: \"multi-user\"\n{source}");
        let refused = ServableSettings::check(settings_with(&multi_user)).expect_err("multi-user does not serve it either");
        assert!(
            matches!(
                refused.refusals(),
                [
                    NotFitToServe::QuerySurfaceWithoutInboundIdentity,
                    NotFitToServe::QuerySurfaceOverAnImpersonatingSource { .. }
                ]
            ),
            "{refused}"
        );
    }
}
