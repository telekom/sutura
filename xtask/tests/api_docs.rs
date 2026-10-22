#![forbid(unsafe_code)]
//! Exercise the real API-doc gate with fake Cargo and renderer process boundaries.

// The fixture tree is a [`scratch_tree::Tree`] - the sweep-before-create and the `Drop` sweep the
// module holds are the two halves #938's leftover trap had - rather than the exclusive
// `create_dir` on a pid-keyed path this file carried until #938. The module carries `seal` and
// `Fixture` for the in-tree unit tests; this file does not use them, so the include expects
// `dead_code` on it rather than weakening the module for everyone.
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
    use std::path::Path;
    use std::process::{Command, Output};

    use super::scratch_tree::Tree;

    const PAGE: &str = "<!-- GENERATED FILE - do not edit. -->\nfixture\n";
    /// The one line of the real generator the link scan reads: the schemes it will leave alone.
    const GENERATOR: &str = "REAL_SCHEMES = (\"http\", \"https\")\n";
    const METADATA_CALL: &str = "<metadata><--format-version><1><--locked><--no-deps>\n";
    /// ONE cargo invocation documents every member. The rustdoc flags are NOT on this line -
    /// `cargo doc` takes no trailing rustdoc arguments - so the fake asserts them out of
    /// `RUSTDOCFLAGS` instead, which is the only thing that proves they reached the child.
    const DOC_CALL: &str = "<doc><-q><--no-deps><--workspace><--all-features><--profile><ci>\n";
    const WROTE_JSON: &str = "<documented><target>\n";
    const RENDERED: &str = "<render><library-only>\n";

    /// The `just api` writer the gate compares itself against - the shape `nix/api-docs.nix`
    /// really has. Two anchored lines now: the flags, which travel in the environment, and the
    /// selection arguments, which decide feature unification. The rest of the cargo line differs
    /// from the gate's on purpose; only those two things are held equal.
    const WRITER: &str = r#"
pkgs.writeShellApplication {
  text = ''
    # cargo doc is named in this comment and must not be read as the invocation.
    export RUSTDOCFLAGS="-Z unstable-options --output-format json --document-private-items"
    cargo doc -q --no-deps --workspace --all-features --profile ci
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
# The flags are on no argument list now, so this is the ONLY witness that they reached rustdoc.
[ "$RUSTDOCFLAGS" = '-Z unstable-options --output-format json --document-private-items' ] || exit 42
docs_target=${CARGO_TARGET_DIR:-"$PWD/target"}
[ "$*" = 'doc -q --no-deps --workspace --all-features --profile ci' ] || exit 42
if [ "$SUTURA_DOC_CASE" = broken ]; then
  printf 'unresolved link in a binary target\n' >&2
  exit 23
fi
# One run documents every member: the library page input, and each binary-only target for links.
printf '{}\n' >"$docs_target/doc/doc_library.json"
for binary in tool-a doc-library-extra tool-b tool-c; do
  printf 'binary %s\n' "$binary" >"$docs_target/doc/${binary//-/_}.json"
done
printf '<documented><%s>\n' "${docs_target#"$PWD/"}" >>"$SUTURA_DOC_LEDGER"
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

    /// The renderer of the generated-pages fixture: the one above, except that it writes to the page
    /// name the case asks for, which is how a library crate comes to have no page.
    const RENDERER_GENERATED: &str = r#"
set -eu
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
printf '%s' "$SUTURA_DOC_PAGE" >"$3/$SUTURA_DOC_OUT"
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

    fn observe(case: &str) -> Observed {
        let shell = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("resolve the interpreter before replacing PATH");
        assert!(shell.status.success());
        let shell = String::from_utf8(shell.stdout).expect("interpreter path");
        let tree = Tree::of(
            &format!("api-docs-{case}"),
            &[
                ("flake.nix", b"{}\n" as &[u8]),
                ("nix/api-docs.nix", WRITER.as_bytes()),
                (
                    "Cargo.toml",
                    b"[workspace]\nmembers = [\n\"crates/doc-library\",\n\"crates/bin-one\",\n\"crates/bin-two\",\n\"crates/macro-only\",\n]\n[workspace.lints.rustdoc]\nbroken_intra_doc_links = \"forbid\"\n",
                ),
                ("docs/.tools/rustdoc_to_markdown.py", b"# fake renderer input\n"),
                ("docs/api/doc-library.md", PAGE.as_bytes()),
            ],
        );
        // macOS resolves `/var` through a symlink, and the gate's renderer compares its
        // arguments against `$PWD` - which bash takes from `getcwd()` and therefore canonical.
        // A fixture root built from `temp_dir()` is the symlinked form, so the fake renderer's
        // equality checks would compare a symlinked argument against a canonical `$PWD` and fail
        // before rendering. Canonicalizing the root once makes the two agree on every host.
        let root = tree.root().canonicalize().expect("canonical fixture root");
        for directory in ["bin", "target/doc"] {
            std::fs::create_dir_all(root.join(directory)).expect("fixture directories");
        }
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
        drop(tree);
        Observed {
            output: output.expect("run the real gate"),
            ledger: calls.expect("read the invocation ledger"),
        }
    }

    /// What a run over the fixture that commits no page left behind.
    ///
    /// Apart from `observe`, which the three refusal cells further down still call exactly as on
    /// `main`: those hold behaviour this change did not touch, and the gate reads an edited helper as
    /// an edit to each of its callers.
    #[derive(Debug)]
    struct Generated {
        output: Output,
        ledger: String,
        /// What the run left in the directory it was told to keep the pages in, if anything.
        kept: Option<String>,
    }

    /// Every workspace member of the generated-pages fixture: a library, two binary-only packages
    /// and a proc-macro, as `cargo metadata` would list them.
    fn fixture_members(root: &Path, case: &str) -> Vec<serde_json::Value> {
        let lib = serde_json::json!([{"kind":["lib"],"name":"doc_library"},{"kind":["bin"],"name":"not-binary-only"}]);
        let one = serde_json::json!([{"kind":["bin"],"name":"tool-a"}]);
        let two = target_answer(
            case,
            serde_json::json!([{"kind":["bin"],"name":"doc-library-extra"},{"kind":["bin"],"name":"tool-b"},{"kind":["bin"],"name":"tool-c"}]),
        );
        let derive = serde_json::json!([{"kind":["proc-macro"],"name":"derive"}]);
        [
            ("doc-library", lib),
            ("bin-one", one),
            ("bin-two", two),
            ("macro-only", derive),
        ]
        .into_iter()
        .map(|(name, targets)| {
            let manifest = root.join("crates").join(name).join("Cargo.toml");
            std::fs::create_dir_all(manifest.parent().expect("a member directory")).expect("member directory");
            std::fs::write(
                &manifest,
                format!("[package]\nname = \"{name}\"\n[lints]\nworkspace = true\n"),
            )
            .expect("inheriting member manifest");
            serde_json::json!({"name":name,"manifest_path":manifest,"targets":targets})
        })
        .collect()
    }

    fn observe_generated(case: &str) -> Generated {
        let bash = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", "command -v bash"])
            .env_remove("BASH_ENV")
            .output()
            .expect("resolve the interpreter before replacing PATH");
        assert!(bash.status.success());
        let bash = String::from_utf8(bash.stdout).expect("interpreter path");
        let manifest = "[workspace]\nmembers = [\n\"crates/doc-library\",\n\"crates/bin-one\",\n\"crates/bin-two\",\n\"crates/macro-only\",\n]\n[workspace.lints.rustdoc]\nbroken_intra_doc_links = \"forbid\"\n";
        let tree = Tree::of(
            &format!("api-docs-generated-{case}"),
            &[
                ("flake.nix", b"{}\n" as &[u8]),
                ("nix/api-docs.nix", WRITER.as_bytes()),
                ("Cargo.toml", manifest.as_bytes()),
                ("docs/.tools/rustdoc_to_markdown.py", GENERATOR.as_bytes()),
            ],
        );
        // macOS resolves `/var` through a symlink and the renderer compares its arguments with `$PWD`.
        let root = tree.root().canonicalize().expect("canonical fixture root");
        for directory in ["bin", "target/doc"] {
            std::fs::create_dir_all(root.join(directory)).expect("fixture directories");
        }
        let metadata = serde_json::json!({"packages":fixture_members(&root, case),"target_directory":root.join("target")});
        executable(&root.join("bin/cargo"), bash.trim(), CARGO);
        executable(&root.join("bin/renderer"), bash.trim(), RENDERER_GENERATED);
        let ledger = root.join("calls");
        let out_name = if case == "no-page" { "index.md" } else { "doc-library.md" };
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-api-docs")
            .current_dir(&root)
            .env("PATH", root.join("bin"))
            .env("CARGO", root.join("bin/cargo"))
            .env("CARGO_UNSTABLE_CODEGEN_BACKEND", "fixture-backend")
            .env("CARGO_PROFILE_DEV_CODEGEN_BACKEND", "fixture-backend")
            .env("SUTURA_API_DOCS_PROFILE", "ci")
            .env("SUTURA_API_DOCS_PYTHON", root.join("bin/renderer"))
            .env("SUTURA_API_DOCS_KEEP", root.join("kept"))
            .env("SUTURA_DOC_CASE", case)
            .env("SUTURA_DOC_METADATA", metadata.to_string())
            .env("SUTURA_DOC_LEDGER", &ledger)
            .env("SUTURA_DOC_PAGE", page_for(case))
            .env("SUTURA_DOC_OUT", out_name)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("BASH_ENV")
            .output();
        let calls = std::fs::read_to_string(&ledger);
        let kept = std::fs::read_to_string(root.join("kept/doc-library.md")).ok();
        drop(tree);
        Generated {
            output: output.expect("run the real gate"),
            ledger: calls.expect("read the invocation ledger"),
            kept,
        }
    }

    /// What the fake renderer writes. The tree commits no page: this is the whole of the reference.
    fn page_for(case: &str) -> String {
        match case {
            "dead-link" => format!("{PAGE}\nSee [`Q`](crate::plan::QueryPlan).\n"),
            _ => String::from(PAGE),
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

    /// The ledger of a run that documented everything: metadata, the one cargo call, and the
    /// JSON it wrote. A second cargo line appearing here is the serial loop coming back.
    fn documented() -> String {
        let mut expected = String::from(METADATA_CALL);
        expected.push_str(DOC_CALL);
        expected.push_str(WROTE_JSON);
        expected
    }

    /// A rustdoc failure anywhere in the workspace run refuses before a page is rendered.
    ///
    /// The gate no longer wraps the diagnostic with a package name, because it no longer knows
    /// which unit failed - rustdoc's own output does, and passing it through unaltered is what
    /// keeps the failure attributable. So the assertion is on rustdoc's message reaching the
    /// reader and on the renderer never running, not on a name the gate would have to invent.
    #[test]
    fn a_failed_unit_refuses_before_rendering() {
        let observed = observe("broken");
        assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
        let mut expected = String::from(METADATA_CALL);
        expected.push_str(DOC_CALL);
        assert_eq!(
            observed.ledger, expected,
            "cargo reached, no JSON written, renderer unreached"
        );
        let stderr = String::from_utf8(observed.output.stderr).expect("diagnostics");
        assert!(stderr.contains("unresolved link in a binary target"), "{stderr}");
        assert!(
            stderr.contains("cargo doc --workspace --no-deps"),
            "name the invocation: {stderr}"
        );
    }

    /// The tree commits no page, and the gate still passes: it generates, then reads what it made.
    #[test]
    fn a_tree_with_no_committed_page_generates_and_passes() {
        let observed = observe_generated("clean");
        assert_eq!(observed.output.status.code(), Some(0), "{observed:?}");
        let stdout = String::from_utf8(observed.output.stdout).expect("verdict");
        assert!(stdout.contains("1 page(s) generated and read"), "{stdout}");
    }

    /// The nix check publishes through this: a clean run leaves the pages it judged where it is told,
    /// and a refused one leaves nothing a workflow could copy.
    #[test]
    fn a_clean_run_keeps_its_pages_and_a_refused_run_keeps_none() {
        let clean = observe_generated("clean");
        assert_eq!(clean.kept.as_deref(), Some(PAGE), "{clean:?}");
        let refused = observe_generated("dead-link");
        assert_eq!(refused.output.status.code(), Some(1), "{refused:?}");
        assert_eq!(refused.kept, None, "{refused:?}");
    }

    /// A destination mkdocs publishes dead is a finding on the page it came from, before any publish.
    #[test]
    fn a_dead_destination_on_a_generated_page_fails_the_gate() {
        let observed = observe_generated("dead-link");
        assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
        let mut expected = documented();
        expected.push_str(RENDERED);
        assert_eq!(observed.ledger, expected, "the page was generated before it was judged");
        let stderr = String::from_utf8(observed.output.stderr).expect("diagnostics");
        assert!(stderr.contains("does not generate cleanly"), "{stderr}");
        assert!(stderr.contains("doc-library.md") && stderr.contains("line 4"), "{stderr}");
    }

    /// A library crate the generator wrote nothing for is named, so the missing input is findable.
    #[test]
    fn a_library_crate_with_no_generated_page_fails_the_gate() {
        let observed = observe_generated("no-page");
        assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
        let stderr = String::from_utf8(observed.output.stderr).expect("diagnostics");
        assert!(stderr.contains("does not generate cleanly"), "{stderr}");
        assert!(stderr.contains("doc-library: the generator wrote no page"), "{stderr}");
    }

    /// One invocation covers every binary-only target, and only library JSON becomes a page.
    #[test]
    fn every_binary_only_target_is_checked_but_only_library_json_is_rendered() {
        let observed = observe_generated("clean");
        assert_eq!(observed.output.status.code(), Some(0), "{observed:?}");
        let mut expected = documented();
        expected.push_str(RENDERED);
        assert_eq!(
            observed.ledger, expected,
            "multiple and differently named binaries must each be checked"
        );
    }

    // Library and binary are units of ONE cargo run now, so a shared JSON file name is a race
    // rather than an overwrite and there is no ordering left to impose. This refusal is what
    // keeps that impossible: a binary named after a library crate writes that crate's page
    // input, and a page rendered from a binary's surface reads as a drift.
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
