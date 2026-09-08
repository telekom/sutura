//! The real gate must finish resolved-feature admission before it compiles either shipped root.
//! Fake Cargo owns only the subprocess boundary; no process-global environment or cwd is changed.

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fmt::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use std::process::{Command, Output};

    const TARGETS: &[&str] = &[
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];

    const SHIPPED: &str = r#"
let
  crossTargets = [
    "aarch64-unknown-linux-gnu"
    "x86_64-unknown-linux-musl"
    "aarch64-unknown-linux-musl"
  ];
  binaries = [
    { package = "root-one"; }
    { package = "root-two"; }
  ];
  hostRustTarget = {
    "x86_64-linux" = "x86_64-unknown-linux-gnu";
    "aarch64-linux" = "aarch64-unknown-linux-gnu";
    "x86_64-darwin" = "x86_64-apple-darwin";
    "aarch64-darwin" = "aarch64-apple-darwin";
  }.${system} or null;
in {}
"#;

    const CARGO: &str = r#"
set -eu
printf '<%s>' "$@" >>"$SUTURA_FEATURE_LEDGER"
printf '\n' >>"$SUTURA_FEATURE_LEDGER"
case "$1" in
  check|clippy)
    if [ "$SUTURA_FEATURE_CASE" = check-fails ] && [ "$1" = check ] && [ "$4" = root-one ]; then
      exit 23
    fi
    exit 0
    ;;
  tree) ;;
  *) exit 42 ;;
esac
package=$5
target=$7
expected="tree --offline --locked --package $package --target $target --edges normal,no-proc-macro --prefix none --format {p}|{f} --no-dedupe --color never"
[ "$*" = "$expected" ] || exit 42
last=false
if [ "$package" = root-two ] && [ "$target" = aarch64-apple-darwin ]; then last=true; fi
if "$last"; then
  case "$SUTURA_FEATURE_CASE" in
    empty-output) exit 0 ;;
    invalid-utf8) printf '\377'; exit 0 ;;
    malformed) printf 'sutura-domain not-a-version|\n'; exit 0 ;;
    missing-root) printf 'sutura-domain v0.1.0|\n'; exit 0 ;;
    missing-domain) printf '%s v0.1.0|\n' "$package"; exit 0 ;;
    bad-features) printf '%s v0.1.0|\nsutura-domain v0.1.0|default,,agreement\n' "$package"; exit 0 ;;
  esac
fi
printf '%s v0.1.0 (%s/crates/%s)|\n' "$package" "$PWD" "$package"
printf 'sutura-domain v0.1.0 (%s/crates/sutura-domain)|\n' "$PWD"
printf 'sutura-domain-extra v0.1.0|agreement\n'
printf 'other v1.2.3-alpha.1+build (https://example.com/source#commit)|default\n'
if "$last" && [ "$SUTURA_FEATURE_CASE" = forbidden ]; then
  printf 'sutura-domain v0.1.0 (%s/crates/sutura-domain)|agreement,default\n' "$PWD"
else
  printf 'sutura-domain v0.1.0 (%s/crates/sutura-domain)|agreement-extra,default\n' "$PWD"
fi
if "$last" && [ "$SUTURA_FEATURE_CASE" = nonzero ]; then exit 23; fi
"#;

    #[derive(Debug)]
    struct Observed {
        output: Output,
        ledger: String,
    }

    #[expect(clippy::create_dir, reason = "the PID-keyed fixture must be newly allocated, never reused")]
    fn observe(case: &str) -> Observed {
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("resolve the fixture interpreter before replacing PATH");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("the interpreter path is text");
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("default-features-{case}-{}", std::process::id()));
        std::fs::create_dir_all(Path::new(env!("CARGO_TARGET_TMPDIR"))).expect("the integration scratch parent");
        std::fs::create_dir(&root).expect("an exclusive fixture directory");
        std::fs::create_dir_all(root.join("nix")).expect("the declaration directory");
        std::fs::create_dir_all(root.join("bin")).expect("the fake command directory");
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("the workspace marker");
        std::fs::write(root.join("flake.nix"), "{}\n").expect("the Nix marker");
        std::fs::write(root.join("nix/shipped.nix"), SHIPPED).expect("the actual gate's declaration input");
        let cargo = root.join("bin/cargo");
        std::fs::write(&cargo, format!("#!{}\n{CARGO}", shell.trim())).expect("fake Cargo");
        std::fs::set_permissions(cargo, std::fs::Permissions::from_mode(0o755)).expect("executable fake Cargo");
        let ledger = root.join("calls");
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-default-features")
            .current_dir(&root)
            .env("PATH", root.join("bin"))
            .env("SUTURA_FEATURE_CASE", case)
            .env("SUTURA_FEATURE_LEDGER", &ledger)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("BASH_ENV")
            .output();
        let calls = std::fs::read_to_string(ledger);
        let cleanup = std::fs::remove_dir_all(&root);
        cleanup.expect("remove the owned fixture before verdict assertions");
        Observed {
            output: output.expect("execute the real xtask binary"),
            ledger: calls.expect("Cargo invocation ledger"),
        }
    }

    fn queries() -> String {
        let mut expected = String::new();
        for package in ["root-one", "root-two"] {
            for target in TARGETS {
                writeln!(expected,
                    "<tree><--offline><--locked><--package><{package}><--target><{target}><--edges><normal,no-proc-macro><--prefix><none><--format><{{p}}|{{f}}><--no-dedupe><--color><never>"
                ).expect("writing the expected command ledger");
            }
        }
        expected
    }

    const fn expensive() -> &'static str {
        "<check><--all-targets><--package><root-one>\n\
         <clippy><--all-targets><--package><root-one><--><-D><warnings>\n\
         <check><--all-targets><--package><root-two>\n\
         <clippy><--all-targets><--package><root-two><--><-D><warnings>\n"
    }

    #[test]
    fn a_forbidden_feature_at_the_last_root_target_prevents_every_compile() {
        let observed = observe("forbidden");
        assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
        assert_eq!(
            observed.ledger,
            queries(),
            "no expensive command may precede complete admission"
        );
        let stderr = String::from_utf8(observed.output.stderr).expect("gate diagnostics");
        for detail in ["root-two", "aarch64-apple-darwin", "sutura-domain", "agreement"] {
            assert!(stderr.contains(detail), "the refusal must attribute {detail}: {stderr}");
        }
    }

    #[test]
    fn complete_feature_queries_precede_the_unchanged_compile_and_lint_passes() {
        let mut failures = Vec::new();
        for (case, expected_exit) in [("clean", 0), ("check-fails", 1)] {
            let observed = observe(case);
            let mut expected = queries();
            expected.push_str(expensive());
            if observed.output.status.code() != Some(expected_exit) || observed.ledger != expected {
                failures.push(format!("{case}: {observed:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "query coverage, ordering, or original passes changed: {failures:#?}"
        );
    }

    #[test]
    fn an_unreadable_or_incomplete_last_answer_is_not_a_clean_feature_set() {
        let mut failures = Vec::new();
        for case in [
            "empty-output",
            "invalid-utf8",
            "malformed",
            "missing-root",
            "missing-domain",
            "bad-features",
            "nonzero",
        ] {
            let observed = observe(case);
            if observed.output.status.code() != Some(1) || observed.ledger != queries() {
                failures.push(format!("{case}: {observed:?}"));
            }
        }
        assert!(failures.is_empty(), "unmeasured features reached compilation: {failures:#?}");
    }
}
