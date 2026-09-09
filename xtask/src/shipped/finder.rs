//! The FINDER: which files under `.github` this gate's verdict is about, and the two arms that
//! decide it fails closed rather than shrinking.
//!
//! **Its own file because the parent is against the unexemptable 1000-line cap** - it crossed it
//! on this change - and because these three items are one job: producing the map every other rule
//! in [`super`] then reads. Its tests are NEW and come with it, so nothing here can be orphaned by
//! reverting the parent.
//!
//! **Every gate-local pair in this tree sits ABOVE the discovery.** [`super::declaration::read`]
//! compares the walk's own list against the map's keys, `guidance::pages` counts `read` against
//! `offered`, `check-api-links` counts `scanned == pages.len()` - and all three are counted off
//! the map a discovery produced, so none of them can see a failure that happened before that map
//! was populated. That is why both arms here are refusals at the point of READING, and why
//! [`MUST_JUDGE`] asks for files by name instead of counting them.

use super::Yaml;
use std::collections::BTreeMap;

/// Every workflow and every local composite action, as `(repo-relative path, contents)`.
///
/// The same two directories `check-workflows` reads, and for the same reason: since #111 a build
/// step is a composite action, so a literal can live in either place.
///
/// **FAIL CLOSED ON WHAT IT CANNOT READ *AND* ON WHAT IT CANNOT LIST**, which is one rule and used
/// to be two. A file the FINDER never hands over is not in `files` either, so `declaration`'s
/// `inspected` against `offered` holds trivially over it - and both sides of that pair are counted
/// off this map, so a narrowed DISCOVERY shrinks the denominator with the numerator. Two
/// measurements, one per arm:
///
/// * `embedded-dependency-list/action.yml` made non-UTF-8: `ok - 3 literal(s) across 11 file(s)`,
///   exit 0, and the sibling loop rule's `8 step(s)` quietly became `7`. That arm was fixed first.
/// * `chmod 000 .github/actions`: `ok - 2 literal(s) across 8 file(s)`, exit 0, the loop rule
///   `8 step(s)` -> `4 step(s)` and still `ok`. **The count agreed with itself over a tree it never
///   looked at** - the state this function's own caller warns about - because `if let Ok(entries) =
///   read_dir(..)` swallowed the error and `.flatten()` dropped a `DirEntry` that errors mid-walk.
///   Both composite actions carrying a literal, the files #111 was about, left the scan in silence.
///
/// So the `NotFound`-versus-anything-else split the `read_to_string` below already had now covers
/// the DIRECTORIES as well, via [`entries_in`]. `github.com/telekom/sutura#414` is the class.
///
/// **And the half this gate could not reach is closed one layer down now.** `documented::pages`
/// walks `docs/` through `repo::collect_files`, which used to fail open the same way: `chmod 000
/// docs/adr` - the directory holding the very ADR this gate's remedies cite - was `ok - 4
/// literal(s) across 12 file(s)` at **exit 0 and silent**, measured on `110591d5`. That walker
/// returns a `repo::Census` now and records what it could not reach, so the same seed is **exit 1**
/// naming `docs/adr`, and this gate got the property without being migrated - which is the whole
/// argument for fixing the three shared doors rather than each gate above them.
pub(super) fn yaml_files(root: &std::path::Path) -> Result<Yaml, String> {
    let mut files = BTreeMap::new();
    let github = root.join(".github");
    for entry in entries_in(&github.join("workflows"), ".github/workflows")? {
        let path = entry.path();
        let yaml = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "yml" || e == "yaml");
        if !yaml {
            continue;
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |n| format!(".github/workflows/{}", n.to_string_lossy()));
        let text = std::fs::read_to_string(&path).map_err(|error| format!("{name}: {error}"))?;
        files.insert(name, text);
    }
    // One level deep, which is not an approximation: an action IS
    // `.github/actions/<name>/action.yml` by GitHub's own resolution rules.
    for dir in entries_in(&github.join("actions"), ".github/actions")? {
        for leaf in ["action.yml", "action.yaml"] {
            let path = dir.path().join(leaf);
            let name = dir
                .path()
                .file_name()
                .map_or_else(String::new, |n| format!(".github/actions/{}/{leaf}", n.to_string_lossy()));
            // ABSENT IS NOT UNREADABLE, and only the first is legitimate: an action declares
            // ONE of the two spellings, so the other is missing by construction. Anything else
            // - a non-UTF-8 file, a permission - is a file this gate was meant to read and did
            // not, and it fails closed rather than shrinking the denominator.
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    files.insert(name, text);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("{name}: {error}")),
            }
        }
    }
    Ok(files)
}

/// Every entry one directory holds, or the reason it could not be listed.
///
/// **ABSENT IS NOT UNLISTABLE, and it is the same split [`yaml_files`] already applied to a FILE
/// one level down.** A repository with no `.github/actions` at all has nothing to compare; a
/// directory that exists and cannot be read is a set of files this gate was meant to read and did
/// not, and returning early from it shrinks the denominator along with the scan. The iterator's
/// own `Err` is propagated rather than `.flatten()`ed away for the same reason: a `DirEntry` that
/// errors mid-walk is a file dropped in silence. `github.com/telekom/sutura#414`.
///
/// `label` rather than the path: this prints, and a repo-relative name is the one a reader acts on.
fn entries_in(dir: &std::path::Path, label: &str) -> Result<Vec<std::fs::DirEntry>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{label}: {error}")),
    };
    entries
        .map(|entry| entry.map_err(|error| format!("{label}: {error}")))
        .collect()
}

/// The files this gate's subject must CONTAIN, whatever any predicate says about it.
///
/// **AN ANCHOR RATHER THAN AN `== 0` FLOOR, and it is strictly stronger: it survives a scope
/// predicate that stopped matching.** A count floor asks *did I judge anything*, which almost
/// anything satisfies; this asks *did I judge THESE*, and a walk that lost the composite actions
/// cannot answer yes by reading the workflows. Generalises `nix_files`' `flake.nix` rule.
///
/// **It is checked HERE rather than in `declaration::read`, and that placement is the point.**
/// Every gate-local pair in this tree - this one, `guidance::pages`, `check-api-links` - compares
/// two things counted off the map the discovery produced, so it sits ABOVE the discovery and
/// cannot see a failure that happened before the map was populated. This is the one arm that is
/// about the map itself.
///
/// **Both entries are load-bearing and named in this gate's own prose:** `release.yml` is the
/// release path, and `build-artefacts` is the composite action `github.com/telekom/sutura#111`
/// was about - the file that leaves the scan first when the `.github/actions` walk fails.
/// Renaming one is a deliberate edit to the release path and the remedy says so.
pub(super) const MUST_JUDGE: [&str; 2] = [".github/workflows/release.yml", ".github/actions/build-artefacts/action.yml"];

/// The files [`MUST_JUDGE`] names that this tree did not hand over.
pub(super) fn unanchored(files: &Yaml) -> Vec<&'static str> {
    MUST_JUDGE.into_iter().filter(|name| !files.contains_key(*name)).collect()
}

#[cfg(test)]
mod tests {
    use super::{MUST_JUDGE, entries_in, unanchored, yaml_files};

    #[test]
    fn the_files_this_verdict_is_about_are_asked_for_by_name() {
        // THE ANCHOR, and what it does that a count cannot: `declaration::read`'s pair compares
        // two things counted off the map the DISCOVERY produced, so a map that never received the
        // composite actions satisfies it - and the literal floor is satisfied by the workflows.
        // This asks for the files by name, so it survives both.
        let root = crate::repo::root().expect("could not locate the repo");
        let files = yaml_files(&root).expect("a file under `.github` could not be read");
        assert!(
            unanchored(&files).is_empty(),
            "the real tree does not hold this gate's own anchors: {:?}",
            unanchored(&files)
        );
        // And each one dropped is a verdict, over a tree where everything else still agrees.
        for dropped in MUST_JUDGE {
            let narrowed: super::Yaml = files
                .iter()
                .filter(|(name, _)| name.as_str() != dropped)
                .map(|(name, text)| (name.clone(), text.clone()))
                .collect();
            assert_eq!(
                super::unanchored(&narrowed),
                vec![dropped],
                "{dropped} left the subject and nothing said so"
            );
        }
    }

    #[test]
    fn a_directory_that_is_absent_is_not_one_that_cannot_be_listed() {
        // THE SPLIT B1 WAS ABOUT, at the level it belongs. `if let Ok(entries) = read_dir(..)`
        // dropped an unreadable directory in silence, so `chmod 000 .github/actions` printed
        // `ok - 2 literal(s) across 8 file(s)` at exit 0 - the DENOMINATOR MOVING WITH THE
        // NUMERATOR, agreeing with itself over a tree it never looked at. Absent stays `Ok`,
        // because a repository with no composite actions has nothing to disagree with.
        let root = crate::repo::root().expect("could not locate the repo");
        let absent = entries_in(&root.join("no-such-directory-here"), "no-such-directory-here");
        assert_eq!(absent.as_deref().map(<[_]>::len), Ok(0), "{absent:?}");
        // ANYTHING ELSE IS A VERDICT. A path that is a FILE gives `NotADirectory` rather than
        // `NotFound`, which is the same arm a permission gives and is deterministic wherever this
        // runs - a `chmod 000` assertion is not, since a run as root can read it anyway.
        let not_a_directory = entries_in(&root.join("flake.nix"), "flake.nix");
        assert!(not_a_directory.is_err(), "a file is not an empty directory");
        assert!(
            not_a_directory.unwrap_err().starts_with("flake.nix: "),
            "the row names the directory a reader has to act on"
        );
    }
}
