//! `operator.workers`/`.serving` through `OperatorSettings::load`, refused when set to something
//! other than a lowercase RFC 1123 subdomain. A separate file from `at_load`'s, so a causality run
//! that reverts `operator.rs` (which declares both `DeploymentName` and this module) also reverts
//! this file to an orphan, rather than holding it at a shape the base tree cannot build.

use super::{InvalidDeploymentName, OperatorSettings, OperatorSettingsError};
use crate::environment::Environment;
use crate::settings::Sources;

#[test]
fn a_worker_or_serving_name_that_is_not_a_lowercase_subdomain_is_refused_at_load() {
    let workers =
        OperatorSettings::load(&Sources::defaults(Environment::Production).with_overlay("operator:\n  workers: Not_Valid\n"));
    assert!(
        matches!(
            workers,
            Err(OperatorSettingsError::Workers {
                cause: InvalidDeploymentName::NotSubdomain { .. }
            })
        ),
        "{workers:?}"
    );
    let serving =
        OperatorSettings::load(&Sources::defaults(Environment::Production).with_overlay("operator:\n  serving: Not_Valid\n"));
    assert!(
        matches!(
            serving,
            Err(OperatorSettingsError::Serving {
                cause: InvalidDeploymentName::NotSubdomain { .. }
            })
        ),
        "{serving:?}"
    );
}
