//! The `sources:` tree at the DEPLOYMENT level: the shared-identity acknowledgement, the mode
//! refusal, more than one source, and the one key no source may ever carry.
//!
//! A file of its own because `settings/tests.rs` reached the thousand-line limit `cargo xtask
//! max-lines` enforces. The seam is the one `crate::sources` already has: this crate's PARSE
//! refusals for one entry live in `crate::sources::tests`, and these are the refusals that depend on
//! the tree AS A WHOLE - the declared mode, and more than one source at once.

use sutura_domain::model::SourceName;

use super::inbound::{DIRECT_INBOUND, GATEWAY_INBOUND};
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

/// One source entry, so a test changes exactly one thing about it.
///
/// The path is absolute because a relative one is refused at parse - that refusal has its own test in
/// [`crate::sources`] - and this helper exists to reach the *deployment-level* refusals underneath it.
fn source_overlay(posture: &str, extra: &str) -> String {
    let workload = if posture == "impersonation-at-source" {
        "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n"
    } else {
        ""
    };
    format!(
        "sources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/data\"\n    posture: \"{posture}\"\n{workload}{extra}"
    )
}

#[test]
fn a_shared_source_in_a_multi_user_deployment_without_an_acknowledgement_is_not_fit_to_serve() {
    // THE refusal the shared posture exists to be caught by. The wrong outcome here is not a failure,
    // it is an answer: rows a caller's own permissions never filtered, served under a certified metric
    // name. Sutura declares no data sensitivity so it cannot tell whether that was fine; what it can do
    // is make the posture impossible to arrive at by accident and impossible to arrive at in silence.
    //
    // Asserted through `Settings::refusals` as well as through `Settings::load`, because that method is
    // public and side-effect-free precisely so a test can - and because the LIST is the contract: a
    // deployment with three unacknowledged sources should learn about all three on one restart.
    let sources = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n{}",
        source_overlay("shared-service-user", "")
    ));
    let error = Settings::load(&sources).expect_err("a shared source nobody acknowledged is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::SharedSourceNotAcknowledged {
            alias: SourceName::parse("local").expect("a legal identifier")
        }]
    );
    // The message names the entry to change and says there is no global acknowledgement, because an
    // operator who reads "acknowledge it" and writes one key at the top of the file has not.
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("sources.local.acknowledged_because"), "{rendered}");
    assert!(rendered.contains("no global acknowledgement"), "{rendered}");

    // And with the acknowledgement on that source's own entry it serves - so the refusal is not
    // satisfied by refusing every shared source.
    let acknowledged = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n{}",
        source_overlay(
            "shared-service-user",
            "    acknowledged_because: \"a read-only reporting replica every caller may see\"\n"
        )
    ));
    let settings = Settings::load(&acknowledged).expect("an acknowledged shared source is servable");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());
    assert_eq!(
        settings
            .sources()
            .get(&sutura_domain::model::SourceName::parse("local").expect("a name"))
            .and_then(crate::sources::ConfiguredSource::posture)
            .map(sutura_domain::source::SourcePosture::as_str),
        Some("shared-service-user")
    );
}

#[test]
fn a_single_user_deployment_needs_no_per_source_acknowledgement_and_nor_does_an_impersonating_source() {
    // The other two arms of the same rule, so the refusal above cannot be a check that fires on every
    // configured source.
    //
    // Single-user is a FIRST-CLASS deployment: the one user reads all, by design, and the configured
    // credential is that user's own - so the mode's own declaration is the acknowledgement, and
    // `examples/single-player` does not have to restate the obvious per source.
    let single = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one operator, their own files\"\n{}",
        source_overlay("shared-service-user", "")
    ));
    let settings = Settings::load(&single).expect("single-user mode supplies the witness");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());

    // And an impersonating source is never the subject of this refusal: there is no shared identity to
    // acknowledge.
    let impersonating = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"multi-user\"\n{}",
        source_overlay("impersonation-at-source", "")
    ));
    let settings = Settings::load(&impersonating).expect("an impersonating source needs no acknowledgement");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());
}

#[test]
fn a_deployment_that_configures_a_source_and_declares_no_mode_does_not_start() {
    // The mode has no default, and this is the refusal that makes that true rather than a sentence in a
    // comment. It is keyed on a source being configured rather than raised unconditionally - the
    // embedded defaults have to keep loading, and a deployment with no source cannot answer anything
    // anyway - so the assertion below has two halves: refused with a source, and silent without one.
    let sources = Sources::defaults(Environment::Development).with_overlay(source_overlay("impersonation-at-source", ""));
    let error = Settings::load(&sources).expect_err("a configured source with no declared mode does not start");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(*refusals, vec![NotFitToServe::DeploymentIdentityUndeclared { count: 1 }]);
    // The message says why no combination of source postures may answer it, because "derive it" is the
    // first thing anybody proposes here and it is unsound in the configuration that most needs it.
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("all shared"), "{rendered}");

    // With no source configured there is nothing for a mode to govern, so the defaults still load -
    // which is what keeps every other test in this file, and the `prompt` command, reading a real tree.
    let defaults = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert!(defaults.sources().is_empty());
    assert!(defaults.security().identity().is_none());
    assert!(defaults.refusals().is_empty(), "the development defaults refuse nothing");
}

#[test]
fn two_sources_can_be_configured_and_each_says_what_it_is() {
    // The Done-when of the source registry, from the configuration side: more than one source, each
    // declaring its own posture, each with its own file location.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"multi-user\"\nsources:\n  \
         local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/local\"\n    posture: \"shared-service-user\"\n    \
         acknowledged_because: \"a directory of CSVs this deployment owns\"\n  \
         warehouse:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/warehouse\"\n    posture: \"impersonation-at-source\"\n    \
         workload_identity:\n      audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n    \
         verification_identity: \"sutura_anchor_reader\"\n",
    );
    let settings = Settings::load(&sources).expect("two sources load");
    assert_eq!(settings.sources().count(), 2);
    assert_eq!(
        settings
            .sources()
            .each()
            .map(|(alias, source)| (
                alias.as_str(),
                source.posture().map(sutura_domain::source::SourcePosture::as_str),
                match source.placement() {
                    crate::sources::placement::SourcePlacement::Files { data_dir } => data_dir.to_string_lossy().into_owned(),
                    crate::sources::placement::SourcePlacement::BigQuery { .. }
                    | crate::sources::placement::SourcePlacement::Postgres { .. }
                    | crate::sources::placement::SourcePlacement::ClickHouse { .. }
                    | crate::sources::placement::SourcePlacement::Oracle { .. }
                    | crate::sources::placement::SourcePlacement::Duckdb { .. } => {
                        panic!("the fixture tree declares files sources");
                    }
                }
            ))
            .collect::<Vec<(&str, Option<&str>, String)>>(),
        vec![
            ("local", Some("shared-service-user"), String::from("/srv/sutura/local")),
            (
                "warehouse",
                Some("impersonation-at-source"),
                String::from("/srv/sutura/warehouse")
            ),
        ]
    );
    // A per-source working-set ceiling is still an unknown key: there is one working set, so a
    // per-source bound would be a number with nothing to bound.
    let with_ceiling = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"multi-user\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/d\"\n    \
         posture: \"impersonation-at-source\"\n    working_set_max_bytes: 2048\n",
    );
    assert!(Settings::load(&with_ceiling).is_err_and(|error| matches!(error.reason(), SettingsError::Source { .. })));
}

#[test]
fn a_password_literal_on_a_source_is_refused_as_an_unknown_key() {
    // `crate::raw::RawSource` has no `password` field at all - see
    // `crate::sources::placement::SourcePlacement::Postgres`'s doc. `deny_unknown_fields` is what
    // makes the ABSENCE of the field a mechanism rather than a fact nobody pins: without this test
    // the first convenience patch that adds the key ships a secret in the settings tree with every
    // other test still green.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "sources:\n  local:\n    kind: \"files\"\n    posture: \"shared-service-user\"\n    password: \"hunter2\"\n",
    );
    let error = Settings::load(&sources).expect_err("a password literal is not a setting");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(rendered.contains("password"), "the error should name the key: {rendered}");
}

#[test]
fn a_bigquery_credential_file_is_refused_as_an_unknown_key() {
    // The key was required, checked absolute and read by nothing - the driver authenticates itself.
    // `deny_unknown_fields` is what turns a config that still writes it into a refusal naming the
    // key rather than a path that reads as a control in place.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"one analyst on one laptop\"\nsources:\n  warehouse:\n    \
         kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    dataset: \"warehouse\"\n    \
         credential_file: \"/etc/sutura/bigquery.json\"\n    max_bytes_billed: 1073741824\n    \
         posture: \"shared-service-user\"\n",
    );
    let error = Settings::load(&sources).expect_err("a credential file is not a setting");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(
        rendered.contains("unknown field") && rendered.contains("credential_file"),
        "the refusal names the key: {rendered}"
    );
}

#[test]
fn a_workload_identity_scope_is_refused_as_an_unknown_key() {
    // The scope was required, parsed and sent by nothing: the credential document has no `scopes`
    // member. A config that still writes it is refused by name.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"multi-user\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/d\"\n    \
         posture: \"impersonation-at-source\"\n    workload_identity:\n      audience: \"//iam.googleapis.com/projects/acme-analytics/locations/global/workloadIdentityPools/analysts/providers/sso\"\n      \
         scope: \"https://www.googleapis.com/auth/bigquery.readonly\"\n",
    );
    let error = Settings::load(&sources).expect_err("a workload identity scope is not a setting");
    assert!(matches!(*error.reason(), SettingsError::Source { .. }), "{error:?}");
    let rendered = format!("{:?}", core::error::Error::source(&error));
    assert!(
        rendered.contains("unknown field") && rendered.contains("scope"),
        "the refusal names the key: {rendered}"
    );
}

/// A multi-user deployment whose SECOND shared source is unacknowledged does not start. No other
/// cell here fails if the check stops after the first source it walks; a
/// federated answer reads two or three sources (`telekom/sutura#780`), and the acknowledgement is
/// what answers for each of them (`docs/adr/0040`).
#[test]
fn a_second_shared_source_without_an_acknowledgement_is_not_fit_to_serve_either() {
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  identity: \"multi-user\"\nsources:\n  local:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/local\"\n    posture: \"shared-service-user\"\n    acknowledged_because: \"a directory of CSVs this deployment owns\"\n  orders:\n    kind: \"files\"\n    data_dir: \"/srv/sutura/orders\"\n    posture: \"shared-service-user\"\n",
    );
    let error = Settings::load(&sources).expect_err("a second unacknowledged shared source is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::SharedSourceNotAcknowledged {
            alias: SourceName::parse("orders").expect("a legal identifier")
        }]
    );
    let rendered = refusals.first().expect("one refusal").to_string();
    assert!(rendered.contains("sources.orders.acknowledged_because"), "{rendered}");
}

// The exchange a `direct` deployment runs for an impersonating source - nested under
// `workload_identity:` at 6 spaces, with its four required keys at 8. `client_secret_file` is a
// PATH: the secret itself is read by the composition root, never the settings tree.
const DELEGATION: &str = "      delegation:\n        token_endpoint: \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        client_secret_file: \"/run/secrets/idp-client\"\n        audience: \"pool-client-id\"\n";

#[test]
fn a_delegation_on_a_deployment_that_does_not_verify_direct_callers_is_not_fit_to_serve() {
    // A delegation exchange sends the caller's OWN inbound token - leg 1's - to the identity
    // provider, so it only exists on a deployment that verifies one. A non-direct deployment that
    // declares the block holds a client credential nothing could use, which reads as a control that
    // is in place: refused, naming `security.inbound.mode`.
    let refused = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{}",
        source_overlay("impersonation-at-source", DELEGATION)
    ));
    let error = Settings::load(&refused).expect_err("a delegation on a non-direct deployment is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a delegation refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::DelegationWithoutDirectInbound {
            alias: SourceName::parse("local").expect("a legal identifier")
        }]
    );
    assert_eq!(
        refusals.first().expect("one refusal").to_string(),
        "`sources.local.workload_identity.delegation` is declared and security.inbound.mode is not \
         `direct`. The delegation exchange sends the caller's own inbound token, which only a `direct` \
         deployment verifies - set security.inbound.mode: direct, or remove the block"
    );

    // A `behind-gateway` deployment verifies a gateway's assertion, not the caller's own token, so
    // it is refused exactly like one with no inbound block.
    let gateway = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{GATEWAY_INBOUND}{}",
        source_overlay("impersonation-at-source", DELEGATION)
    ));
    let error = Settings::load(&gateway).expect_err("a delegation behind a gateway is not fit to serve");
    let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
        panic!("expected a delegation refusal, got {error:?}");
    };
    assert_eq!(
        *refusals,
        vec![NotFitToServe::DelegationWithoutDirectInbound {
            alias: SourceName::parse("local").expect("a legal identifier")
        }]
    );

    // The CONTROL: the same block on a `direct` deployment serves, and the declaration survives
    // into the parsed tree so the exchange has somewhere to read it from.
    let direct = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{DIRECT_INBOUND}{}",
        source_overlay("impersonation-at-source", DELEGATION)
    ));
    let settings = Settings::load(&direct).expect("a direct deployment admits a delegation");
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());
    let local = settings
        .sources()
        .each()
        .find(|(alias, _)| alias.as_str() == "local")
        .expect("local is configured")
        .1;
    assert_eq!(
        local
            .workload_identity()
            .and_then(crate::sources::workload_identity::WorkloadIdentityConfig::delegation)
            .map(crate::sources::workload_identity::DelegationDeclared::audience),
        Some("pool-client-id")
    );

    // And a delegation block missing one of its REQUIRED keys is a parse refusal naming it, so a
    // block that lost its `client_secret_file` cannot silently fall back to no exchange.
    let missing = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{}",
        source_overlay(
            "impersonation-at-source",
            "      delegation:\n        token_endpoint: \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        audience: \"pool-client-id\"\n"
        )
    ));
    let error = Settings::load(&missing).expect_err("a delegation block missing its client secret file is refused");
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("client_secret_file"),
        "the refusal must name the missing key: {rendered}"
    );
}

#[test]
fn a_relative_client_secret_file_is_refused_at_load() {
    // A relative path resolves against the working directory - a different file on every host - so
    // the delegation's client secret is held to the rule every other secret file on a source is.
    let relative = Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{DIRECT_INBOUND}{}",
        source_overlay(
            "impersonation-at-source",
            &DELEGATION.replace("/run/secrets/idp-client", "secrets/idp-client")
        )
    ));
    let error = Settings::load(&relative).expect_err("a relative client secret file is refused");
    let SettingsError::Sources { ref cause } = *error.reason() else {
        panic!("expected a sources refusal, got {error:?}");
    };
    assert!(
        cause.to_string().starts_with(
            "`sources.local.workload_identity.delegation.client_secret_file` is `secrets/idp-client`, which is relative"
        ),
        "{cause}"
    );
}

/// Diagnostics and logs show endpoint URLs without userinfo: the startup log prints the resolved
/// settings before the exchange parses its token endpoint, so an `@` there is refused at load.
#[test]
fn a_delegation_token_endpoint_with_userinfo_is_refused_at_load_without_being_quoted() {
    for endpoint in [
        "https://svc:s3cret@idp.example.com/token",
        "https://svc/x:s3cret@idp.example.com/token",
    ] {
        let declared = DELEGATION.replace("https://idp.example.com/token", endpoint);
        let sources = Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n{DIRECT_INBOUND}{}",
            source_overlay("impersonation-at-source", &declared)
        ));
        let error = Settings::load(&sources).expect_err("a userinfo token endpoint is refused");
        let rendered = format!(
            "{error} {error:?} {}",
            core::error::Error::source(error.reason()).map_or_default(ToString::to_string)
        );
        assert!(rendered.contains("token_endpoint` carries an `@`"), "{rendered}");
        assert!(!rendered.contains("s3cret"), "{rendered}");
    }
}
