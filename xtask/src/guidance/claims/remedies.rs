//! The remedy a claim prints, held to the standard of the prose it corrects.
//!
//! **Split out of `claims.rs` under the 1000-line cap, and the seam is the one
//! `.agents/skills/sutura/gates` names: the HARNESS moves and every assertion stays.** The tests
//! for what is here are in the parent's `tests` module, beside the table they read - a file that
//! adds no `#[test]` is one the causality gate may revert, so moving them here would take this
//! module's own declaration with them and orphan them.
//!
//! Why it exists: `github.com/telekom/sutura#241`. `check-guidance`'s scope is prose files, so the
//! gate never reads its own source, and the sentence one entry handed a reader AS the correction
//! said a transport surface was absent for as long as it took a person to notice it.
//!
//! Three properties of a remedy are mechanical here - it may not repeat a wording any live claim
//! forbids, every repo path it cites must resolve, and every task it cites must exist. **The
//! sentence itself is not**, and [`Contradicted::instead`] says so: nothing derives prose.

use std::path::Path;

use super::{CONTRADICTED, Contradicted, flatten};

/// What a remedy sends a reader to, read out of one backtick span.
///
/// Three shapes and no fourth. Everything else a remedy puts in backticks - a type, a flag, a
/// settings key, a refusal code - is prose this cannot judge, and it does not try to.
enum Cited<'a> {
    /// A `just <name>` recipe.
    Recipe(&'a str),
    /// A `cargo xtask <name>` gate.
    Gate(&'a str),
    /// A repo-relative path.
    Path(&'a str),
}

/// Is this span shaped like a repo path?
///
/// A slash plus path characters and nothing else, which is narrower than "holds a slash" on
/// purpose: `sources.<alias>.kind` is a settings key and `GET /v1/catalog` is a route, and neither
/// is something to open. A bare filename with no slash - `ci.yml` - is deliberately not read
/// either, because which of five workflows a remedy meant would be a guess and a gate may not
/// guess. Both are limits rather than oversights: they under-claim.
pub(super) fn path_shaped(span: &str) -> bool {
    span.contains('/')
        && !span.starts_with('/')
        && span
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// Read one backtick span as a citation, or `None` when it is prose.
///
/// `task_name_at` is the parent module's, unchanged: a flag and a placeholder are correct prose
/// there and correct prose here, and a second copy of that judgement would be a second thing to
/// keep true.
fn cited(span: &str) -> Option<Cited<'_>> {
    if let Some(tail) = span.strip_prefix("just ") {
        return super::super::task_name_at(tail.trim_start()).map(Cited::Recipe);
    }
    if let Some(tail) = span.strip_prefix("cargo xtask ") {
        return super::super::task_name_at(tail.trim_start()).map(Cited::Gate);
    }
    path_shaped(span).then_some(Cited::Path(span))
}

/// Every citation in one remedy, in order.
///
/// Fields 1, 3, 5 ... of a backtick split are the spans, and the bound stops an unterminated
/// trailing backtick being read as one - the same walk `check-tasks` does over the justfile.
fn citations(remedy: &str) -> Vec<Cited<'_>> {
    let spans: Vec<&str> = remedy.split('`').collect();
    let mut out = Vec::new();
    let mut at = 1_usize;
    while at.saturating_add(1) < spans.len() {
        if let Some(span) = spans.get(at)
            && let Some(one) = cited(span)
        {
            out.push(one);
        }
        at = at.saturating_add(2);
    }
    out
}

/// Does the tree hold what a citation names?
///
/// Exact first, then a PREFIX of the last segment, because `docs/adr/0002` is how this repo cites
/// a numbered record and the file is `0002-<slug>.md`. That is the resolution a reader performs;
/// without it the only citable form would be the whole filename, which is not how the records are
/// cited anywhere else.
fn resolves(root: &Path, cited: &str) -> bool {
    let full = root.join(cited);
    if full.exists() {
        return true;
    }
    let Some(parent) = full.parent() else {
        return false;
    };
    let Some(prefix) = full.file_name().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    std::fs::read_dir(parent).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_str().is_some_and(|name| name.starts_with(prefix)))
    })
}

/// The remedies, held to the standard of the prose they correct.
///
/// **This is `github.com/telekom/sutura#241`'s mechanism.** Takes the live rules rather than
/// reading the table, so the fixtures in `tests` exercise the code the gate runs and not a
/// re-implementation of it.
pub(super) fn remedies_hold(root: &Path, live: &[&Contradicted]) -> Vec<String> {
    let gates = super::super::known_tasks();
    let recipes = crate::tasks::recipe_names(root);
    let mut problems = Vec::new();
    for rule in live {
        // Flattened with the same function the prose goes through, so a wording that is found in a
        // page is found in a remedy. The continuation-joined literals in this file need it for
        // nothing today, and a remedy written any other way would need it.
        let (remedy, _) = flatten(rule.instead);
        for other in live {
            for wording in other.wordings {
                if remedy.contains(wording) {
                    problems.push(format!(
                        "the `{}` remedy repeats \"{wording}\", which `{}` forbids in prose - the \
                         correction may not restate the claim",
                        rule.name, other.name
                    ));
                }
            }
        }
        for one in citations(&remedy) {
            match one {
                Cited::Recipe(name) => match &recipes {
                    Some(known) if !known.contains(name) => problems.push(format!(
                        "the `{}` remedy cites `just {name}`, which names no recipe",
                        rule.name
                    )),
                    None => problems.push(format!(
                        "the `{}` remedy cites `just {name}` and the justfile could not be read to \
                         check it",
                        rule.name
                    )),
                    Some(_) => {}
                },
                Cited::Gate(name) if !gates.contains(name) => problems.push(format!(
                    "the `{}` remedy cites `cargo xtask {name}`, which is not a task",
                    rule.name
                )),
                Cited::Path(path) if !resolves(root, path) => {
                    problems.push(format!("the `{}` remedy cites `{path}`, which is not in the tree", rule.name));
                }
                Cited::Gate(_) | Cited::Path(_) => {}
            }
        }
    }
    problems
}

/// Did the remedy scan read anything, rather than the remedies being clean?
///
/// Separate from [`remedies_hold`] for the reason `check-tasks` separates its own: this is a
/// property of THIS table, not of the rule. Ten path citations stand in it, so a walk that finds
/// none means the span reader stopped reading and a check that reads nothing passes everything.
///
/// **Deliberately not the same fail-closed for the task half.** No remedy has to cite a task, so
/// demanding one would gate a habit rather than a rule.
pub(super) fn remedy_scan_broke(live: &[&Contradicted]) -> Vec<String> {
    let mut paths = 0_usize;
    for rule in live {
        let (remedy, _) = flatten(rule.instead);
        paths = paths.saturating_add(citations(&remedy).iter().filter(|one| matches!(one, Cited::Path(_))).count());
    }
    if paths == 0 {
        return vec![String::from(
            "read no repo path out of any live remedy - the citation scan is broken, not the table",
        )];
    }
    Vec::new()
}

pub(in crate::guidance) fn remedy_problems(root: &Path) -> Vec<String> {
    let live: Vec<&Contradicted> = CONTRADICTED.iter().filter(|rule| rule.is_live(root)).collect();
    let mut problems = remedy_scan_broke(&live);
    problems.extend(remedies_hold(root, &live));
    problems
}
