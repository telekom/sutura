//! Change classification: work out what a diff actually requires, so the hooks and CI can
//! skip what it cannot affect.
//!
//! Two entry points, one shared table:
//!
//! * `classify` - maps changed paths to areas and prints a verdict. Writes
//!   `GITHUB_OUTPUT` when CI sets it, so a workflow can gate steps on the result.
//! * `changed-packages` - maps changed `.rs` paths to the cargo packages that own them, so
//!   a hook can check those packages instead of the workspace.
//!
//! THE SAFETY PROPERTY, which is the whole reason this is careful code: a path matching no
//! area **fails open**. It sets `run_all` and says which path caused it. The failure mode
//! worth engineering against is not a wasted CI minute - it is a new directory nobody added
//! to the table being silently excluded from every check while the pipeline reports green.
//!
//! The table is Rust rather than a config file on purpose. It needs no parser (xtask has one
//! dependency and adding a TOML crate to read six patterns is a poor trade), the compiler
//! checks it, and the tests below exercise the real thing rather than a fixture.

use std::collections::BTreeSet;
use std::io::Write as _;

use crate::Verdict;
use crate::repo;

/// One area of the repo, and what depends on it.
struct Area {
    /// Output name. CI gates steps on these.
    name: &'static str,
    /// Glob patterns, matched by [`repo::matches`].
    patterns: &'static [&'static str],
    /// Areas that must also run when this one matches. A dependency edge, not a category:
    /// changing the build definition means the build must run even if no Rust changed.
    consumers: &'static [&'static str],
}

/// Every area, and the reason each pattern is where it is.
const AREAS: &[Area] = &[
    Area {
        // The code, the manifests, the lint config and the compiler pin: anything that can
        // change what `cargo` produces or what it rejects.
        name: "rust",
        patterns: &[
            "crates/**",
            "xtask/**",
            "Cargo.toml",
            "Cargo.lock",
            "clippy.toml",
            "rust-toolchain.toml",
            ".cargo/**",
        ],
        consumers: &["build"],
    },
    Area {
        // The build definition itself. A flake edit can break the release without touching
        // a line of Rust, so it pulls in the full chain.
        name: "nix",
        patterns: &["flake.nix", "flake.lock", "devenv.nix", "devenv.yaml", "devenv.lock"],
        consumers: &["rust", "build"],
    },
    Area {
        // Supply-chain policy and the lock it judges.
        name: "deps",
        patterns: &["deny.toml", "Cargo.lock"],
        consumers: &[],
    },
    Area {
        // The release artifacts.
        name: "build",
        patterns: &["Dockerfile", "compose*.yaml", "compose*.yml"],
        consumers: &[],
    },
    Area {
        // The committed API reference pages, which are GENERATED from the library crates' doc
        // comments. `cargo xtask check-api-docs` regenerates them and byte-compares; this area
        // is what decides when that runs. It needs its own area rather than riding on `rust`
        // because it needs the NIGHTLY toolchain, so it is the one gate CI cannot fold into
        // the others.
        //
        // `crates/*/src/**` and not one crate name: the gate DERIVES the library crates from
        // `cargo metadata`, and naming a crate here would be a second list to keep in step
        // with it. The pattern is deliberately a SUPERSET - it matches the binary crates too,
        // whose sources cannot change a page - because the error directions are not
        // symmetrical. An unnecessary run regenerates identical output and passes; a missing
        // pattern is a stale page nothing reports.
        //
        // `Cargo.toml` is here because the page prints the crate VERSION, so a version bump
        // alone makes every page stale. That one is easy to miss precisely because it changes
        // no doc comment.
        //
        // The generator and the pages are here too: both decide what a fresh generation
        // produces, and `DOCS_ONLY` below would otherwise read a hand-edited page or a changed
        // renderer as needing nothing at all. An area match is tested before `DOCS_ONLY`,
        // which is what makes that work. (`ci.yml` additionally ignores `docs/**` for the
        // purpose of STARTING a run, so in CI these two patterns bite on a change that also
        // touches something else. They still bite in the hooks and in a local `classify`.)
        //
        // NOT here: `rust-toolchain-nightly.toml`. It sets rustdoc's `format_version` and so
        // can change every page - but it matches no area today, which means it fails open to
        // `run_all`, and `run_all` already subsumes this area. Adding it here would NARROW
        // that to `api` alone, which is strictly less checking.
        name: "api",
        patterns: &[
            "crates/*/src/**",
            "Cargo.toml",
            "docs/.tools/rustdoc_to_markdown.py",
            "docs/api/**",
            // The gate's own source, so a change to it is judged by running it.
            "xtask/src/api_docs.rs",
        ],
        consumers: &[],
    },
];

/// Paths that cannot affect a build. Deliberately short: everything else is either an area
/// or unclassified, and unclassified fails open.
const DOCS_ONLY: &[&str] = &["**/*.md", "docs/**", "LICENSE", "NOTICE", ".gitattributes", ".gitignore"];

/// CI configuration changes the pipeline itself, so nothing may be skipped on the strength
/// of a classification the change may have just altered.
const RUN_ALL_PATTERNS: &[&str] = &[
    ".github/**",
    ".pre-commit-config.yaml",
    // This file. A commit that narrows `AREAS` would otherwise be the first thing judged by
    // the narrowed table, which is the same mistake as the two above.
    "xtask/src/changes.rs",
];

/// What a diff requires.
#[derive(Default, Debug)]
pub(crate) struct Classification {
    pub(crate) areas: BTreeSet<String>,
    pub(crate) docs_only: Vec<String>,
    pub(crate) unclassified: Vec<String>,
    pub(crate) run_all: bool,
    pub(crate) reasons: Vec<String>,
}

impl Classification {
    /// Is this area required? `run_all` subsumes every area, which is what makes a
    /// fail-open verdict safe for a caller that only asks about one.
    pub(crate) fn needs(&self, area: &str) -> bool {
        self.run_all || self.areas.contains(area)
    }
}

/// Classify a set of repo-relative paths.
pub(crate) fn classify(paths: &[String]) -> Classification {
    let mut result = Classification::default();

    // An empty diff is not evidence that nothing is needed - it usually means the range was
    // wrong. Fail open rather than skipping the whole pipeline on a bad base ref.
    if paths.is_empty() {
        result.run_all = true;
        result
            .reasons
            .push(String::from("no changed paths supplied - running everything"));
        return result;
    }

    for path in paths {
        if RUN_ALL_PATTERNS.iter().any(|p| repo::matches(p, path)) {
            result.run_all = true;
            result.reasons.push(format!("{path} changes CI itself - running everything"));
            continue;
        }

        let matched: Vec<&str> = AREAS
            .iter()
            .filter(|a| a.patterns.iter().any(|p| repo::matches(p, path)))
            .map(|a| a.name)
            .collect();

        if !matched.is_empty() {
            for name in matched {
                result.areas.insert(String::from(name));
            }
            continue;
        }

        if DOCS_ONLY.iter().any(|p| repo::matches(p, path)) {
            result.docs_only.push(path.clone());
            continue;
        }

        // The important branch. Not "assume harmless".
        result.unclassified.push(path.clone());
        result.run_all = true;
        result
            .reasons
            .push(format!("{path} matches no area - running everything (add it to AREAS)"));
    }

    apply_consumers(&mut result);
    result
}

/// Pull in every area that depends on one already matched, until it settles. Iterative
/// rather than recursive so a future cycle in the table cannot blow the stack.
fn apply_consumers(result: &mut Classification) {
    loop {
        let mut added = Vec::new();
        for area in AREAS {
            if !result.areas.contains(area.name) {
                continue;
            }
            for consumer in area.consumers {
                if !result.areas.contains(*consumer) {
                    added.push((area.name, *consumer));
                }
            }
        }
        if added.is_empty() {
            return;
        }
        for (from, to) in added {
            result.reasons.push(format!("{from} fans out to {to}"));
            result.areas.insert(String::from(to));
        }
    }
}

/// Changed paths from git. `None` when git cannot answer, which the caller must treat as
/// "run everything" rather than "nothing changed".
fn changed_paths(since: &str) -> Option<Vec<String>> {
    let out = std::process::Command::new("git")
        // `--no-renames` is load-bearing. With rename detection on - the default - a move is
        // reported as its DESTINATION only, so moving a file out of `crates/` into `docs/`
        // yields one docs path, classifies as docs-only, and skips the whole Rust chain:
        // green CI over a workspace that no longer compiles. It also makes the result
        // independent of the runner's `diff.renames` setting.
        .args(["diff", "--name-only", "--no-renames", "-z", since, "--"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|raw| !raw.is_empty())
            .map(|raw| String::from(String::from_utf8_lossy(raw)))
            .collect(),
    )
}

/// Is this a Rust source path? Case-insensitive, because clippy is right that a
/// case-sensitive extension test is a bug on a case-insensitive filesystem.
fn is_rust(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
}

/// GitHub Actions reads `true`/`false` literals from an output, so the bool becomes text
/// exactly once, here.
const fn flag(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// Emit `name=true|false` lines for CI to read. Silent when not running under Actions.
fn write_github_output(result: &Classification) {
    let Ok(path) = std::env::var("GITHUB_OUTPUT") else {
        return;
    };
    let docs_only = !result.run_all && result.areas.is_empty() && !result.docs_only.is_empty();

    // Assembled with `push_str` rather than `format!`/`write!`: the workspace denies
    // `format_push_string` and `let_underscore_must_use`, and a `write!` into a `String`
    // returns a `Result` that cannot fail but still has to be handled.
    let mut body = String::new();
    for area in AREAS {
        body.push_str(area.name);
        body.push('=');
        body.push_str(flag(result.needs(area.name)));
        body.push('\n');
    }
    body.push_str("run_all=");
    body.push_str(flag(result.run_all));
    body.push_str("\ndocs_only=");
    body.push_str(flag(docs_only));
    body.push('\n');

    match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(body.as_bytes()) {
                eprintln!("xtask classify: could not write GITHUB_OUTPUT: {e}");
            }
        }
        Err(e) => eprintln!("xtask classify: could not open GITHUB_OUTPUT: {e}"),
    }
}

fn print_report(paths: &[String], result: &Classification) {
    println!("xtask classify: {} changed path(s)", paths.len());
    if result.run_all {
        println!("  verdict: RUN EVERYTHING");
    } else if result.areas.is_empty() {
        println!("  verdict: docs only - no build or test work required");
    } else {
        let names: Vec<&str> = result.areas.iter().map(String::as_str).collect();
        println!("  verdict: areas {}", names.join(", "));
    }
    for reason in &result.reasons {
        println!("  reason: {reason}");
    }
    if !result.unclassified.is_empty() {
        println!("  unclassified ({}):", result.unclassified.len());
        for p in &result.unclassified {
            println!("    {p}");
        }
    }
    if !result.docs_only.is_empty() {
        println!("  docs-only: {} path(s)", result.docs_only.len());
    }
}

/// `xtask classify [--since <ref>] [path...]` - the hook and CI entry point.
pub(crate) fn run_classify(args: &[String]) -> Verdict {
    let paths = match args.split_first() {
        Some((flag, rest)) if flag == "--since" => {
            let Some(base) = rest.first() else {
                eprintln!("xtask classify: --since needs a git ref");
                return Verdict::Usage;
            };
            if let Some(found) = changed_paths(base) {
                found
            } else {
                // A bad or unreachable base ref must not look like an empty diff.
                println!("xtask classify: git could not diff against `{base}` - running everything");
                let result = Classification {
                    run_all: true,
                    reasons: vec![format!("git diff against `{base}` failed")],
                    ..Classification::default()
                };
                write_github_output(&result);
                return Verdict::Pass;
            }
        }
        _ => args.to_vec(),
    };

    let result = classify(&paths);
    print_report(&paths, &result);
    write_github_output(&result);
    Verdict::Pass
}

/// Paths whose owning package is not a workspace member, so `cargo check -p` cannot take it.
///
/// A path dependency inside the repository has a real `Cargo.toml` with a real package name,
/// so `owning_package` finds it - but `[workspace] exclude` keeps it out of the member list and
/// `cargo check -p libmimalloc-sys` then fails with "did not match any packages". That is what
/// broke the commit hook the moment mimalloc was vendored.
///
/// Skipped rather than treated as an orphan: an orphan widens to checking everything, which is
/// right when the workspace layout surprised us and wrong here, where the answer is simply that
/// third-party source is not ours to compile-check.
const NON_MEMBER_PATHS: &[&str] = &["vendor/"];

/// Is this path outside every workspace member?
fn is_non_member(path: &str) -> bool {
    NON_MEMBER_PATHS.iter().any(|prefix| path.starts_with(prefix))
}

/// The cargo package owning `path`: the nearest ancestor directory with a `Cargo.toml` that
/// declares a `[package]` name. Returns `None` for a path no package owns.
fn owning_package(root: &std::path::Path, path: &str) -> Option<String> {
    let mut dir = root.join(path).parent().map(std::path::Path::to_path_buf)?;
    loop {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file()
            && let Ok(text) = std::fs::read_to_string(&manifest)
            && let Some(name) = package_name(&text)
        {
            return Some(name);
        }
        if dir == root {
            return None;
        }
        dir = dir.parent()?.to_path_buf();
    }
}

/// The `name` under `[package]`. Hand-parsed because xtask has no TOML dependency, and the
/// shape it needs to read is two lines of a file this repo controls.
fn package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = t.strip_prefix("name") {
            let value = rest.trim_start().strip_prefix('=')?.trim();
            return Some(String::from(value.trim_matches('"')));
        }
    }
    None
}

/// `xtask changed-packages [path...]` - prints the owning packages, one per line.
///
/// A hook turns that into `cargo check -p a -p b`, so an edit to one crate does not pay for
/// a workspace check. Prints nothing and succeeds when no Rust file changed.
pub(crate) fn run_changed_packages(args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask changed-packages: could not determine the repo root");
        return Verdict::Fail;
    };

    let rust: Vec<&String> = args
        .iter()
        .filter(|p| {
            std::path::Path::new(p.as_str())
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("rs"))
        })
        .collect();
    if rust.is_empty() {
        println!("xtask changed-packages: no Rust files changed");
        return Verdict::Pass;
    }

    let mut packages = BTreeSet::new();
    let mut orphans = Vec::new();
    let mut skipped = 0_usize;
    for path in rust {
        if is_non_member(path) {
            skipped = skipped.saturating_add(1);
            continue;
        }
        match owning_package(&root, path) {
            Some(name) => {
                packages.insert(name);
            }
            None => orphans.push(path.clone()),
        }
    }

    if skipped > 0 {
        println!("xtask changed-packages: skipped {skipped} vendored file(s); not workspace members");
    }

    // A .rs file no package owns means the workspace layout changed under us. Widen rather
    // than quietly check less than was asked for.
    if !orphans.is_empty() {
        eprintln!("xtask changed-packages: no package owns {} path(s):", orphans.len());
        for p in &orphans {
            eprintln!("    {p}");
        }
        eprintln!("  falling back to the whole workspace");
        println!("--workspace");
        return Verdict::Pass;
    }

    for name in &packages {
        println!("{name}");
    }
    Verdict::Pass
}

/// The packages owning the given `.rs` paths, or `None` to mean "the whole workspace".
///
/// `None` is returned when a path belongs to no package, because a layout that moved is a
/// reason to check MORE, not less.
fn packages_for(root: &std::path::Path, args: &[String]) -> Option<BTreeSet<String>> {
    let mut packages = BTreeSet::new();
    for path in args.iter().filter(|p| is_rust(p)) {
        // Vendored source has a real package name that `cargo check -p` cannot accept, because
        // `[workspace] exclude` keeps it out of the member list. Passing it through produced
        // "cannot specify features for packages outside of workspace" and failed the hook. See
        // NON_MEMBER_PATHS.
        if is_non_member(path) {
            continue;
        }
        packages.insert(owning_package(root, path)?);
    }
    Some(packages)
}

/// `xtask check-changed [path...]` - `cargo check` for the packages that changed.
///
/// The commit-time counterpart to CI's classification: editing one crate should not pay for
/// a workspace check. Clippy over the workspace still runs as its own hook, so this is a
/// fast-feedback narrowing, never the only thing that sees the code.
pub(crate) fn run_check_changed(args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-changed: could not determine the repo root");
        return Verdict::Fail;
    };

    let mut command = std::process::Command::new("cargo");
    command.current_dir(&root).args(["check", "--all-features", "--all-targets"]);

    match packages_for(&root, args) {
        Some(packages) if packages.is_empty() => {
            println!("xtask check-changed: no Rust files changed");
            return Verdict::Pass;
        }
        Some(packages) => {
            let names: Vec<&str> = packages.iter().map(String::as_str).collect();
            println!("xtask check-changed: {}", names.join(", "));
            for name in packages {
                command.args(["--package", &name]);
            }
        }
        None => {
            println!("xtask check-changed: a path belongs to no package - checking the workspace");
            command.arg("--workspace");
        }
    }

    match command.status() {
        Ok(status) if status.success() => Verdict::Pass,
        Ok(_) => Verdict::Fail,
        Err(e) => {
            eprintln!("xtask check-changed: could not run cargo: {e}");
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Classification, classify, package_name};

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn a_rust_change_pulls_in_the_build() {
        let r = classify(&paths(&["crates/sutura-domain/src/lib.rs"]));
        assert!(!r.run_all);
        assert!(r.needs("rust"));
        // The fan-out edge: shipping must be re-validated when the code changes.
        assert!(r.needs("build"));
        assert!(!r.needs("deps"));
    }

    #[test]
    fn a_nix_change_pulls_in_rust_and_build() {
        let r = classify(&paths(&["flake.nix"]));
        assert!(!r.run_all);
        assert!(r.needs("nix"));
        assert!(r.needs("rust"));
        assert!(r.needs("build"));
    }

    #[test]
    fn the_lock_is_both_rust_and_deps() {
        let r = classify(&paths(&["Cargo.lock"]));
        assert!(r.needs("rust"));
        assert!(r.needs("deps"));
    }

    #[test]
    fn a_library_source_change_needs_the_api_docs_gate() {
        // The primary direction of staleness: a doc comment changes and the committed page
        // does not. `rust` is not enough on its own - the API pages need nightly rustdoc, so
        // they are their own step.
        let r = classify(&paths(&["crates/sutura-domain/src/definitions.rs"]));
        assert!(!r.run_all);
        assert!(
            r.needs("api"),
            "a library source change must regenerate the pages: {:?}",
            r.reasons
        );
        assert!(r.needs("rust"));
    }

    #[test]
    fn a_hand_edited_generated_page_needs_the_api_docs_gate() {
        // The other direction: somebody edits the generated page instead of the doc comment.
        // Without this pattern `DOCS_ONLY` swallows it and the byte-compare never runs.
        let r = classify(&paths(&["docs/api/sutura-domain.md"]));
        assert!(r.needs("api"), "a generated page is not prose: {:?}", r.reasons);
        assert!(r.docs_only.is_empty(), "it must not be classified as docs-only");
    }

    #[test]
    fn a_change_to_the_generator_needs_the_api_docs_gate() {
        // The renderer decides what every page contains, so changing it can make all of them
        // stale without touching a line of Rust.
        let r = classify(&paths(&["docs/.tools/rustdoc_to_markdown.py"]));
        assert!(r.needs("api"), "{:?}", r.reasons);
    }

    #[test]
    fn a_version_bump_needs_the_api_docs_gate() {
        // The page prints the crate version, so `version = "0.2.0"` in the manifest makes
        // every page stale while changing no doc comment at all.
        let r = classify(&paths(&["Cargo.toml"]));
        assert!(r.needs("api"), "the page carries the version: {:?}", r.reasons);
    }

    #[test]
    fn docs_require_nothing() {
        let r = classify(&paths(&["README.md", "docs/adr/0001-x.md", "AGENTS.md"]));
        assert!(!r.run_all, "docs must not fan out: {:?}", r.reasons);
        assert!(r.areas.is_empty());
        assert_eq!(r.docs_only.len(), 3);
    }

    #[test]
    fn an_unknown_path_fails_open() {
        let r = classify(&paths(&["services/new-thing/main.go"]));
        assert!(r.run_all, "an unmapped path must run everything, not be skipped");
        assert_eq!(r.unclassified.len(), 1);
        // And it must say why, or the fail-open is invisible and nobody adds the area.
        assert!(r.reasons.iter().any(|m| m.contains("matches no area")));
    }

    #[test]
    fn a_file_moved_out_of_an_area_still_classifies_as_that_area() {
        // The `--no-renames` case, at the classify level: git reports both sides, so the
        // source path is present and pulls in `rust`. Without both sides this is docs-only and
        // every Rust gate is skipped over a workspace that no longer compiles.
        let both_sides = paths(&["crates/sutura-cli/src/main.rs", "docs/src/moved.md"]);
        let r = classify(&both_sides);
        assert!(r.needs("rust"), "the source side must still count: {:?}", r.reasons);

        // And the destination alone - what rename detection would have given us - is exactly
        // the wrong answer this guards against.
        let destination_only = paths(&["docs/src/moved.md"]);
        assert!(!classify(&destination_only).needs("rust"));
    }

    #[test]
    fn a_change_to_the_classifier_runs_everything() {
        let r = classify(&paths(&["xtask/src/changes.rs"]));
        assert!(r.run_all, "the classifier cannot judge its own narrowing: {:?}", r.reasons);
    }

    #[test]
    fn a_ci_change_runs_everything() {
        let r = classify(&paths(&[".github/workflows/ci.yml"]));
        assert!(r.run_all);
        assert!(r.needs("rust") && r.needs("build") && r.needs("deps"));
    }

    #[test]
    fn an_empty_diff_is_treated_as_a_broken_range() {
        let r = classify(&[]);
        assert!(r.run_all, "an empty diff usually means a bad base ref");
    }

    #[test]
    fn run_all_subsumes_every_area() {
        let r = Classification {
            run_all: true,
            ..Classification::default()
        };
        assert!(r.needs("rust") && r.needs("nix") && r.needs("deps") && r.needs("build") && r.needs("api"));
    }

    #[test]
    fn reads_a_package_name() {
        let manifest = "[package]\nname = \"sutura-domain\"\nversion.workspace = true\n";
        assert_eq!(package_name(manifest).as_deref(), Some("sutura-domain"));
        // A virtual workspace root has no [package], so nothing owns it.
        assert_eq!(package_name("[workspace]\nmembers = []\n"), None);
        // A `name` under another table must not be mistaken for the package name.
        assert_eq!(package_name("[dependencies]\nname = \"nope\"\n"), None);
    }
}
