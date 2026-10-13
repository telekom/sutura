//! `/v1` with no `security.inbound`: the two refusals `Settings::query_surface_refusals` returns,
//! and the deployments that start `/v1` without leg 1.
//!
//! A file of its own because `settings/tests.rs` is near the thousand-line limit `cargo xtask
//! max-lines` enforces.

use sutura_domain::model::SourceName;

use super::inbound::DIRECT_INBOUND;
use crate::settings::{Environment, NotFitToServe, Settings, Sources};

const SINGLE_USER: &str = "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator, their own files\"\n";

/// `local` runs each question as the asking subject.
const IMPERSONATING: &str = "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    \
     posture: \"impersonation-at-source\"\n    workload_identity:\n      \
     audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n";

/// Why the deployment an overlay loads to may not start `/v1`.
fn refusals(overlay: &str) -> Vec<NotFitToServe> {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
        .expect("the test settings load")
        .query_surface_refusals()
}

#[test]
fn the_v1_refusals_name_what_is_missing_and_vanish_with_leg_1() {
    for overlay in ["security:\n  identity: \"multi-user\"\n", ""] {
        assert_eq!(
            refusals(overlay),
            vec![NotFitToServe::QuerySurfaceWithoutInboundIdentity],
            "{overlay:?}"
        );
    }
    let alias = SourceName::parse("local").expect("a legal identifier");
    assert_eq!(
        refusals(&format!("{SINGLE_USER}{IMPERSONATING}")),
        vec![NotFitToServe::QuerySurfaceOverAnImpersonatingSource { alias }]
    );
    assert_eq!(refusals(&format!("{SINGLE_USER}{DIRECT_INBOUND}{IMPERSONATING}")), Vec::new());
    assert_eq!(refusals(SINGLE_USER), Vec::new());
}
