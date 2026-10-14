//! `causality::run` over a tier-backed cell: the base worktree is under `target/`, and the root's
//! gitignored discovery file is what tells a harness its tier is there.

use std::sync::atomic::Ordering;

use super::{SEQ, git, git_output};
use crate::Verdict;

#[test]
fn a_tier_cell_runs_on_base_against_the_head_runs_endpoint() {
    assert!(std::env::var_os("NEXTEST").is_some(), "run through `just test`");
    let dir = std::env::temp_dir().join(format!(
        "sutura-causality-endpoint-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _swept = std::fs::remove_dir_all(&dir);
    // Keyed on the discovery file rather than the requirement, so every venue gives one answer.
    let here = "pub fn here() -> bool {\n    std::path::Path::new(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../.sutura-dev/endpoints.json\")).exists()\n}\n";
    for (rel, text) in [
        (
            "Cargo.toml",
            "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndev = { path = \"dev\" }\n\n[workspace]\n\n[profile.ci]\ninherits = \"dev\"\n",
        ),
        (
            "dev/Cargo.toml",
            "[package]\nname = \"dev\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("dev/src/lib.rs", here),
        ("flake.nix", "{ }\n"),
        (".gitignore", ".sutura-dev/\ntarget/\n"),
        ("src/lib.rs", "pub fn f() -> u8 { 1 }\n"),
        (".sutura-dev/endpoints.json", "{}\n"),
    ] {
        let at = dir.join(rel);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "test"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 2 }\n").unwrap();
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("tests/t.rs"),
        "#[test]\nfn reaches_its_tier() {\n    if !dev::here() {\n        return;\n    }\n    assert_eq!(wired::f(), 2);\n}\n",
    )
    .unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "feat: f is two"]);

    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let verdict = super::super::run(&[String::from("--since"), String::from(base.trim())]);
    std::env::set_current_dir(original).unwrap();
    drop(std::fs::remove_dir_all(dir));
    assert_eq!(
        verdict,
        Verdict::Pass,
        "the base run reached the tier, so f's change reddened it"
    );
}
