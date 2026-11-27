#![forbid(unsafe_code)]
//! `xtask classify`'s `fuzz_compile` output, driven through the real binary - not the `classify`
//! function called in-process.
//!
//! Why the real binary rather than a unit test beside `classify` (`xtask/src/changes.rs`,
//! `github.com/telekom/sutura#2095`): `cargo xtask causality` does not follow a `#[path]` module
//! include, so a unit test living beside the field it asserts on is CHANGED in the same commit as
//! the field - base never runs a version of that test that lacks the field, so nothing can redden
//! it there. Spawning the compiled binary and reading back the `GITHUB_OUTPUT` file it writes is
//! a test base genuinely fails: on base, `classify` writes no `fuzz_compile=` line at all, so
//! every assertion here is an assertion over a line that does not exist - a clean, mechanical red.

#![cfg(test)]

use std::path::Path;
use std::process::Command;

/// Per-call rather than per-process: nextest runs this file's tests concurrently, and a shared
/// `GITHUB_OUTPUT` path would let one test's append land in another's assertion.
static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Run the real `xtask classify <path>` (no `--since` - a literal path list, the same contract a
/// hook or `ci.yml`'s `classify` job uses) and return the `fuzz_compile=` line it wrote to
/// `GITHUB_OUTPUT`.
fn fuzz_compile_line(path: &str) -> String {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let out_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "fuzz-compile-{}-{}",
        std::process::id(),
        CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    drop(std::fs::remove_file(&out_path));

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("classify")
        .arg(path)
        .current_dir(&repo_root)
        .env("GITHUB_OUTPUT", &out_path)
        .output()
        .expect("run the real xtask binary");
    assert!(output.status.success(), "{:?}", String::from_utf8_lossy(&output.stderr));

    let body = std::fs::read_to_string(&out_path).expect("xtask classify writes GITHUB_OUTPUT");
    drop(std::fs::remove_file(&out_path));
    body.lines().find(|line| line.starts_with("fuzz_compile=")).map_or_else(
        || panic!("no fuzz_compile= line in GITHUB_OUTPUT for {path}:\n{body}"),
        String::from,
    )
}

#[test]
fn a_fuzzed_tree_path_sets_fuzz_compile() {
    // Listed verbatim in the "fuzzed tree" surface (`xtask/src/hook_coverage/surfaces.rs`,
    // `reached_by: "fuzz-smoke"`) and read by `changes::touches_fuzzed_tree`.
    assert_eq!(fuzz_compile_line("crates/sutura-domain/src/catalog.rs"), "fuzz_compile=true");
}

#[test]
fn a_fuzz_dir_path_sets_fuzz_compile() {
    // `fuzz/**` is the first pattern in the same row - the harness tree itself, not one of the
    // first-party crates it binds to. `fuzz/Cargo.lock` rather than a `fuzz_targets/*.rs` source:
    // the lock also matches the `deps` area (`DEPS_PATTERNS`), so `classify` does not fail open to
    // `run_all` here - a path no area matches proves nothing about `touches_fuzzed_tree` itself,
    // because `run_all` alone would already set `fuzz_compile=true` regardless of the row.
    assert_eq!(fuzz_compile_line("fuzz/Cargo.lock"), "fuzz_compile=true");
}

#[test]
fn an_unrelated_path_leaves_fuzz_compile_false() {
    // No pattern in the "fuzzed tree" row names `sutura-cli` at all, so the compile-only leg must
    // not select on this path.
    assert_eq!(fuzz_compile_line("crates/sutura-cli/src/main.rs"), "fuzz_compile=false");
}

/// Pins `nix/fuzz-compile.nix`'s own cargo invocation to `--bins` (every target) rather than a
/// `--bin <name>` (one target) - the whole point of the leg is that EVERY target still compiles,
/// not just whichever one a prior edit happened to name.
///
/// The limit: this reads one file's text for one line's tokens, not what `cargo check` actually
/// builds - `xtask check-fuzz` (`xtask/src/fuzz.rs`) is what holds every declared `[[bin]]`
/// reachable and seeded; this test only keeps the flag from regressing to a single target.
#[test]
fn the_fuzz_compile_cargo_line_keeps_bins_not_a_single_bin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let nix = std::fs::read_to_string(root.join("nix/fuzz-compile.nix")).expect("nix/fuzz-compile.nix is readable");
    let cargo_lines: Vec<&str> = nix
        .lines()
        .filter(|line| !line.trim_start().starts_with('#') && line.contains("cargo check"))
        .collect();
    assert!(!cargo_lines.is_empty(), "no `cargo check` invocation in nix/fuzz-compile.nix");
    for line in cargo_lines {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        assert!(tokens.contains(&"--bins"), "missing `--bins`: {line}");
        assert!(!tokens.contains(&"--bin"), "narrowed to a single `--bin`: {line}");
    }
}
