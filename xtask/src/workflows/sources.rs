//! The three places CI invokes something from, in one walk.
//!
//! **A module of its own because it has two callers and the drift it prevents has already happened
//! twice.** [`crate::workflows`] reads these files for `nix run .#` / `nix build .#` references;
//! [`crate::venues`] reads the same files to answer *does CI invoke this venue's `Reached by`
//! task*. A second walk would be a second answer to *where does CI invoke things from*, and the
//! recorded failure mode is precisely that a step moved out of a workflow leaves one scan's sight:
//! `nix run .#cosign` lived in a composite action nothing read, and `ci.yml`'s workflow-analysis
//! body moved into `nix/lint-workflows.sh` taking five references with it - the count dropped from
//! 60 to 55 and nothing failed. A hard line cap is what forces steps out, so it will happen again,
//! and when it does both gates follow together or neither does.
//!
//! The three places, and why a missing one is not the same failure in each:
//!
//! * `.github/workflows/*.y{a,}ml` - a repository with no workflows directory cannot be this one,
//!   so [`ci_sources`] answers `None` and its caller fails.
//! * `.github/actions/*/action.y{a,}ml` - labelled by their DIRECTORY, because every one of these
//!   files is called `action.yml` and a failure saying `action.yml:118` names nothing a reader can
//!   open. A repository with no composite action is a repository with none, so a missing directory
//!   is not a failure.
//! * `nix/*.sh` - the shared shell CI reaches through. Same reasoning as the actions half: absent
//!   is legitimate, so it is skipped rather than refused.

use std::path::Path;

/// One file CI reads, and the label a failure should name it by.
pub(crate) struct Source {
    /// `ci.yml`, `actions/attest-and-sign` or `nix/run-gate.sh` - what a reader can open.
    pub(crate) label: String,
    /// The file's whole text. Unparsed: each caller lexes it its own way.
    pub(crate) text: String,
}

/// Every file CI invokes something from, in one pass.
///
/// `None` only for a missing `.github/workflows`, which is the scan being broken rather than an
/// answer - see the module header for why the other two directories are optional.
pub(crate) fn ci_sources(root: &Path) -> Option<Vec<Source>> {
    let mut out = Vec::new();

    let Ok(entries) = std::fs::read_dir(root.join(".github").join("workflows")) else {
        eprintln!("xtask: no .github/workflows directory - the scan is broken, not the workflows");
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
        let label = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.push(Source { label, text });
    }

    let actions = root.join(".github").join("actions");
    for entry in std::fs::read_dir(&actions).into_iter().flatten().flatten() {
        let dir = entry.path();
        for candidate in ["action.yml", "action.yaml"] {
            let Ok(text) = std::fs::read_to_string(dir.join(candidate)) else {
                continue;
            };
            let named = dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            out.push(Source {
                label: format!("actions/{named}"),
                text,
            });
        }
    }

    let scripts = root.join("nix");
    for entry in std::fs::read_dir(&scripts).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sh") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let named = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.push(Source {
            label: format!("nix/{named}"),
            text,
        });
    }

    Some(out)
}
