#![forbid(unsafe_code)]
//! `check-attribution` over a scratch workspace, with a fake Cargo answering `cargo metadata`, so
//! the refusal is the gate's own - `generate` and `refuse` together, not a helper beside them.

#[path = "scratch_tree/mod.rs"]
#[expect(
    dead_code,
    reason = "the module's own tests use `seal` and `Fixture`; this integration test does not"
)]
#[cfg(test)]
mod scratch_tree;

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::process::{Command, Output};

    use super::scratch_tree::Tree;

    const MANIFEST: &[u8] = b"[workspace]\nmembers = [\n  \"crates/ours\",\n]\n";
    const LOCK: &[u8] = b"[[package]]\nname = \"ours\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";

    /// The gate's verdict over a workspace whose one member declares `member_licence`.
    fn check_attribution(tag: &str, member_licence: &str) -> Output {
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("resolve the interpreter before building the fake cargo");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("the interpreter path is text");
        let tree = Tree::of(
            tag,
            &[
                ("flake.nix", b"{}\n" as &[u8]),
                ("Cargo.toml", MANIFEST),
                ("Cargo.lock", LOCK),
                ("crates/ours/Cargo.toml", b"[package]\nname = \"ours\"\n"),
            ],
        );
        let metadata = format!(
            r#"{{"packages":[{{"name":"ours","version":"0.1.0","license":{member_licence}}},{{"name":"serde","version":"1.0.0","license":"MIT"}}]}}"#
        );
        let bin = tree.root().join("bin");
        std::fs::create_dir_all(&bin).expect("the fake cargo directory");
        let cargo = bin.join("cargo");
        std::fs::write(&cargo, format!("#!{}\nprintf '%s' '{metadata}'\n", shell.trim())).expect("fake Cargo");
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).expect("executable fake Cargo");
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-attribution")
            .current_dir(tree.root())
            .env("PATH", &bin)
            .env_remove("BASH_ENV")
            .output()
            .expect("execute the real xtask binary")
    }

    #[test]
    fn a_workspace_member_declaring_no_licence_is_refused() {
        let control = check_attribution("attribution-member-licensed", r#""Apache-2.0""#);
        assert!(
            control.status.success(),
            "the control: a licensed member passes\n{}",
            String::from_utf8_lossy(&control.stderr)
        );

        let refused = check_attribution("attribution-member-unlicensed", "null");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert_eq!(refused.status.code(), Some(1), "a member with no licence passed:\n{stderr}");
        assert!(
            stderr.contains("declare no licence") && stderr.contains("ours 0.1.0"),
            "{stderr}"
        );
    }
}
