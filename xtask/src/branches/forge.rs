//! What the forge says about a branch, through `gh`.
//!
//! One question, asked once: which local branch names are the head of a pull request, and what
//! state is it in. Two signals come out of it and they do different jobs:
//!
//! * an **open** pull request is a guard - somebody is still reading that branch, and there is no
//!   git-only way to know it from inside a checkout;
//! * a **merged** pull request is evidence, and only together with the head commit the forge
//!   recorded for it. A pull request merged this morning says nothing about a commit written since,
//!   and comparing the recorded head against the branch tip is what makes that difference visible.
//!
//! When `gh` cannot answer, this reports [`Forge::Silent`] and the decision keeps every branch.
//! That is the fail-safe direction: degrading to the git signals alone would drop the open-pull-
//! request guard silently, which is the one signal a checkout cannot reconstruct.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use super::decide::{Forge, PullRequest};

/// How many pull requests to ask about.
///
/// A branch whose pull request is older than this window simply has no forge signal, and no signal
/// keeps the branch - so the bound costs cleanliness rather than safety.
const LIMIT: &str = "300";

/// What one row of `gh pr list --json ...` carries.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    /// The branch name the pull request is opened from.
    head: String,
    /// The head commit as the forge records it.
    oid: String,
    /// `OPEN`, `MERGED` or `CLOSED`.
    state: String,
    /// The pull request number.
    number: u64,
}

/// What the forge knows, by branch name.
pub(crate) struct Pulls {
    /// Whether it could be asked at all.
    pub(crate) forge: Forge,
    /// Per branch name. Empty when the forge was silent.
    pub(crate) by_branch: BTreeMap<String, PullRequest>,
}

impl Pulls {
    /// What the forge says about one branch. `Unknown` when it says nothing.
    pub(crate) fn of(&self, branch: &str) -> PullRequest {
        self.by_branch.get(branch).cloned().unwrap_or(PullRequest::Unknown)
    }

    /// How many are open and how many merged, for the report's header.
    pub(crate) fn tally(&self) -> (usize, usize) {
        let open = self
            .by_branch
            .values()
            .filter(|pull| matches!(**pull, PullRequest::Open { .. }))
            .count();
        let merged = self
            .by_branch
            .values()
            .filter(|pull| matches!(**pull, PullRequest::Merged { .. }))
            .count();
        (open, merged)
    }
}

/// Do not ask, because the caller stated there is nothing to ask.
///
/// A checkout with no remote is the honest case for this: there are no pull requests, so the guard
/// has nothing to protect. It is a flag rather than a detection because "gh could not tell whether
/// this repository has pull requests" and "this repository has no pull requests" look identical
/// from here, and only one of them is safe to act on.
pub(crate) const fn waived() -> Pulls {
    Pulls {
        forge: Forge::Waived,
        by_branch: BTreeMap::new(),
    }
}

/// Ask the forge.
pub(crate) fn ask(root: &Path) -> Pulls {
    match rows(root) {
        Err(because) => Pulls {
            forge: Forge::Silent { because },
            by_branch: BTreeMap::new(),
        },
        Ok(rows) => Pulls {
            forge: Forge::Answered,
            by_branch: fold(&rows),
        },
    }
}

/// Collapse the rows into one answer per branch.
///
/// A branch can be the head of several pull requests over its life. An OPEN one wins, because it
/// is a guard and a guard must not be outvoted by history; otherwise the highest-numbered merged
/// one wins, which is the most recent.
fn fold(rows: &[Row]) -> BTreeMap<String, PullRequest> {
    let mut out: BTreeMap<String, PullRequest> = BTreeMap::new();
    for row in rows {
        let candidate = match row.state.as_str() {
            "OPEN" => PullRequest::Open { number: row.number },
            "MERGED" => PullRequest::Merged {
                number: row.number,
                head: row.oid.clone(),
            },
            // A closed pull request that was never merged is not evidence of anything, and it is
            // not a guard either.
            _ => continue,
        };
        let keep = match out.get(&row.head) {
            None | Some(PullRequest::Unknown) => true,
            Some(PullRequest::Open { .. }) => false,
            Some(PullRequest::Merged { number, .. }) => matches!(candidate, PullRequest::Open { .. }) || row.number > *number,
        };
        if keep {
            out.insert(row.head.clone(), candidate);
        }
    }
    out
}

/// Run `gh` and read its rows, or say what went wrong in one line.
fn rows(root: &Path) -> Result<Vec<Row>, String> {
    let mut command = Command::new("gh");
    crate::repo::strip_git_env(&mut command);
    let out = command
        .current_dir(root)
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--limit",
            LIMIT,
            "--json",
            "number,state,headRefName,headRefOid",
        ])
        .output()
        .map_err(|error| format!("gh did not run: {error}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let line = stderr.lines().find(|line| !line.trim().is_empty()).unwrap_or("");
        return Err(format!(
            "gh exited {}: {}",
            out.status.code().unwrap_or(-1),
            line.trim().chars().take(160).collect::<String>()
        ));
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

/// Read `gh`'s JSON. A row missing a field it needs is dropped; a document that is not an array is
/// a silent forge rather than an empty answer.
fn parse(text: &str) -> Result<Vec<Row>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("gh returned no readable JSON: {error}"))?;
    let array = value
        .as_array()
        .ok_or_else(|| String::from("gh returned something that is not a list of pull requests"))?;
    Ok(array.iter().filter_map(row).collect())
}

fn row(value: &serde_json::Value) -> Option<Row> {
    Some(Row {
        head: String::from(value.get("headRefName")?.as_str()?),
        oid: String::from(value.get("headRefOid")?.as_str()?),
        state: String::from(value.get("state")?.as_str()?),
        number: value.get("number")?.as_u64()?,
    })
}

#[cfg(test)]
mod tests {
    use super::{PullRequest, Row, fold, parse};

    /// The shape `gh pr list --json number,state,headRefName,headRefOid` returns.
    const ANSWER: &str = r#"[
      {"number": 52, "state": "MERGED", "headRefName": "feat/scope", "headRefOid": "aaaa111"},
      {"number": 61, "state": "OPEN",   "headRefName": "feat/identity", "headRefOid": "bbbb222"},
      {"number": 44, "state": "CLOSED", "headRefName": "feat/abandoned", "headRefOid": "cccc333"}
    ]"#;

    #[test]
    fn an_answer_becomes_one_signal_per_branch() {
        let rows = parse(ANSWER).expect("the answer parses");
        assert_eq!(rows.len(), 3);
        let folded = fold(&rows);

        assert_eq!(
            folded.get("feat/scope"),
            Some(&PullRequest::Merged {
                number: 52,
                head: String::from("aaaa111")
            })
        );
        assert_eq!(folded.get("feat/identity"), Some(&PullRequest::Open { number: 61 }));
        // Closed and never merged is neither evidence nor a guard.
        assert_eq!(folded.get("feat/abandoned"), None);
    }

    #[test]
    fn an_open_pull_request_is_never_outvoted_by_a_merged_one() {
        // A branch reopened after a merge: the guard has to win, whatever order the rows arrive in.
        let rows = vec![
            Row {
                head: String::from("feat/again"),
                oid: String::from("aaaa111"),
                state: String::from("MERGED"),
                number: 10,
            },
            Row {
                head: String::from("feat/again"),
                oid: String::from("bbbb222"),
                state: String::from("OPEN"),
                number: 9,
            },
        ];
        assert_eq!(fold(&rows).get("feat/again"), Some(&PullRequest::Open { number: 9 }));

        let mut reversed = rows;
        reversed.reverse();
        assert_eq!(fold(&reversed).get("feat/again"), Some(&PullRequest::Open { number: 9 }));
    }

    #[test]
    fn the_most_recent_merged_pull_request_is_the_one_that_counts() {
        let rows = vec![
            Row {
                head: String::from("feat/twice"),
                oid: String::from("aaaa111"),
                state: String::from("MERGED"),
                number: 3,
            },
            Row {
                head: String::from("feat/twice"),
                oid: String::from("bbbb222"),
                state: String::from("MERGED"),
                number: 7,
            },
        ];
        assert_eq!(
            fold(&rows).get("feat/twice"),
            Some(&PullRequest::Merged {
                number: 7,
                head: String::from("bbbb222")
            })
        );
    }

    #[test]
    fn a_document_that_is_not_a_list_is_a_silent_forge_rather_than_an_empty_answer() {
        // The difference matters: an empty answer would delete on the git signals alone, which
        // drops the open-pull-request guard without saying so.
        drop(parse("{\"message\": \"not authenticated\"}").unwrap_err());
        drop(parse("not json at all").unwrap_err());
        // A row missing a field is dropped, and the rest of the answer still counts.
        let partial = r#"[{"number": 1, "state": "OPEN"}, {"number": 2, "state": "OPEN", "headRefName": "feat/x", "headRefOid": "dddd444"}]"#;
        let rows = parse(partial).expect("the document parses");
        assert_eq!(rows.len(), 1);
    }
}
