#![forbid(unsafe_code)]
//! Every golden render of `charts/sutura` is a configuration this binary starts with under `sutura serve`.
//!
//! The chart's own gate proves a render is what the template produces; this proves the binary
//! accepts it. Read from the RENDERED output, so no second copy of the template's env mapping
//! exists to drift: the `ConfigMap`'s files become the settings directory and the Deployment
//! container's `env` becomes the variables, a `secretKeyRef` standing in as a distinct value.
//!
//! The other direction, a chart refusal the binary would not make, is
//! `a_chart_refusal_is_the_binarys_or_a_stated_gap`. A refused case has no render, so the chart gate
//! renders each one through a copy of the chart without its posture guard
//! (`charts/sutura/testdata/golden/unguarded/`): the env mapping is the template's own, and a render that
//! drifts from the template is a diff in `just chart`.
//!
//! # What this does NOT reach
//!
//! * **A rule `sutura serve` asks only at the listener.** `serve_refusals` asks
//!   `Settings::query_surface_refusals` where `sutura serve` does, a deployment with no
//!   `security.inbound` block; the `/mcp` rule is in `Settings::refusals`. The serve cells hold the
//!   rule itself, and nothing here proves `serve_refusals` is the call site's whole condition.
//! * **A refusal outside the posture guard.** The unguarded copy drops only that guard. A refusal
//!   raised while deriving a value from a limit still fails the render, so such a case is declared
//!   `Binary::Derivation` by hand. A `Derivation` case must have no unguarded golden, so one the
//!   guard-less chart renders is refused; a case that really fails derivation, and that the binary
//!   would also refuse, is held by review alone.
//! * **Secret and file contents.** A placeholder token is never empty, and `Settings::load` opens
//!   no key-set or TLS file, so neither a Secret holding `""` nor a missing mount is seen.
//! * **`extraEnv`**, beyond the names the chart refuses (one the binary reads under `security`, or as a
//!   variable the chart sets) and what a golden's values file happens to set: no guard reads another
//!   `SUTURA__*` variable, a source's posture included.
//! * **The worker pool's operator.** It reads no `Settings`, so only the pool's serving pod and its
//!   worker are loaded here. The pool's refusals fire outside the posture guard, so each is declared
//!   `Binary::NeverReads`.
//! * **A reading worker group's refusals the serving pod does not raise.** The serving pod's settings
//!   are what a refusal case loads, so a group whose own pod the binary would refuse (no catalog) is
//!   declared `Binary::NeverReads`; the group pods of a golden are loaded, and nothing else reads the
//!   credentials a group's `extraVolumes` name.
//! * **A binary without `tls`.** Gated on the feature: the chart's `tls.enabled` render assumes a
//!   binary that can terminate TLS, and every venue that runs this crate's tests enables it.

#![cfg(feature = "tls")]

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use serde::Deserialize as _;
    use serde_norway::Value;
    use sutura_config::{
        Environment, InvalidAccessToken, InvalidDeclaredNode, InvalidInboundValue, NotFitToServe, ServerRole, Settings,
        SettingsError, Sources, TlsMaterial, UnknownEnvironment,
    };
    use sutura_domain::model::SourceName;

    /// What the pod's process is handed: `SUTURA_ENVIRONMENT`, the settings directory, and every
    /// `SUTURA__*` variable.
    struct Inputs {
        environment: String,
        directory: Option<PathBuf>,
        variables: BTreeMap<String, String>,
    }

    fn goldens() -> Vec<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../charts/sutura/testdata/golden");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", dir.display()))
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
            .collect();
        found.sort();
        found
    }

    fn documents(text: &str) -> Vec<Value> {
        serde_norway::Deserializer::from_str(text)
            .map(|doc| Value::deserialize(doc).expect("a YAML document"))
            .filter(|doc| !doc.is_null())
            .collect()
    }

    fn of_kind<'a>(docs: &'a [Value], kind: &str) -> &'a Value {
        let mut matching = docs.iter().filter(|doc| doc["kind"].as_str() == Some(kind));
        let found = matching.next().unwrap_or_else(|| panic!("one {kind}"));
        assert!(matching.next().is_none(), "exactly one {kind}");
        found
    }

    /// The `ConfigMap`'s files, written where `SUTURA_CONFIG_DIR` would point.
    fn settings_directory(name: &str, config_map: &Value) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-chart-render-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        for (file, content) in config_map["data"].as_mapping().expect("ConfigMap data") {
            let file = file.as_str().expect("a file name");
            let content = content.as_str().expect("file content");
            std::fs::write(dir.join(file), content).expect("a settings file");
        }
        dir
    }

    /// What `sutura serve` refuses once `Settings::load` accepted: `/v1` with no `security.inbound` block.
    fn serve_refusals(settings: &Settings) -> Vec<NotFitToServe> {
        if settings.security().inbound().is_some() {
            Vec::new()
        } else {
            settings.query_surface_refusals()
        }
    }

    /// Why the binary refuses these inputs, or `None` when `sutura serve` starts with them.
    fn refusal(environment: &str, directory: Option<&Path>, variables: BTreeMap<String, String>) -> Option<String> {
        let environment = match Environment::parse(environment) {
            Ok(environment) => environment,
            Err(error) => return Some(format!("SUTURA_ENVIRONMENT: {error}")),
        };
        let mut sources = Sources::defaults(environment).with_variables(variables);
        if let Some(dir) = directory {
            sources = sources.with_directory(dir.to_path_buf());
        }
        match Settings::load(&sources) {
            Err(error) => Some(format!("{:?}", error.reason())),
            Ok(settings) if !settings.refusals().is_empty() => Some(format!("{:?}", settings.refusals())),
            Ok(settings) if !serve_refusals(&settings).is_empty() => Some(format!("{:?}", serve_refusals(&settings))),
            Ok(_) => None,
        }
    }

    /// The Deployment container's image in the golden render named `name`; `None` when no such golden exists.
    fn rendered_image(name: &str) -> Option<String> {
        let golden = goldens()
            .into_iter()
            .find(|path| path.file_stem().is_some_and(|stem| stem == name))?;
        let docs = documents(&std::fs::read_to_string(golden).expect("a readable golden"));
        of_kind(&docs, "Deployment")["spec"]["template"]["spec"]["containers"][0]["image"]
            .as_str()
            .map(String::from)
    }

    /// The parsed settings file `file` of the `ConfigMap` in the golden render named `name`.
    fn rendered_config(name: &str, file: &str) -> Option<Value> {
        let golden = goldens()
            .into_iter()
            .find(|path| path.file_stem().is_some_and(|stem| stem == name))?;
        let docs = documents(&std::fs::read_to_string(golden).expect("a readable golden"));
        let text = of_kind(&docs, "ConfigMap")["data"][file].as_str()?.to_owned();
        Some(serde_norway::from_str(&text).expect("a settings file is YAML"))
    }

    fn app_version() -> String {
        let chart = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../charts/sutura/Chart.yaml");
        let text = std::fs::read_to_string(&chart).unwrap_or_else(|error| panic!("{} is readable: {error}", chart.display()));
        let chart: Value = serde_norway::from_str(&text).expect("Chart.yaml is YAML");
        String::from(chart["appVersion"].as_str().expect("an appVersion"))
    }

    #[test]
    fn the_performance_image_flag_adds_the_suffix_to_the_app_version() {
        assert_eq!(
            rendered_image("performance-image"),
            Some(format!("ghcr.io/telekom/sutura:{}-performance-musl", app_version()))
        );
    }

    #[test]
    fn the_performance_image_flag_off_keeps_the_app_version_tag() {
        assert_eq!(
            rendered_image("performance-image-off"),
            Some(format!("ghcr.io/telekom/sutura:{}-musl", app_version()))
        );
    }

    #[test]
    fn the_default_image_is_the_musl_variant() {
        assert_eq!(
            rendered_image("minimal"),
            Some(format!("ghcr.io/telekom/sutura:{}-musl", app_version()))
        );
    }

    #[test]
    fn the_glibc_variant_keeps_the_app_version_tag() {
        assert_eq!(
            rendered_image("image-glibc"),
            Some(format!("ghcr.io/telekom/sutura:{}", app_version()))
        );
    }

    #[test]
    fn a_default_render_logs_bunyan_at_info() {
        let telemetry = &rendered_config("minimal", "base.yaml").expect("a minimal render")["telemetry"];
        assert_eq!(telemetry["format"].as_str(), Some("bunyan"));
        assert_eq!(telemetry["filter"].as_str(), Some("info"));
    }

    #[test]
    fn the_telemetry_values_are_rendered() {
        let telemetry = &rendered_config("telemetry-values", "base.yaml").expect("a telemetry render")["telemetry"];
        assert_eq!(telemetry["format"].as_str(), Some("pretty"));
        assert_eq!(telemetry["filter"].as_str(), Some("sutura=debug"));
    }

    #[test]
    fn a_telemetry_key_in_config_base_keeps_priority() {
        let telemetry = &rendered_config("telemetry-config-base", "base.yaml").expect("a telemetry render")["telemetry"];
        assert_eq!(telemetry["format"].as_str(), Some("pretty"));
        assert_eq!(telemetry["filter"].as_str(), Some("info"));
    }

    #[test]
    fn an_explicit_tag_is_used_as_is_with_the_performance_image_flag_on() {
        assert_eq!(
            rendered_image("performance-image-tag-wins"),
            Some(String::from("ghcr.io/telekom/sutura:pinned-tag"))
        );
    }

    #[test]
    fn an_explicit_tag_is_used_as_is_with_the_performance_image_flag_off() {
        assert_eq!(
            rendered_image("explicit-tag"),
            Some(String::from("ghcr.io/telekom/sutura:pinned-tag"))
        );
    }

    /// The key set volume projects the one entry `SUTURA__SECURITY__INBOUND__KEY_SET_FILE` names, so
    /// no other entry of the Secret reaches the pod.
    #[test]
    fn the_key_set_volume_projects_only_the_key_set_entry() {
        let mut checked = Vec::new();
        for path in goldens() {
            let docs = documents(&std::fs::read_to_string(&path).expect("a readable golden"));
            let pod = &of_kind(&docs, "Deployment")["spec"]["template"]["spec"];
            let Some(volume) = pod["volumes"]
                .as_sequence()
                .and_then(|volumes| volumes.iter().find(|volume| volume["name"].as_str() == Some("inbound-keys")))
            else {
                continue;
            };
            let file = pod["containers"][0]["env"]
                .as_sequence()
                .and_then(|env| {
                    env.iter()
                        .find(|entry| entry["name"].as_str() == Some("SUTURA__SECURITY__INBOUND__KEY_SET_FILE"))
                })
                .and_then(|entry| entry["value"].as_str())
                .and_then(|value| value.strip_prefix("/etc/sutura/inbound-keys/"))
                .unwrap_or_else(|| panic!("{}: the key set file is under the mount", path.display()));
            let only: Value = serde_norway::from_str(&format!("[{{key: {file}, path: {file}}}]")).expect("a YAML list");
            assert_eq!(volume["secret"]["items"], only, "{}", path.display());
            checked.push(path);
        }
        assert!(!checked.is_empty(), "no golden mounts a key set");
    }

    #[test]
    fn every_chart_render_is_a_configuration_the_binary_starts_with() {
        let goldens = goldens();
        assert!(!goldens.is_empty(), "no golden render found");
        let refused: Vec<String> = goldens
            .iter()
            .filter_map(|path| {
                let name = path.file_stem().and_then(|stem| stem.to_str()).expect("a golden name");
                let docs = documents(&std::fs::read_to_string(path).expect("a readable golden"));
                let Inputs {
                    environment,
                    directory,
                    variables,
                } = pod_inputs(name, &docs, RELEASE);
                let why = refusal(&environment, directory.as_deref(), variables);
                if let Some(dir) = directory {
                    std::fs::remove_dir_all(dir).expect("the scratch directory is removed");
                }
                why.map(|why| format!("{name}: {why}"))
            })
            .collect();
        assert!(
            refused.is_empty(),
            "the binary refuses these chart renders:\n{}",
            refused.join("\n")
        );
    }

    /// The release every golden is rendered as, so its serving Deployment and `ConfigMap` are named for it.
    const RELEASE: &str = "test-release";

    /// What the render's Deployment `deployment` hands its process, `$(POD_IP)` resolved as the
    /// kubelet would and the variable it reads from the pod itself left out. Its settings directory is
    /// the files of the `ConfigMap` named `<deployment>-config`, when it sets `SUTURA_CONFIG_DIR`; `label`
    /// makes that directory and a `secretKeyRef`'s stand-in value distinct per caller.
    fn pod_inputs(label: &str, docs: &[Value], deployment: &str) -> Inputs {
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

    /// The settings the Deployment `deployment` of the golden `golden` under `golden/pool/` loads.
    fn settings_of(golden: &str, deployment: &str) -> Settings {
        let Inputs {
            environment,
            directory,
            variables,
        } = pod_inputs(&format!("{golden}-{deployment}"), &golden_docs(golden), deployment);
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

    fn pool_settings(deployment: &str) -> Settings {
        settings_of("worker-pool", deployment)
    }

    /// The parsed documents of the golden `golden` under `golden/pool/`.
    fn golden_docs(golden: &str) -> Vec<Value> {
        let path = testdata().join(format!("golden/pool/{golden}.yaml"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
        documents(&text)
    }

    #[test]
    fn the_worker_pools_serving_pod_starts_and_names_the_workers() {
        let settings = pool_settings("test-release");
        assert!(serve_refusals(&settings).is_empty(), "{:?}", serve_refusals(&settings));
        let nodes = settings
            .server()
            .tls()
            .and_then(TlsMaterial::nodes)
            .expect("the serving pod declares nodes");
        let peer_urls: Vec<&str> = nodes.others().iter().map(|node| node.url().as_str()).collect();
        assert_eq!(peer_urls, ["https://test-release-workers.default.svc:8080/"]);
    }

    #[test]
    fn the_worker_pools_worker_starts_as_a_worker() {
        let settings = pool_settings("test-release-workers");
        assert_eq!(settings.server().role(), ServerRole::Worker);
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
        let settings = settings_of("reading-workers", RELEASE);
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
            let settings = settings_of("reading-workers", deployment);
            assert_eq!(settings.server().role(), ServerRole::ReadingWorker, "{deployment}");
            let mut declared: Vec<&str> = settings.sources().each().map(|(alias, _)| alias.as_str()).collect();
            declared.sort_unstable();
            assert_eq!(declared, read, "{deployment}");
        }
        assert_eq!(settings_of("reading-workers", RELEASE).sources().count(), 4);
    }

    /// The release's own `extraVolumes` reach the serving pod and no group; each group's pod mounts the
    /// volumes its own values list and no other group's.
    #[test]
    fn a_reading_workers_pod_mounts_only_its_own_credentials() {
        let docs = golden_docs("reading-workers");
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
        let docs = golden_docs("reading-workers-network-policy");
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

    fn pool_docs() -> Vec<Value> {
        golden_docs("worker-pool")
    }

    fn named<'a>(docs: &'a [Value], kind: &str, name: &str) -> &'a Value {
        docs.iter()
            .find(|doc| doc["kind"].as_str() == Some(kind) && doc["metadata"]["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("a {kind} named {name}"))
    }

    /// `github.com/telekom/sutura#1680` finding 4: the operator's Role may only `get` the serving
    /// Deployment's scale, and may `get` or `patch` the workers' - never an unnamed Deployment, which
    /// would let it read or resize whatever a `SuturaWorkerPool` names rather than what the chart
    /// itself deployed. `pods` cannot be scoped by `resourceNames`, so it stays unnamed on purpose
    /// (`docs/adr/20261030193029-operator-sizes-the-worker-pool.md`, Limits).
    #[test]
    fn the_operator_roles_scale_rules_name_only_its_two_deployments() {
        let docs = pool_docs();
        let role = named(&docs, "Role", "test-release-operator");
        let rules = role["rules"].as_sequence().expect("the Role has rules");
        let scale_rules: Vec<&Value> = rules
            .iter()
            .filter(|rule| {
                rule["resources"]
                    .as_sequence()
                    .is_some_and(|r| r.iter().any(|v| v.as_str() == Some("deployments/scale")))
            })
            .collect();
        for rule in &scale_rules {
            let names: Vec<&str> = rule["resourceNames"]
                .as_sequence()
                .unwrap_or_else(|| panic!("a deployments/scale rule with resourceNames: {rule:?}"))
                .iter()
                .map(|v| v.as_str().expect("a resource name"))
                .collect();
            let is_patch = rule["verbs"]
                .as_sequence()
                .expect("verbs")
                .iter()
                .any(|v| v.as_str().expect("a verb") == "patch");
            if is_patch {
                assert_eq!(names, vec!["test-release-workers"], "only the workers' scale is ever patched");
            } else {
                assert_eq!(
                    names,
                    vec!["test-release", "test-release-workers"],
                    "only the two Deployments the chart named are ever read"
                );
            }
        }
        assert!(
            scale_rules.len() >= 2,
            "a get rule and a patch rule, at least: {scale_rules:?}"
        );
    }

    /// `github.com/telekom/sutura#1680` finding 4: the operator pod mounts only the CA bundle from
    /// its node certificate volume, never the pair cert-manager also issues into it - the operator
    /// verifies a server certificate and holds no client identity of its own.
    #[test]
    fn the_operators_node_mount_is_the_ca_bundle_only() {
        let docs = pool_docs();
        let deployment = named(&docs, "Deployment", "test-release-operator");
        let mounts = deployment["spec"]["template"]["spec"]["containers"][0]["volumeMounts"]
            .as_sequence()
            .expect("the operator container has volumeMounts");
        let node_mount = mounts
            .iter()
            .find(|mount| mount["name"].as_str() == Some("node"))
            .expect("a volumeMount named node");
        assert_eq!(node_mount["subPath"].as_str(), Some("ca.crt"));
        assert_eq!(node_mount["mountPath"].as_str(), Some("/etc/sutura/node/ca.crt"));
        let volumes = deployment["spec"]["template"]["spec"]["volumes"]
            .as_sequence()
            .expect("the operator pod has volumes");
        let node_volume = volumes
            .iter()
            .find(|volume| volume["name"].as_str() == Some("node"))
            .expect("a volume named node");
        let usages = node_volume["csi"]["volumeAttributes"]["csi.cert-manager.io/key-usages"]
            .as_str()
            .expect("the node volume declares key usages");
        assert!(
            !usages.contains("client auth"),
            "the operator's own certificate carries no client auth usage: {usages}"
        );
    }

    /// How the binary stands to one chart refusal case.
    enum Binary {
        /// The binary refuses the same configuration, by a refusal this accepts.
        Refuses(fn(&NotFitToServe) -> bool),
        /// The binary refuses it while reading the settings, by an error this accepts.
        Fails(fn(&SettingsError) -> bool),
        /// The chart refuses a configuration the binary starts with, on purpose: the reason.
        Accepts(&'static str),
        /// The chart fails deriving a value from a limit, so no configuration reaches the binary.
        Derivation,
        /// The chart refuses an object the binary never reads, so no configuration reaches it: what.
        NeverReads(&'static str),
    }

    /// Every `testdata/refusals/` case, by name.
    const REFUSALS: [(&str, Binary); 46] = [
        (
            "access-token-missing",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::AccessTokenRequired { .. })),
        ),
        (
            "access-token-padded",
            Binary::Fails(|error| {
                matches!(
                    error,
                    SettingsError::AccessToken {
                        cause: InvalidAccessToken::Untrimmed
                    }
                )
            }),
        ),
        (
            "access-token-whitespace-only",
            Binary::Fails(|error| {
                matches!(
                    error,
                    SettingsError::AccessToken {
                        cause: InvalidAccessToken::Untrimmed
                    }
                )
            }),
        ),
        (
            "autoscaling-cpu-without-request",
            Binary::NeverReads("the autoscaler, which measures CPU as a share of a request"),
        ),
        ("autoscaling-no-target", Binary::NeverReads("the autoscaler's target")),
        (
            "behind-gateway-authorization-in-base-with-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "behind-gateway-authorization-with-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "behind-gateway-transit-header-not-ascii",
            Binary::Fails(|error| {
                matches!(
                    error,
                    SettingsError::InboundValue {
                        cause: InvalidInboundValue::NotPermitted {
                            key: "security.inbound.transit_header",
                            ..
                        }
                    }
                )
            }),
        ),
        ("cpu-limit-unparseable", Binary::Derivation),
        (
            "direct-padded-with-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "direct-with-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "direct-with-config-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "environment-not-exact",
            Binary::Accepts("the binary reads any letter case; the chart names the config file by the exact spelling"),
        ),
        (
            "extra-env-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "extra-env-chart-environment",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::QuerySurfaceWithoutInboundIdentity)),
        ),
        (
            "extra-env-chart-port",
            Binary::Accepts("the binary folds both names into one key; the chart refuses an order it cannot fix"),
        ),
        (
            "extra-env-dotted-access-token",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "extra-env-inbound-mode",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "extra-env-indexed-access-token",
            Binary::Fails(|error| {
                matches!(
                    error,
                    SettingsError::AccessToken {
                        cause: InvalidAccessToken::Untrimmed
                    }
                )
            }),
        ),
        (
            "extra-env-transit-header",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeploymentTokenSharesTheHeader)),
        ),
        (
            "identity-undeclared-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::QuerySurfaceWithoutInboundIdentity)),
        ),
        ("image-digest-malformed", Binary::Derivation),
        ("image-variant-capitalised", Binary::Derivation),
        ("image-variant-padded", Binary::Derivation),
        ("image-variant-unknown", Binary::Derivation),
        ("image-variant-upper-case", Binary::Derivation),
        (
            "impersonating-source-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::QuerySurfaceOverAnImpersonatingSource { .. })),
        ),
        (
            "inbound-mode-empty",
            Binary::Fails(|error| matches!(error, SettingsError::InboundModeUndeclared)),
        ),
        (
            "inbound-mode-empty-environment",
            Binary::Fails(|error| matches!(error, SettingsError::InboundModeUndeclared)),
        ),
        (
            "inbound-mode-null-override",
            Binary::Fails(|error| matches!(error, SettingsError::InboundModeUndeclared)),
        ),
        (
            "inbound-mode-undeclared",
            Binary::Fails(|error| matches!(error, SettingsError::InboundModeUndeclared)),
        ),
        (
            "inbound-mode-unknown-spelling",
            Binary::Fails(|error| matches!(error, SettingsError::InboundModeUnknown { .. })),
        ),
        (
            "inbound-null-override",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::AccessTokenRequired { .. })),
        ),
        ("memory-limit-unparseable", Binary::Derivation),
        (
            "metrics-token-missing",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::MetricsTokenRequired { .. })),
        ),
        (
            "multi-user-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::QuerySurfaceWithoutInboundIdentity)),
        ),
        (
            "reading-workers-group-name-invalid",
            Binary::NeverReads("a group's name, which becomes the group's Service name"),
        ),
        (
            "reading-workers-group-without-catalog",
            Binary::NeverReads("a reading worker's own catalog, a refusal of the group's pod and not of the serving pod"),
        ),
        (
            "reading-workers-group-without-sources",
            Binary::Fails(|error| {
                matches!(
                    error,
                    SettingsError::InvalidDeclaredNode {
                        cause: InvalidDeclaredNode::ReadingWorkerNamesNoSource { .. }
                    }
                )
            }),
        ),
        (
            "reading-workers-undeclared-source",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::DeclaredNodeReadsUndeclaredSource { .. })),
        ),
        (
            "reading-workers-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::ReadingNodeWithoutInboundIdentity { .. })),
        ),
        (
            "reading-workers-without-worker-pool",
            Binary::NeverReads("the worker pool's issuer, which every reading worker's certificate comes from"),
        ),
        (
            "worker-pool-issuer-missing",
            Binary::NeverReads("the cert-manager issuer every node's certificate comes from"),
        ),
        (
            "worker-pool-shared-cluster-issuer",
            Binary::NeverReads("the issuer's kind, Issuer or ClusterIssuer - the chart refuses this at render time"),
        ),
        (
            "worker-pool-tls-enabled",
            Binary::NeverReads("a second certificate, which the pool's node certificate would replace"),
        ),
        (
            "worker-pool-tls-terminated-elsewhere",
            Binary::NeverReads("a TLS termination the pool's own in-process TLS would replace"),
        ),
    ];

    fn testdata() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../charts/sutura/testdata")
    }

    /// The refusal cases the chart gate holds, by file stem.
    fn refusal_cases() -> Vec<String> {
        let dir = testdata().join("refusals");
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("{} is readable: {error}", dir.display()))
            .filter_map(|entry| {
                let path = entry.expect("a directory entry").path();
                let is_case = path.extension().is_some_and(|ext| ext == "yaml");
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|_| is_case)
                    .map(String::from)
            })
            .collect();
        found.sort();
        found
    }

    /// What the binary would be handed by one unguarded render, or the environment it does not read,
    /// and the render's scratch directory.
    type Handed = (Result<Sources, UnknownEnvironment>, Option<PathBuf>);

    fn sources_of(name: &str, render: &Path) -> Handed {
        let docs = documents(&std::fs::read_to_string(render).expect("a readable render"));
        let Inputs {
            environment,
            directory,
            variables,
        } = pod_inputs(name, &docs, RELEASE);
        let sources = Environment::parse(&environment).map(|environment| {
            let sources = Sources::defaults(environment).with_variables(variables);
            match &directory {
                Some(dir) => sources.with_directory(dir.clone()),
                None => sources,
            }
        });
        (sources, directory)
    }

    #[test]
    fn a_chart_refusal_is_the_binarys_or_a_stated_gap() {
        let declared: Vec<&str> = REFUSALS.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            refusal_cases(),
            declared,
            "every testdata/refusals case declares how the binary stands to it, and no declaration is orphaned"
        );
        let unguarded = testdata().join("golden/unguarded");
        let mut wrong = Vec::new();
        for (name, binary) in &REFUSALS {
            let render = unguarded.join(format!("{name}.yaml"));
            if matches!(binary, Binary::Derivation) {
                if render.is_file() {
                    wrong.push(format!(
                        "{name}: declared a derivation failure, and testdata/golden/unguarded/{name}.yaml renders"
                    ));
                }
                continue;
            }
            if let Binary::NeverReads(what) = binary {
                if render.is_file() {
                    wrong.push(format!(
                        "{name}: declared as refusing {what}, and testdata/golden/unguarded/{name}.yaml renders"
                    ));
                }
                continue;
            }
            if !render.is_file() {
                wrong.push(format!("{name}: no testdata/golden/unguarded/{name}.yaml"));
                continue;
            }
            let (sources, directory) = sources_of(name, &render);
            let loaded = sources.map(|sources| Settings::load(&sources));
            if let Some(dir) = directory {
                std::fs::remove_dir_all(dir).expect("the scratch directory is removed");
            }
            match (binary, loaded) {
                (_, Err(error)) => wrong.push(format!("{name}: the binary does not read the render's environment: {error}")),
                (Binary::Refuses(held), Ok(Err(error))) => match error.reason() {
                    SettingsError::NotFitToServe { refusals } if refusals.iter().any(held) => {}
                    other => wrong.push(format!("{name}: the binary refuses it another way: {other:?}")),
                },
                (Binary::Fails(held), Ok(Err(error))) if !held(error.reason()) => {
                    wrong.push(format!("{name}: the binary refuses it another way: {:?}", error.reason()));
                }
                (Binary::Refuses(held), Ok(Ok(settings))) if serve_refusals(&settings).iter().any(held) => {}
                (Binary::Refuses(_) | Binary::Fails(_), Ok(Ok(_))) => {
                    wrong.push(format!("{name}: the chart refuses it and the binary starts with it"));
                }
                (Binary::Accepts(why), Ok(Err(error))) => wrong.push(format!(
                    "{name}: declared stricter than the binary ({why}), which refuses: {:?}",
                    error.reason()
                )),
                (Binary::Accepts(why), Ok(Ok(settings))) if !serve_refusals(&settings).is_empty() => {
                    wrong.push(format!(
                        "{name}: declared as one the binary accepts ({why}), and `sutura serve` refuses it: {:?}",
                        serve_refusals(&settings)
                    ));
                }
                (Binary::Accepts(_) | Binary::Derivation | Binary::NeverReads(_) | Binary::Fails(_), Ok(_)) => {}
            }
        }
        assert!(
            wrong.is_empty(),
            "a chart refusal the binary does not make:\n{}",
            wrong.join("\n")
        );
    }
}
