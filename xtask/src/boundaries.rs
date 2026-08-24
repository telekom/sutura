//! The architecture-boundary gate. Two halves, because the hexagon's boundary has two sides
//! and only one of them was ever checked:
//!
//! * which way dependencies point - the domain crate acquires no framework dependency (here)
//! * what the crossing looks like - a library's types and errors are a typed contract, not a
//!   struct with public fields returning `Result<_, String>` (`api_shape`)
//!
//! One gate rather than two, because both answer "is the boundary real?", and because a rule
//! in its own task has to be transcribed into the justfile, twice into devenv.nix, into
//! flake.nix and into a hook before it runs anywhere.
//!
//! An ALLOWLIST over the whole transitive tree, not a denylist over direct dependencies. Two
//! corrections to how this started, both found by review:
//!
//! * It read the *declared* dependency list, which is a manifest grep with extra steps. The
//!   domain could acquire an async runtime through any innocuous-looking crate and the gate
//!   printed "ok". It now walks `resolve.nodes`.
//! * A denylist only catches what somebody thought to name. `tower`, `tonic`, `rustls`,
//!   `sqlx` and everything else were permitted. Inverting it means a new dependency is a
//!   one-line diff to the list below - visible, arguable, and impossible to miss.

mod api_shape;

use std::collections::BTreeSet;

use crate::Verdict;

/// The complete transitive dependency tree `sutura-domain` is permitted.
///
/// Serde and thiserror, plus what their derive macros pull in. Anything else - an async
/// runtime, an HTTP client, a query engine, a TLS stack - makes the hexagon decoration and
/// makes every domain test pay for a framework build.
///
/// Adding a name here is an architecture decision. That is the point.
const ALLOWED_IN_DOMAIN: &[&str] = &[
    "serde",
    "serde_core",
    "serde_derive",
    "thiserror",
    "thiserror-impl",
    // The proc-macro chain the two derives above require.
    "proc-macro2",
    "quote",
    "syn",
    "unicode-ident",
];

const DOMAIN: &str = "sutura-domain";

/// Every package name reachable from `start` in the resolve graph.
fn transitive_names(meta: &serde_json::Value, start: &str) -> Result<BTreeSet<String>, String> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let nodes = meta
        .get("resolve")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `resolve.nodes`"))?;

    let name_of = |id: &str| -> Option<String> {
        packages.iter().find_map(|p| {
            (p.get("id").and_then(|i| i.as_str()) == Some(id))
                .then(|| p.get("name")?.as_str().map(String::from))
                .flatten()
        })
    };
    let start_id = packages
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(start))
        .and_then(|p| p.get("id")?.as_str())
        .ok_or_else(|| format!("{start} not found in workspace metadata"))?;

    let deps_of = |id: &str| -> Vec<String> {
        nodes
            .iter()
            .find(|n| n.get("id").and_then(|i| i.as_str()) == Some(id))
            .and_then(|n| n.get("deps")?.as_array())
            .map(|deps| deps.iter().filter_map(|d| d.get("pkg")?.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };

    // Iterative, so a dependency cycle cannot blow the stack.
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![String::from(start_id)];
    while let Some(current) = stack.pop() {
        for dep in deps_of(&current) {
            if seen_ids.insert(dep.clone()) {
                if let Some(name) = name_of(&dep) {
                    names.insert(name);
                }
                stack.push(dep);
            }
        }
    }
    Ok(names)
}

/// Names in `tree` that the allowlist does not permit.
fn violations(tree: &BTreeSet<String>) -> Vec<&String> {
    tree.iter()
        .filter(|name| !ALLOWED_IN_DOMAIN.contains(&name.as_str()))
        .collect()
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    // Both halves run even when the first fails. They are independent findings, and a gate
    // that stops early makes the second violation look like it appeared after the first fix.
    let direction = dependency_direction();
    let surface = typed_surface();
    if direction == Verdict::Pass && surface == Verdict::Pass {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

/// Which way dependencies point: nothing framework-shaped is reachable from the domain.
fn dependency_direction() -> Verdict {
    // `--all-features` for the same reason every other gate uses it: adapters are default-off,
    // so the default graph is nearly empty and would hide exactly what this checks.
    let meta = match crate::cargo_metadata(&["--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };

    let tree = match transitive_names(&meta, DOMAIN) {
        Ok(names) => names,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };

    let bad = violations(&tree);
    if bad.is_empty() {
        println!(
            "xtask check-boundaries: ok - {DOMAIN}'s whole tree is {} crate(s), all allowlisted",
            tree.len()
        );
        return Verdict::Pass;
    }

    eprintln!("xtask check-boundaries: FAILED - {DOMAIN} reaches crates it may not:");
    for name in bad {
        eprintln!("  {name}");
    }
    eprintln!();
    eprintln!("The domain holds the types, and the port traits that arrive with the first");
    eprintln!("adapter. A dependency it can reach - directly or transitively - is one every");
    eprintln!("domain test pays for and one the hexagon leaks. If it genuinely belongs, add it");
    eprintln!("to ALLOWED_IN_DOMAIN with the reason: that is an architecture decision and");
    eprintln!("should be a visible diff.");
    Verdict::Fail
}

/// What the crossing looks like: a library's types and errors are a typed contract.
fn typed_surface() -> Verdict {
    match api_shape::check() {
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            Verdict::Fail
        }
        Ok(report) if report.problems.is_empty() => {
            println!(
                "xtask check-boundaries: ok - {} library source file(s), typed surface intact",
                report.files
            );
            Verdict::Pass
        }
        Ok(report) => {
            eprintln!("xtask check-boundaries: FAILED - a library crate's contract is not typed:");
            for problem in &report.problems {
                eprintln!("  {problem}");
            }
            eprintln!();
            api_shape::explain();
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ALLOWED_IN_DOMAIN, transitive_names, violations};

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    #[test]
    fn an_allowlisted_tree_has_no_violations() {
        assert!(violations(&set(ALLOWED_IN_DOMAIN)).is_empty());
    }

    #[test]
    fn a_framework_anywhere_in_the_tree_is_a_violation() {
        // The case a denylist over DIRECT dependencies missed: reached transitively, and not
        // a name anybody had thought to forbid.
        let tree = set(&["serde", "tower", "rustls"]);
        let found = violations(&tree);
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn the_walk_is_transitive() {
        // b is only reachable through a; a denylist reading declared deps would not see it.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "root", "name": "sutura-domain"},
                    {"id": "a-id", "name": "a"},
                    {"id": "b-id", "name": "b"}
                ],
                "resolve": {"nodes": [
                    {"id": "root", "deps": [{"pkg": "a-id"}]},
                    {"id": "a-id", "deps": [{"pkg": "b-id"}]},
                    {"id": "b-id", "deps": []}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let tree = transitive_names(&meta, "sutura-domain").expect("walk succeeds");
        assert_eq!(tree, set(&["a", "b"]));
    }

    #[test]
    fn a_cycle_terminates() {
        // Cargo will not produce one, but an iterative walk should not depend on that.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "root", "name": "sutura-domain"},
                    {"id": "a-id", "name": "a"}
                ],
                "resolve": {"nodes": [
                    {"id": "root", "deps": [{"pkg": "a-id"}]},
                    {"id": "a-id", "deps": [{"pkg": "root"}]}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let tree = transitive_names(&meta, "sutura-domain").expect("walk succeeds");
        assert!(tree.contains("a"));
    }

    #[test]
    fn a_missing_crate_is_an_error_not_a_pass() {
        let meta: serde_json::Value = serde_json::from_str(r#"{"packages": [], "resolve": {"nodes": []}}"#).expect("parses");
        drop(transitive_names(&meta, "sutura-domain").unwrap_err());
    }
}
