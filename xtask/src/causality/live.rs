//! `Live-Cell: <test-fn-name> <system> <ci-job>` - moving a golden-matrix cell out of the offline
//! scope, the way an `#[ignore]`d test leaves it.
//!
//! A golden-matrix cell whose changed lines run only for a data system unavailable offline
//! (bigquery) passes on base for the same reason it passes on HEAD: that system skips, so no offline
//! run can redden it and the per-test rule refuses it as a green sibling. The trailer is a CLAIM
//! this module CHECKS, never a permission that replaces the check, and it refuses the whole run
//! on either of two failures: `<system>` must be named `from: Exempt::Unavailable` in
//! [`EXEMPTIONS`], and `<ci-job>` must be a job key under `jobs:` in [`CI`]. A malformed trailer -
//! not exactly three whitespace-separated words, or a first word that is not a Rust identifier -
//! declares nothing and is skipped.
//!
//! **THE LIMIT, stated plainly.** Offline causality cannot measure a live cell; the named CI job's
//! runs are its evidence. Nothing here reads whether the changed lines really run only for that
//! system, or whether the job selects the cell - review holds both. And the trailer is read from
//! base..HEAD only, so it exempts nothing once its commit has landed.

use std::collections::BTreeSet;

use super::names;
use super::scoped::Scan;
use super::worktree;
use crate::workflows::contexts::block_keys;

const TRAILER: &str = "Live-Cell:";

/// The repository-relative path of the exemptions a system must be exempted under.
pub(super) const EXEMPTIONS: &str = "crates/sutura-app/tests/adapters/exemptions.rs";

/// The repository-relative path of the workflow whose `jobs:` a trailer may name.
pub(super) const CI: &str = ".github/workflows/ci.yml";

/// One well-formed `Live-Cell:` trailer: the cell it exempts, the system it runs only for, and the
/// CI job whose runs are its evidence.
#[derive(Debug, PartialEq, Eq)]
struct Cell {
    /// The added test function's name.
    name: String,
    /// The data system the cell runs only for.
    system: String,
    /// The `jobs:` key whose runs measure the cell.
    job: String,
}

/// Every `Live-Cell:` trailer the range carried, each of them checked against [`EXEMPTIONS`] and
/// [`CI`].
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Live(Vec<Cell>);

/// Why a trailer refused the whole run.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LiveError {
    /// `<system>` is not exempted `Unavailable`, so the cell is not known to be unmeasurable here.
    NotExempt { cell: String, system: String },
    /// `<ci-job>` is not a job the workflow declares.
    NoJob { cell: String, job: String },
}

impl std::fmt::Display for LiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotExempt { cell, system } => write!(
                f,
                "`Live-Cell: {cell}` names {system}, which {EXEMPTIONS} does not exempt \
                 `from: Exempt::Unavailable` - only a system unavailable offline may leave the scope"
            ),
            Self::NoJob { cell, job } => write!(
                f,
                "`Live-Cell: {cell}` names {job}, which is no job in {CI} - name the live job whose \
                 runs measure the cell"
            ),
        }
    }
}

impl Live {
    /// Every `Live-Cell:` trailer in `log` (`base..HEAD`'s NUL-delimited messages, split per commit
    /// exactly as [`worktree::commit_logs`] does - see `super::weakens::Waived::of` for why), refused
    /// on the first trailer whose system or job fails its check.
    pub(super) fn parse(log: &str, exemptions: &str, ci: &str) -> Result<Self, LiveError> {
        let entries: Vec<&str> = exemptions.lines().map(str::trim).collect();
        let jobs = block_keys(ci, "jobs:");
        let mut cells = Vec::new();
        for (_, message) in worktree::commit_logs(log) {
            for line in message.lines() {
                let Some(rest) = line.trim().strip_prefix(TRAILER) else {
                    continue;
                };
                let words: Vec<&str> = rest.split_whitespace().collect();
                let [name, system, job] = words.as_slice() else {
                    continue;
                };
                if names::Ident::parse(name).is_none() {
                    continue;
                }
                let exempted = entries.windows(2).any(|pair| {
                    matches!(pair, [first, second]
                        if *first == format!("system: \"{system}\",")
                        && *second == "from: Exempt::Unavailable,")
                });
                if !exempted {
                    return Err(LiveError::NotExempt {
                        cell: String::from(*name),
                        system: String::from(*system),
                    });
                }
                if !jobs.iter().any(|one| one == job) {
                    return Err(LiveError::NoJob {
                        cell: String::from(*name),
                        job: String::from(*job),
                    });
                }
                cells.push(Cell {
                    name: String::from(*name),
                    system: String::from(*system),
                    job: String::from(*job),
                });
            }
        }
        Ok(Self(cells))
    }

    /// [`EXEMPTIONS`] and [`CI`] under `root`, then [`Self::parse`]. A file that cannot be read is
    /// empty - an absent exemptions file makes every trailer refuse, the fail-closed direction.
    pub(super) fn read(root: &std::path::Path, log: &str) -> Option<Self> {
        let exemptions = std::fs::read_to_string(root.join(EXEMPTIONS)).unwrap_or_default();
        let ci = std::fs::read_to_string(root.join(CI)).unwrap_or_default();
        match Self::parse(log, &exemptions, &ci) {
            Err(e) => {
                eprintln!("xtask test-causality: FAILED - {e}");
                None
            }
            Ok(live) => {
                for cell in &live.0 {
                    println!(
                        "  live cell: {} - runs only for {}, so offline causality cannot measure it; \
                         ci.yml:{}'s runs are its evidence",
                        cell.name, cell.system, cell.job
                    );
                }
                Some(live)
            }
        }
    }

    /// Remove every live cell from a runnable scope, so its cell leaves the offline scope the way
    /// an `#[ignore]`d test does.
    pub(super) fn scan(&self, scan: Scan) -> Scan {
        if self.0.is_empty() {
            return scan;
        }
        let Scan::Runnable(scoped) = scan else {
            return scan;
        };
        let names: BTreeSet<String> = self.0.iter().map(|cell| cell.name.clone()).collect();
        if let Some(kept) = scoped.minus(&names) {
            return Scan::Runnable(kept);
        }
        let mut out: Vec<names::Ident> = scoped.ignored().to_vec();
        for test in scoped.tests() {
            if let Some(ident) = names::Ident::parse(test.name())
                && !out.contains(&ident)
            {
                out.push(ident);
            }
        }
        Scan::OnlyIgnored(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{Live, LiveError};
    use crate::causality::fixtures::scoped as added;
    use crate::causality::scoped::{Scan, Scoped};

    /// One commit's entry in [`super::worktree::messages`]'s own NUL-delimited format.
    fn commit(hash: &str, body: &str) -> String {
        format!("{hash}\u{0}{body}\u{0}")
    }

    /// An exemptions text where only `bigquery` is exempted `Unavailable`.
    const EXEMPTED: &str = "\
        system: \"bigquery\",\n\
        from: Exempt::Unavailable,\n\
        \n\
        system: \"bigquery\",\n\
        from: Exempt::KeyProbe,\n\
        \n\
        system: \"oracle\",\n\
        from: Exempt::KeyProbe,\n\
    ";

    /// A minimal workflow whose jobs are `ci` and `bigquery-conformance`.
    const CI_FIXTURE: &str = concat!(
        "on:\n",
        "  push:\n",
        "jobs:\n",
        "  ci:\n",
        "    runs-on: x\n",
        "  bigquery-conformance:\n",
        "    runs-on: x\n",
    );

    /// The names a runnable scan scopes.
    fn runnable_names(scan: &Scan) -> Vec<String> {
        match scan {
            Scan::Runnable(scoped) => scoped.tests().iter().map(|one| String::from(one.name())).collect(),
            other => panic!("expected runnable, got {other:?}"),
        }
    }

    #[test]
    fn a_live_cell_leaves_the_scope_and_an_ordinary_test_stays() {
        let live = Live::parse(
            &commit("h", "Live-Cell: the_live_one bigquery bigquery-conformance\n"),
            EXEMPTED,
            CI_FIXTURE,
        )
        .unwrap();
        let scope = Scan::Runnable(Scoped::of_named(added(
            "p",
            "crates/p/tests/t.rs",
            &["the_live_one", "an_ordinary_one"],
        )));
        assert_eq!(runnable_names(&live.scan(scope)), vec!["an_ordinary_one"]);
    }

    #[test]
    fn a_scope_of_live_cells_only_is_not_runnable_here() {
        let live = Live::parse(
            &commit("h", "Live-Cell: the_live_one bigquery bigquery-conformance\n"),
            EXEMPTED,
            CI_FIXTURE,
        )
        .unwrap();
        let scope = Scan::Runnable(Scoped::of_named(added("p", "crates/p/tests/t.rs", &["the_live_one"])));
        match live.scan(scope) {
            Scan::OnlyIgnored(names) => {
                let names: Vec<String> = names.iter().map(|name| String::from(name.as_str())).collect();
                assert_eq!(names, vec!["the_live_one"]);
            }
            other => panic!("expected OnlyIgnored, got {other:?}"),
        }
    }

    #[test]
    fn a_live_cell_whose_system_is_not_exempted_unavailable_is_refused() {
        let log = commit("h", "Live-Cell: t oracle bigquery-conformance\n");
        assert_eq!(
            Live::parse(&log, EXEMPTED, CI_FIXTURE),
            Err(LiveError::NotExempt {
                cell: String::from("t"),
                system: String::from("oracle")
            })
        );
        let duck = commit("h", "Live-Cell: t duckdb bigquery-conformance\n");
        let err = Live::parse(&duck, EXEMPTED, CI_FIXTURE).unwrap_err();
        assert!(err.to_string().contains("does not exempt"));
    }

    #[test]
    fn a_live_cell_whose_job_ci_does_not_declare_is_refused() {
        let push = commit("h", "Live-Cell: t bigquery push\n");
        assert_eq!(
            Live::parse(&push, EXEMPTED, CI_FIXTURE),
            Err(LiveError::NoJob {
                cell: String::from("t"),
                job: String::from("push")
            })
        );
        let nightly = commit("h", "Live-Cell: t bigquery nightly\n");
        assert!(matches!(
            Live::parse(&nightly, EXEMPTED, CI_FIXTURE),
            Err(LiveError::NoJob { job, .. }) if job == "nightly"
        ));
    }

    #[test]
    fn a_malformed_live_cell_declares_nothing() {
        assert_eq!(
            Live::parse(&commit("h", "Live-Cell: t bigquery\n"), EXEMPTED, CI_FIXTURE),
            Ok(Live::default())
        );
        assert_eq!(
            Live::parse(
                &commit("h", "Live-Cell: not-an-ident bigquery bigquery-conformance\n"),
                EXEMPTED,
                CI_FIXTURE
            ),
            Ok(Live::default())
        );
        let well_formed = Live::parse(&commit("h", "Live-Cell: t bigquery bigquery-conformance\n"), "", "");
        assert!(matches!(well_formed, Err(LiveError::NotExempt { .. })));
    }
}
