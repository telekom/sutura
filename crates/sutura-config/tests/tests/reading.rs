//! The reading worker groups' renders, read as the binary reads them: the serving pod's `server.nodes`,
//! each group's own pod and settings, and the policy that selects the group's pods.

use std::collections::BTreeMap;

use serde_norway::Value;
use sutura_config::{Environment, ServerRole, Settings, Sources, TlsMaterial};
use sutura_domain::model::SourceName;

use super::{Inputs, documents, named, serve_refusals, settings_directory, testdata};

/// The release every reading worker golden is rendered as, so its serving Deployment and `ConfigMap`
/// are named for it.
const RELEASE: &str = "test-release";

/// What the render's Deployment `deployment` hands its process, `$(POD_IP)` resolved as the
/// kubelet would and the variable it reads from the pod itself left out. A render holds one
/// `ConfigMap` per settings directory, so it is the one named `<deployment>-config`; `label` makes
/// that directory and a `secretKeyRef`'s stand-in value distinct per caller.
fn reading_inputs(label: &str, docs: &[Value], deployment: &str) -> Inputs {
    let env = named(docs, "Deployment", deployment)["spec"]["template"]["spec"]["containers"][0]["env"]
        .as_sequence()
        .unwrap_or_else(|| panic!("a Deployment {deployment} with an env list"));
    let mut environment = None;
    let mut directory = None;
    let mut variables = BTreeMap::new();
    for entry in env {
        let variable = entry["name"].as_str().expect("an env name");
        let value = match entry["value"].as_str() {
            Some(value) => value.replace("$(POD_IP)", "10.0.0.5"),
            None if entry["valueFrom"]["secretKeyRef"].is_mapping() => {
                format!("{label}-{variable}-placeholder-secret-value")
            }
            None if entry["valueFrom"]["fieldRef"].is_mapping() => continue,
            None => panic!("{deployment}: {variable} has no value, secretKeyRef or fieldRef"),
        };
        match variable {
            "SUTURA_ENVIRONMENT" => environment = Some(value),
            "SUTURA_CONFIG_DIR" => {
                directory = Some(settings_directory(
                    label,
                    named(docs, "ConfigMap", &format!("{deployment}-config")),
                ));
            }
            _ => {
                variables.insert(String::from(variable), value);
            }
        }
    }
    Inputs {
        environment: environment.unwrap_or_else(|| panic!("{deployment}: no SUTURA_ENVIRONMENT")),
        directory,
        variables,
    }
}

/// The serving Deployment's inputs in a render that also holds worker Deployments and `ConfigMap`s.
pub(super) fn release_inputs(label: &str, docs: &[Value]) -> Inputs {
    reading_inputs(label, docs, RELEASE)
}

/// The parsed documents of the golden `golden` under `golden/pool/`.
fn reading_docs(golden: &str) -> Vec<Value> {
    let path = testdata().join(format!("golden/pool/{golden}.yaml"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
    documents(&text)
}

/// The settings the Deployment `deployment` of the golden `golden` under `golden/pool/` loads.
fn reading_settings(golden: &str, deployment: &str) -> Settings {
    let Inputs {
        environment,
        directory,
        variables,
    } = reading_inputs(&format!("{golden}-{deployment}"), &reading_docs(golden), deployment);
    let environment = Environment::parse(&environment).unwrap_or_else(|error| panic!("{deployment}: {error}"));
    let sources = Sources::defaults(environment).with_variables(variables);
    let loaded = match &directory {
        Some(dir) => Settings::load(&sources.with_directory(dir.clone())),
        None => Settings::load(&sources),
    };
    if let Some(dir) = directory {
        std::fs::remove_dir_all(dir).expect("the scratch directory is removed");
    }
    loaded.unwrap_or_else(|error| panic!("the binary refuses {deployment}: {:?}", error.reason()))
}

/// The names of one Deployment's volumes, then of its first container's volume mounts.
type VolumeNames = (Vec<String>, Vec<String>);

fn volume_names(docs: &[Value], deployment: &str) -> VolumeNames {
    let pod = &named(docs, "Deployment", deployment)["spec"]["template"]["spec"];
    let names = |list: &Value| -> Vec<String> {
        list.as_sequence()
            .unwrap_or_else(|| panic!("{deployment}: a list of volumes"))
            .iter()
            .map(|entry| String::from(entry["name"].as_str().expect("a volume name")))
            .collect()
    };
    (names(&pod["volumes"]), names(&pod["containers"][0]["volumeMounts"]))
}

/// A declared peer: its address, its role and the source aliases it reads.
type Peer<'a> = (&'a str, ServerRole, Vec<&'a str>);

#[test]
fn the_serving_pod_names_each_reading_worker_group_with_the_sources_it_reads() {
    let settings = reading_settings("reading-workers", RELEASE);
    assert!(serve_refusals(&settings).is_empty(), "{:?}", serve_refusals(&settings));
    let nodes = settings
        .server()
        .tls()
        .and_then(TlsMaterial::nodes)
        .expect("the serving pod declares nodes");
    let peers: Vec<Peer<'_>> = nodes
        .others()
        .iter()
        .map(|node| {
            (
                node.url().as_str(),
                node.role(),
                node.sources().iter().map(SourceName::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        peers,
        [
            ("https://test-release-workers.default.svc:8080/", ServerRole::Worker, vec![]),
            (
                "https://test-release-reading-finance.default.svc:8080/",
                ServerRole::ReadingWorker,
                vec!["billing", "ledger"]
            ),
            (
                "https://test-release-reading-orders.default.svc:8080/",
                ServerRole::ReadingWorker,
                vec!["orders"]
            ),
        ]
    );
}

/// A group's pod starts as a reading worker, and its settings declare the sources it reads and no
/// other - the serving pod of the same render declares four.
#[test]
fn a_reading_workers_pod_starts_as_a_reading_worker_over_its_own_sources_only() {
    for (deployment, read) in [
        ("test-release-reading-orders", vec!["orders"]),
        ("test-release-reading-finance", vec!["billing", "ledger"]),
    ] {
        let settings = reading_settings("reading-workers", deployment);
        assert_eq!(settings.server().role(), ServerRole::ReadingWorker, "{deployment}");
        let mut declared: Vec<&str> = settings.sources().each().map(|(alias, _)| alias.as_str()).collect();
        declared.sort_unstable();
        assert_eq!(declared, read, "{deployment}");
    }
    assert_eq!(reading_settings("reading-workers", RELEASE).sources().count(), 4);
}

/// The release's own `extraVolumes` reach the serving pod and no group; each group's pod mounts the
/// volumes its own values list and no other group's.
#[test]
fn a_reading_workers_pod_mounts_only_its_own_credentials() {
    let docs = reading_docs("reading-workers");
    let shared = String::from("all-source-credentials");
    let (serving_volumes, serving_mounts) = volume_names(&docs, RELEASE);
    assert!(serving_volumes.contains(&shared) && serving_mounts.contains(&shared));
    for (deployment, own) in [
        ("test-release-reading-orders", "orders-credentials"),
        ("test-release-reading-finance", "finance-credentials"),
    ] {
        let expected = ["config", "node", "inbound-keys", own];
        let (volumes, mounts) = volume_names(&docs, deployment);
        assert_eq!(volumes, expected, "{deployment}");
        assert_eq!(mounts, expected, "{deployment}");
    }
}

/// The policy for the reading workers selects the labels a group's pods carry, and admits the
/// serving pods, the combining workers and the reading workers only.
#[test]
fn the_reading_workers_network_policy_selects_the_group_pods_and_admits_the_releases_own() {
    let docs = reading_docs("reading-workers-network-policy");
    let policy = named(&docs, "NetworkPolicy", "test-release-reading-workers");
    let pod = &named(&docs, "Deployment", "test-release-reading-orders")["spec"]["template"]["metadata"]["labels"];
    let selector = policy["spec"]["podSelector"]["matchLabels"]
        .as_mapping()
        .expect("the policy selects by labels");
    for (key, value) in selector {
        let key = key.as_str().expect("a label name");
        assert_eq!(
            &pod[key], value,
            "the group's pod carries label {key} as the policy selects it"
        );
    }
    let admitted: Vec<&str> = policy["spec"]["ingress"][0]["from"]
        .as_sequence()
        .expect("an ingress rule with sources")
        .iter()
        .map(|entry| {
            entry["podSelector"]["matchLabels"]["app.kubernetes.io/name"]
                .as_str()
                .expect("a name label")
        })
        .collect();
    assert_eq!(admitted, ["sutura", "sutura-worker", "sutura-reading-worker"]);
}
