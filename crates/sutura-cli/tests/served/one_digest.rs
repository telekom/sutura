//! One digest per catalog, across the surfaces that print one: a served answer, `sutura catalog`
//! and `sutura prompt`, all over the same configuration directory.
//!
//! The digest covers the contribution manifest, which is keyed by the catalog's name. A catalog the
//! configuration calls `sales` is the case that tells a name read from the configuration from the
//! default `model`: a command that names the catalog by a constant prints another digest.

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    use sutura_config::Environment;

    use crate::harness::{Served, TOKEN, config_path, example_root, recurring_revenue_june, settings, start_configured, v1};

    /// The example, served with its catalog declared as `sales`, and the digest an answer reports.
    fn served_as_sales(case: &str) -> (Served, String) {
        let declared = settings(&example_root());
        let renamed = declared.replace("name: \"model\"", "name: \"sales\"");
        assert_ne!(
            declared, renamed,
            "the example deployment no longer declares a catalog called model"
        );
        let served = start_configured(case, &renamed);
        let reply = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(TOKEN),
            &recurring_revenue_june(),
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let digest = reply.json()["provenance"]["definition_digest"]
            .as_str()
            .expect("an answer carries a digest")
            .to_owned();
        (served, digest)
    }

    /// `sutura <args>` with this shell's own `SUTURA*` variables removed, the development
    /// environment and, when given, the configuration directory the deployment was written to;
    /// its standard output.
    fn sutura(args: &[&OsStr], config_dir: Option<&Path>) -> String {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sutura"));
        command.args(args);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("SUTURA") {
                command.env_remove(key);
            }
        }
        command.env("SUTURA_ENVIRONMENT", Environment::Development.as_str());
        if let Some(dir) = config_dir {
            command.env(sutura_config::CONFIG_DIR_VARIABLE, dir);
        }
        let output = command.output().expect("the sutura binary runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[test]
    fn a_prompt_over_a_configuration_argument_names_the_catalog_the_deployment_does() {
        // The configuration directory is the ARGUMENT here and the variable is unset, which is the
        // one place the command reads its settings from the command line.
        let (_served, digest) = served_as_sales("one-digest-prompt");
        let catalog = example_root().join("catalog");
        let config_dir = config_path("one-digest-prompt");
        let prompt = sutura(&[OsStr::new("prompt"), catalog.as_os_str(), config_dir.as_os_str()], None);
        let stated = prompt.lines().find(|line| line.contains("Definitions digest:"));
        assert!(
            prompt.contains(&digest),
            "the prompt does not carry the digest the served deployment reports ({digest}): {stated:?}"
        );
    }
}
