#![forbid(unsafe_code)]
//! `check-docs` refuses an iframe or an image whose `src` names no file, and a published page that
//! names a path into the decision-record directory, through the real binary.
//!
//! A dedicated `tests/` target, so `just causality` can run it against the base tree: the base
//! gate exits 0 over each fixture below, and these cells are red there by assertion.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A fresh fixture site: the two root markers `repo::root` looks for, a two-page nav and the
/// given `architecture.md`. `index.md` links it, because the gate refuses a tree it read no link in.
fn site(case: &str, architecture: &str, asset: Option<&str>) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("docs-embeds-{case}-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clearing a stale fixture");
    }
    std::fs::create_dir_all(root.join("docs/assets")).expect("the fixture docs directory");
    for (rel, text) in [
        ("flake.nix", "{ }\n"),
        ("Cargo.toml", "[workspace]\nmembers = []\n"),
        (
            "mkdocs.yml",
            "site_name: x\nnav:\n  - Home: index.md\n  - Architecture: architecture.md\n",
        ),
        ("docs/index.md", "# x\n\n[the architecture](architecture.md)\n"),
        ("docs/architecture.md", architecture),
    ] {
        std::fs::write(root.join(rel), text).expect("a fixture file");
    }
    if let Some(name) = asset {
        std::fs::write(root.join("docs/assets").join(name), "<p>x</p>\n").expect("the asset");
    }
    root
}

fn check_docs(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-docs")
        .current_dir(root)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("execute the real xtask binary")
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const FRAME: &str =
    "# Architecture\n\n<iframe\n  src=\"../assets/diagram.html\"\n  title=\"d\"\n></iframe>\n\n[home](index.md)\n";

#[test]
fn an_iframe_over_a_file_that_exists_is_clean_and_counted() {
    let root = site("frame-present", FRAME, Some("diagram.html"));
    let output = check_docs(&root);
    let text = said(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("1 iframe and image source(s) resolve"), "{text}");
}

#[test]
fn an_iframe_over_a_missing_file_is_refused_naming_the_page_and_the_src() {
    let root = site("frame-missing", FRAME, None);
    let output = check_docs(&root);
    let text = said(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("docs/architecture.md"), "{text}");
    assert!(text.contains("../assets/diagram.html"), "{text}");
}

#[test]
fn a_markdown_image_over_a_missing_file_is_refused_naming_the_page_and_the_src() {
    let page = "# Architecture\n\n![the diagram](assets/diagram.png)\n\n[home](index.md)\n";
    let root = site("image-missing", page, None);
    let output = check_docs(&root);
    let text = said(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("docs/architecture.md"), "{text}");
    assert!(text.contains("assets/diagram.png"), "{text}");
    let present = site("image-present", page, Some("diagram.png"));
    assert!(check_docs(&present).status.success(), "{}", said(&check_docs(&present)));
}

#[test]
fn a_published_page_naming_the_decision_record_directory_is_refused_naming_the_page() {
    for (case, body) in [
        (
            "adr-link",
            "# Architecture\n\n[a decision](https://example.com/blob/main/docs/adr/0008-x.md)\n\n[home](index.md)\n",
        ),
        (
            "adr-mention",
            "# Architecture\n\nThe decision is in `docs/adr/0008-x.md`.\n\n[home](index.md)\n",
        ),
    ] {
        let root = site(case, body, None);
        let output = check_docs(&root);
        let text = said(&output);
        assert_eq!(output.status.code(), Some(1), "{text}");
        assert!(text.contains("docs/architecture.md"), "{text}");
        assert!(text.contains("docs/adr/"), "{text}");
    }
    let control = site(
        "adr-absent",
        "# Architecture\n\nThe decision is in a record.\n\n[home](index.md)\n",
        None,
    );
    let output = check_docs(&control);
    assert!(output.status.success(), "{}", said(&output));
}
