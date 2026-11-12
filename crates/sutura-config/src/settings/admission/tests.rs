use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};
use sutura_domain::identity::VerifierId;
use sutura_domain::model::SourceName;

fn direct(id: &str, path: &str) -> String {
    format!(
        "    - id: {id}\n      path: {path}\n      mode: direct\n      resource: https://{id}.example.com{path}\n      authorization_server: https://{id}-issuer.example.com\n      key_set_file: /keys.json\n      algorithms: [RS256]\n"
    )
}

fn gateway(id: &str, path: &str) -> String {
    format!(
        "    - id: {id}\n      path: {path}\n      mode: behind-gateway\n      transit_header: x-transit-proof\n      transit_issuer: https://{id}-issuer.example.com\n      transit_audience: /{id}\n      key_set_file: /keys.json\n      algorithms: [RS256]\n      transit_token_type: at+jwt\n"
    )
}

fn sources(entries: &str, source: &str) -> Sources {
    Sources::defaults(Environment::Development).with_overlay(format!(
        "security:\n  identity: single-user\n  single_user_because: fixture\n  inbound:\n{entries}{source}"
    ))
}

fn local(extra: &str) -> String {
    format!("sources:\n  local:\n    kind: files\n    data_dir: /data\n    posture: shared-service-user\n{extra}")
}

fn refused(entries: &str, source: &str) -> Vec<NotFitToServe> {
    let Err(error) = Settings::load(&sources(entries, source)) else {
        return Vec::new();
    };
    match error.reason() {
        SettingsError::NotFitToServe { refusals } => refusals.clone(),
        reason => panic!("expected startup refusals: {reason:?}"),
    }
}

#[test]
fn named_verifiers_keep_complete_policies_and_legacy_objects_normalize() {
    let entries = format!(
        "{}{}{}",
        direct("staff", "/staff"),
        direct("partners", "/partners"),
        gateway("gateway", "/staff")
    );
    let settings = Settings::load(&sources(&entries, &local("    admitted_by: [staff, gateway]\n")))
        .expect("two directs and a shared gateway path load");
    let verifiers = settings.security().inbound();
    assert_eq!(verifiers.len(), 3);
    assert_eq!(verifiers[0].id().as_str(), "staff");
    assert_eq!(verifiers[1].path().as_str(), "/partners");
    assert_eq!(verifiers[2].identity().mode(), "behind-gateway");
    assert_eq!(
        verifiers[0].requirement().issuer().as_str(),
        "https://staff-issuer.example.com"
    );
    assert_eq!(verifiers[2].requirement().audience().as_str(), "/gateway");
    assert_eq!(verifiers[0].metadata_path(), Some(verifiers[0].path()));

    let legacy = Sources::defaults(Environment::Development).with_overlay("security:\n  inbound:\n    mode: direct\n    resource: https://resource.example.com\n    authorization_server: https://issuer.example.com\n    key_set_file: /keys.json\n    algorithms: [RS256]\n");
    let settings = Settings::load(&legacy).expect("legacy pathless resource retains discovery");
    let verifier = &settings.security().inbound()[0];
    assert_eq!(verifier.id(), &VerifierId::primary());
    assert_eq!(verifier.path().as_str(), "/mcp");
    assert_eq!(verifier.metadata_path(), None);
}

#[test]
fn list_entries_require_ids_and_literal_paths_and_nonempty_lists() {
    for (key, text) in [
        ("path", direct("staff", "/staff").replace("      path: /staff\n", "")),
        (
            "id",
            direct("staff", "/staff").replace("    - id: staff\n      path: /staff\n", "    - path: /staff\n"),
        ),
    ] {
        let error = Settings::load(&sources(&text, "")).expect_err("a required list key is missing");
        assert!(matches!(error.reason(), SettingsError::InboundEntryKeyMissing { key: found, index: Some(0) } if *found == key));
    }
    let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay("security:\n  inbound: []\n"))
        .expect_err("an empty declaration is refused");
    assert!(matches!(error.reason(), SettingsError::InboundListEmpty));
    for path in [
        concat!("/mcp/", "{", "id", "}"),
        "/mcp/*",
        "/mcp/%2f",
        "/mcp/../other",
        "/mcp//other",
        "/mcp/",
        "/",
        "relative",
    ] {
        let error = Settings::load(&sources(&direct("staff", path), "")).expect_err("nonliteral path is refused");
        assert!(
            matches!(error.reason(), SettingsError::McpPath { index: Some(0), .. }),
            "{path}: {error:?}"
        );
    }
    for id in ["staff.provider", " staff", "", "staff/provider"] {
        VerifierId::parse(id).expect_err("a malformed verifier id is refused");
    }
}

#[test]
fn malformed_admission_ids_and_mcp_path_byte_bounds_are_checked_at_load() {
    let error = Settings::load(&sources(&direct("bad.id", "/mcp"), "")).expect_err("an id is a literal verifier name");
    assert!(matches!(error.reason(), SettingsError::VerifierId { index: Some(0), .. }));
    let error = Settings::load(&sources(&direct("staff", "/mcp"), &local("    admitted_by: [bad.id]\n")))
        .expect_err("an admitted id is parsed");
    assert!(matches!(
        error.reason(),
        SettingsError::SourceAdmission {
            cause: crate::sources::admission::InvalidSourceAdmission::Verifier { index: 0, .. }
        }
    ));
    let path = format!("/{}", "a".repeat(255));
    drop(Settings::load(&sources(&direct("staff", &path), "")).expect("a 256-byte literal path loads"));
    let error = Settings::load(&sources(&direct("staff", &format!("{path}a")), "")).expect_err("a 257-byte path is refused");
    assert!(matches!(error.reason(), SettingsError::McpPath { index: Some(0), .. }));
}

#[test]
fn every_verifier_pair_is_checked_for_id_issuer_audience_and_direct_path_collisions() {
    let first = direct("first", "/first");
    let second = direct("second", "/second");
    let base = format!("{first}{second}");
    let collision = direct("third", "/third");
    let ids = refused(&format!("{base}{}", collision.replace("id: third", "id: second")), "");
    assert!(
        ids.iter()
            .any(|r| matches!(r, NotFitToServe::DuplicateVerifierId { id } if id.as_str() == "second"))
    );
    let issuers = refused(&format!("{base}{}", collision.replace("third-issuer", "second-issuer")), "");
    assert!(issuers.iter().any(|r| matches!(r, NotFitToServe::DuplicateVerifierIssuer { earlier, later, .. } if earlier.as_str() == "second" && later.as_str() == "third")));
    let audiences = refused(
        &format!(
            "{base}{}",
            collision.replace("https://third.example.com/third", "https://second.example.com/second")
        ),
        "",
    );
    assert!(audiences.iter().any(|r| matches!(r, NotFitToServe::DuplicateVerifierAudience { earlier, later, .. } if earlier.as_str() == "second" && later.as_str() == "third")));
    let paths = refused(&format!("{base}{}", direct("third", "/second")), "");
    assert!(paths.iter().any(|r| matches!(r, NotFitToServe::DirectVerifiersSharePath { earlier, later, .. } if earlier.as_str() == "second" && later.as_str() == "third")));
}

#[test]
fn mounts_cannot_overlap_reserved_routes_each_other_or_the_resource_path() {
    for path in [
        "/v1",
        "/v1/query",
        "/health",
        "/metrics",
        "/openapi.json",
        "/docs/extra",
        "/.well-known/extra",
        "/arrow.flight.protocol.FlightService",
        "/worker.WorkerService",
    ] {
        assert!(
            refused(&direct("staff", path), "")
                .iter()
                .any(|r| matches!(r, NotFitToServe::ReservedMcpPath { .. })),
            "{path}"
        );
    }
    let entries = format!("{}{}", direct("staff", "/agent"), gateway("gateway", "/agent/child"));
    assert!(
        refused(&entries, "")
            .iter()
            .any(|r| matches!(r, NotFitToServe::OverlappingMcpPaths { .. }))
    );
    for resource in [
        "https://staff.example.com",
        "https://staff.example.com/proxy/staff",
        "https://staff.example.com/other/../staff",
    ] {
        let entry = direct("staff", "/staff").replace("https://staff.example.com/staff", resource);
        assert!(
            refused(&entry, "")
                .iter()
                .any(|r| matches!(r, NotFitToServe::InboundResourcePathMismatch { .. })),
            "{resource}"
        );
    }
}

#[test]
fn source_admission_is_explicit_for_multiple_verifiers_and_resolved_for_one() {
    let entries = format!("{}{}", direct("staff", "/staff"), gateway("gateway", "/gateway"));
    let local_alias = SourceName::parse("local").unwrap();
    assert!(
        refused(&entries, &local("")).contains(&NotFitToServe::SourceVerifiersUndeclared {
            alias: local_alias.clone()
        })
    );
    assert!(
        refused(&entries, &local("    admitted_by: [missing]\n"))
            .iter()
            .any(|r| matches!(r, NotFitToServe::UnknownAdmittedVerifier { verifier, .. } if verifier.as_str() == "missing"))
    );
    let one = Settings::load(&sources(&direct("staff", "/staff"), &local(""))).expect("single omission resolves");
    assert_eq!(
        one.sources()
            .get(&local_alias)
            .unwrap()
            .admitted_by()
            .unwrap()
            .iter()
            .map(VerifierId::as_str)
            .collect::<Vec<_>>(),
        ["staff"]
    );
    let none = Settings::load(&sources(&entries, &local("    admitted_by: []\n"))).expect("an explicit empty set admits nobody");
    assert!(none.sources().get(&local_alias).unwrap().admitted_by().unwrap().is_empty());
}

#[test]
fn every_authorization_reader_and_direct_audience_is_checked() {
    let entries = format!(
        "{}{}",
        gateway("first", "/first"),
        gateway("second", "/second").replace("x-transit-proof", "Authorization")
    );
    let source = sources(&format!("{entries}  access_token: 0123456789abcdef0123456789abcdef\n"), "");
    let error = Settings::load(&source).expect_err("a later Authorization reader collides with the deployment token");
    assert!(
        matches!(error.reason(), SettingsError::NotFitToServe { refusals } if refusals.contains(&NotFitToServe::DeploymentTokenSharesTheHeader))
    );
    let entries = format!(
        "{}{}",
        direct("first", "/first"),
        direct("second", "/second").replace("https://second-issuer.example.com", "https://second.example.com/second")
    );
    assert!(refused(&entries, "").contains(&NotFitToServe::InboundAudienceIsTheIssuer));
}

#[test]
fn caller_run_sources_admit_only_direct_verifiers_and_delegation_admits_exactly_one() {
    let entries = format!(
        "{}{}{}",
        direct("first", "/first"),
        direct("second", "/second"),
        gateway("gateway", "/gateway")
    );
    let hop = "    workload_identity:\n      audience: //iam.googleapis.com/projects/example/locations/global/workloadIdentityPools/example/providers/example\n      delegation:\n        - token_endpoint: https://issuer.example.com/token\n          client_id: example\n          client_secret_file: /client.secret\n          audience: pool\n";
    let delegated = |admitted: &str| {
        format!(
            "sources:\n  local:\n    kind: files\n    data_dir: /data\n    posture: impersonation-at-source\n    verification_identity: fixture\n    admitted_by: [{admitted}]\n{hop}"
        )
    };
    for (admitted, count) in [("", 0), ("first, second", 2)] {
        assert!(
            refused(&entries, &delegated(admitted))
                .iter()
                .any(|r| matches!(r, NotFitToServe::DelegationVerifierCardinality { found, .. } if *found == count))
        );
    }
    let refused_gateway = refused(&entries, &delegated("gateway"));
    assert!(
        refused_gateway
            .iter()
            .any(|r| matches!(r, NotFitToServe::CallerSourceAdmitsGateway { verifier, .. } if verifier.as_str() == "gateway"))
    );
    drop(Settings::load(&sources(&entries, &delegated("second"))).expect("one direct verifier is eligible for delegation"));

    for block in [
        "    google_login:\n      token_endpoint: https://issuer.example.com/google/token\n      client_id: example\n      client_secret_file: /client.secret\n",
        "    domain_delegation:\n      delegating_account: example@example.iam.gserviceaccount.com\n",
    ] {
        let source = |admitted: &str| {
            format!(
                "sources:\n  local:\n    kind: bigquery\n    max_bytes_billed: 1000\n    posture: impersonation-at-source\n    verification_identity: fixture\n    admitted_by: [{admitted}]\n{block}"
            )
        };
        assert!(
            refused(&entries, &source("gateway"))
                .iter()
                .any(|r| matches!(r, NotFitToServe::CallerSourceAdmitsGateway { .. }))
        );
        drop(
            Settings::load(&sources(&entries, &source("first, second")))
                .expect("both direct verifiers can supply the caller identity"),
        );
        drop(Settings::load(&sources(&entries, &source(""))).expect("an explicit empty set disables admission to this source"));
    }
}

#[test]
fn hopless_caller_sources_also_refuse_gateway_admission() {
    let entries = format!("{}{}", direct("staff", "/staff"), gateway("gateway", "/gateway"));
    for kind in [
        "    kind: bigquery\n    max_bytes_billed: 1000\n    workload_identity:\n      audience: //iam.googleapis.com/projects/example/locations/global/workloadIdentityPools/example/providers/example\n    inbound_token_at_pool: accepted\n",
        "    kind: clickhouse\n    host: 127.0.0.1\n    port: 8123\n    user: deployment\n    password_file: /password\n    transport_mode: plaintext\n",
    ] {
        let source = |admitted: &str| {
            format!(
                "sources:\n  local:\n{kind}    posture: impersonation-at-source\n    verification_identity: fixture\n    admitted_by: [{admitted}]\n"
            )
        };
        assert!(
            refused(&entries, &source("gateway"))
                .iter()
                .any(|r| matches!(r, NotFitToServe::CallerSourceAdmitsGateway { .. }))
        );
        drop(Settings::load(&sources(&entries, &source("staff"))).expect("the direct verifier may supply caller identity"));
    }
}
