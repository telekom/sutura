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

/// Read one in-scope file, distinguishing *not there* from *could not look*.
///
/// **THE WHOLE POINT OF THIS FUNCTION IS THAT THOSE TWO ARE DIFFERENT**, and it is
/// `github.com/telekom/sutura#412`. Every arm below used to spell `let Ok(text) = read else
/// { continue; }`, so a file CI reads and this walk could not was dropped in silence - and because
/// `crate::workflows::gather` derives its printed `files` count from `read.len()`, the witness
/// shrank along with the walk. An undeclared `nix run .#` inside such a file is then invisible at
/// exit 0, which is the exact failure this module's header records having happened twice for a
/// different reason.
///
/// And it is scoped rather than blanket, which is the trap #412 names: `check-shipped-binaries`
/// reddened a correct tree because a file *out of scope* is not a file *unreadable*. Here the
/// legitimate absence is real and load-bearing - the actions arm probes both `action.yml` and
/// `action.yaml` and exactly one of them exists, so `NotFound` is the answer *no* and anything else
/// is the scan being broken.
///
/// `Ok(None)` is a file that is not there. `Err` names the file and the OS error.
fn in_scope_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(why) => Err(format!("{}: {why}", path.display())),
    }
}

/// List a directory, distinguishing an absent one from one that cannot be listed.
///
/// The same distinction one level up. `read_dir(..).into_iter().flatten().flatten()` used to make
/// a directory that EXISTS and cannot be read indistinguishable from one that is not there, and
/// the module header's own argument says the second is legitimate - not the first.
fn in_scope_dir(path: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(entries.flatten().map(|entry| entry.path()).collect()),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(why) => Err(format!("{}: {why}", path.display())),
    }
}

/// Every file CI invokes something from, in one pass.
///
/// `None` for a missing `.github/workflows`, which is the scan being broken rather than an answer -
/// see the module header for why the other two directories are optional - **and now also for an
/// in-scope file or directory this walk could not read**, which is [`in_scope_text`]'s subject.
pub(crate) fn ci_sources(root: &Path) -> Option<Vec<Source>> {
    match collect(root) {
        Ok(found) => Some(found),
        Err(why) => {
            eprintln!("xtask: {why}");
            eprintln!("  This walk is what five gates read CI from, and a file it drops takes that");
            eprintln!("  file's `nix run .#` references out of the scan AND out of the count that");
            eprintln!("  would have shown it. So it is a refusal rather than a smaller number.");
            None
        }
    }
}

/// The walk, with every read that can fail named.
fn collect(root: &Path) -> Result<Vec<Source>, String> {
    let mut out = Vec::new();

    let workflows = root.join(".github").join("workflows");
    let Ok(entries) = std::fs::read_dir(&workflows) else {
        return Err(String::from(
            "no .github/workflows directory - the scan is broken, not the workflows",
        ));
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let yaml = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "yml" || e == "yaml");
        if !yaml {
            continue;
        }
        // No `NotFound` arm to take here in practice - the entry came out of a listing of this very
        // directory - and it is still routed through the same reader so there is one answer to
        // *could this file be read* rather than two that can disagree.
        let Some(text) = in_scope_text(&path)? else {
            continue;
        };
        let label = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        out.push(Source { label, text });
    }

    for dir in in_scope_dir(&root.join(".github").join("actions"))? {
        for candidate in ["action.yml", "action.yaml"] {
            // THE ARM THAT MUST STAY LEGITIMATE: exactly one of the two spellings exists, so
            // `NotFound` here is the answer *this composite action is written the other way* and
            // not a file anybody failed to read.
            let Some(text) = in_scope_text(&dir.join(candidate))? else {
                continue;
            };
            let named = dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            out.push(Source {
                label: format!("actions/{named}"),
                text,
            });
        }
    }

    for path in in_scope_dir(&root.join("nix"))? {
        if path.extension().and_then(|e| e.to_str()) != Some("sh") {
            continue;
        }
        let Some(text) = in_scope_text(&path)? else {
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

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{collect, in_scope_text};

    /// A scratch root keyed on the process id, removed first: a pid is reusable, and inheriting a
    /// previous run's tree is `crate::falsifier`'s recorded defect.
    fn scratch(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-ci-sources-{}-{}", name, std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(root.join(".github").join("workflows")).expect("a workflows directory");
        root
    }

    #[test]
    fn a_workflow_that_cannot_be_read_is_a_refusal_and_not_a_smaller_scan() {
        // `github.com/telekom/sutura#412`. THE FIXTURE IS A DIRECTORY, NOT `chmod 000`, and that is
        // deliberate: a root process is exempt from mode bits and is not exempt from `EISDIR`, so a
        // permission fixture is one that silently stops reproducing under a CI user that happens to
        // be root. Before this change the walk returned one source and no error.
        let root = scratch("unreadable");
        std::fs::write(root.join(".github/workflows/ci.yml"), "on: push\n").expect("a readable workflow");
        std::fs::create_dir_all(root.join(".github/workflows/broken.yml")).expect("the unreadable one");

        let found = collect(&root);
        drop(std::fs::remove_dir_all(&root));

        // Matched rather than `expect_err`: `Source` carries whole file texts, so deriving `Debug`
        // on it to satisfy that helper would put every workflow in the tree into a failure message.
        let Err(why) = found else {
            panic!("an unreadable in-scope workflow was dropped in silence");
        };
        assert!(
            why.contains("broken.yml"),
            "the refusal has to name the file a reader would open: {why}"
        );
    }

    #[test]
    fn a_composite_action_written_the_other_way_is_not_a_failure() {
        // THE TRAP #412 NAMES, and the reason the reader distinguishes `NotFound` from every other
        // error rather than guarding every read: `action.yml` and `action.yaml` are probed as a
        // pair and exactly one exists, so a blanket guard would redden a correct tree - which is
        // what `check-shipped-binaries` did over a PNG that was out of scope rather than unreadable.
        let root = scratch("either-spelling");
        std::fs::write(root.join(".github/workflows/ci.yml"), "on: push\n").expect("a readable workflow");
        std::fs::create_dir_all(root.join(".github/actions/signer")).expect("an action directory");
        std::fs::write(root.join(".github/actions/signer/action.yaml"), "name: signer\n").expect("the yaml spelling");

        let found = collect(&root);
        drop(std::fs::remove_dir_all(&root));

        let Ok(found) = found else {
            panic!("the missing `action.yml` spelling was read as a broken scan");
        };
        assert!(
            found.iter().any(|source| source.label == "actions/signer"),
            "the action written as `action.yaml` was not collected"
        );
    }

    #[test]
    fn the_reader_separates_absent_from_unreadable() {
        // The two answers the old `let Ok(..) else { continue }` collapsed into one.
        let root = scratch("two-answers");
        assert_eq!(
            in_scope_text(&root.join(".github/workflows/nothing-here.yml")),
            Ok(None),
            "an absent file has to be the answer `no`, or the actions arm cannot work"
        );
        std::fs::create_dir_all(root.join(".github/workflows/a-directory.yml")).expect("a directory in its place");
        let refused = in_scope_text(&root.join(".github/workflows/a-directory.yml"));
        drop(std::fs::remove_dir_all(&root));
        assert!(
            refused.is_err(),
            "a path that exists and cannot be read as text has to be an error: {refused:?}"
        );
    }
}
