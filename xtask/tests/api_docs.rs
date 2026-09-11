//! Exercise the real API-doc gate with fake Cargo and renderer process boundaries.

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;
    use std::process::{Command, Output};

    const PAGE: &str = "<!-- GENERATED FILE - do not edit. -->\nfixture\n";
    const METADATA_CALL: &str = "<metadata><--format-version><1><--locked><--no-deps>\n";
    const LIBRARY_CALL: &str = "<rustdoc><-q><-p><doc-library><--all-features><--profile><ci><--><-Z><unstable-options><--output-format><json><--document-private-items>\n";
    const RENDERED: &str = "<render><library-only>\n";

    /// The `just api` writer the gate compares its own rustdoc flags against, as a CONTINUED
    /// shell line - the shape `nix/api-docs.nix` really has, so the fixture exercises the join
    /// rather than a one-line convenience. The cargo arguments before `--` differ from the
    /// gate's on purpose; only what rustdoc reads is held equal.
    const WRITER: &str = r#"
pkgs.writeShellApplication {
  text = ''
    # cargo rustdoc is named in this comment and must not be read as the invocation.
    cargo rustdoc -q -p "$lib" --all-features --profile ci -- \
      -Z unstable-options --output-format json --document-private-items
  '';
}
"#;

    const CARGO: &str = r#"
set -eu
printf '<%s>' "$@" >>"$SUTURA_DOC_LEDGER"
printf '\n' >>"$SUTURA_DOC_LEDGER"
if [ "$1" = metadata ]; then
  [ "$*" = 'metadata --format-version 1 --locked --no-deps' ] || exit 42
  printf '%s\n' "$SUTURA_DOC_METADATA"
  exit 0
fi
[ "${CARGO_UNSTABLE_CODEGEN_BACKEND+x}" != x ] || exit 42
[ "${CARGO_PROFILE_DEV_CODEGEN_BACKEND+x}" != x ] || exit 42
docs_target=${CARGO_TARGET_DIR:-"$PWD/target"}
if [ "$4" = doc-library ]; then
  [ "$*" = 'rustdoc -q -p doc-library --all-features --profile ci -- -Z unstable-options --output-format json --document-private-items' ] || exit 42
  printf '{}\n' >"$docs_target/doc/doc_library.json"
  exit 0
fi
[ "$*" = "rustdoc -q -p $4 --all-features --bin $7 --profile ci -- -Z unstable-options --output-format json --document-private-items" ] || exit 42
case "$4/$7" in
  bin-one/tool-a|bin-two/doc-library-extra|bin-two/doc_library|bin-two/tool-b|bin-two/tool-c) ;;
  *) exit 42 ;;
esac
printf '<binary-target><%s>\n' "${docs_target#"$PWD/"}" >>"$SUTURA_DOC_LEDGER"
printf 'binary %s\n' "$7" >"$docs_target/doc/${7//-/_}.json"
if [ "$SUTURA_DOC_CASE" = broken ] && [ "$4/$7" = bin-two/tool-c ]; then
  printf 'unresolved link in final binary\n' >&2
  exit 23
fi
exit 0
"#;

    const RENDERER: &str = r#"
set -eu
# The generator's own self-test, which check-api-docs now runs first: it renders a
# fixture and exits non-zero on a regression. The real script passes on it; the fake
# must too, without touching the render ledger the cases below assert on.
if [ "$#" -eq 2 ] && [ "$2" = --self-test ]; then
  exit 0
fi
[ "$#" -eq 3 ] || exit 42
[ "$1" = "$PWD/docs/.tools/rustdoc_to_markdown.py" ] || exit 42
[ "$2" = "$PWD/target/doc/doc_library.json" ] || exit 42
if [ "$(<"$2")" != '{}' ]; then
  printf 'library JSON was overwritten by a binary\n' >&2
  exit 42
fi
printf '<render><library-only>\n' >>"$SUTURA_DOC_LEDGER"
printf '%s' "$SUTURA_DOC_PAGE" >"$3/doc-library.md"
"#;

    #[derive(Debug)]
    struct Observed {
        output: Output,
        ledger: String,
    }

    fn executable(path: &Path, shell: &str, body: &str) {
        std::fs::write(path, format!("#!{shell}\n{body}")).expect("write the fake process");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("make the fake executable");
    }

    #[expect(
        clippy::create_dir,
        reason = "a PID-and-case-keyed fixture must be newly allocated, never reused"
    )]
    fn observe(case: &str) -> Observed {
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("resolve the interpreter before replacing PATH");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("interpreter path");
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("api-docs-{case}-{}", std::process::id()));
        std::fs::create_dir_all(Path::new(env!("CARGO_TARGET_TMPDIR"))).expect("scratch parent");
        std::fs::create_dir(&root).expect("exclusive fixture root");
        for directory in ["bin", "docs/.tools", "docs/api", "nix", "target/doc"] {
            std::fs::create_dir_all(root.join(directory)).expect("fixture directories");
        }
        std::fs::write(root.join("flake.nix"), "{}\n").expect("root marker");
        std::fs::write(root.join("nix/api-docs.nix"), WRITER).expect("the writer the gate must agree with");
        std::fs::write(root.join("Cargo.toml"),
            "[workspace]\nmembers = [\n\"crates/doc-library\",\n\"crates/bin-one\",\n\"crates/bin-two\",\n\"crates/macro-only\",\n]\n[workspace.lints.rustdoc]\nbroken_intra_doc_links = \"forbid\"\n"
        ).expect("armed workspace manifest");
        let mut packages = Vec::new();
        for (name, targets) in [
            (
                "doc-library",
                serde_json::json!([{"kind":["lib"],"name":"doc_library"},{"kind":["bin"],"name":"not-binary-only"}]),
            ),
            ("bin-one", serde_json::json!([{"kind":["bin"],"name":"tool-a"}])),
            (
                "bin-two",
                serde_json::json!([{"kind":["bin"],"name":"doc-library-extra"},{"kind":["bin"],"name":"tool-b"},{"kind":["bin"],"name":"tool-c"}]),
            ),
            ("macro-only", serde_json::json!([{"kind":["proc-macro"],"name":"derive"}])),
        ] {
            let dir = root.join("crates").join(name);
            std::fs::create_dir_all(&dir).expect("member directory");
            let manifest = dir.join("Cargo.toml");
            std::fs::write(
                &manifest,
                format!("[package]\nname = \"{name}\"\n[lints]\nworkspace = true\n"),
            )
            .expect("inheriting member manifest");
            let targets = if name == "bin-two" {
                target_answer(case, targets)
            } else {
                targets
            };
            packages.push(serde_json::json!({"name":name,"manifest_path":manifest,"targets":targets}));
        }
        let metadata = serde_json::json!({"packages":packages,"target_directory":root.join("target")});
        std::fs::write(root.join("docs/.tools/rustdoc_to_markdown.py"), "# fake renderer input\n").expect("generator marker");
        std::fs::write(root.join("docs/api/doc-library.md"), PAGE).expect("committed library page");
        let cargo = root.join("bin/cargo");
        let renderer = root.join("bin/renderer");
        executable(&cargo, shell.trim(), CARGO);
        executable(&renderer, shell.trim(), RENDERER);
        let ledger = root.join("calls");
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-api-docs")
            .current_dir(&root)
            .env("PATH", root.join("bin"))
            .env("CARGO", &cargo)
            .env("CARGO_UNSTABLE_CODEGEN_BACKEND", "fixture-backend")
            .env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "fixture-backend")
            .env("SUTURA_API_DOCS_PROFILE", "ci")
            .env("SUTURA_API_DOCS_PYTHON", &renderer)
            .env("SUTURA_DOC_CASE", case)
            .env("SUTURA_DOC_METADATA", metadata.to_string())
            .env("SUTURA_DOC_LEDGER", &ledger)
            .env("SUTURA_DOC_PAGE", PAGE)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("BASH_ENV")
            .output();
        let calls = std::fs::read_to_string(ledger);
        let cleanup = std::fs::remove_dir_all(&root);
        cleanup.expect("remove the fixture before verdict assertions");
        Observed {
            output: output.expect("run the real gate"),
            ledger: calls.expect("read the invocation ledger"),
        }
    }

    fn target_answer(case: &str, targets: serde_json::Value) -> serde_json::Value {
        match case {
            "missing-targets" => serde_json::Value::Null,
            "empty-targets" => serde_json::json!([]),
            "missing-kind" => serde_json::json!([{"name":"tool-c"}]),
            "non-text-kind" => serde_json::json!([{"kind":[3],"name":"tool-c"}]),
            "missing-name" => serde_json::json!([{"kind":["bin"]}]),
            "empty-name" => serde_json::json!([{"kind":["bin"],"name":""}]),
            // A binary named after the library crate: one rustdoc JSON file, two writers.
            "collision" => serde_json::json!([{"kind":["bin"],"name":"doc_library"}]),
            _ => targets,
        }
    }

    fn documented() -> String {
        use std::fmt::Write as _;
        let mut expected = String::from(METADATA_CALL);
        expected.push_str(LIBRARY_CALL);
        for (package, binary) in [
            ("bin-one", "tool-a"),
            ("bin-two", "doc-library-extra"),
            ("bin-two", "tool-b"),
            ("bin-two", "tool-c"),
        ] {
            writeln!(expected,
                "<rustdoc><-q><-p><{package}><--all-features><--bin><{binary}><--profile><ci><--><-Z><unstable-options><--output-format><json><--document-private-items>"
            ).expect("expected invocation ledger");
            expected.push_str("<binary-target><target>\n");
        }
        expected
    }

    #[test]
    fn a_bad_final_binary_refuses_before_rendering() {
        let observed = observe("broken");
        assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
        assert_eq!(observed.ledger, documented(), "every binary reached, renderer unreached");
        let stderr = String::from_utf8(observed.output.stderr).expect("diagnostics");
        assert!(stderr.contains("bin-two/tool-c"), "{stderr}");
        assert!(stderr.contains("unresolved link in final binary"), "{stderr}");
    }

    #[test]
    fn every_binary_only_target_is_checked_but_only_library_json_is_rendered() {
        let observed = observe("clean");
        assert_eq!(observed.output.status.code(), Some(0), "{observed:?}");
        let mut expected = documented();
        expected.push_str(RENDERED);
        assert_eq!(
            observed.ledger, expected,
            "multiple and differently named binaries must each be checked"
        );
    }

    // The binary runs share the library target directory, which is what keeps them cheap - a
    // directory of their own made the first one rebuild the whole dependency closure. The
    // fixture's `<binary-target><target>` ledger line above is what says they share it, and this
    // is the refusal that makes the sharing sound: a binary named after a library crate writes
    // that crate's page input, and a page rendered from a binary's surface reads as a drift.
    #[test]
    fn a_binary_named_after_a_library_crate_refuses_before_documenting_anything() {
        let observed = observe("collision");
        assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
        assert_eq!(
            observed.ledger, METADATA_CALL,
            "the refusal has to come before any rustdoc run, or the overwrite has already happened"
        );
        let stderr = String::from_utf8(observed.output.stderr).expect("diagnostics");
        assert!(
            stderr.contains("bin-two/doc_library"),
            "name the binary, because renaming it is the fix: {stderr}"
        );
        assert!(
            stderr.contains("doc_library.json"),
            "and name the file the two would share: {stderr}"
        );
    }

    #[test]
    fn incomplete_binary_metadata_cannot_be_reported_clean() {
        let mut failures = Vec::new();
        for case in [
            "missing-targets",
            "empty-targets",
            "missing-kind",
            "non-text-kind",
            "missing-name",
            "empty-name",
        ] {
            let observed = observe(case);
            // The diagnostic has to NAME the package whose targets are malformed. Exit 1 with a
            // bare metadata ledger is also what every precondition before `binary_targets`
            // produces - the arming check and the writer check - so without this the whole loop
            // would pass over a gate that never reached the targets at all.
            let named = String::from_utf8_lossy(&observed.output.stderr).contains("bin-two");
            if observed.output.status.code() != Some(1) || observed.ledger != METADATA_CALL || !named {
                failures.push(format!("{case}: {observed:?}"));
            }
        }
        assert!(
            failures.is_empty(),
            "incomplete targets reached rustdoc or rendering: {failures:#?}"
        );
    }
}
