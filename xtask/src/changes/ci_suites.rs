//! Select QA suites from the manifest and the workspace's conservative dependency graph.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::{affected, changed_paths, classify};
use crate::{Verdict, repo};

const MANIFEST: &str = "devco/ci-suites.json";

#[derive(Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum Venue {
    Standard,
    Kvm,
    BigQuery,
    Caller,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    id: String,
    venue: Venue,
    timeout: u16,
    #[serde(default, skip_serializing)]
    before_merge: bool,
    #[serde(default, skip_serializing)]
    always: bool,
    #[serde(default, skip_serializing)]
    advisory_pr_check: Option<String>,
    #[serde(default, skip_serializing)]
    areas: Vec<String>,
    #[serde(skip_serializing)]
    categories: Vec<String>,
    #[serde(skip_serializing)]
    paths: Vec<String>,
    #[serde(skip_serializing)]
    crates: Vec<String>,
    #[serde(default, skip_serializing)]
    checks: Vec<String>,
}

impl Suite {
    fn passed(&self, green: &BTreeSet<String>) -> bool {
        if self.advisory_pr_check.as_ref().is_some_and(|check| green.contains(check)) {
            return true;
        }
        match self.venue {
            Venue::Standard | Venue::Kvm => green.contains(&format!("qa / suite ({})", self.id)),
            Venue::BigQuery => green.contains(&self.id),
            Venue::Caller => !self.checks.is_empty() && self.checks.iter().all(|check| green.contains(check)),
        }
    }
}

#[derive(Deserialize)]
struct Metadata {
    workspace_root: PathBuf,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    path: Option<PathBuf>,
}

/// All optional, build and dev path edges participate. A missing root widens the selection.
fn affected_crates(metadata: &Metadata, paths: &[String]) -> Option<BTreeSet<String>> {
    let packages: BTreeMap<&str, &Package> = metadata.packages.iter().map(|p| (p.name.as_str(), p)).collect();
    if packages.is_empty() || packages.len() != metadata.packages.len() {
        return None;
    }
    let mut affected = BTreeSet::new();
    for path in paths {
        let mut matched = false;
        for (name, package) in &packages {
            let dir = package.manifest_path.parent()?.strip_prefix(&metadata.workspace_root).ok()?;
            if Path::new(path).starts_with(dir) {
                matched = true;
                affected.insert(String::from(*name));
            }
        }
        if path.starts_with("crates/") && !matched {
            return None;
        }
    }
    loop {
        let consumers: Vec<String> = metadata
            .packages
            .iter()
            .filter(|p| {
                !affected.contains(&p.name)
                    && p.dependencies
                        .iter()
                        .any(|dependency| dependency.path.is_some() && affected.contains(&dependency.name))
            })
            .map(|p| p.name.clone())
            .collect();
        if consumers.is_empty() {
            return Some(affected);
        }
        affected.extend(consumers);
    }
}

// None means shared code changed or the affected set could not be established.
fn affected_suites<'a>(suites: &'a [Suite], paths: &[String], base: &str, metadata: &Metadata) -> Option<Vec<&'a Suite>> {
    let areas = classify(paths);
    let categories = affected::derive(paths, Some(base));
    let known: BTreeSet<&str> = metadata.packages.iter().map(|p| p.name.as_str()).collect();
    let complete = suites
        .iter()
        .flat_map(|suite| suite.crates.iter())
        .all(|name| known.contains(name.as_str()));
    if areas.needs("nix") || areas.run_all || categories.core || !complete {
        return None;
    }
    let crates = affected_crates(metadata, paths)?;
    Some(
        suites
            .iter()
            .filter(|suite| {
                suite.areas.iter().any(|area| areas.needs(area))
                    || suite.categories.iter().any(|category| categories.needs(category))
                    || suite
                        .paths
                        .iter()
                        .any(|pattern| paths.iter().any(|path| repo::matches(pattern, path)))
                    || suite.crates.iter().any(|name| crates.contains(name))
            })
            .collect(),
    )
}

fn load_manifest(root: &Path) -> Result<Vec<Suite>, String> {
    let source = std::fs::read_to_string(root.join(MANIFEST)).map_err(|e| format!("cannot read {MANIFEST}: {e}"))?;
    let suites: Vec<Suite> = serde_json::from_str(&source).map_err(|e| format!("invalid {MANIFEST}: {e}"))?;
    let mut ids = BTreeSet::new();
    for suite in &suites {
        if !ids.insert(suite.id.as_str())
            || suite.id.is_empty()
            || !suite
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !(1..=120).contains(&suite.timeout)
            || (suite.venue == Venue::Caller) == suite.checks.is_empty()
            || (!suite.always
                && suite.areas.is_empty()
                && suite.categories.is_empty()
                && suite.paths.is_empty()
                && suite.crates.is_empty())
        {
            return Err(format!("invalid QA suite in {MANIFEST}: {}", suite.id));
        }
    }
    if ids.is_empty() {
        return Err(format!("{MANIFEST} names no QA suites"));
    }
    Ok(suites)
}

/// The linked-driver exercise stays in the matrix for either adapter.
pub(crate) fn linked_driver_is_enrolled(root: &Path) -> bool {
    load_manifest(root).is_ok_and(|suites| {
        suites.iter().any(|suite| {
            suite.id == "bigquery-driver"
                && suite.venue == Venue::Standard
                && !suite.before_merge
                && suite.advisory_pr_check.is_none()
                && ["data_source_bigquery", "data_source_postgres"]
                    .iter()
                    .all(|name| suite.categories.iter().any(|category| category == *name))
        })
    })
}

/// IDs that the reusable QA workflow can select for a live matrix exemption.
pub(crate) fn ids(root: &Path) -> Result<Vec<String>, String> {
    Ok(load_manifest(root)?.into_iter().map(|suite| suite.id).collect())
}

#[derive(Clone, Copy)]
enum Run<'a> {
    All,
    Main,
    Pr(Option<&'a str>),
    Queue { base: &'a str, tested: &'a str },
}

fn sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn tree(commit: &str) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(["rev-parse", "--verify", &format!("{commit}^{{tree}}")])
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn graph() -> Option<Metadata> {
    serde_json::from_value(crate::cargo_metadata(&["--no-deps", "--all-features"]).ok()?).ok()
}

fn changes_since_tested<'a>(suites: &'a [Suite], tested: &str, metadata: &Metadata) -> Option<Vec<&'a Suite>> {
    if tree(tested)? == tree("HEAD")? {
        return Some(Vec::new());
    }
    affected_suites(suites, &changed_paths(tested)?, tested, metadata)
}

fn planned_suites<'a>(
    suites: &'a [Suite],
    topic: &[&Suite],
    changed_since_pr: Option<&[&Suite]>,
    green: &BTreeSet<String>,
) -> Vec<&'a Suite> {
    suites
        .iter()
        .filter(|suite| {
            let changed_on_main = changed_since_pr.is_some_and(|delta| delta.iter().any(|other| other.id == suite.id));
            let selected = changed_on_main || suite.always || topic.iter().any(|other| other.id == suite.id);
            let reused = selected && changed_since_pr.is_some() && !changed_on_main && suite.passed(green);
            if reused {
                println!("ci-suites: reusing PR verdict for {}", suite.id);
            }
            selected && !reused
        })
        .collect()
}

fn select_changed<'a>(suites: &'a [Suite], base: &str, tested: Option<&str>) -> Option<Vec<&'a Suite>> {
    let metadata = graph()?;
    let topic = affected_suites(suites, &changed_paths(base)?, base, &metadata)?;
    let since_pr = match tested {
        Some(commit) => Some(changes_since_tested(suites, commit, &metadata)?),
        None => None,
    };
    let green = std::env::var("GREEN_JOBS")
        .ok()
        .and_then(|value| serde_json::from_str::<BTreeSet<String>>(&value).ok())
        .unwrap_or_default();
    Some(planned_suites(suites, &topic, since_pr.as_deref(), &green))
}

/// Emit the matrix and selected IDs used by the standalone, credentialed and called jobs.
pub(crate) fn run(args: &[String]) -> Verdict {
    let mode = match args {
        [all] if all == "--all" => Run::All,
        [main] if main == "--main" => Run::Main,
        [pr] if pr == "--pr" => Run::Pr(None),
        [pr, base] if pr == "--pr" && sha(base) => Run::Pr(Some(base)),
        [queue, base, tested] if queue == "--queue" && sha(base) && sha(tested) => Run::Queue { base, tested },
        _ => {
            eprintln!("ci-suites: expected --all, --main, --pr [<base>], or --queue <base> <tested>");
            return Verdict::Usage;
        }
    };
    match emit(mode) {
        Ok(()) => Verdict::Pass,
        Err(cause) => {
            eprintln!("ci-suites: {cause}");
            Verdict::Fail
        }
    }
}

fn select_mode<'a>(suites: &'a [Suite], mode: Run<'_>) -> Vec<&'a Suite> {
    match mode {
        Run::All => suites.iter().collect(),
        Run::Main => suites.iter().filter(|suite| !suite.before_merge).collect(),
        Run::Pr(base) => base
            .and_then(|base| select_changed(suites, base, None))
            .unwrap_or_else(|| suites.iter().collect())
            .into_iter()
            .filter(|suite| suite.advisory_pr_check.is_none())
            .collect(),
        Run::Queue { base, tested } => select_changed(suites, base, Some(tested)).unwrap_or_else(|| {
            eprintln!("ci-suites: shared or unknown changes, selecting every suite");
            suites.iter().collect()
        }),
    }
}

fn emit(mode: Run<'_>) -> Result<(), String> {
    let suites = load_manifest(Path::new("."))?;
    let selected = select_mode(&suites, mode);
    let ordinary: Vec<&Suite> = selected
        .iter()
        .copied()
        .filter(|suite| matches!(suite.venue, Venue::Standard | Venue::Kvm))
        .collect();
    let ids: Vec<&str> = selected.iter().map(|suite| suite.id.as_str()).collect();
    let plan = serde_json::json!({ "selected": ids, "matrix": { "include": ordinary }, "count": ordinary.len() });
    println!("ci-suites: selected {ids:?}");
    if let Ok(output) = std::env::var("GITHUB_OUTPUT") {
        let mut target = std::fs::OpenOptions::new()
            .append(true)
            .open(output)
            .map_err(|e| e.to_string())?;
        writeln!(target, "suite-plan={plan}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passed_suites(root: &Path, green: &[&str]) -> Vec<String> {
        let green = green.iter().map(|name| String::from(*name)).collect();
        load_manifest(root)
            .expect("suite manifest")
            .into_iter()
            .filter(|suite| suite.passed(&green))
            .map(|suite| suite.id)
            .collect()
    }

    /// The acceptance verdict does not stand in for either of the native shards.
    #[test]
    fn oracle_receipts_distinguish_acceptance_native_and_legacy() {
        let root = crate::repo::root().expect("repository root");
        let acceptance = "qa / suite (oracle-tier)";
        let native = "qa / suite (oracle-native)";
        let legacy = "qa / suite (oracle-legacy)";
        for (green, expected) in [
            (vec![acceptance], vec!["oracle-tier"]),
            (vec![acceptance, native], vec!["oracle-tier", "oracle-native"]),
            (vec![native, legacy], vec!["oracle-native", "oracle-legacy"]),
            (
                vec![acceptance, native, legacy],
                vec!["oracle-tier", "oracle-native", "oracle-legacy"],
            ),
            (vec!["qa / suite (oracle-native-extra)"], vec![]),
        ] {
            assert_eq!(passed_suites(&root, &green), expected);
        }
    }

    #[test]
    fn a_changed_leaf_selects_its_transitive_consumers_and_unknown_crates_widen() {
        let metadata: Metadata = serde_json::from_value(serde_json::json!({
            "workspace_root": "/fixture",
            "packages": [
                {"name": "leaf", "manifest_path": "/fixture/crates/leaf/Cargo.toml", "dependencies": []},
                {"name": "mid", "manifest_path": "/fixture/crates/mid/Cargo.toml",
                 "dependencies": [{"name": "leaf", "path": "/fixture/crates/leaf"}]},
                {"name": "top", "manifest_path": "/fixture/crates/top/Cargo.toml",
                 "dependencies": [{"name": "mid", "path": "/fixture/crates/mid"}]},
                {"name": "peer", "manifest_path": "/fixture/crates/peer/Cargo.toml",
                 "dependencies": [{"name": "leaf", "path": null}]}
            ]
        }))
        .expect("metadata fixture");
        let changed = affected_crates(&metadata, &[String::from("crates/leaf/src/lib.rs")]).expect("known crate path");
        assert_eq!(changed, BTreeSet::from_iter(["leaf", "mid", "top"].map(String::from)));
        assert!(affected_crates(&metadata, &[String::from("crates/deleted/src/lib.rs")]).is_none());
    }

    #[test]
    fn a_suite_changed_on_main_runs_even_when_the_topic_did_not_touch_it() {
        let root = crate::repo::root().expect("repository root");
        let suites = load_manifest(&root).expect("suite manifest");
        let bigquery = suites
            .iter()
            .find(|one| one.id == "bigquery-conformance")
            .expect("BigQuery suite");
        let oracle = suites.iter().find(|one| one.id == "oracle-native").expect("Oracle suite");
        let green = BTreeSet::from([
            String::from("bigquery-conformance"),
            String::from("qa / suite (oracle-native)"),
        ]);
        let selected = planned_suites(&suites, &[bigquery], Some(&[oracle]), &green);
        assert!(
            selected.iter().any(|one| one.id == "oracle-native"),
            "main changed the Oracle venue"
        );
        assert!(
            !selected.iter().any(|one| one.id == "bigquery-conformance"),
            "the same BigQuery venue was already green"
        );

        let without_evidence = planned_suites(&suites, &[bigquery], Some(&[oracle]), &BTreeSet::new());
        assert!(without_evidence.iter().any(|one| one.id == "oracle-native"));
        assert!(without_evidence.iter().any(|one| one.id == "bigquery-conformance"));
    }

    #[test]
    fn smoke_suites_are_advisory_on_an_ordinary_pr_and_selected_in_full_mode() {
        let root = crate::repo::root().expect("repository root");
        let suites = load_manifest(&root).expect("suite manifest");
        for (mode, absent) in [
            (Run::Pr(None), &["kind-smoke", "demo-container"][..]),
            (
                Run::Main,
                &["cross-link", "e2e-datahub-adbc", "kind-smoke", "demo-container"][..],
            ),
            (Run::All, &[][..]),
        ] {
            let selected = select_mode(&suites, mode);
            for suite in &suites {
                assert_eq!(
                    selected.iter().any(|one| one.id == suite.id),
                    !absent.contains(&suite.id.as_str()),
                    "{}",
                    suite.id
                );
            }
        }
    }

    #[test]
    fn a_smoke_skip_requires_an_exact_successful_pr_check() {
        let root = crate::repo::root().expect("repository root");
        for (green, expected) in [
            (&["ci-aggregate", "qa / integration-verdict"][..], &[][..]),
            (&["kind-smoke"][..], &["kind-smoke"][..]),
            (&["demo-container / smoke-test"][..], &["demo-container"][..]),
            (
                &["qa / suite (kind-smoke)", "qa / demo / smoke-test"][..],
                &["kind-smoke", "demo-container"][..],
            ),
            (&["kind-smoke-replay", "demo-container / smoke-test (old)"][..], &[][..]),
        ] {
            assert_eq!(passed_suites(&root, green), expected);
        }
    }
}
