#![forbid(unsafe_code)]
#![cfg(test)]

#[path = "scratch_tree/mod.rs"]
#[expect(dead_code, reason = "the shared fixture helper also serves tests that seal a file")]
mod scratch_tree;

use std::process::{Command, Output};

use scratch_tree::Tree;

fn gate(tree: &Tree, name: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg(name)
        .current_dir(tree.root())
        .output()
        .expect("run the gate against the scratch tree")
}

#[test]
fn quoted_block_push_stage_is_checked() {
    let tree = Tree::of(
        "quoted-push-stage",
        &[
            ("Cargo.toml", b"[workspace]\n"),
            ("flake.nix", b"{}\n"),
            (
                ".pre-commit-config.yaml",
                b"default_install_hook_types: [pre-commit, pre-push, commit-msg]
default_stages: [pre-commit]
repos:
  - repo: local
    hooks:
      - id: secret-sweep
        entry: bash nix/run-gate.sh secrets
        stages: [pre-push]
      - id: cargo-deny
        entry: bash nix/run-gate.sh supply-chain
        stages: [pre-push]
      - id: compiling-push
        entry: cargo clippy
        stages:
          - 'pre-push' # quoted YAML scalar
",
            ),
        ],
    );
    let output = gate(&tree, "check-hook-tiers");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("compiling-push"), "{error}");
}

#[test]
fn commented_flow_push_stage_is_checked() {
    let tree = Tree::of(
        "commented-push-stage",
        &[
            ("Cargo.toml", b"[workspace]\n"),
            ("flake.nix", b"{}\n"),
            (
                ".pre-commit-config.yaml",
                b"default_install_hook_types: [pre-commit, pre-push, commit-msg]\n\
default_stages: [pre-commit]\n\
repos:\n\
  - repo: local\n\
    hooks:\n\
      - id: secret-sweep\n\
        entry: bash nix/run-gate.sh secrets\n\
        stages: [pre-push]\n\
      - id: cargo-deny\n\
        entry: bash nix/run-gate.sh supply-chain\n\
        stages: [pre-push]\n\
      - id: compiling-push\n\
        entry: cargo clippy\n\
        stages: [pre-push] # YAML comment\n",
            ),
        ],
    );
    let output = gate(&tree, "check-hook-tiers");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("compiling-push"), "{error}");
}

#[test]
fn bare_ignore_does_not_exempt_nested_source() {
    let tree = Tree::of(
        "bare-source-ignore",
        &[
            ("Cargo.toml", b"[workspace]\n"),
            ("flake.nix", b"{}\n"),
            ("devco/max-lines-ignore", b"[silent]\nlib.rs\n"),
            ("xtask/src/lib.rs", b"fn item() {}\n"),
        ],
    );
    let output = gate(&tree, "max-lines");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("`lib.rs` names no file"), "{error}");
}

#[test]
fn globbed_first_party_source_cannot_be_exempted() {
    let tree = Tree::of(
        "globbed-source-ignore",
        &[
            ("Cargo.toml", b"[workspace]\n"),
            ("flake.nix", b"{}\n"),
            ("devco/max-lines-ignore", b"[silent]\n**/lib.rs\n"),
            ("xtask/src/lib.rs", b"fn item() {}\n"),
        ],
    );
    let output = gate(&tree, "max-lines");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("first-party source cannot be exempted"), "{error}");
}

#[test]
fn fenced_and_indented_headings_do_not_supply_provenance() {
    let tree = Tree::of(
        "false-provenance",
        &[
            ("Cargo.toml", b"[workspace]\n"),
            ("flake.nix", b"{}\n"),
            (".agents/skills/skill-router.json", b"{\"groups\":{}}\n"),
            (
                ".agents/skill-library/group/example/SKILL.md",
                b"---\nname: example\n---\n````md\n```\n## Provenance\n````\n\t## Provenance\n",
            ),
        ],
    );
    let output = gate(&tree, "check-skills");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("no `## Provenance` section"), "{error}");
}

#[test]
fn hard_link_to_shared_path_is_reported_as_a_write() {
    let tree = Tree::of(
        "shared-hard-link",
        &[
            ("Cargo.toml", b"[workspace]\n"),
            ("flake.nix", b"{}\n"),
            (
                "xtask/src/lib.rs",
                b"fn go() { std::fs::hard_link(\"source\", \"/tmp/sutura-shared/link\").unwrap(); }\n",
            ),
        ],
    );
    let output = gate(&tree, "check-worktree-state");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("xtask/src/lib.rs"), "{error}");
    assert!(error.contains("machine-shared"), "{error}");
}
