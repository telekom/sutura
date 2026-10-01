//! The kill worktree's own tier, over real cargo: a fixture workspace whose `dev/src/provisioned.rs`
//! fails with the requirement's sentence unless the worktree publishes an endpoint matching the
//! credential the run carries - the two halves `claim::tier` supplies. A fake tier script for the
//! arm's own branches, and the real `sutura-postgres-tier` for one cell.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::tier::{TierError, command, credentials};
use super::{Caller, Claim};
use crate::Verdict;
use crate::causality::diff::commit_additions;
use crate::causality::scoped::{Scan, Scoped};

static SEQ: AtomicUsize = AtomicUsize::new(0);

const KILLS: &str = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-pub fn f() -> u8 { 1 }\n+pub fn f() -> u8 { 2 }\n pub fn g() -> u8 { 1 }\n";
const SPARES: &str = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n pub fn f() -> u8 { 1 }\n-pub fn g() -> u8 { 1 }\n+pub fn g() -> u8 { 2 }\n";
/// Publishes an endpoint in its working directory and the matching credential; logs each step.
const WORKS: &str = "case \"$1\" in\n  start) mkdir -p .sutura-dev && echo fixture-token > .sutura-dev/endpoints.json ;;\n  credentials) echo export SUTURA_POSTGRES_TIER_PASSWORD=fixture-token ;;\nesac\n";
const FAILS: &str = "[ \"$1\" != start ]\n";

/// A temp dir holding a fixture repo at `repo/` and the fake tier at `tier`, logging to `tier.log`.
struct Fixture(PathBuf);

impl Fixture {
    fn new(patch: &str, tier: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sutura-claim-tier-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _swept = std::fs::remove_dir_all(&dir);
        let fixture = Self(dir);
        // The fake publishes the password itself; the real tier publishes its socket dir as
        // `host`, with the password in `<host>.cred` beside it.
        let provisioned = format!(
            "pub fn here() {{\n    let published = std::fs::read_to_string(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../.sutura-dev/endpoints.json\")).unwrap_or_default();\n    let expected = match published.split('\"').skip_while(|field| *field != \"host\").nth(2) {{\n        Some(host) => std::fs::read_to_string(format!(\"{{host}}.cred\")).unwrap_or_default(),\n        None => published,\n    }};\n    if expected.trim().is_empty() || std::env::var(\"SUTURA_POSTGRES_TIER_PASSWORD\").ok().as_deref() != Some(expected.trim()) {{\n        panic!({:?});\n    }}\n}}\n",
            sutura_dev::requirement::REQUIRED
        );
        for (rel, text) in [
            (
                "repo/Cargo.toml",
                "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ndev = { path = \"dev\" }\n\n[workspace]\n\n[profile.ci]\ninherits = \"dev\"\n",
            ),
            (
                "repo/dev/Cargo.toml",
                "[package]\nname = \"dev\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("repo/dev/src/lib.rs", "pub mod provisioned;\n"),
            ("repo/dev/src/provisioned.rs", &provisioned),
            ("repo/src/lib.rs", "pub fn f() -> u8 { 1 }\npub fn g() -> u8 { 1 }\n"),
            ("repo/devco/claim-mutations/tier_cell.patch", patch),
            (
                "tier",
                &format!(
                    "#!/bin/sh\necho \"$1\" >> \"{}\"\n{tier}",
                    fixture.0.join("tier.log").display()
                ),
            ),
        ] {
            let at = fixture.0.join(rel);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(at, text).unwrap();
        }
        fixture.chmod_tier();
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.git(&["add", "-A"]);
        fixture.git(&[
            "-c",
            "user.email=user@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-qm",
            "init",
        ]);
        let test = "#[test]\nfn tier_cell() {\n    dev::provisioned::here();\n    assert_eq!(wired::f(), 1);\n}\n";
        std::fs::create_dir_all(fixture.repo().join("tests")).unwrap();
        std::fs::write(fixture.repo().join("tests/t.rs"), test).unwrap();
        fixture.git(&["add", "-A"]);
        fixture.git(&[
            "-c",
            "user.email=user@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-qm",
            "test",
        ]);
        fixture
    }

    fn chmod_tier(&self) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(self.0.join("tier"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn git(&self, args: &[&str]) {
        let mut command = Command::new("git");
        crate::repo::strip_git_env(&mut command);
        let out = command.current_dir(self.repo()).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn scoped(&self) -> Scoped {
        let files = commit_additions(&self.repo(), "HEAD").unwrap();
        let read = |path: &str| std::fs::read_to_string(self.repo().join(path)).ok();
        match Scan::of(&files, &[String::from("tests/t.rs")], &read) {
            Scan::Runnable(scoped) => scoped,
            other => panic!("expected the tier cell, got {other:?}"),
        }
    }

    /// The arm's verdict on `tier_cell`, and every step the tier was asked for.
    fn run(&self) -> (Verdict, String) {
        let claim = Claim::synthetic(["tier_cell"].into_iter()).unwrap();
        let tests = [String::from("tests/t.rs")];
        let verdict = super::run_with_tier(
            &self.repo(),
            &self.scoped(),
            &tests,
            &claim,
            Caller::TEST_CAUSALITY,
            &self.0.join("tier"),
        );
        (verdict, std::fs::read_to_string(self.0.join("tier.log")).unwrap_or_default())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _swept = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_tier_backed_cell_is_killed_against_the_kill_worktrees_own_tier() {
    let (verdict, steps) = Fixture::new(KILLS, WORKS).run();
    assert_eq!(verdict, Verdict::Pass);
    // Stopped, and before the worktree went: a stop in a removed directory never runs.
    assert_eq!(steps, "start\ncredentials\nstop\n");
}

#[test]
fn a_tier_backed_cell_is_killed_against_the_real_postgres_tier() {
    // Where nothing provisioned a tier, the binary may be absent: skip loudly, as every tier cell does.
    if !sutura_dev::requirement::Requirement::from_env().is_required() {
        eprintln!("skipped: no tier is required here, so `sutura-postgres-tier` may be absent");
        return;
    }
    let fixture = Fixture::new(KILLS, WORKS);
    let tests = [String::from("tests/t.rs")];
    let claim = Claim::synthetic(["tier_cell"].into_iter()).unwrap();
    let verdict = super::run(&fixture.repo(), &fixture.scoped(), &tests, &claim, Caller::TEST_CAUSALITY);
    assert_eq!(verdict, Verdict::Pass, "the real tier served the kill run its own credential");
}

#[test]
fn the_kill_tier_keys_on_the_kill_worktree_inside_a_nix_sandbox_too() {
    // `NIX_BUILD_TOP` would key it on the sandbox, whose cluster is the suite's own: `stop` then
    // deleted that cluster mid-suite.
    let tier = command(Path::new("tier"), Path::new("/tmp/kill-wt"), "start");
    assert_eq!(tier.get_current_dir(), Some(Path::new("/tmp/kill-wt")));
    assert!(
        tier.get_envs()
            .any(|(name, value)| name == "NIX_BUILD_TOP" && value.is_none()),
        "the sandbox's key must not reach the kill tier"
    );
}

#[test]
fn a_failed_tier_step_does_not_print_the_password() {
    let said = String::from("ERROR:  unrecognized role option \"logn\"\nLINE 1: CREATE ROLE \"x\" LOGN PASSWORD 'deadbeef00'\n");
    let shown = TierError::Exited { step: "start", said }.to_string();
    assert!(!shown.contains("deadbeef00"), "{shown}");
    assert!(shown.contains("PASSWORD '<redacted>'"), "{shown}");
}

#[test]
fn a_tier_backed_cell_its_patch_does_not_kill_is_refused() {
    let (verdict, _) = Fixture::new(SPARES, WORKS).run();
    assert_eq!(verdict, Verdict::Fail);
}

#[test]
fn a_tier_that_cannot_start_still_reads_no_tier() {
    let (verdict, steps) = Fixture::new(KILLS, FAILS).run();
    assert_eq!(verdict, Verdict::Inconclusive);
    assert_eq!(steps, "start\nstop\n", "a half-started tier is still stopped");
}

#[test]
fn a_credential_outside_the_tiers_namespace_is_refused_without_its_value() {
    let refused = credentials("export SUTURA_POSTGRES_TIER_USER=sutura\nexport PATH=/s3cret\n").unwrap_err();
    assert!(matches!(refused, TierError::Credential { line: 2 }), "{refused:?}");
    assert!(!refused.to_string().contains("s3cret"), "{refused}");
    let parsed = credentials("export SUTURA_POSTGRES_TIER_PASSWORD=a=b\n").unwrap();
    assert_eq!(parsed, [(String::from("SUTURA_POSTGRES_TIER_PASSWORD"), String::from("a=b"))]);
}
