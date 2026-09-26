#![forbid(unsafe_code)]
//! The verdict loop in `run()` that turns a non-zero `nextest` exit into `Verdict::Fail` is the
//! path no unit test in the gate's own module reaches: every cell there asserts on `invocation`
//! or `profile_for`, the pieces `run` is built from, and the wiring test reads files rather than
//! running the gate. So the loop is compiled by the gate and executed by none - the same state the
//! module's own header says it exists to end, one category up. This integration test drives the
//! REAL `run` (the xtask binary's entry point) over a fake `cargo` that always exits 1, and
//! asserts the process exits 1 (`Verdict::Fail`).
//!
//! Lives in `xtask/tests/` rather than an inline `#[cfg(test)] mod tests` because `env!(
//! "CARGO_BIN_EXE_xtask")` is a compile-time `env!` that cargo sets for integration test targets
//! and not for a binary crate's own unit tests - the macro fails at compile time in that position.

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
    use std::process::Command;

    use super::scratch_tree::Tree;

    /// The gate's registered task name.
    const TASK: &str = "check-default-feature-tests";

    /// A `nix/shipped.nix` with one package: `shipped_or_fail` parses the `binaries` list, so the
    /// gate reaches the verdict loop and calls the fake `cargo`.
    const SHIPPED: &str = "binaries = [\n  { package = \"one-package\"; }\n];\n";

    #[test]
    fn a_failing_invocation_drives_run_to_fail() {
        // The fake `cargo` is the pattern `xtask/tests/default_features.rs` established: a shell
        // script on a custom `PATH`, so `Command::new("cargo")` inside `run` finds it. No process-
        // global `set_var` (forbidden by `unsafe_code`) is needed - the child `Command` carries its
        // own `PATH`.
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("resolve the interpreter before building the fake cargo");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("the interpreter path is text");
        let tree = Tree::of(
            "default-feature-tests-run-fail",
            &[
                ("Cargo.toml", b"[workspace]\n" as &[u8]),
                ("flake.nix", b"{}\n"),
                ("nix/shipped.nix", SHIPPED.as_bytes()),
            ],
        );
        let root = tree.root().to_path_buf();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("the fake cargo directory");
        let cargo = bin.join("cargo");
        std::fs::write(&cargo, format!("#!{}\nexit 1\n", shell.trim())).expect("fake Cargo");
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).expect("executable fake Cargo");
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg(TASK)
            .current_dir(&root)
            .env("PATH", &bin)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("BASH_ENV")
            .output()
            .expect("execute the real xtask binary");
        drop(tree);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(1),
            "a failing nextest must drive run to Verdict::Fail:\n{stderr}"
        );
        assert!(
            stderr.contains("FAILED - 1: one-package"),
            "the failure has to be the verdict loop's, naming the package:\n{stderr}"
        );
    }
}
