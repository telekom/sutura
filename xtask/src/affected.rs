//! The category axis over the workspace, emitted by `xtask classify` for the category filter.
//!
//! One diff is judged at two granularities that do not share a key. [`crate::changes::classify`] maps a
//! changed path to AREAS (rust, nix, build, ...) which the existing CI steps already gate on. This
//! module maps the same paths to CATEGORIES, named by the adapter CRATE the change lives in, and
//! lets a workflow start or skip the legs that cost real money - provisioning a tier, calling a
//! cloud provider - on that category.
//!
//! **THE CATEGORY IS THE CRATE, NOT THE CELL'S `NAME`.** The golden matrix registers the reference
//! catalog under the snapshot key `markdown` while the adapter it names is
//! `sutura_catalog_local::LocalCatalog`; a filter keyed on the name would call the category
//! `catalog_markdown`, match no crate, and silently drop the reference catalog's cells. The
//! registry's adapter TYPE's first path segment is the crate, and the category is derived from it,
//! not transcribed.
//!
//! The selection fails OPEN exactly the way [`crate::changes::classify`] does: an empty diff, a git refusal,
//! an unmapped path or a registry the derive could not read all set `core`, and `core` subsumes
//! every category. A new adapter arrives by registering it in the registry (or by being a crate);
//! no workflow line is asked to know its name in advance.
//!
//! **That fail-open is held at the OUTPUT boundary, not only in `Categories::needs`, and the
//! difference is the whole reason `crate_categories` exists.** A workflow condition reads an
//! emitted line; a name that is never emitted is not `false` but ABSENT, renders `''`, and matches
//! no `== 'true'` - so the leg SKIPS. `needs` answering `true` for a category no line names buys
//! nothing. Whatever `declared` holds is therefore the list of legs that can be switched on, and an
//! empty one on a registry failure turned the guarantee above into a silent skip for every category
//! the diff did not itself select.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;

use crate::conformance::scan;

/// The registry, read as data. **Read-only** - it is the input, never a target of this change.
const REGISTRY: &str = "crates/sutura-app/tests/adapters/adapters.rs";

/// The one category that is not a crate: the runtime's identity half, gated as its own tier.
const IDENTITY: &str = "identity";

/// The crate prefixes the two component axes strip to name a category, and their path prefixes.
const DATA_SOURCE_PREFIX: &str = "sutura-exec-";
const CATALOG_PREFIX: &str = "sutura-catalog-";
const DATA_SOURCE_PATH: &str = "crates/sutura-exec-";
const CATALOG_PATH: &str = "crates/sutura-catalog-";

/// Paths that are the identity half of the runtime: the inbound transport, the security settings
/// and the brokers. Named here so a direct edit starts the identity tier; everything else under
/// these crates would fall through to `core`, which runs more, never less - this only keeps the
/// skip honest.
const IDENTITY_PATHS: &[&str] = &[
    "crates/sutura-http/src/inbound/",
    "crates/sutura-config/src/security.rs",
    "crates/sutura-mcp/",
    "crates/sutura-http/",
    // Added for `just keycloak-served-test`: the composed binary's own leg-1 e2e suite and the
    // tier it now needs. Before this, a path here matched no category and fell open
    // to `core` - which still ran every category-gated leg, just more of them than the diff
    // touched. NARROWS that: a `sutura-serve` or `keycloak-tier` change now selects `identity`
    // specifically rather than everything, which is the precision this leg's own JVM-boot cost
    // asks for - it should not run on, say, a BigQuery-only diff, and under the old fail-open it
    // did not need to (nothing gated on it existed yet).
    "crates/sutura-serve/",
    "nix/keycloak-tier",
];

/// A path's category selection: whether it fell open to `core`, the categories it selected, and
/// the reasons each path ran everything.
type Selection = (bool, BTreeSet<String>, Vec<String>);

/// The parsed `$cell!(..)` argument lists of one registry arm.
type Cells = Vec<Vec<String>>;

/// The category verdict for one diff.
#[derive(Debug)]
pub(crate) struct Categories {
    /// `core` subsumes every category: a path no category owns, an empty diff, or a read failure.
    pub(crate) core: bool,
    /// The non-core categories the diff selected.
    pub(crate) selected: BTreeSet<String>,
    /// Every category the registry declares, plus `identity`, so CI can emit a `false` line for
    /// the ones no diff selected.
    pub(crate) declared: BTreeSet<String>,
    pub(crate) reasons: Vec<String>,
}

impl Categories {
    /// Is this category's leg required? `core` subsumes every category, the same fail-open rule
    /// [`crate::changes::Classification::needs`] gives every area.
    pub(crate) fn needs(&self, category: &str) -> bool {
        self.core || self.selected.contains(category)
    }
}

/// Derive the categories a diff selects, report them and append them to `GITHUB_OUTPUT`. Called
/// from [`crate::changes::run_classify`] beside the area emission, so CI reads one classification, one
/// verdict.
pub(crate) fn finish(paths: &[String]) -> Categories {
    let cats = derive(paths);
    print_report(&cats);
    write_github_output(&cats);
    cats
}

/// The category half of the classify report, so a human run sees which legs a diff selects.
fn print_report(cats: &Categories) {
    if cats.core {
        println!("  categories: EVERYTHING (core)");
    } else if cats.selected.is_empty() {
        println!("  categories: none");
    } else {
        let names: Vec<&str> = cats.selected.iter().map(String::as_str).collect();
        println!("  categories: {}", names.join(", "));
    }
    for reason in &cats.reasons {
        println!("  category-reason: {reason}");
    }
}

fn derive(paths: &[String]) -> Categories {
    let root = Path::new(".");
    derive_from(paths, registry_categories(root), root)
}

/// [`derive()`] with the registry read handed in, so a test can BREAK that read and compare the
/// emitted lines against a successful one. Injected rather than reached for: the property this
/// module documents is about what a *failed* read emits, and a read that only fails when the
/// working tree is damaged is a property nothing can assert.
fn derive_from(paths: &[String], registry: Result<BTreeSet<String>, String>, root: &Path) -> Categories {
    let (mut core, selected, mut reasons) = select(paths);
    let declared = match registry {
        Ok(mut set) => {
            set.insert(String::from(IDENTITY));
            set
        }
        Err(why) => {
            // Fail open: a registry the derive cannot read is not a reason to skip a leg - and
            // that takes a set of NAMES here, not just `core`, per the boundary note above.
            core = true;
            reasons.push(why);
            crate_categories(root, &mut reasons)
        }
    };
    Categories {
        core,
        selected,
        declared,
        reasons,
    }
}

/// Which categories the changed paths select. `core` is set when any path matches no category, and
/// subsumes everything; the reasons say exactly which path did it, so a category nobody can see is
/// reported rather than assumed harmless.
fn select(paths: &[String]) -> Selection {
    let mut selected = BTreeSet::new();
    let mut reasons = Vec::new();
    let mut core = paths.is_empty();
    if core {
        reasons.push(String::from("no changed paths - running every category"));
    }
    for path in paths {
        if IDENTITY_PATHS.iter().any(|p| path.starts_with(p)) {
            selected.insert(String::from(IDENTITY));
        } else if let Some(tail) = crate_tail(path, DATA_SOURCE_PATH) {
            selected.insert(format!("data_source_{tail}"));
        } else if let Some(tail) = crate_tail(path, CATALOG_PATH) {
            selected.insert(format!("catalog_{tail}"));
        } else {
            core = true;
            reasons.push(format!("{path} matches no category - running every category"));
        }
    }
    (core, selected, reasons)
}

/// The adapter crate tail a changed path picks out, if it is under one of the two adapter dirs.
fn crate_tail(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let tail = rest.split('/').next()?;
    if tail.is_empty() { None } else { Some(tail.to_owned()) }
}

/// The categories the registry declares, joined by the crate each adapter type names.
fn registry_categories(root: &Path) -> Result<BTreeSet<String>, String> {
    let path = root.join(REGISTRY);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("could not read {REGISTRY}: {e}"))?;
    let exec_adapters = exec_adapters(root)?;
    categories_from_registry(&text, &exec_adapters)
}

/// Every `sutura-exec-<name>` crate dir present, so a dialect maps to a data source only when the
/// matching execution crate exists - a category planning for a nonexistent cell is noise.
fn exec_adapters(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for name in crate_dirs(root)? {
        if let Some(tail) = name.strip_prefix(DATA_SOURCE_PREFIX) {
            out.insert(tail.to_owned());
        }
    }
    Ok(out)
}

/// The adapter categories the crate DIRECTORIES name, plus `identity`: the floor the emission
/// falls back to when the registry cannot be read.
///
/// A superset of anything a registry can declare, because every category name is derived from a
/// crate prefix in the first place - and readable when the registry file is not, which is the one
/// situation it is for. A listing that fails too is REPORTED rather than swallowed: `core` is
/// already set by then, so the run is not wrong, but a floor smaller than the tree is exactly the
/// #619 shape where the gate's own output hides how little it looked at.
fn crate_categories(root: &Path, reasons: &mut Vec<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::from([String::from(IDENTITY)]);
    match crate_dirs(root) {
        Ok(names) => out.extend(names.iter().filter_map(|name| category_from_crate(name))),
        Err(why) => reasons.push(format!("{why} - the category floor is `identity` alone")),
    }
    out
}

/// The crate directory names, read once for both of the callers above.
fn crate_dirs(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("crates");
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("could not list crates/: {e}"))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("could not list crates/: {e}"))?;
        out.push(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(out)
}

/// The categories a registry declares, derived from the adapter TYPE each cell names - never from
/// the cell's snapshot `NAME`. `exec_crates` is which dialect names have a real execution crate.
fn categories_from_registry(text: &str, exec_crates: &BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    let (_, dense) = scan::lexed(text);
    let mut out = BTreeSet::new();
    out.extend(arm_categories(&dense, scan::REGISTRY_ARM, 1)?);
    out.extend(arm_categories(&dense, scan::CATALOGS_ARM, 2)?);
    for args in arm_raw(&dense, scan::DIALECTS_ARM)? {
        if let Some(name) = args.first()
            && exec_crates.contains(name)
        {
            out.insert(format!("data_source_{name}"));
        }
    }
    Ok(out)
}

/// The categories an arm's cells name, from the adapter type at argument `adapter_at`.
fn arm_categories(dense: &[String], arm: &str, adapter_at: usize) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for args in arm_raw(dense, arm)? {
        let adapter = args.get(adapter_at).ok_or_else(|| {
            format!(
                "`{arm}` cell has no adapter type at argument {}",
                adapter_at.saturating_add(1)
            )
        })?;
        if let Some(category) = category_from_crate(&crate_from_type(adapter)) {
            out.insert(category);
        }
    }
    Ok(out)
}

/// The raw `$cell!(..)` argument lists of one arm, located by its matcher line.
fn arm_raw(dense: &[String], arm: &str) -> Result<Cells, String> {
    let line = *scan::sites(dense, arm)
        .first()
        .ok_or_else(|| format!("`{arm}` is not declared in {REGISTRY}"))?;
    scan::raw_cells(dense, line, arm).map_err(|why| format!("{REGISTRY}: {why}"))
}

/// The first path segment of an adapter type, `_`-to-`-`, which is the crate it derives.
fn crate_from_type(adapter: &str) -> String {
    adapter.split("::").next().unwrap_or_default().replace('_', "-")
}

/// The category a crate names: a data-source or catalog crate, or nothing for a crate on neither
/// axis (a dialect's own `sutura_sql`, for example).
fn category_from_crate(crate_name: &str) -> Option<String> {
    crate_name
        .strip_prefix(DATA_SOURCE_PREFIX)
        .map(|tail| format!("data_source_{tail}"))
        .or_else(|| crate_name.strip_prefix(CATALOG_PREFIX).map(|tail| format!("catalog_{tail}")))
}

/// The `name=true|false` category lines CI reads, as text.
///
/// Separated from the write so a test can assert on the EMITTED SET rather than on `declared`: an
/// emitted line is what a workflow condition can see, and the two disagreeing is the defect this
/// split exists to make visible.
fn output_body(cats: &Categories) -> String {
    let mut body = String::new();
    // Every category the registry declares (or the crate floor, when the registry failed), plus
    // whatever the diff selected, one line each, valued `core || selected` so a `core` diff flips
    // every leg on and a skipped leg can never satisfy one.
    let mut names: BTreeSet<String> = cats.declared.clone();
    names.extend(cats.selected.iter().cloned());
    for name in &names {
        body.push_str(name);
        body.push('=');
        body.push_str(super::flag(cats.needs(name)));
        body.push('\n');
    }
    body.push_str("core=");
    body.push_str(super::flag(cats.core));
    body.push('\n');
    body
}

/// Emit `name=true|false` category lines for CI to read. Silent when not running under Actions.
fn write_github_output(cats: &Categories) {
    let Ok(path) = std::env::var("GITHUB_OUTPUT") else {
        return;
    };
    let body = output_body(cats);
    match std::fs::OpenOptions::new().append(true).create(true).open(&path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(body.as_bytes()) {
                eprintln!("xtask classify: could not write category outputs: {e}");
            }
        }
        Err(e) => eprintln!("xtask classify: could not open GITHUB_OUTPUT: {e}"),
    }
}

/// The `ci-aggregate` allowed-skips rule. This repository keeps the rule itself in the workflow's
/// `ci-aggregate` shell (it must read live job results), and spells it here once as pure, tested
/// logic so the shell has a reference to be a faithful transcription of - a selected-but-skipped
/// leg is a buggy filter, and it must fail green.
///
/// The category axis below is the pure half. One exception lives ONLY in the shell and is proven
/// there rather than here: the release commit skips the whole belt, so a skipped `ci` is valid and
/// a skipped `ci` implies a skipped bigquery leg, which this two-axis model cannot express. The
/// `shell_simulation` module at the bottom extracts the real `run:` block from
/// `.github/workflows/ci.yml` and executes it against canned job results, so the shell stays the
/// source of truth.
#[cfg(test)]
mod tests {
    use super::*;

    /// One category-gated leg: its name, its category (None means always-required, like `ci`), and
    /// its reported job result.
    type CategoryLeg<'a> = (&'a str, Option<&'a str>, &'a str);

    /// The `ci-aggregate` allowed-skips rule, spelled here so a test can provoke it. A category-
    /// gated leg may be green BY SKIP only when its category was not selected (and the diff did
    /// not fall open to `core`); any other non-`success` state fails. A leg with `category: None`
    /// is always required. Returns the offender names, empty meaning green. The `ci-aggregate`
    /// shell in `ci.yml` is a transcription of this for the CATEGORY axis; the release-commit skip
    /// (a skipped `ci` when the whole belt is intentionally skipped) is an event-level exception
    /// the shell adds, proven by `shell_simulation` below against the real shell.
    fn aggregator_failures(cats: &Categories, legs: &[CategoryLeg<'_>]) -> Vec<String> {
        legs.iter()
            .filter_map(|(name, category, result)| {
                let always = category.is_none();
                let must_run = always || cats.needs(category.expect("checked above"));
                let green = *result == "success" || (!must_run && *result == "skipped");
                if green { None } else { Some((*name).to_owned()) }
            })
            .collect()
    }

    /// A `Categories` built from a path set against a fixed declared set, so the selection
    /// properties are tested without a filesystem read.
    fn selected(paths: &[&str]) -> Categories {
        let owned: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
        let (core, selected, reasons) = select(&owned);
        let declared = BTreeSet::from(
            [
                "data_source_datafusion",
                "data_source_duckdb",
                "data_source_postgres",
                "data_source_bigquery",
                "catalog_local",
                "catalog_datahub",
                "identity",
            ]
            .map(String::from),
        );
        Categories {
            core,
            selected,
            declared,
            reasons,
        }
    }

    #[test]
    fn a_change_to_one_adapter_selects_that_adapter_s_cells_and_no_other() {
        let cats = selected(&["crates/sutura-exec-postgres/src/lib.rs"]);
        assert!(!cats.core);
        assert!(cats.needs("data_source_postgres"));
        for other in [
            "data_source_datafusion",
            "data_source_duckdb",
            "data_source_bigquery",
            "catalog_local",
            "catalog_datahub",
            "identity",
        ] {
            assert!(!cats.needs(other), "{other} must not be selected by a postgres change");
        }
    }

    #[test]
    fn a_change_to_a_shared_crate_emits_every_cell() {
        let cats = selected(&["crates/sutura-domain/src/lib.rs"]);
        assert!(cats.core, "a shared crate matches no category, so it must run everything");
        for cat in [
            "data_source_datafusion",
            "data_source_duckdb",
            "data_source_postgres",
            "data_source_bigquery",
            "catalog_local",
            "catalog_datahub",
            "identity",
        ] {
            assert!(cats.needs(cat), "{cat} must run when core runs");
        }
    }

    #[test]
    fn an_unmapped_path_selects_core() {
        let (core, _selected, reasons) = select(&["devco/whatever.toml".to_owned()]);
        assert!(core, "an unmapped path must run every category, not be skipped");
        assert!(reasons.iter().any(|r| r.contains("devco/whatever.toml")), "{reasons:?}");
    }

    #[test]
    fn the_registry_join_keys_on_the_crate_not_the_name() {
        let registry = "
macro_rules! registered {
    (catalogs: $cell:ident) => {
        $cell!(markdown, golden, sutura_catalog_local::LocalCatalog);
        $cell!(datahub, declaring, sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::fixture::FixtureReader>);
    };
    (data_systems: $cell:ident) => {
        $cell!(datafusion, sutura_exec_datafusion::DataFusionWarehouse);
        $cell!(oracle, sutura_exec_oracle::Oracle);
    };
    (dialects: $cell:ident) => {
        $cell!(bigquery, sutura_sql::Dialect::BigQuery, polyglot_sql::DialectType::BigQuery);
        $cell!(clickhouse, sutura_sql::Dialect::ClickHouse, polyglot_sql::DialectType::ClickHouse);
    };
}
";
        let exec_crates = BTreeSet::from(["datafusion", "duckdb", "postgres", "bigquery"].map(String::from));
        let cats = categories_from_registry(registry, &exec_crates).expect("the fixture registry parses");
        // A NEW adapter appears as its crate's category with no workflow edit...
        assert!(
            cats.contains("data_source_oracle"),
            "oracle must derive from sutura-exec-oracle: {cats:?}"
        );
        // ...and the reference catalog is keyed by its crate, not its snapshot NAME.
        assert!(cats.contains("catalog_local"), "{cats:?}");
        assert!(
            !cats.contains("catalog_markdown"),
            "the category is the crate, not the NAME: {cats:?}"
        );
        // BigQuery has no data_systems cell, but a dialect arm and an exec crate → data_source_bigquery.
        assert!(cats.contains("data_source_bigquery"), "{cats:?}");
        // The ClickHouse dialect has no exec crate; a category for it would be a wish-list.
        assert!(!cats.contains("data_source_clickhouse"), "{cats:?}");
        assert!(cats.contains("data_source_datafusion"));
        assert!(cats.contains("catalog_datahub"));
    }

    /// The category names `ci.yml` actually reads off the classify step.
    ///
    /// An INDEPENDENT oracle, and that is the point: a list derived from this module's own loop
    /// cannot witness that loop narrowing (#414), so the expectation is read from the consumer.
    fn categories_ci_reads(root: &Path) -> BTreeSet<String> {
        const MARKER: &str = "steps.classify.outputs.";
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read .github/workflows/ci.yml");
        let mut out = BTreeSet::new();
        // `split` rather than `match_indices` plus a slice: indexing a `str` by byte offset is
        // `clippy::string_slice`, a restriction lint that only `-D warnings` surfaces.
        for after in ci.split(MARKER).skip(1) {
            let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            // The area axis (`run_all`, `nix`, ...) comes from `changes.rs`; only these are ours.
            if name == IDENTITY || name.starts_with("data_source_") || name.starts_with("catalog_") {
                out.insert(name);
            }
        }
        out
    }

    /// The names the emission actually writes - parsed back out of the body, so the assertion is
    /// about what a workflow can READ rather than about `declared`.
    fn emitted_names(cats: &Categories) -> BTreeSet<String> {
        output_body(cats)
            .lines()
            .filter_map(|line| line.split_once('=').map(|(name, _)| name.to_owned()))
            .collect()
    }

    /// Breaking the registry read may not shrink the emitted set, because an unemitted name is
    /// ABSENT rather than `false` and a workflow condition on it is then never true.
    ///
    /// The read is broken by injection rather than by damaging the tree, and the expectation comes
    /// from `ci.yml`. Before the crate floor existed this failed on both halves: the emitted set
    /// collapsed to the one category the diff selected, and every other leg - the identity tier and
    /// `bigquery-acceptance`, which has no `run_all` fallback at all - lost its line.
    #[test]
    fn breaking_the_registry_read_never_shrinks_the_emitted_set() {
        let root = crate::repo::root().expect("the repo root");
        let gated = categories_ci_reads(&root);
        assert!(
            gated.len() > 1,
            "the oracle matched nothing in ci.yml, so this test would pass over anything: {gated:?}"
        );

        let paths = vec![String::from("crates/sutura-exec-duckdb/src/lib.rs")];
        let read = derive_from(&paths, registry_categories(&root), &root);
        let broken = derive_from(&paths, Err(String::from("planted: the registry could not be read")), &root);

        assert!(!read.core, "one adapter path selects one category");
        assert!(broken.core, "a registry the derive cannot read must fail open");
        let (before, after) = (emitted_names(&read), emitted_names(&broken));
        // A superset rather than equality: the floor is the crate directories, which may legitimately
        // be wider than what the registry declares. What it may never be is narrower.
        assert!(
            after.is_superset(&before),
            "breaking the registry read dropped {:?} from the emitted set",
            before.difference(&after).collect::<Vec<_>>()
        );
        for name in &gated {
            assert!(
                after.contains(name),
                "ci.yml gates a leg on `{name}` and no line emits it: {after:?}"
            );
        }
        for line in output_body(&broken).lines() {
            assert!(
                line.ends_with("=true"),
                "a failed read must switch every leg ON, not off: {line}"
            );
        }
    }

    #[test]
    fn aggregator_fails_a_selected_but_skipped_leg() {
        let cats = selected(&["crates/sutura-exec-bigquery/src/lib.rs"]);
        let failures = aggregator_failures(
            &cats,
            &[
                ("ci", None, "success"),
                ("bigquery-acceptance", Some("data_source_bigquery"), "skipped"),
            ],
        );
        assert!(
            failures.iter().any(|f| f == "bigquery-acceptance"),
            "a leg whose category is selected may not satisfy its leg by skipping: {failures:?}"
        );
    }

    #[test]
    fn aggregator_allows_a_skip_for_an_unselected_leg() {
        let cats = selected(&["crates/sutura-exec-duckdb/src/lib.rs"]);
        let failures = aggregator_failures(
            &cats,
            &[
                ("ci", None, "success"),
                ("bigquery-acceptance", Some("data_source_bigquery"), "skipped"),
            ],
        );
        assert!(failures.is_empty(), "an unselected leg may skip green: {failures:?}");
    }

    /// Execute the REAL ci-aggregate `run:` shell from `.github/workflows/ci.yml` against canned
    /// job results. The P1 defect lived in that file and nowhere else - the pure category rule
    /// above cannot see the release-commit skip (a skipped `ci` implies a skipped `bigquery` leg,
    /// an event shape the two-axis model has no vocabulary for) - so these tests extract and run
    /// the script the workflow actually ships.
    mod shell_simulation {
        /// The `ci-aggregate` job's `run: |` block, extracted from `.github/workflows/ci.yml` and
        /// de-indented, so the simulation exercises the exact script `bash` runs in CI.
        fn aggregator_shell() -> String {
            let root = crate::repo::root().expect("the repo root");
            let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read .github/workflows/ci.yml");
            let lines: Vec<&str> = ci.lines().collect();
            let job = lines
                .iter()
                .position(|l| l.starts_with("  ci-aggregate:"))
                .expect("ci-aggregate job present in ci.yml");
            let run = lines[job..]
                .iter()
                .position(|l| l.trim_start().starts_with("run: |"))
                .map(|i| job + i)
                .expect("the ci-aggregate job has a run: | step");
            let body = &lines[run + 1..];
            let script_indent = body
                .iter()
                .find(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .expect("the run body is not empty");
            let mut out = Vec::new();
            for line in body {
                if line.trim().is_empty() {
                    out.push(String::new());
                } else {
                    // A non-blank line shallower than the body is the next step, so it ends the
                    // block. The leading bytes are all ASCII spaces (YAML block-scalar indent),
                    // so a byte offset is also a char boundary; `split_at` keeps the slice rather
                    // than indexing the string.
                    let indent = line.len() - line.trim_start().len();
                    if indent < script_indent {
                        break;
                    }
                    let (_, rest) = line.split_at(script_indent);
                    out.push(rest.to_owned());
                }
            }
            out.join("\n")
        }

        /// Run the aggregator shell with the given environment; returns (exit ok, combined output).
        fn run_aggregator(envs: &[(&str, &str)]) -> (bool, String) {
            let script = aggregator_shell();
            let mut cmd = std::process::Command::new("bash");
            cmd.arg("-c").arg(&script);
            for (k, v) in envs {
                cmd.env(k, v);
            }
            let out = cmd.output().expect("bash runs the ci-aggregate shell");
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), text)
        }

        #[test]
        fn a_release_commit_skip_of_ci_is_green() {
            // The release push: `chore(release):` skips `ci`, and the skip propagates to every
            // leg that `needs: [ci]` - bigquery-acceptance included - so the belt is intentionally
            // skipped together and a skipped `ci` reports no category outputs. This verdict was
            // RED before the fix (the shell read `skipped` as a failure on the always-required
            // base) and is the documented, valid release shape now.
            let (ok, text) = run_aggregator(&[
                ("CI_RESULT", "skipped"),
                ("BQ_RESULT", "skipped"),
                ("BQ_SELECTED", ""),
                ("KC_RESULT", "skipped"),
                ("KC_SELECTED", ""),
                ("EVENT", "push"),
                ("PR_HEAD", ""),
                ("REPO", "telekom/sutura"),
            ]);
            assert!(ok, "release-commit skip must aggregate GREEN, got: {text}");
            assert!(
                text.contains("ok - every category-gated leg green"),
                "the aggregator should affirm the belt: {text}"
            );
        }

        #[test]
        fn a_selected_but_skipped_bigquery_leg_is_still_red() {
            // THE #135 RULE the aggregator exists to hold: a buggy filter that silently skips an
            // affected adapter leaves the category selected while its leg reports skipped. `ci`
            // ran fine here, yet the leg must still fail - the fix must not widen to forgive it.
            let (ok, text) = run_aggregator(&[
                ("CI_RESULT", "success"),
                ("BQ_RESULT", "skipped"),
                ("BQ_SELECTED", "true"),
                ("KC_RESULT", "skipped"),
                ("KC_SELECTED", ""),
                ("EVENT", "push"),
                ("PR_HEAD", ""),
                ("REPO", "telekom/sutura"),
            ]);
            assert!(!ok, "selected-but-skipped must stay RED, got: {text}");
            assert!(
                text.contains("bigquery-acceptance must run"),
                "the verdict should name the required-but-skipped leg: {text}"
            );
        }

        #[test]
        fn a_selected_but_skipped_keycloak_leg_is_still_red() {
            // The same #135 rule, over `keycloak-served-test`: it reads no secret, so it carries
            // no event exception at all - every event that selects `identity` must see it succeed.
            let (ok, text) = run_aggregator(&[
                ("CI_RESULT", "success"),
                ("BQ_RESULT", "skipped"),
                ("BQ_SELECTED", ""),
                ("KC_RESULT", "skipped"),
                ("KC_SELECTED", "true"),
                ("EVENT", "push"),
                ("PR_HEAD", ""),
                ("REPO", "telekom/sutura"),
            ]);
            assert!(!ok, "selected-but-skipped must stay RED, got: {text}");
            assert!(
                text.contains("keycloak-served-test must run"),
                "the verdict should name the required-but-skipped leg: {text}"
            );
        }

        #[test]
        fn a_clean_run_is_green() {
            let (ok, text) = run_aggregator(&[
                ("CI_RESULT", "success"),
                ("BQ_RESULT", "success"),
                ("BQ_SELECTED", "true"),
                ("KC_RESULT", "success"),
                ("KC_SELECTED", "true"),
                ("EVENT", "push"),
                ("PR_HEAD", ""),
                ("REPO", "telekom/sutura"),
            ]);
            assert!(ok, "a clean run must aggregate GREEN, got: {text}");
        }
    }
}
