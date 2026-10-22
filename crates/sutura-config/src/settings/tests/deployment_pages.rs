//! The configurations that `docs/serving.md` and `docs/serving-behind-a-gateway.md` print, loaded.
//!
//! The gateway page's values fence is read from the page. The published-port table is a copy, so a
//! change to the table in `serving.md` is not caught here. Neither opens a key set or a TLS file.

use std::path::Path;

use serde_norway::Value;

use super::{METRICS_TOKEN, TOKEN, variables};
use crate::inbound::TokenLocation;
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

/// The table of the `a-published-container-port` section of `docs/serving.md`: the wildcard bind, a credential, a
/// metrics token and the limiter, with `terminator` and its `material` under `server:`.
fn published_port(terminator: &str, material: &str) -> String {
    format!(
        "server:\n  host: \"0.0.0.0\"\n{material}security:\n  access_token: \"{TOKEN}\"\n  \
         metrics_token: \"{METRICS_TOKEN}\"\n  tls_termination: \"{terminator}\"\nrate_limit:\n  enabled: true\n"
    )
}

/// What `sources` is refused with, or nothing where it serves.
fn refusals(sources: &Sources) -> Vec<NotFitToServe> {
    Settings::load(sources).err().map_or_else(Vec::new, |error| {
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a posture refusal, got {error:?}");
        };
        refusals.clone()
    })
}

#[test]
fn a_published_port_is_the_off_host_shape_with_the_terminator_declared() {
    for environment in [Environment::Development, Environment::Test, Environment::Production] {
        let sources =
            |terminator: &str, material: &str| Sources::defaults(environment).with_overlay(published_port(terminator, material));
        let in_process = "  tls_certificate: \"/tls/chain.pem\"\n  tls_key: \"/tls/key.pem\"\n";
        let declared = [("sidecar", ""), ("ingress", ""), ("in-process", in_process)];
        for (terminator, material) in declared
            .into_iter()
            .filter(|&(terminator, _)| terminator != "in-process" || cfg!(feature = "tls"))
        {
            let settings = Settings::load(&sources(terminator, material))
                .unwrap_or_else(|error| panic!("{environment}, {terminator}: {error:?}"));
            assert!(!settings.server().bind().is_loopback());
        }
        assert!(
            refusals(&sources("none", ""))
                .iter()
                .any(|refusal| matches!(*refusal, NotFitToServe::TlsTerminationUndeclared { .. })),
            "{environment}: a published port that declares no terminator is refused"
        );
    }
}

/// The values file the gateway page prints: its one `yaml` fence.
fn gateway_values() -> Value {
    let page = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/serving-behind-a-gateway.md"))
        .expect("the gateway page reads");
    let fence = page
        .split("```yaml\n")
        .nth(1)
        .and_then(|rest| rest.split("\n```").next())
        .expect("the gateway page prints a values fence");
    serde_norway::from_str(fence).expect("the fence is YAML")
}

#[test]
fn the_values_the_gateway_page_prints_start_a_deployment_that_reads_authorization() {
    let values = gateway_values();
    let environment = Environment::parse(values["environment"].as_str().expect("an environment")).expect("a known environment");
    let file = serde_norway::to_string(&values["config"]["environment"]).expect("the settings file");
    let terminator = values["security"]["tlsTermination"].as_str().expect("a declared terminator");
    // What the chart adds around `config.environment`: the wildcard bind, the declared terminator,
    // the metrics token and the path of the mounted key set.
    let sources = |extra: &[(&str, &str)]| {
        let mut pairs = vec![
            ("SUTURA__SERVER__HOST", "0.0.0.0"),
            ("SUTURA__SECURITY__TLS_TERMINATION", terminator),
            ("SUTURA__SECURITY__METRICS_TOKEN", METRICS_TOKEN),
            (
                "SUTURA__SECURITY__INBOUND__KEY_SET_FILE",
                "/etc/sutura/inbound-keys/jwks.json",
            ),
        ];
        pairs.extend_from_slice(extra);
        Sources::defaults(environment)
            .with_overlay(file.clone())
            .with_variables(variables(&pairs))
    };

    let settings = Settings::load(&sources(&[])).expect("the printed values start the binary");
    let inbound = settings.security().inbound().expect("the values declare an inbound identity");
    assert_eq!(inbound.mode(), "behind-gateway");
    assert!(
        matches!(inbound.requirement().location(), TokenLocation::GatewayBearer),
        "the gateway signs into `Authorization`"
    );
    assert_eq!(
        refusals(&sources(&[("SUTURA__SECURITY__ACCESS_TOKEN", TOKEN)])),
        vec![NotFitToServe::DeploymentTokenSharesTheHeader],
        "an access token beside it shares the header"
    );
}
