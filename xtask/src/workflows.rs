//! Do the workflows reference flake outputs that exist?
//!
//! CI reaches every tool through `nix run .#name` or `nix build .#checks.<system>.name`. A
//! renamed or deleted output is not a build error - it is a workflow that fails at the moment
//! that step runs, minutes into a run, on a push that already happened.
//!
//! That is exactly what happened: the docs toolchain moved from a nix Python environment to
//! pixi, `apps.mkdocs` and `apps.mike` were deleted, and `docs.yml` still called them. Nothing
//! local could have noticed - clippy does not read YAML and zizmor does not read flake.nix -
//! so the first report was a red run on the pull request.
//!
//! BOTH `.github/workflows` AND `.github/actions`, and the second was a hole rather than a
//! widening: `nix run .#cosign` has lived in a local composite action since that sequence was split
//! out of `release.yml`, and this gate read the workflows directory alone - so the one reference
//! that publishes a release was the one reference nothing checked.
//!
//! Text scanning on both sides, because this has to run where there is no nix. It cannot know
//! whether an output BUILDS; it knows whether it is declared, which is the failure that recurs.

use crate::Verdict;
use crate::repo;
use std::collections::BTreeSet;

/// Which output namespace a reference points into.
///
/// `Runnable` and not `App`: `nix run .#name` resolves an app OR a package with a matching main
/// program, and this repo relies on that - `nix run .#xtask` runs `packages.xtask`, which has no
/// `apps.xtask`. Checking only `apps` reported every such call as missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Runnable,
    Check,
}

impl Kind {
    const fn label(self) -> &'static str {
        match self {
            Self::Runnable => "apps or packages",
            Self::Check => "checks",
        }
    }
}

/// One reference, and where it was written.
struct Reference {
    workflow: String,
    line: usize,
    kind: Kind,
    name: String,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-workflows: could not locate the repo root");
        return Verdict::Fail;
    };

    let flake = match std::fs::read_to_string(root.join("flake.nix")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-workflows: could not read flake.nix: {error}");
            return Verdict::Fail;
        }
    };
    // Apps and packages together, because `nix run` accepts either.
    let mut runnable = declared_apps(&flake);
    runnable.extend(declared_block(&flake, "packages = "));
    let checks = declared_block(&flake, "checks = {");

    // An empty side would make this gate pass by finding nothing - the failure mode a
    // text-scanning check is most prone to.
    if runnable.is_empty() || checks.is_empty() {
        eprintln!("xtask check-workflows: parsed no runnables or no checks out of flake.nix");
        eprintln!("  the scan is broken, not the workflows");
        return Verdict::Fail;
    }

    let Some(Scan { references, files }) = gather(&root) else {
        return Verdict::Fail;
    };

    let missing: Vec<&Reference> = references
        .iter()
        .filter(|r| {
            let known = match r.kind {
                Kind::Runnable => &runnable,
                Kind::Check => &checks,
            };
            !known.contains(&r.name)
        })
        .collect();

    if missing.is_empty() {
        println!(
            "xtask check-workflows: ok - {} reference(s) in {files} workflow(s) and action(s), all declared",
            references.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-workflows: these name flake outputs that do not exist\n");
    for r in &missing {
        eprintln!("  {}:{}  {}.{}", r.workflow, r.line, r.kind.label(), r.name);
    }
    eprintln!();
    eprintln!("Runnable:  {}", joined(&runnable));
    eprintln!("Checks:    {}", joined(&checks));
    eprintln!();
    eprintln!("A deleted output is a workflow that fails minutes into a run, after the push.");
    Verdict::Fail
}

fn joined(names: &BTreeSet<String>) -> String {
    names.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// Every `apps.<name>` declaration. Comments are skipped: they name outputs in prose, and a
/// comment is not a declaration.
fn declared_apps(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("apps.")
            && let Some(name) = rest.split([' ', '=', '.']).next()
            && !name.is_empty()
        {
            names.insert(String::from(name));
        }
    }
    names
}

/// Every attribute at the top level of an output block.
///
/// Depth-tracked rather than stopping at the first `};`. The first version broke out there and
/// so missed everything after the first NESTED close - which meant `checks.hygiene`, declared
/// well below `clippy`, read as undeclared while CI built it happily every run. A parser that
/// silently sees half a file is worse than no parser.
fn declared_block(text: &str, header: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut depth = 0_i32;
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !inside {
            if trimmed.starts_with(header) {
                inside = true;
                depth = 1;
            }
            continue;
        }

        // Only the outermost level of the block declares an output; everything deeper belongs
        // to one. Counted before the name check so the closing line of a nested attrset does
        // not look like a declaration.
        let opens = i32::try_from(trimmed.matches('{').count()).unwrap_or(0);
        let closes = i32::try_from(trimmed.matches('}').count()).unwrap_or(0);

        if depth == 1
            && !trimmed.starts_with('#')
            && let Some((key, _)) = trimmed.split_once('=')
        {
            let key = key.trim();
            let plain = !key.is_empty()
                && !key.contains(' ')
                && !key.contains('.')
                && key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if plain {
                names.insert(String::from(key));
            }
        }

        depth = depth.saturating_add(opens).saturating_sub(closes);
        if depth <= 0 {
            break;
        }
    }
    names
}

/// What one scan of `.github` found: the references, and how many files were read.
///
/// A named struct rather than a tuple, because `clippy::type_complexity` refuses the tuple - and it
/// is right to: `usize` beside a `Vec` says nothing about which count it is.
struct Scan {
    references: Vec<Reference>,
    files: usize,
}

/// Every `nix run .#` / `nix build .#` reference in `.github`, and how many files were read.
///
/// A function rather than the body of `run`, so a test can assert WHERE the references came from.
/// The composite-action half is only observable that way: a gate that walked one directory and a
/// gate that walks two return the same verdict on a correct tree, which is exactly how the hole
/// this closes went unnoticed.
fn gather(root: &std::path::Path) -> Option<Scan> {
    let mut references = Vec::new();
    let mut files = 0_usize;
    let Ok(entries) = std::fs::read_dir(root.join(".github").join("workflows")) else {
        eprintln!("xtask check-workflows: no .github/workflows directory");
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let yaml = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "yml" || e == "yaml");
        if !yaml {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        files = files.saturating_add(1);
        collect(&text, &name, &mut references);
    }

    // AND THE LOCAL COMPOSITE ACTIONS, which is a hole this gate had rather than a widening of
    // what it claims. `nix run .#cosign` has lived in `.github/actions/attest-and-sign` since that
    // sequence was split out of `release.yml`, and this scan read `.github/workflows` only - so
    // the one reference that publishes a release was the one reference nothing checked. Splitting
    // a step into an action is how a reference leaves this gate's sight, and the split is exactly
    // what this repository does when a workflow reaches the 1000-line cap, so it will happen
    // again. Named by their DIRECTORY, because every one of these files is called `action.yml` and
    // a failure saying `action.yml:118` names nothing a reader can open.
    //
    // A missing `.github/actions` is not a failure, unlike a missing `.github/workflows`: a
    // repository with no composite action is a repository with none, and this gate must not start
    // failing on one.
    let actions = root.join(".github").join("actions");
    for entry in std::fs::read_dir(&actions).into_iter().flatten().flatten() {
        let dir = entry.path();
        for candidate in ["action.yml", "action.yaml"] {
            let Ok(text) = std::fs::read_to_string(dir.join(candidate)) else {
                continue;
            };
            let label = dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            files = files.saturating_add(1);
            collect(&text, &format!("actions/{label}"), &mut references);
        }
    }

    Some(Scan { references, files })
}

/// Find every `nix run .#...` and `nix build .#...` in one workflow.
fn collect(text: &str, workflow: &str, out: &mut Vec<Reference>) {
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        for (needle, kind) in [("nix run .#", Kind::Runnable), ("nix build .#", Kind::Check)] {
            let mut rest = line;
            while let Some(at) = rest.find(needle) {
                let after = rest.get(at.saturating_add(needle.len())..).unwrap_or_default();
                let token: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
                    .collect();
                if let Some(name) = attribute(&token, kind) {
                    out.push(Reference {
                        workflow: String::from(workflow),
                        line: index.saturating_add(1),
                        kind,
                        name,
                    });
                }
                rest = after;
            }
        }
    }
}

/// The attribute a token refers to.
///
/// An app is `.#name`. A check is `.#checks.<system>.name`. `nix build .#sutura` names a
/// PACKAGE, which this gate does not track, so it is ignored rather than reported missing.
fn attribute(token: &str, kind: Kind) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    match kind {
        Kind::Runnable => parts.first().filter(|p| !p.is_empty()).map(|p| String::from(*p)),
        Kind::Check => {
            if parts.first().copied() == Some("checks") {
                parts.get(2).filter(|p| !p.is_empty()).map(|p| String::from(*p))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn an_app_and_a_check_are_told_apart() {
        let mut found = Vec::new();
        super::collect(
            "        run: nix run .#zizmor -- .github/workflows\n        run: nix build .#checks.x86_64-linux.hygiene -L\n",
            "ci.yml",
            &mut found,
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "zizmor");
        assert_eq!(found[0].kind, Kind::Runnable);
        assert_eq!(found[1].name, "hygiene");
        assert_eq!(found[1].kind, Kind::Check);
    }

    #[test]
    fn a_package_build_is_not_a_check() {
        // Reading `.#sutura` as a check would report every release build as missing.
        let mut found = Vec::new();
        super::collect("          nix build .#sutura -L\n", "release.yml", &mut found);
        assert!(found.is_empty());
    }

    #[test]
    fn the_local_composite_actions_are_scanned_too() {
        // RED against the previous behaviour, which read `.github/workflows` alone: this asserts
        // where a reference came FROM, because a verdict cannot tell the two scans apart on a
        // correct tree. `attest-and-sign` reaches `cosign` and is the reference that publishes a
        // release, so it is the one worth naming rather than a synthetic fixture.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Some(scan) = super::gather(&root) else {
            panic!("the scan could not read .github/workflows");
        };
        let references = scan.references;
        let from_actions: Vec<&str> = references
            .iter()
            .map(|r| r.workflow.as_str())
            .filter(|w| w.starts_with("actions/"))
            .collect();
        assert!(
            !from_actions.is_empty(),
            "no flake reference was collected from .github/actions, so a step split out of a \
             workflow has left this gate's sight"
        );
        assert!(
            references
                .iter()
                .any(|r| r.workflow == "actions/attest-and-sign" && r.name == "cosign"),
            "attest-and-sign's `nix run .#cosign` was not seen: collected {from_actions:?}"
        );
    }

    #[test]
    fn a_comment_is_not_a_reference() {
        let mut found = Vec::new();
        super::collect("      # was: nix run .#mkdocs -- build\n", "docs.yml", &mut found);
        assert!(found.is_empty());
    }

    #[test]
    fn apps_come_from_declarations_not_prose() {
        let flake = concat!(
            "        # CI used to reach these through apps.mkdocs, in prose.\n",
            "        apps.zizmor = {\n",
            "        apps.pixi = {\n",
        );
        let apps = super::declared_apps(flake);
        assert_eq!(apps.len(), 2, "a comment must not contribute a name");
        assert!(apps.contains("zizmor"));
    }

    #[test]
    fn a_deleted_app_is_what_this_catches() {
        // The regression that motivated the gate, as a unit test: docs.yml called an app that
        // flake.nix no longer declares.
        let flake = "        apps.pixi = {\n";
        let apps = super::declared_apps(flake);
        let mut found = Vec::new();
        super::collect("        run: nix run .#mkdocs -- build --strict\n", "docs.yml", &mut found);
        assert_eq!(found.len(), 1);
        assert!(!apps.contains(&found[0].name), "mkdocs must read as missing");
    }
}
