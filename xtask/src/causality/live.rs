//! `Live-Cell: <test-fn-name> <system> <ci-job>` - a `crates/sutura-app/tests/` matrix cell whose
//! changed lines run only for a data system unavailable offline leaves the offline scope the way
//! an `#[ignore]`d test does, and this module is the check that holds the claim rather than the
//! permission that replaces it.
//!
//! TWO CHECKS. `<system>` must have a `from: Exempt::Unavailable` entry in [`EXEMPTIONS`] whose
//! `runs_in` is `<ci-job>`, and `<ci-job>` must be a job key under `jobs:` in [`CI`]. The coupling
//! is that [`EXEMPTIONS`] names the `runs_in` as a `just` task, the trailer names a ci.yml job,
//! and for bigquery both are `bigquery-conformance`. Only a test under [`MATRIX`] may leave the
//! scope, and a name `Claim-Cell:` also declares is refused. A malformed trailer - not exactly
//! three whitespace-separated words, or a first word that is not a Rust identifier - declares
//! nothing and is skipped.
//!
//! THIRD CHECK, AT HEAD (`github.com/telekom/sutura#1274`): every declared cell is run once here with
//! its output captured, and the trailer is refused unless that output carries the matrix's own
//! `exempt: <system> from Unavailable` line - so a cell whose run never reaches that system's
//! exemption cannot leave the scope.
//!
//! **LIMITS.** The witness run is the checkout as it stands, not the reconstructed HEAD worktree,
//! so an uncommitted edit is what it reads. Its I/O half, `head_output`, is reached by no hermetic
//! cell: that it captures the line rests on one run over #1264's range, where the first two of its
//! nine trailers were witnessed and the third was refused. The line proves the cell reached that system's
//! exemption, not that the changed lines run only for that system, nor that the job selects the
//! cell. The name is a bare test name, so every system's row
//! of a per-system macro cell leaves the measurement, the offline ones too. The live job skips a
//! fork or a Dependabot pull request, and `ci-aggregate` requires it only on any other
//! same-repository pull request or a merge group that selects its category, so elsewhere nothing that gates a merge measures a live cell. And the trailer
//! is read from base..HEAD only, so it exempts nothing once its commit has landed.

use std::collections::BTreeSet;

use super::names;
use super::scoped::Scan;
use super::worktree;
use crate::causality::coverage::Coverage;
use crate::workflows::contexts::block_keys;

const TRAILER: &str = "Live-Cell:";

/// The repository-relative path of the exemptions a system must be exempted under.
pub(super) const EXEMPTIONS: &str = "crates/sutura-app/tests/adapters/exemptions.rs";

/// The repository-relative path of the workflow whose `jobs:` a trailer may name.
pub(super) const CI: &str = ".github/workflows/ci.yml";

/// The test target that declares the golden matrix; any other test stays in scope, so the gate
/// fails closed.
pub(super) const MATRIX: &str = "crates/sutura-app/tests/";

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
    /// `<system>` has no `Unavailable` entry in [`EXEMPTIONS`] whose `runs_in` is `<job>`.
    NotExempt {
        /// The added test function the trailer named.
        cell: String,
        /// The data system the trailer named.
        system: String,
        /// The `<ci-job>` whose runs should measure it.
        job: String,
    },
    /// `<ci-job>` is not a job the workflow declares.
    NoJob {
        /// The added test function the trailer named.
        cell: String,
        /// The `<ci-job>` the workflow does not declare.
        job: String,
    },
    /// The name is also declared `Claim-Cell:`.
    AlsoClaimed {
        /// The test function both trailers named.
        cell: String,
    },
    /// The HEAD run of the cell printed no `exempt: <system> from Unavailable` line.
    NotWitnessed {
        /// The added test function the trailer named.
        cell: String,
        /// The data system whose exemption the run never reached.
        system: String,
    },
}

impl std::fmt::Display for LiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotExempt { cell, system, job } => write!(
                f,
                "`Live-Cell: {cell}` names {system} and {job}, and {EXEMPTIONS} has no \
                 `from: Exempt::Unavailable` entry for {system} whose `runs_in` is {job} - only a \
                 system unavailable offline, measured by that job, may leave the scope"
            ),
            Self::NoJob { cell, job } => write!(
                f,
                "`Live-Cell: {cell}` names {job}, which is no job in {CI} - name the live job whose \
                 runs measure the cell"
            ),
            Self::AlsoClaimed { cell } => write!(
                f,
                "`{cell}` is declared by both `Live-Cell:` and `Claim-Cell:` - a claim cell is \
                 proved by its killing mutation, so it may not also leave the scope"
            ),
            Self::NotWitnessed { cell, system } => write!(
                f,
                "`Live-Cell: {cell}` names {system}, and its run here printed no `exempt: {system} \
                 from Unavailable` line - only a cell that reaches that system's exemption may \
                 leave the scope"
            ),
        }
    }
}

impl Live {
    /// Every `Live-Cell:` trailer in `log` (`base..HEAD`'s NUL-delimited messages, split per commit
    /// exactly as [`worktree::commit_logs`] does - see `super::weakens::Waived::of` for why), refused
    /// on the first trailer whose system or job fails its check, or whose name `Claim-Cell:` also
    /// declares.
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
                let exempted = entries.windows(3).any(|window| {
                    matches!(window, [a, b, c]
                        if *a == format!("system: \"{system}\",")
                        && *b == "from: Exempt::Unavailable,"
                        && *c == format!("runs_in: Some(\"{job}\"),"))
                });
                if !exempted {
                    return Err(LiveError::NotExempt {
                        cell: String::from(*name),
                        system: String::from(*system),
                        job: String::from(*job),
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
        if let Some(claim) = crate::causality::claim::Claim::of(log)
            && let Some(conflict) = cells.iter().find(|cell| claim.cells().contains(&cell.name))
        {
            return Err(LiveError::AlsoClaimed {
                cell: conflict.name.clone(),
            });
        }
        Ok(Self(cells))
    }

    /// [`EXEMPTIONS`] and [`CI`] under `root`, then [`Self::parse`]. A file that cannot be read is
    /// empty - an absent exemptions file makes every trailer refuse, the fail-closed direction.
    pub(super) fn read(root: &std::path::Path, log: &str) -> Option<Self> {
        let exemptions = std::fs::read_to_string(root.join(EXEMPTIONS)).unwrap_or_default();
        let ci = std::fs::read_to_string(root.join(CI)).unwrap_or_default();
        match Self::parse(log, &exemptions, &ci).and_then(|live| live.witnessed(|cell| head_output(root, cell))) {
            Err(e) => {
                eprintln!("xtask test-causality: FAILED - {e}");
                None
            }
            Ok(live) => {
                for cell in &live.0 {
                    println!(
                        "  live cell: {} - declared for {}; under {MATRIX} it leaves the offline scope, \
                         and ci.yml:{}'s runs are its only evidence",
                        cell.name, cell.system, cell.job
                    );
                }
                Some(live)
            }
        }
    }

    /// Refuse the first declared cell whose output, as `output_of` reports it, carries no
    /// `exempt: <system> from Unavailable` line for the system its trailer names.
    pub(super) fn witnessed(self, output_of: impl Fn(&str) -> String) -> Result<Self, LiveError> {
        for cell in &self.0 {
            let line = format!("exempt: {} from Unavailable", cell.system);
            if !output_of(&cell.name).lines().any(|one| one.trim_start().starts_with(&line)) {
                return Err(LiveError::NotWitnessed {
                    cell: cell.name.clone(),
                    system: cell.system.clone(),
                });
            }
        }
        Ok(self)
    }

    /// Remove every live cell from a runnable scope, so its cell leaves the offline scope the way
    /// an `#[ignore]`d test does.
    ///
    /// A declared name leaves the scope ONLY when every scoped test carrying it lives under the
    /// golden matrix ([`MATRIX`]); any other test stays in scope, so the gate fails closed.
    pub(super) fn scan(&self, scan: Scan) -> Scan {
        if self.0.is_empty() {
            return scan;
        }
        let Scan::Runnable(scoped) = scan else {
            return scan;
        };
        let names: BTreeSet<String> = self
            .0
            .iter()
            .map(|cell| cell.name.clone())
            .filter(|name| {
                scoped
                    .tests()
                    .iter()
                    .all(|test| test.name() != name || test.file().starts_with(MATRIX))
            })
            .collect();
        if names.is_empty() {
            return Scan::Runnable(scoped);
        }
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

    /// Move every live cell a scan left `unmeasured` into `not_runnable`, so the ratio reads the
    /// cell as unreachable here rather than as work the proof skipped. Any other variant passes
    /// through unchanged.
    pub(super) fn coverage(&self, coverage: Coverage) -> Coverage {
        match coverage {
            Coverage::Measured {
                measured,
                unmeasured,
                mut not_runnable,
            } => {
                let declared: BTreeSet<&str> = self.0.iter().map(|cell| cell.name.as_str()).collect();
                let mut kept = Vec::new();
                for name in unmeasured {
                    if declared.contains(name.as_str()) {
                        not_runnable.push(name);
                    } else {
                        kept.push(name);
                    }
                }
                Coverage::Measured {
                    measured,
                    unmeasured: kept,
                    not_runnable,
                }
            }
            other @ Coverage::Unknown { .. } => other,
        }
    }
}

/// Every line the matrix cells named `cell` print when run here, captured, or empty when the run
/// cannot start - which [`Live::witnessed`] refuses, the closed direction.
fn head_output(root: &std::path::Path, cell: &str) -> String {
    let filter = format!("test(/(^|::){cell}$/)");
    std::process::Command::new("cargo")
        .current_dir(root)
        .env_remove("NEXTEST_PROFILE")
        .args([
            "nextest",
            "run",
            "-p",
            "sutura-app",
            "--all-features",
            "--no-capture",
            "--no-fail-fast",
            "-E",
        ])
        .arg(&filter)
        .output()
        .map_or_default(|out| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{Live, LiveError};
    use crate::causality::coverage::Coverage;
    use crate::causality::fixtures::scoped as added;
    use crate::causality::scoped::{Scan, Scoped};

    /// One commit's entry in [`super::worktree::messages`]'s own NUL-delimited format.
    fn commit(hash: &str, body: &str) -> String {
        format!("{hash}\u{0}{body}\u{0}")
    }

    /// An exemptions text with `bigquery` measured by `bigquery-conformance`, `postgres` by
    /// `validate`, `oracle` declared unavailable with no measured run, and `clickhouse` by an `on:` key.
    const EXEMPTED: &str = "\
        system: \"bigquery\",\n\
        from: Exempt::Unavailable,\n\
        runs_in: Some(\"bigquery-conformance\"),\n\
        \n\
        system: \"bigquery\",\n\
        from: Exempt::KeyProbe,\n\
        \n\
        system: \"postgres\",\n\
        from: Exempt::Unavailable,\n\
        runs_in: Some(\"validate\"),\n\
        \n\
        system: \"oracle\",\n\
        from: Exempt::Unavailable,\n\
        runs_in: None,\n\
        \n\
        system: \"clickhouse\",\n\
        from: Exempt::Unavailable,\n\
        runs_in: Some(\"push\"),\n\
    ";

    /// A minimal workflow whose jobs are `ci`, `bigquery-conformance`, `ci-aggregate` and
    /// `oracle-tier`.
    const CI_FIXTURE: &str = concat!(
        "on:\n",
        "  push:\n",
        "jobs:\n",
        "  ci:\n",
        "    runs-on: x\n",
        "  bigquery-conformance:\n",
        "    runs-on: x\n",
        "  ci-aggregate:\n",
        "    runs-on: x\n",
        "  oracle-tier:\n",
        "    runs-on: x\n",
    );

    /// The names a runnable scan scopes.
    fn runnable_names(scan: &Scan) -> Vec<String> {
        match scan {
            Scan::Runnable(scoped) => scoped.tests().iter().map(|one| String::from(one.name())).collect(),
            other => panic!("expected runnable, got {other:?}"),
        }
    }

    /// A live cell under the golden matrix leaves the scope while an ordinary sibling stays.
    #[test]
    fn a_live_cell_leaves_the_scope_and_an_ordinary_test_stays() {
        let live = Live::parse(
            &commit("h", "Live-Cell: the_live_one bigquery bigquery-conformance\n"),
            EXEMPTED,
            CI_FIXTURE,
        )
        .unwrap();
        let scope = Scan::Runnable(Scoped::of_named(added(
            "sutura-app",
            "crates/sutura-app/tests/t.rs",
            &["the_live_one", "an_ordinary_one"],
        )));
        assert_eq!(runnable_names(&live.scan(scope)), vec!["an_ordinary_one"]);
    }

    /// A scope whose every test is a live cell is not runnable here.
    #[test]
    fn a_scope_of_live_cells_only_is_not_runnable_here() {
        let live = Live::parse(
            &commit("h", "Live-Cell: the_live_one bigquery bigquery-conformance\n"),
            EXEMPTED,
            CI_FIXTURE,
        )
        .unwrap();
        let scope = Scan::Runnable(Scoped::of_named(added(
            "sutura-app",
            "crates/sutura-app/tests/t.rs",
            &["the_live_one"],
        )));
        match live.scan(scope) {
            Scan::OnlyIgnored(names) => {
                let names: Vec<String> = names.iter().map(|name| String::from(name.as_str())).collect();
                assert_eq!(names, vec!["the_live_one"]);
            }
            other => panic!("expected OnlyIgnored, got {other:?}"),
        }
    }

    /// A system with no `Unavailable` entry at all is refused, and the error carries the job too.
    #[test]
    fn a_live_cell_whose_system_is_not_exempted_unavailable_is_refused() {
        let log = commit("h", "Live-Cell: t duckdb bigquery-conformance\n");
        assert_eq!(
            Live::parse(&log, EXEMPTED, CI_FIXTURE),
            Err(LiveError::NotExempt {
                cell: String::from("t"),
                system: String::from("duckdb"),
                job: String::from("bigquery-conformance"),
            })
        );
    }

    /// A trailer whose job is not the system's `runs_in` - or whose system has none at all - is
    /// refused, while the bigquery pairing matches.
    #[test]
    fn a_live_cell_whose_job_is_not_its_systems_runs_in_is_refused() {
        for (system, job) in [("oracle", "ci"), ("postgres", "ci-aggregate"), ("oracle", "oracle-tier")] {
            let log = commit("h", &format!("Live-Cell: t {system} {job}\n"));
            assert!(
                matches!(Live::parse(&log, EXEMPTED, CI_FIXTURE), Err(LiveError::NotExempt { .. })),
                "{system} {job} should refuse"
            );
        }
        let ok = commit("h", "Live-Cell: t bigquery bigquery-conformance\n");
        match Live::parse(&ok, EXEMPTED, CI_FIXTURE) {
            Ok(live) => assert_eq!(live.0.len(), 1),
            Err(e) => panic!("expected one cell, got {e}"),
        }
    }

    /// A declared cell whose test does not live under the golden matrix stays in scope, so the
    /// gate fails closed rather than dropping a real proof.
    #[test]
    fn a_live_cell_outside_the_matrix_stays_in_scope() {
        let live = Live::parse(
            &commit("h", "Live-Cell: the_live_one bigquery bigquery-conformance\n"),
            EXEMPTED,
            CI_FIXTURE,
        )
        .unwrap();
        let scope = Scan::Runnable(Scoped::of_named(added("p", "crates/p/tests/t.rs", &["the_live_one"])));
        assert_eq!(runnable_names(&live.scan(scope)), vec!["the_live_one"]);
    }

    /// A name both `Live-Cell:` and `Claim-Cell:` declare refuses: a claim cell's proof is its
    /// killing mutation, and it may not also leave the scope.
    #[test]
    fn a_name_both_live_and_claimed_is_refused() {
        let log = commit(
            "h",
            "Claim-Cell: the_live_one\nLive-Cell: the_live_one bigquery bigquery-conformance\n",
        );
        assert_eq!(
            Live::parse(&log, EXEMPTED, CI_FIXTURE),
            Err(LiveError::AlsoClaimed {
                cell: String::from("the_live_one")
            })
        );
    }

    /// A live cell a scan leaves unmeasured is moved to `not_runnable`, so the ratio reads it as
    /// unreachable here rather than as work the proof skipped.
    #[test]
    fn a_live_cell_is_reported_not_runnable_rather_than_unmeasured() {
        let live = Live::parse(
            &commit("h", "Live-Cell: the_live_one bigquery bigquery-conformance\n"),
            EXEMPTED,
            CI_FIXTURE,
        )
        .unwrap();
        let coverage = Coverage::Measured {
            measured: 1,
            unmeasured: vec!["the_live_one".into(), "other".into()],
            not_runnable: Vec::new(),
        };
        let after = live.coverage(coverage);
        match after {
            Coverage::Measured {
                unmeasured,
                not_runnable,
                ..
            } => {
                assert_eq!(unmeasured, vec!["other"]);
                assert_eq!(not_runnable, vec!["the_live_one"]);
            }
            other @ Coverage::Unknown { .. } => panic!("expected Measured, got {other:?}"),
        }
    }

    /// `causality::run` over a one-commit git fixture whose HEAD message is `message`: `g` goes from
    /// 1 to 2, and `the_red_one` asserts 2, so the run is red on base and green on HEAD.
    fn run_under(message: &str) -> crate::Verdict {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this changes the process directory; run it under `just test`"
        );
        let dir = std::env::temp_dir().join(format!("sutura-causality-live-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        let git = |args: &[&str]| {
            let mut command = std::process::Command::new("git");
            crate::repo::strip_git_env(&mut command);
            let out = command.current_dir(&dir).args(args).output().expect("git runs");
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).expect("utf8")
        };
        let write = |path: &str, text: &str| {
            std::fs::create_dir_all(dir.join(path).parent().expect("a file path")).expect("a directory");
            std::fs::write(dir.join(path), text).expect("a fixture file");
        };
        write(
            "Cargo.toml",
            "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[profile.ci]\ninherits = \"dev\"\n",
        );
        write("flake.nix", "{ }\n");
        write("src/lib.rs", "pub fn g() -> u8 { 1 }\n");
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            "base",
        ]);
        let base = git(&["rev-parse", "HEAD"]).trim().to_owned();
        write("src/lib.rs", "pub fn g() -> u8 { 2 }\n");
        write(
            "tests/t.rs",
            "#[test]\nfn the_red_one() {\n    assert_eq!(wired::g(), 2);\n}\n",
        );
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=test",
            "commit",
            "-q",
            "-m",
            message,
        ]);
        let original = std::env::current_dir().expect("a current directory");
        std::env::set_current_dir(&dir).expect("the fixture directory");
        let verdict = super::super::run(&[String::from("--since"), base]);
        std::env::set_current_dir(&original).expect("restore the directory");
        drop(std::fs::remove_dir_all(&dir));
        verdict
    }

    /// A refused trailer fails a run that passes without it, so this holds both the refusal in
    /// `read` and its delivery in `causality::run`.
    #[test]
    fn a_refused_live_cell_is_delivered_as_a_failed_run() {
        assert_eq!(
            (
                run_under("test: an ordinary red test\n"),
                run_under("test: a refused live cell\n\nLive-Cell: the_red_one duckdb nightly\n")
            ),
            (crate::Verdict::Pass, crate::Verdict::Fail)
        );
    }

    /// A `runs_in` the workflow does not declare under `jobs:` is refused - `validate` is a `just`
    /// task, and `push` is keyed under `on:` only.
    #[test]
    fn a_live_cell_whose_job_ci_does_not_declare_is_refused() {
        let push = commit("h", "Live-Cell: t clickhouse push\n");
        assert_eq!(
            Live::parse(&push, EXEMPTED, CI_FIXTURE),
            Err(LiveError::NoJob {
                cell: String::from("t"),
                job: String::from("push")
            })
        );
        let validate = commit("h", "Live-Cell: t postgres validate\n");
        assert!(matches!(
            Live::parse(&validate, EXEMPTED, CI_FIXTURE),
            Err(LiveError::NoJob { job, .. }) if job == "validate"
        ));
    }

    /// `github.com/telekom/sutura#1274`: a trailer whose cell's run shows no exemption for its own
    /// system is refused, and one whose run shows it is kept.
    #[test]
    fn a_live_cell_whose_run_shows_no_exemption_is_refused() {
        let declared = || {
            Live::parse(
                &commit("h", "Live-Cell: t bigquery bigquery-conformance\n"),
                EXEMPTED,
                CI_FIXTURE,
            )
        };
        let refused = |output: &'static str| -> Result<Live, LiveError> { declared()?.witnessed(|_| String::from(output)) };
        let not_witnessed = Err(LiveError::NotWitnessed {
            cell: String::from("t"),
            system: String::from("bigquery"),
        });
        assert_eq!(refused("test t ... ok\n"), not_witnessed);
        assert_eq!(refused("exempt: postgres from Unavailable - no tier\n"), not_witnessed);
        assert_eq!(
            refused("exempt: bigquery from Unavailable - no dataset offline\n"),
            declared()
        );
    }

    /// Every declared cell is witnessed for its OWN system: a second trailer does not ride on the
    /// first one's line, and another system's line is not this one's.
    #[test]
    fn every_live_cell_is_witnessed_for_its_own_system() {
        const BIGQUERY: &str = "exempt: bigquery from Unavailable - no dataset offline\n";
        const SNOWFLAKE: &str = "exempt: snowflake from Unavailable - no account offline\n";
        let exempted = concat!(
            "system: \"bigquery\",\nfrom: Exempt::Unavailable,\nruns_in: Some(\"bigquery-conformance\"),\n",
            "system: \"snowflake\",\nfrom: Exempt::Unavailable,\nruns_in: Some(\"bigquery-conformance\"),\n",
        );
        let log = commit(
            "h",
            "Live-Cell: a bigquery bigquery-conformance\nLive-Cell: b snowflake bigquery-conformance\n",
        );
        let run = |b: &'static str| {
            Live::parse(&log, exempted, CI_FIXTURE)
                .expect("both trailers are exempted")
                .witnessed(|cell| String::from(if cell == "a" { BIGQUERY } else { b }))
        };
        let b_refused = Err(LiveError::NotWitnessed {
            cell: String::from("b"),
            system: String::from("snowflake"),
        });
        assert_eq!(run(""), b_refused);
        assert_eq!(run(BIGQUERY), b_refused);
        assert_eq!(run(SNOWFLAKE), Live::parse(&log, exempted, CI_FIXTURE));
    }

    /// A malformed trailer declares nothing, and an absent exemptions file refuses a well-formed one.
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
