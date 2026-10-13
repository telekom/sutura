#![forbid(unsafe_code)]
//! Every golden render of `charts/sutura` is a configuration this binary starts with.
//!
//! The chart's own gate proves a render is what the template produces; this proves the binary
//! accepts it. Read from the RENDERED output, so no second copy of the template's env mapping
//! exists to drift: the `ConfigMap`'s files become the settings directory and the Deployment
//! container's `env` becomes the variables, a `secretKeyRef` standing in as a distinct value.
//!
//! # What this does NOT reach
//!
//! * **The refusal direction.** A chart refusal has no render, so nothing here asserts that the
//!   binary would also have refused it.
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
    use sutura_config::{Environment, Settings, Sources};

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

    /// Why the binary refuses these inputs, or `None` when it starts.
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
            Ok(_) => None,
        }
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
}
