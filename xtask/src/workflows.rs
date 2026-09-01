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

    let mut references = Vec::new();
    let mut files = 0_usize;
    let github = root.join(".github");
    let Ok(entries) = std::fs::read_dir(github.join("workflows")) else {
        eprintln!("xtask check-workflows: no .github/workflows directory");
        return Verdict::Fail;
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
        files += 1;
        collect(&text, &name, &mut references);
    }

    // AND EVERY LOCAL COMPOSITE ACTION, which this gate did not read and which is where the
    // references now live. `.github/actions/attest-and-sign` runs `nix run .#cosign` and
    // `.github/actions/build-artefacts` runs `nix run .#syft`, so the flake outputs a release
    // depends on were invisible to the one gate whose whole job is to notice a renamed output.
    //
    // The hole was PRE-EXISTING - `attest-and-sign` has held `nix run .#cosign` since it was
    // split out - and it widened rather than appeared: `build-artefacts` was carved out of
    // `release.yml` under the 1000-line cap and took `nix run .#syft` with it. A gate that gets
    // narrower every time a file is split is a gate on its way to reading nothing.
    //
    // Not recursive, and one level deep is not an approximation: an action is
    // `.github/actions/<name>/action.yml` by GitHub's own resolution rules, so there is no
    // deeper place for one to hide.
    if let Ok(dirs) = std::fs::read_dir(github.join("actions")) {
        for dir in dirs.flatten() {
            for leaf in ["action.yml", "action.yaml"] {
                let path = dir.path().join(leaf);
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let name = dir
                    .path()
                    .file_name()
                    .map_or_else(String::new, |n| format!("actions/{}/{leaf}", n.to_string_lossy()));
                files += 1;
                collect(&text, &name, &mut references);
            }
        }
    }

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
///
/// THE BLOCK'S OWN OPENING BRACE IS COUNTED rather than assumed to be on the header line, and
/// that is a second version of the same bug. `depth` used to be set to 1 the moment the header
/// matched, which is right only while the `{` is on that line: written as
///
/// ```text
/// packages = crossPackages // ociImages
///   // nativeImages // {
/// ```
///
/// the brace on the continuation line read as a NESTED attrset, so depth became 2 and every name
/// in the block was invisible - `packages.xtask` among them, which `ci.yml` runs three times.
/// Measured, on the change that split that line. Counting the header's braces like any other
/// line's makes both shapes the same case, and `opened` is what keeps the `depth <= 0` break from
/// firing before the block has started.
fn declared_block(text: &str, header: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut depth = 0_i32;
    let mut inside = false;
    let mut opened = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let header_line = !inside && trimmed.starts_with(header);
        if header_line {
            inside = true;
        } else if !inside {
            continue;
        }

        // Only the outermost level of the block declares an output; everything deeper belongs
        // to one. Counted after the name check so the closing line of a nested attrset does
        // not look like a declaration, and never on the header line, which declares the block
        // rather than a member of it.
        let opens = i32::try_from(trimmed.matches('{').count()).unwrap_or(0);
        let closes = i32::try_from(trimmed.matches('}').count()).unwrap_or(0);

        if !header_line
            && opened
            && depth == 1
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
        if depth > 0 {
            opened = true;
        }
        if opened && depth <= 0 {
            break;
        }
    }
    names
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

    #[test]
    fn a_block_whose_opening_brace_is_on_a_continuation_line_still_declares_its_members() {
        // The `packages = ` line in flake.nix grew past one line when a second shipped binary
        // was added, and the brace moved with it. Depth was pinned to 1 at the header, so the
        // brace on the second line read as a NESTED attrset and every member of the block became
        // invisible - including `xtask`, which `ci.yml` runs three times. This is that shape.
        let flake = concat!(
            "        packages = crossPackages // ociImages\n",
            "          // nativeImages // {\n",
            "          default = sutura;\n",
            "          xtask = craneLib.buildPackage (ciArgs // {\n",
            "            pname = \"xtask\";\n",
            "          });\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "packages = ");
        assert!(names.contains("xtask"), "xtask must be declared, got {names:?}");
        assert!(names.contains("default"), "default must be declared, got {names:?}");
        assert!(!names.contains("pname"), "a nested attribute is not a declaration");
    }

    #[test]
    fn a_block_whose_opening_brace_is_on_the_header_line_is_unchanged() {
        // The shape every other block in flake.nix has, asserted beside the one above so a fix
        // for one cannot quietly become a regression in the other.
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy (ciArgs // {\n",
            "            cargoArtifacts = ciArtifacts;\n",
            "          });\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {");
        assert!(names.contains("clippy"), "got {names:?}");
        assert!(names.contains("hygiene"), "declared below a nested close, got {names:?}");
        assert!(!names.contains("cargoArtifacts"), "a nested attribute is not a declaration");
    }
}
