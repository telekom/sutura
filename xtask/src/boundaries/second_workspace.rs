//! A cargo workspace `cargo metadata` from the repo root never resolves - and what makes
//! [`super::dependency_direction`] and [`super::forbidden_edges`], and no other half here, the
//! right two to run against it a second time.
//!
//! **`telekom/sutura#863`'s own finding.** `fuzz/Cargo.toml` declares an empty `[workspace]`
//! table, which makes it a workspace root of its own rather than a member of this one - `cargo
//! metadata` invoked from the repo root, with or without `--all-features`, never sees the package
//! at all. So an edge visible only through fuzz's OWN manifest - a `[patch]` table, a feature
//! request unique to it, or its own separately pinned `Cargo.lock` - passed every half in
//! `boundaries.rs` in silence, not because any of them permitted it, but because none of them
//! could see it.
//!
//! **Why two rules generalise here and the other seven do not.** [`super::dependency_direction`]
//! (walks `sutura-domain`'s tree against an allowlist) and [`super::forbidden_edges`] (walks a
//! NAMED crate's tree for a NAMED target) each ask a question about ONE crate's own reachability,
//! answerable inside any graph that happens to contain it. The rest ask a question that only the
//! ROOT workspace's membership can answer: an intra-class edge needs TWO members of a class
//! ([`super::adapter_classes`], and a one-member satellite fails every class as "matches no
//! member" before reading a single dependency); `sutura-app`'s and the conformance harness's own
//! trees are questions about THOSE crates, which are not packages in `fuzz/`'s graph at all -
//! `transitive_names` errors on a `start` it cannot find, which would make
//! [`super::no_adapter_in_application`] and [`super::harness_reaches_no_adapter`] refuse a fuzz
//! satellite unconditionally, for a reason that has nothing to do with an edge; and the driving-port
//! and typed-surface halves read root MEMBERS' source by name (`sutura-cli`, `sutura-http`,
//! `sutura-mcp`), which `fuzz/`'s `--no-deps` view would never contain regardless. Measured before
//! being built rather than discovered in a red run, per the issue's own request.
//!
//! **What this does not close.** `fuzz/Cargo.lock`'s own dependency-version divergence from the
//! root lock is a SEPARATE axis - `telekom/sutura#863` names it explicitly - and nothing here
//! compares the two locks. This reads fuzz's resolved graph AS IT STANDS and applies the same two
//! rules the root graph gets; it does not reconcile the versions those two graphs happen to pin.

use std::collections::BTreeSet;

use super::{DOMAIN, Edges, FORBIDDEN_EDGES, edges, reaches, transitive_names, violations};

/// A cargo workspace this repository declares on purpose, other than the root one.
pub(super) struct Satellite {
    /// The manifest, repo-relative.
    pub(super) manifest: &'static str,
    /// Why it is a separate workspace rather than a member of the root one - printed on the
    /// success line, the same reason [`super::edges::FORBIDDEN_EDGES`] prints its own `why` on a
    /// failure: a declared exemption with no argument invites a second one nobody has to justify.
    pub(super) why: &'static str,
}

/// The complete, declared set. **Adding one is an architecture decision, the same sentence
/// [`super::edges::ALLOWED_IN_DOMAIN`] and [`super::edges::FORBIDDEN_EDGES`] carry** - and
/// [`undeclared`] is the backstop for the entry that never gets added: a manifest anywhere in this
/// tree that declares its own `[workspace]` table and is not named here is a workspace this half
/// cannot see, found rather than assumed absent.
pub(super) const DECLARED: &[Satellite] = &[Satellite {
    manifest: "fuzz/Cargo.toml",
    why: "libFuzzer targets must not enter the shipped graph or inherit the root workspace's \
          `[profile.*]` tables - fuzz/Cargo.toml's own header",
}];

/// What the check looked at, and what it found.
pub(super) struct Report {
    /// One line per satellite actually walked, printed on success.
    pub(super) walked: Vec<String>,
    /// One line per violation, already formatted for stderr.
    pub(super) problems: Vec<String>,
}

/// Walks every [`DECLARED`] satellite's own `cargo metadata`, and checks for a third, undeclared
/// one.
pub(super) fn check() -> Result<Report, String> {
    let mut walked = Vec::new();
    let mut problems = Vec::new();
    let root = crate::repo::root().ok_or_else(|| String::from("could not find the repo root"))?;

    for satellite in DECLARED {
        let meta = crate::cargo_metadata_at(&root.join(satellite.manifest), &["--all-features"])?;
        let evaluation = evaluate(satellite.manifest, &meta)?;
        problems.extend(evaluation.problems);
        walked.push(format!(
            "{} - {} ({} crate(s) in {DOMAIN}'s tree, {} forbidden-edge entr{} evaluated)",
            satellite.manifest,
            satellite.why,
            evaluation.domain_tree_size,
            evaluation.evaluated,
            if evaluation.evaluated == 1 { "y" } else { "ies" }
        ));
    }

    problems.extend(undeclared()?);
    Ok(Report { walked, problems })
}

/// What [`evaluate`] found in one satellite's own graph.
struct Evaluation {
    /// How many crates are in `sutura-domain`'s tree over THIS graph.
    domain_tree_size: usize,
    /// How many [`FORBIDDEN_EDGES`] entries this graph actually contains the `from` crate for.
    evaluated: usize,
    /// One line per violation, already formatted for stderr.
    problems: Vec<String>,
}

/// The two rules the header argues generalise, run against ONE satellite's own resolved graph.
///
/// Pure over `meta`, so a fixture drives it exactly the way [`super::application::check`] and
/// [`super::adapters::check`] already do - no shelling out to `cargo metadata` to prove a
/// violation is caught.
fn evaluate(manifest: &str, meta: &serde_json::Value) -> Result<Evaluation, String> {
    let mut problems = Vec::new();

    let domain_tree = transitive_names(meta, DOMAIN, Edges::Every).map_err(|message| format!("{manifest}: {message}"))?;
    for name in violations(&domain_tree) {
        problems.push(format!(
            "{manifest}: {DOMAIN} reaches {name}, which is not on ALLOWED_IN_DOMAIN"
        ));
    }

    // Only the entries whose `from` crate is actually a package in THIS graph - absence here is a
    // smaller workspace's structural shape, not the rule going quiet the way an empty ROOT tree
    // would be. `super::forbidden_edges` still walks every entry against the root graph, where all
    // of them are always present; this is additional coverage, not a narrower version of it.
    let mut evaluated = 0_usize;
    for edge in FORBIDDEN_EDGES {
        if !edges::contains_package(meta, edge.from) {
            continue;
        }
        evaluated += 1;
        match reaches(meta, edge) {
            Ok(true) => problems.push(format!(
                "{manifest}: {} reaches {}, and it may not - {}",
                edge.from, edge.forbidden, edge.why
            )),
            Ok(false) => {}
            Err(message) => problems.push(format!("{manifest}: {message}")),
        }
    }

    Ok(Evaluation {
        domain_tree_size: domain_tree.len(),
        evaluated,
        problems,
    })
}

/// Does this line declare a `[workspace]` table? Line-oriented rather than parsed - the same
/// reason `xtask/src/fuzz.rs`'s own header gives: xtask carries no TOML dependency, and a bare
/// table header is unambiguous surface syntax here. A `Cargo.toml` in this tree has no multi-line
/// string that could smuggle one past a line match.
fn declares_its_own_workspace(text: &str) -> bool {
    text.lines().any(|line| line.trim() == "[workspace]")
}

fn is_cargo_manifest(rel: &str) -> bool {
    std::path::Path::new(rel).file_name().is_some_and(|name| name == "Cargo.toml")
}

/// Backstop (3): every `Cargo.toml` that declares its own `[workspace]` table, other than the
/// root's, has to be named in [`DECLARED`] - turning a THIRD workspace nobody added here from
/// invisible into a refusal, and a [`DECLARED`] entry that stopped declaring one into a stale row
/// rather than a silent no-op.
fn undeclared() -> Result<Vec<String>, String> {
    Ok(compare(&discover()?))
}

/// The filesystem half: every `Cargo.toml` in the tree, other than the root's, that declares its
/// own `[workspace]` table.
fn discover() -> Result<BTreeSet<String>, String> {
    let census = crate::repo::all_files().map_err(|refusal| refusal.describe())?;
    let mut found = Vec::new();
    census
        .inspect(&["Cargo.toml"], is_cargo_manifest, |rel, bytes| {
            if rel != "Cargo.toml" && declares_its_own_workspace(&String::from_utf8_lossy(bytes)) {
                found.push(String::from(rel));
            }
        })
        .map_err(|refusal| refusal.describe())?;
    Ok(found.into_iter().collect())
}

/// The pure half: `found` against [`DECLARED`], both directions. Split from [`discover`] so a
/// fixture can drive it with a plain set - no scratch tree, no git - the same separation
/// [`evaluate`] keeps from `cargo metadata`.
fn compare(found: &BTreeSet<String>) -> Vec<String> {
    let declared: BTreeSet<&str> = DECLARED.iter().map(|satellite| satellite.manifest).collect();
    let found_names: BTreeSet<&str> = found.iter().map(String::as_str).collect();

    let mut problems: Vec<String> = found_names
        .difference(&declared)
        .map(|extra| {
            format!(
                "{extra} declares its own `[workspace]` table and is not in DECLARED - a cargo \
                 workspace this gate cannot see appeared unannounced"
            )
        })
        .chain(declared.difference(&found_names).map(|missing| {
            format!(
                "{missing} is in DECLARED but no longer declares its own `[workspace]` table - the \
                 entry is stale"
            )
        }))
        .collect();
    problems.sort();
    problems
}

/// Printed when this half fails, because a rule whose reason is unstated gets reverted.
pub(super) fn explain() {
    eprintln!("A satellite workspace's own manifest is a graph `cargo metadata` from the repo root");
    eprintln!("never resolves. This half reads it directly and applies the domain allowlist and the");
    eprintln!("forbidden-edge denylist EXACTLY as the root halves do - not the intra-class or");
    eprintln!("application rules, which ask a question only the root workspace's own membership can");
    eprintln!("answer (see this file's header for why).");
    eprintln!();
    eprintln!("If the edge genuinely belongs, ALLOWED_IN_DOMAIN or FORBIDDEN_EDGES is what has to");
    eprintln!("change - the same architecture decision as everywhere else in this gate. If a NEW");
    eprintln!("workspace appeared, add it to DECLARED with the reason it must stay separate, or fold");
    eprintln!("it back into the root one.");
}

#[cfg(test)]
mod tests {
    use super::{compare, declares_its_own_workspace, evaluate, is_cargo_manifest};
    use std::collections::BTreeSet;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    /// The package id a fixture crate name resolves to - only `sutura-sql` needs a shorter one.
    fn name_to_id(name: &str) -> &str {
        match name {
            "sutura-sql" => "sql",
            other => other,
        }
    }

    /// A satellite's `sutura-domain`, its allowed tree, and a `sutura-catalog-local` that does
    /// reach `sutura-sql` - the shape `application.rs`'s own fixtures build.
    fn meta(domain_deps: &[&str], catalog_local_deps: &[&str]) -> serde_json::Value {
        let packages = serde_json::json!([
            {"id": "domain", "name": "sutura-domain"},
            {"id": "serde", "name": "serde"},
            {"id": "tokio", "name": "tokio"},
            {"id": "cl", "name": "sutura-catalog-local"},
            {"id": "sql", "name": "sutura-sql"},
        ]);
        let deps = |names: &[&str]| -> serde_json::Value {
            serde_json::Value::Array(names.iter().map(|n| serde_json::json!({"pkg": name_to_id(n)})).collect())
        };
        serde_json::json!({
            "packages": packages,
            "resolve": {"nodes": [
                {"id": "domain", "deps": deps(domain_deps)},
                {"id": "serde", "deps": []},
                {"id": "tokio", "deps": []},
                {"id": "cl", "deps": deps(catalog_local_deps)},
                {"id": "sql", "deps": []},
            ]}
        })
    }

    #[test]
    fn a_clean_satellite_graph_has_no_problems() {
        let evaluation = evaluate("fuzz/Cargo.toml", &meta(&["serde"], &[])).expect("resolves");
        assert!(evaluation.problems.is_empty(), "{:?}", evaluation.problems);
        assert_eq!(evaluation.domain_tree_size, 1, "domain's own tree is just serde");
        // `sutura-catalog-local` is present in this graph and one FORBIDDEN_EDGES row names it as
        // `from`, so it must be evaluated even though it does not reach the forbidden target.
        assert!(evaluation.evaluated >= 1, "a present `from` crate must be evaluated");
    }

    /// **The mutation this half exists for, domain side**: an edge reachable only through the
    /// SATELLITE's own graph - `fuzz/Cargo.toml`'s `[patch]` table constructs exactly this in the
    /// real tree, and this fixture is the same shape without shelling out to cargo.
    #[test]
    fn a_domain_tree_reaching_a_disallowed_crate_is_a_violation() {
        let evaluation = evaluate("fuzz/Cargo.toml", &meta(&["serde", "tokio"], &[])).expect("resolves");
        assert_eq!(evaluation.problems.len(), 1, "{:?}", evaluation.problems);
        assert!(evaluation.problems[0].contains("tokio"), "{:?}", evaluation.problems);
        assert!(
            evaluation.problems[0].starts_with("fuzz/Cargo.toml: "),
            "{:?}",
            evaluation.problems
        );
    }

    /// **The mutation this half exists for, forbidden-edge side**: `sutura-catalog-local` is a
    /// `FORBIDDEN_EDGES` `from` crate, present in this graph, and reaching `sutura-sql`.
    #[test]
    fn a_forbidden_edge_present_in_the_satellite_graph_is_caught() {
        let evaluation = evaluate("fuzz/Cargo.toml", &meta(&["serde"], &["sutura-sql"])).expect("resolves");
        assert!(evaluation.evaluated >= 1);
        assert!(
            evaluation
                .problems
                .iter()
                .any(|p| p.contains("sutura-catalog-local") && p.contains("sutura-sql")),
            "{:?}",
            evaluation.problems
        );
    }

    /// The other half of the same claim: a `FORBIDDEN_EDGES` row whose `from` crate this smaller
    /// graph does not contain at all is SKIPPED rather than an error - `#863`'s own "per-workspace
    /// rule set" caveat, held by a test rather than only by the module doc.
    #[test]
    fn a_forbidden_edges_from_crate_absent_from_the_satellite_is_not_an_error() {
        let meta = serde_json::json!({
            "packages": [{"id": "domain", "name": "sutura-domain"}],
            "resolve": {"nodes": [{"id": "domain", "deps": []}]},
        });
        let evaluation = evaluate("fuzz/Cargo.toml", &meta).expect("resolves - absence is not an error");
        assert!(evaluation.problems.is_empty(), "{:?}", evaluation.problems);
        assert_eq!(
            evaluation.evaluated, 0,
            "no FORBIDDEN_EDGES `from` crate is present in this graph"
        );
    }

    #[test]
    fn a_workspace_table_is_found_regardless_of_surrounding_content() {
        assert!(declares_its_own_workspace(
            "# a comment\n\n[workspace]\n\n[package]\nname = \"x\"\n"
        ));
        assert!(!declares_its_own_workspace("[package]\nname = \"x\"\n"));
    }

    #[test]
    fn a_workspace_dot_table_is_not_a_workspace_table() {
        // `[workspace.dependencies]` in a MEMBER's manifest must not read as a satellite - a member
        // of the root workspace legitimately writes this table.
        assert!(!declares_its_own_workspace("[workspace.dependencies]\nserde = \"1\"\n"));
    }

    #[test]
    fn only_a_cargo_toml_basename_matches() {
        assert!(is_cargo_manifest("fuzz/Cargo.toml"));
        assert!(is_cargo_manifest("Cargo.toml"));
        assert!(!is_cargo_manifest("fuzz/Cargo.lock"));
        assert!(!is_cargo_manifest("crates/sutura-domain/src/Cargo.toml.md"));
    }

    #[test]
    fn a_declared_set_matching_what_was_found_has_no_problems() {
        let empty: Vec<String> = Vec::new();
        assert_eq!(compare(&set(&["fuzz/Cargo.toml"])), empty);
    }

    /// **The mutation this half exists for**: a THIRD workspace nobody declared.
    #[test]
    fn an_undeclared_satellite_is_a_violation() {
        let problems = compare(&set(&["fuzz/Cargo.toml", "sidecar/Cargo.toml"]));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("sidecar/Cargo.toml"), "{problems:?}");
        assert!(problems[0].contains("unannounced"), "{problems:?}");
    }

    /// The other direction: a `DECLARED` entry the tree no longer has.
    #[test]
    fn a_stale_declared_entry_is_a_violation() {
        let problems = compare(&BTreeSet::new());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("fuzz/Cargo.toml"), "{problems:?}");
        assert!(problems[0].contains("stale"), "{problems:?}");
    }
}
