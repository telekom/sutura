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
//! * **A serve rule stricter than the agent rule.** `sutura serve`'s `/v1` rule
//!   (`Settings::query_surface_refusals`) is asked here through `Settings::agent_surface_refusals`,
//!   which refuses at least what it does with no inbound block. So a change that makes the serve
//!   rule refuse more is not seen here; the serve cells hold that rule itself.
//! * **A refusal outside the posture guard.** The unguarded copy drops only that guard. A refusal
//!   raised while deriving a value from a limit still fails the render, so such a case is declared
//!   `Binary::Derivation` by hand. A `Derivation` case must have no unguarded golden, so one the
//!   guard-less chart renders is refused; a case that really fails derivation, and that the binary
//!   would also refuse, is held by review alone.
//! * **Secret and file contents.** A placeholder token is never empty, and `Settings::load` opens
//!   no key-set or TLS file, so neither a Secret holding `""` nor a missing mount is seen.
//! * **`extraEnv`**, beyond what a golden's values file happens to set.
//! * **A binary without `tls`.** Gated on the feature: the chart's `tls.enabled` render assumes a
//!   binary that can terminate TLS, and every venue that runs this crate's tests enables it.

#![cfg(feature = "tls")]

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use serde::Deserialize as _;
    use serde_norway::Value;
    use sutura_config::{Environment, NotFitToServe, Settings, SettingsError, Sources, UnknownEnvironment};

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

    fn inputs(name: &str, docs: &[Value]) -> Inputs {
        let env = of_kind(docs, "Deployment")["spec"]["template"]["spec"]["containers"][0]["env"]
            .as_sequence()
            .unwrap_or_else(|| panic!("{name}: the container has an env list"));
        let mut environment = None;
        let mut directory = None;
        let mut variables = BTreeMap::new();
        for entry in env {
            let variable = entry["name"].as_str().expect("an env name");
            let value = match entry["value"].as_str() {
                Some(value) => String::from(value),
                None if entry["valueFrom"]["secretKeyRef"].is_mapping() => {
                    format!("{name}-{variable}-placeholder-secret-value")
                }
                None => panic!("{name}: {variable} has neither a value nor a secretKeyRef"),
            };
            match variable {
                "SUTURA_ENVIRONMENT" => environment = Some(value),
                "SUTURA_CONFIG_DIR" => directory = Some(settings_directory(name, of_kind(docs, "ConfigMap"))),
                _ => {
                    variables.insert(String::from(variable), value);
                }
            }
        }
        Inputs {
            environment: environment.unwrap_or_else(|| panic!("{name}: no SUTURA_ENVIRONMENT")),
            directory,
            variables,
        }
    }

    /// Why the binary refuses these inputs, or `None` when `sutura serve` starts with them: its `/v1`
    /// rule is asked through the agent rule (module docs).
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
            Ok(settings) if !settings.agent_surface_refusals().is_empty() => {
                Some(format!("{:?}", settings.agent_surface_refusals()))
            }
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
            Some(format!("ghcr.io/telekom/sutura:{}-performance", app_version()))
        );
    }

    #[test]
    fn the_performance_image_flag_off_keeps_the_app_version_tag() {
        assert_eq!(
            rendered_image("performance-image-off"),
            Some(format!("ghcr.io/telekom/sutura:{}", app_version()))
        );
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
                } = inputs(name, &docs);
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

    /// How the binary stands to one chart refusal case.
    enum Binary {
        /// The binary refuses the same configuration, by a refusal this accepts.
        Refuses(fn(&NotFitToServe) -> bool),
        /// The chart refuses a configuration the binary starts with, on purpose: the reason.
        Accepts(&'static str),
        /// The chart fails deriving a value from a limit, so no configuration reaches the binary.
        Derivation,
    }

    /// Every `testdata/refusals/` case, by name.
    const REFUSALS: [(&str, Binary); 11] = [
        (
            "access-token-missing",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::AccessTokenRequired { .. })),
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
        // `sutura serve` refuses it as `QuerySurfaceWithoutInboundIdentity`, asked through the agent rule.
        (
            "identity-undeclared-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::AgentSurfaceWithoutInboundIdentity)),
        ),
        // `sutura serve` refuses it as `QuerySurfaceOverAnImpersonatingSource`, asked through the agent rule.
        (
            "impersonating-source-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::AgentSurfaceOverAnImpersonatingSource { .. })),
        ),
        ("memory-limit-unparseable", Binary::Derivation),
        (
            "metrics-token-missing",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::MetricsTokenRequired { .. })),
        ),
        // `sutura serve` refuses it as `QuerySurfaceWithoutInboundIdentity`, asked through the agent rule.
        (
            "multi-user-without-inbound",
            Binary::Refuses(|refusal| matches!(refusal, NotFitToServe::AgentSurfaceWithoutInboundIdentity)),
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
        } = inputs(name, &docs);
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
                (Binary::Refuses(held), Ok(Ok(settings))) if settings.agent_surface_refusals().iter().any(held) => {}
                (Binary::Refuses(_), Ok(Ok(_))) => {
                    wrong.push(format!("{name}: the chart refuses it and the binary starts with it"));
                }
                (Binary::Accepts(why), Ok(Err(error))) => wrong.push(format!(
                    "{name}: declared stricter than the binary ({why}), which refuses: {:?}",
                    error.reason()
                )),
                (Binary::Accepts(why), Ok(Ok(settings))) if !settings.agent_surface_refusals().is_empty() => {
                    wrong.push(format!(
                        "{name}: declared as one the binary accepts ({why}), and `sutura serve` refuses it: {:?}",
                        settings.agent_surface_refusals()
                    ));
                }
                (Binary::Accepts(_) | Binary::Derivation, Ok(_)) => {}
            }
        }
        assert!(
            wrong.is_empty(),
            "a chart refusal the binary does not make:\n{}",
            wrong.join("\n")
        );
    }
}
