//! The conformance harness reaches no adapter.
//!
//! **A pack is a statement about a PORT, and the whole of what makes that true is a dependency
//! list.** `sutura-conformance` holds test bodies written once against the ports and bound to an
//! adapter by a macro; the moment it can reach one implementor, a pack body can be written against
//! that implementor, and the assertion stops being about the port. `docs/adr/0012` exists to stop
//! exactly that, and until this half existed the rule was a sentence in a manifest comment.
//!
//! **It was disproved rather than doubted.** With `sutura-exec-duckdb` added under
//! `[dependencies]` and used in the crate's public API, so `unused-deps` could not be the catcher,
//! `just hygiene` was green, every other half of this gate was green, and clippy was clean under
//! `-D warnings`. The reason is structural: the harness matches no `Membership::Prefix` and no
//! `Named` list in `super::adapters`, so it joins no adapter class, and `FORBIDDEN_EDGES` names
//! only `sutura-semantic`. Nothing else in the tree had an opinion.
//!
//! # An allowlist, not a denylist.
//!
//! [`ALLOWED_IN_DOMAIN`](super::ALLOWED_IN_DOMAIN)'s shape rather than
//! [`FORBIDDEN_EDGES`](super::FORBIDDEN_EDGES)'.
//!
//! For the same reason the domain rule is one: a denylist catches what somebody thought to name,
//! and the set of adapters grows. Here the permitted set is *the interior*, so it is one entry and
//! a new one is a one-line diff with the argument in it.
//!
//! # What it does NOT reach.
//!
//! **First-party crates only.** A third-party dependency is `deny.toml`'s and
//! `check-attribution`'s business, and a harness that acquired one would still be a harness.
//!
//! **`Edges::Normal`, so a dev-dependency is permitted** - and that is deliberate rather than a
//! gap. Every consumer of this crate takes it as a dev-dependency, and the packs crate's own
//! `tests/` legitimately reaches for whatever it needs to prove the harness fires; what may not
//! happen is a pack BODY compiled against an adapter, and a pack body is `src/`.
//!
//! **It says nothing about which adapters are bound.** A crate that carries no binding at all is
//! `telekom/sutura#351`'s gate, not this one.

use super::adapters::workspace_members;
use super::{Edges, transitive_names};

/// The crate whose closure is walked.
const HARNESS: &str = "sutura-conformance";

/// The first-party crates the harness may reach through a NORMAL dependency.
///
/// **The interior, and nothing else. Adding a name here is an architecture decision** - the same
/// sentence [`ALLOWED_IN_DOMAIN`](super::ALLOWED_IN_DOMAIN) and
/// [`FORBIDDEN_EDGES`](super::FORBIDDEN_EDGES) carry, for the same reason: the diff is where the
/// argument happens. The two names that will be asked for first are `sutura-semantic` and
/// `sutura-sql`, for the COMPILE packs `docs/adr/0012` splits out - and the right answer there is a
/// default-off feature of this crate rather than an entry here, so a data adapter binding the
/// execute packs links neither.
const PERMITTED: &[&str] = &["sutura-domain"];

/// What the check looked at, and what it found.
pub(super) struct Report {
    /// Every crate in the harness's normal closure, first-party or not. Printed, because a closure
    /// that suddenly holds two crates is the shape of a rule that stopped resolving.
    pub(super) closure: usize,
    /// The first-party crates it reaches, permitted or not.
    pub(super) first_party: Vec<String>,
    /// One line per violation, already formatted for stderr.
    pub(super) problems: Vec<String>,
}

/// Walks it.
pub(super) fn check(meta: &serde_json::Value) -> Result<Report, String> {
    let members = workspace_members(meta)?;
    // `Edges::Normal`, argued in the header: a dev-dependency of this crate is how its own tests
    // prove the harness fires, and forbidding that would forbid the evidence.
    let closure = transitive_names(meta, HARNESS, Edges::Normal)?;
    let first_party: Vec<String> = closure
        .iter()
        .filter(|name| name.as_str() != HARNESS && members.contains(*name))
        .cloned()
        .collect();

    let mut problems = Vec::new();
    // Fails closed, like every other half here: the harness reaching the interior is the
    // PRECONDITION of the rule, so its absence is a rule that has stopped checking anything rather
    // than a clean tree.
    for required in PERMITTED {
        if !first_party.iter().any(|name| name == required) {
            problems.push(format!(
                "{HARNESS} no longer reaches {required} - this rule would be checking a crate that \
                 is not the harness"
            ));
        }
    }
    for reached in &first_party {
        if !PERMITTED.contains(&reached.as_str()) {
            problems.push(format!(
                "{HARNESS} reaches {reached} through a normal dependency, and a pack body could be \
                 written against it"
            ));
        }
    }
    // The COMPILE packs sit behind a default-off feature, stated in the manifest comment - the
    // whole reason a data adapter binding the execute packs links neither the compiler nor the
    // renderer. `PERMITTED` above continues to hold the NORMAL closure to the interior, so the
    // gate checks the feature shape here and nowhere else: `--all-features` is what the walk runs
    // under, so without this check turning `compile` on by default would pass every half of this
    // gate silently.
    problems.extend(compile_feature(meta)?);

    Ok(Report {
        closure: closure.len(),
        first_party,
        problems,
    })
}

/// The exact feature shape `HARNESS` must keep: `default = []` and nothing else, and the
/// `compile` feature is the ONE entry allowed to reach the compiler and the renderer.
///
/// **The mutation this half exists for is [`default = ["compile"]`]** - the shape that would grow
/// the compiler and the renderer into every data adapter's closure and make the portability
/// contract `src/corpus.rs` states false. The check reads the resolved overlay literally, so a
/// manifest that lists `compile` under `default` is a gate failure, not a review comment.
const COMPILE_FEATURE: &str = "compile";
const COMPILE_DEPS: &[&str] = &["dep:sutura-semantic", "dep:sutura-sql", "dep:serde_json"];

fn compile_feature(meta: &serde_json::Value) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    let Some(package) = meta
        .get("packages")
        .and_then(|packages| packages.as_array())
        .and_then(|packages| {
            packages
                .iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(HARNESS))
        })
    else {
        return Err(format!(
            "{HARNESS} is not in `cargo metadata`, so the compile-feature contract has nothing to check"
        ));
    };
    let features = package.get("features").and_then(|f| f.as_object());
    // `cargo metadata` always carries a features map; its absence is a malformed `meta` rather than
    // a manifest decision, so fail the gate closed rather than read into a default that would pass.
    let Some(features) = features else {
        return Ok(vec![format!(
            "{HARNESS}'s `cargo metadata` carries no feature map - the compile packs' default-off \
             shape is not enforceable"
        )]);
    };
    let default: Vec<String> = features
        .get("default")
        .and_then(|d| d.as_array())
        .map(|d| d.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if !default.is_empty() {
        problems.push(format!(
            "{HARNESS} enables features by default ({default:?}) - the compile packs must be \
             default-off so a data adapter binding the execute packs links neither the compiler \
             nor the renderer"
        ));
    }
    let compile: Vec<String> = features
        .get(COMPILE_FEATURE)
        .and_then(|c| c.as_array())
        .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if compile != COMPILE_DEPS {
        problems.push(format!(
            "{HARNESS} declares `compile` as {compile:?}, expected exactly {COMPILE_DEPS:?} - the \
             compile packs' one feature may reach the compiler, the renderer and `serde_json` and \
             nothing else"
        ));
    }
    Ok(problems)
}

/// What to do about a violation. Printed, because a gate that only says "no" gets worked around.
pub(super) fn explain() {
    eprintln!("A conformance pack is written ONCE against a port and bound to an adapter by a");
    eprintln!("macro. That is the whole of `docs/adr/0012`: an assertion that appears twice will");
    eprintln!("disagree with itself, and the disagreement gets read as a difference between two");
    eprintln!("data systems rather than as a difference between two copies of a test. A harness");
    eprintln!("that can reach one implementor is a harness whose next pack body can be written");
    eprintln!("against that implementor.");
    eprintln!();
    eprintln!("What to do instead:");
    eprintln!("  * a type both the pack and the adapter need belongs in `sutura-domain`, where");
    eprintln!("    `QueryPlan`, `RowSet` and `warehouse::agreement` already are;");
    eprintln!("  * an adapter-specific affordance - attaching a CSV, opening a schema, reaching a");
    eprintln!("    provisioned tier - belongs in that adapter's own FIXTURE, which is the `open`");
    eprintln!("    path the binding macro takes and the one thing an adapter contributes;");
    eprintln!("  * a whole family of packs that genuinely needs another first-party crate - the");
    eprintln!("    compile packs need the compiler and the renderer - belongs behind a default-off");
    eprintln!("    feature of this crate, so a data adapter binding the execute packs links");
    eprintln!("    neither.");
    eprintln!();
    eprintln!("  `cargo tree -p sutura-conformance -e normal` names the edge.");
    eprintln!("  If it genuinely belongs, the entry in `PERMITTED` is what has to go, and that is");
    eprintln!("  an architecture decision: a visible diff with the argument in it, not a");
    eprintln!("  dependency somebody added on the way past.");
}

#[cfg(test)]
mod tests {
    use super::{HARNESS, check};

    /// One workspace member and the crates it depends on.
    ///
    /// A named alias because the spelled-out slice of pairs is past `clippy.toml`'s
    /// type-complexity threshold, and the lint's own instruction is to factor it out.
    type Edge<'a> = (&'a str, &'a [&'a str]);

    /// Every package with its resolve node, as `cargo metadata` shapes it.
    fn metadata(edges: &[Edge<'_>]) -> serde_json::Value {
        let packages: Vec<serde_json::Value> = edges
            .iter()
            .map(|(name, _)| {
                // The harness's OWN feature shape, so the compile-feature contract has something
                // to check; every other package carries an empty map the contract ignores.
                let features = if *name == HARNESS {
                    serde_json::json!({
                        "default": [],
                        "compile": ["dep:sutura-semantic", "dep:sutura-sql", "dep:serde_json"],
                    })
                } else {
                    serde_json::json!({ "default": [] })
                };
                serde_json::json!({ "id": format!("id-{name}"), "name": name, "features": features })
            })
            .collect();
        let nodes: Vec<serde_json::Value> = edges
            .iter()
            .map(|(name, deps)| {
                serde_json::json!({
                    "id": format!("id-{name}"),
                    "deps": deps
                        .iter()
                        .map(|dep| serde_json::json!({
                            "pkg": format!("id-{dep}"),
                            "dep_kinds": [{ "kind": serde_json::Value::Null }],
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        // WORKSPACE members only, which is the distinction the check turns on: a third-party crate
        // in the closure is `deny.toml`'s business and not this rule's. Every first-party crate in
        // this repository is named `sutura-*`, so the fixture derives membership from the prefix
        // rather than taking a second list nothing would keep in step.
        let members: Vec<String> = edges
            .iter()
            .filter(|(name, _)| name.starts_with("sutura-"))
            .map(|(name, _)| format!("id-{name}"))
            .collect();
        serde_json::json!({
            "packages": packages,
            "workspace_members": members,
            "resolve": { "nodes": nodes },
        })
    }

    /// The tree as it is: the harness reaches the interior and a third party, and nothing else.
    #[test]
    fn a_harness_that_reaches_only_the_domain_passes() {
        let meta = metadata(&[
            (HARNESS, &["sutura-domain", "thiserror"]),
            ("sutura-domain", &[]),
            ("thiserror", &[]),
        ]);
        let report = check(&meta).expect("the fixture resolves");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.first_party, vec![String::from("sutura-domain")]);
    }

    /// **The mutation this half exists for**, and the one every other gate in this repository
    /// passed: an adapter under `[dependencies]`.
    #[test]
    fn a_harness_that_reaches_an_adapter_fails() {
        let meta = metadata(&[
            (HARNESS, &["sutura-domain", "sutura-exec-duckdb"]),
            ("sutura-domain", &[]),
            ("sutura-exec-duckdb", &[]),
        ]);
        let report = check(&meta).expect("the fixture resolves");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("sutura-exec-duckdb"), "{:?}", report.problems);
    }

    /// Laundering it through a third crate does not help: the walk is transitive.
    #[test]
    fn an_adapter_reached_through_a_third_crate_fails() {
        let meta = metadata(&[
            (HARNESS, &["sutura-domain", "sutura-sql"]),
            ("sutura-domain", &[]),
            ("sutura-sql", &["sutura-exec-duckdb"]),
            ("sutura-exec-duckdb", &[]),
        ]);
        let report = check(&meta).expect("the fixture resolves");
        assert_eq!(report.problems.len(), 2, "{:?}", report.problems);
    }

    /// It fails CLOSED: a harness that no longer reaches the interior is a rule that has stopped
    /// checking anything, not a clean tree.
    #[test]
    fn a_harness_that_reaches_nothing_first_party_fails_rather_than_passing() {
        let meta = metadata(&[(HARNESS, &["thiserror"]), ("thiserror", &[])]);
        let report = check(&meta).expect("the fixture resolves");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("no longer reaches"), "{:?}", report.problems);
    }

    /// And a tree with no harness in it is an error rather than a pass.
    #[test]
    fn a_missing_harness_is_an_error() {
        let meta = metadata(&[("sutura-domain", &[])]);
        assert!(check(&meta).is_err());
    }

    /// A harness whose manifest names `default = []` and the `compile` set passes the feature half
    /// of the gate.
    #[test]
    fn a_compile_feature_that_is_default_off_passes() {
        let meta = metadata(&[
            (HARNESS, &["sutura-domain", "thiserror"]),
            ("sutura-domain", &[]),
            ("thiserror", &[]),
        ]);
        let report = check(&meta).expect("the fixture resolves");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    /// **The mutation the feature half exists for:** `default = ["compile"]` turns the compiler and
    /// the renderer into every data adapter's closure, and must be a gate failure - not a review
    /// comment - so the decision stays tied to a mechanism.
    #[test]
    fn a_compile_feature_turned_on_by_default_fails() {
        let mut meta = metadata(&[
            (HARNESS, &["sutura-domain", "thiserror"]),
            ("sutura-domain", &[]),
            ("thiserror", &[]),
        ]);
        // Point the harness's `default` at `compile`: the shape that links a data adapter to the
        // compiler and the renderer for no reason.
        let package = meta["packages"]
            .as_array_mut()
            .expect("the fixture has packages")
            .iter_mut()
            .find(|p| p["name"] == HARNESS)
            .expect("the fixture has the harness");
        package["features"]["default"] = serde_json::json!(["compile"]);

        let report = check(&meta).expect("the fixture resolves");
        assert!(
            report.problems.iter().any(|p| p.contains("default-off")),
            "turning `compile` on by default must be reported: {:?}",
            report.problems
        );
    }
}
