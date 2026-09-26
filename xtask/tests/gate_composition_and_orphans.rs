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

fn error(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_new_cli_error_definition_is_refused_by_the_boundary_gate() {
    let tree = Tree::of(
        "composition-root-error",
        &[
            ("Cargo.toml", b"[workspace]\nmembers = [\"crates/sutura-cli\"]\n"),
            ("flake.nix", b"{}\n"),
            (
                "crates/sutura-cli/Cargo.toml",
                b"[package]\nname = \"sutura-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\ndescription = \"The sutura binary. Composes adapters; contains no business logic.\"\n",
            ),
            ("crates/sutura-cli/src/main.rs", b"fn main() {}\n"),
            ("crates/sutura-cli/src/import.rs", b"pub fn import() {}\n"),
            (
                "crates/sutura-cli/src/serve/kind.rs",
                b"#[derive(Debug, thiserror::Error)]\npub enum AnyWarehouseError { #[error(transparent)] Adapter(()) }\n",
            ),
            (
                "crates/sutura-cli/src/business.rs",
                b"#[derive(Debug, thiserror::Error)]\npub enum NewBusinessError { #[error(\"new\")] New }\n",
            ),
        ],
    );
    let output = gate(&tree, "check-boundaries");
    let error = error(&output);
    assert!(!output.status.success(), "{error}");
    assert!(
        error.contains("`NewBusinessError` defines a new error in the composition root"),
        "{error}"
    );
}

fn cli_error_tree(tag: &str, source: &[u8]) -> Tree {
    Tree::of(
        tag,
        &[
            ("Cargo.toml", b"[workspace]\nmembers = [\"crates/sutura-cli\"]\n"),
            ("flake.nix", b"{}\n"),
            (
                "crates/sutura-cli/Cargo.toml",
                b"[package]\nname = \"sutura-cli\"\nversion = \"0.1.0\"\nedition = \"2024\"\ndescription = \"The sutura binary. Composes adapters; contains no business logic.\"\n",
            ),
            ("crates/sutura-cli/src/main.rs", b"fn main() {}\n"),
            ("crates/sutura-cli/src/import.rs", b"pub fn import() {}\n"),
            (
                "crates/sutura-cli/src/serve/kind.rs",
                b"#[derive(Debug, thiserror::Error)]\npub enum AnyWarehouseError { #[error(transparent)] Adapter(()) }\n",
            ),
            ("crates/sutura-cli/src/business.rs", source),
        ],
    )
}

#[test]
fn a_multiline_derive_error_is_refused_by_the_boundary_gate() {
    let tree = cli_error_tree(
        "composition-root-multiline-error",
        b"#[derive(\n    Debug,\n    thiserror::Error\n)]\npub enum NewBusinessError { #[error(\"new\")] New }\n",
    );
    let output = gate(&tree, "check-boundaries");
    let error = error(&output);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("`NewBusinessError` defines a new error"), "{error}");
}

#[test]
fn an_imported_error_trait_is_refused_by_the_boundary_gate() {
    let tree = cli_error_tree(
        "composition-root-imported-error",
        b"use std::error::Error;\n#[derive(Debug)]\npub struct NewBusinessError;\nimpl Error for NewBusinessError {}\n",
    );
    let output = gate(&tree, "check-boundaries");
    let error = error(&output);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("`NewBusinessError` defines a new error"), "{error}");
}

fn orphan_tree(tag: &str, reference_path: &str, reference: &[u8]) -> Tree {
    Tree::of(
        tag,
        &[
            ("Cargo.toml", b"[workspace]\nmembers = [\"crates/widget\"]\n"),
            (
                "Cargo.lock",
                b"version = 3\n\n[[package]]\nname = \"widget\"\nversion = \"0.1.0\"\n",
            ),
            ("flake.nix", b"{}\n"),
            (
                "crates/widget/Cargo.toml",
                b"[package]\nname = \"widget\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
            ("crates/widget/src/lib.rs", b"pub mod phantom;\n"),
            ("crates/widget/src/phantom.rs", b"pub fn unused() {}\n"),
            (reference_path, reference),
        ],
    )
}

fn assert_orphan(tree: &Tree) {
    let output = gate(tree, "check-unreachable-public-modules");
    let error = error(&output);
    assert!(!output.status.success(), "{error}");
    assert!(error.contains("widget::phantom has no first-party reference"), "{error}");
}

#[test]
fn a_comment_does_not_make_a_public_module_reachable() {
    let tree = orphan_tree(
        "orphan-comment",
        "crates/widget/src/comment.rs",
        b"// phantom::unused is documentation\n",
    );
    assert_orphan(&tree);
}

#[test]
fn an_integration_test_does_not_make_a_public_module_reachable() {
    let tree = orphan_tree(
        "orphan-test-reference",
        "crates/widget/tests/only.rs",
        b"#[test] fn only() { widget::phantom::unused(); }\n",
    );
    assert_orphan(&tree);
}

#[test]
fn an_integration_test_helper_does_not_make_a_public_module_reachable() {
    let tree = orphan_tree(
        "orphan-test-helper-reference",
        "crates/widget/tests/common/mod.rs",
        b"pub fn helper() { widget::phantom::unused(); }\n",
    );
    assert_orphan(&tree);
}

#[test]
fn a_benchmark_does_not_make_a_public_module_reachable() {
    let tree = orphan_tree(
        "orphan-bench-reference",
        "crates/widget/benches/only.rs",
        b"fn main() { widget::phantom::unused(); }\n",
    );
    assert_orphan(&tree);
}
