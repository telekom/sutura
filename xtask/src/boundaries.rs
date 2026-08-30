//! The architecture-boundary gate. Three halves - two of them about which way dependencies
//! point, one about what the crossing looks like:
//!
//! * the domain crate acquires no framework dependency ([`dependency_direction`])
//! * a named crate cannot reach a named crate ([`forbidden_edges`])
//! * a library's types and errors are a typed contract, not a struct with public fields
//!   returning `Result<_, String>` (`api_shape`)
//!
//! One gate rather than three, because they all answer "is the boundary real?", and because a
//! rule in its own task has to be transcribed into the justfile, twice into devenv.nix, into
//! flake.nix and into a hook before it runs anywhere.
//!
//! The domain rule is an ALLOWLIST over the whole transitive tree, not a denylist over direct
//! dependencies. Two corrections to how this started, both found by review:
//!
//! * It read the *declared* dependency list, which is a manifest grep with extra steps. The
//!   domain could acquire an async runtime through any innocuous-looking crate and the gate
//!   printed "ok". It now walks `resolve.nodes`.
//! * A denylist only catches what somebody thought to name. `tower`, `tonic`, `rustls`,
//!   `sqlx` and everything else were permitted. Inverting it means a new dependency is a
//!   one-line diff to the list below - visible, arguable, and impossible to miss.
//!
//! [`forbidden_edges`] is the other shape, and it is deliberately the other shape. It is a
//! denylist, which the paragraph above argues against - and the argument does not apply here,
//! because these are not "dependencies somebody might not have thought to forbid". Each entry
//! names a crate that IS the decision: the whole content of the rule is that this one edge
//! stays absent. An allowlist would mean enumerating a legitimate tree of thirty crates to
//! express a fact about one of them, and then re-enumerating it every time an unrelated
//! dependency moved.

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
    // The canonical form of a definition set, and its hash. These are here because a review showed
    // the digest cannot be computed anywhere else: while `PinnedDefinitions::pin` took the hashing
    // FUNCTION from its caller, safe public code could pass `|_| Ok(elsewhere)` and pair any digest
    // with any definitions - which is the whole invariant `PinnedDefinitions` exists to hold. The
    // hash has to be the domain's own or it is not a guarantee.
    //
    // The cost was measured rather than estimated. Twelve crates transitively, and **no new
    // lockfile entry**: every one of them was already compiled into the shipped binary through
    // `sutura-catalog-local`, which is where this code used to live. This moves an edge, not a
    // dependency. None is a framework - no runtime, no client, no engine - which is the line the
    // doc comment above actually draws.
    "serde_json",
    "sha2",
    // What those two actually compile, confirmed against `cargo tree -p sutura-domain
    // --all-features` rather than assumed: `sha2` brings `cfg-if`, `cpufeatures` and `digest`
    // (which brings `block-buffer`, `crypto-common`, `hybrid-array`, `typenum`), and `serde_json`
    // brings `itoa`, `memchr` and `zmij`.
    "block-buffer",
    "cfg-if",
    "cpufeatures",
    "crypto-common",
    "digest",
    "hybrid-array",
    "itoa",
    "memchr",
    "typenum",
    "zmij",
    // Below here is a DIFFERENT KIND OF ENTRY, and the difference is worth keeping visible.
    //
    // `cargo tree -p sutura-domain --all-features` does not list one of these. They are in this
    // list because this gate walks `cargo metadata`'s whole-workspace resolve graph, which
    // includes every optional edge any crate in the workspace enables and every target's
    // platform-specific ones. So they are what the domain links against when the WORKSPACE is
    // built, not what it needs:
    //
    //   * the `indexmap` stack arrives because `utoipa` enables `serde_json/preserve_order` for
    //     deterministic OpenAPI output, and feature unification applies that to the one
    //     `serde_json` in the graph - including the domain's;
    //   * `const-oid` is `digest`'s optional `oid` feature, on the same mechanism;
    //   * `libc` is declared by `cpufeatures` for `aarch64-linux` only, and appears because
    //     metadata resolves every target rather than the one being built.
    //
    // None of them is reachable from `sutura-domain`'s own code, and the fast inner loop
    // `AGENTS.md` cites - `cargo check -p sutura-domain --no-default-features` - does not compile
    // them. Left allowlisted rather than filtered out of the gate: a gate that reasoned about
    // which edges are "really" enabled would be a second, subtler feature resolver, and being
    // over-broad here fails safe. If one of these ever becomes a framework, this list is where
    // the argument happens.
    "allocator-api2",
    "const-oid",
    "equivalent",
    "foldhash",
    "hashbrown",
    "indexmap",
    "libc",
    // The Postgres driver's SCRAM client enables `digest`'s `mac` feature, which pulls its two
    // constant-time helpers into the one `digest` the workspace shares - the same whole-workspace
    // feature unification as `const-oid` above. `sutura-domain` hashes with `sha2` and calls none
    // of this: the inner loop is `cargo check -p sutura-domain --no-default-features`, which
    // compiles neither.
    "cmov",
    "ctutils",
];

const DOMAIN: &str = "sutura-domain";

/// One crate that must not be reachable from another, and what to do about it.
struct ForbiddenEdge {
    /// The crate whose transitive tree is walked.
    from: &'static str,
    /// The crate that must not appear in it.
    forbidden: &'static str,
    /// Why the edge is forbidden. Printed, because a rule whose reason is unstated gets
    /// reverted by the next person who needs the edge for twenty minutes.
    why: &'static str,
    /// What to do instead. Printed, because a gate that only says "no" gets worked around.
    instead: &'static str,
}

/// Edges that must stay absent.
///
/// **Adding, removing or widening an entry here is an architecture decision. That is the
/// point** - the same sentence [`ALLOWED_IN_DOMAIN`] carries, for the same reason: the diff is
/// where the argument happens.
const FORBIDDEN_EDGES: &[ForbiddenEdge] = &[
    // Rendering is not the compiler's business, and this is the half that a comment could not
    // hold. `compile` already stopped at a `QueryPlan` - the `Warehouse` port carries a plan, so
    // an adapter that executes over Arrow renders nothing - but `generate` and `dialect` were
    // still `pub` modules OF the core. The consequence was in the closure rather than in the
    // call graph: `sutura-serve -> sutura-http -> sutura-app -> sutura-semantic -> polyglot-sql`
    // put a pre-1.0 SQL generator, with three enumerated lowering gaps, into the network binary,
    // which renders nothing and can reach none of it.
    ForbiddenEdge {
        from: "sutura-semantic",
        forbidden: "polyglot-sql",
        why: "the core compiles a question into a plan and renders nothing. A SQL generator in \
              its tree is one every consumer of the core links, including a build that only ever \
              executes plans on the engine",
        instead: "put the rendering in `sutura-sql` and depend on THAT from the SQL adapter that \
                  needs it. `sutura-exec-duckdb` and `sutura-cli` do",
    },
    // The re-entry path, and the reason this is two entries rather than one. Nothing stops
    // somebody adding `sutura-sql` to `sutura-semantic`'s manifest to "share" a type - and that
    // reintroduces the edge above transitively, with no line naming `polyglot-sql` anywhere for a
    // reviewer to notice.
    ForbiddenEdge {
        from: "sutura-semantic",
        forbidden: "sutura-sql",
        why: "it is the same edge one hop further out: `sutura-sql` carries the generator, so \
              reaching it puts the generator back in the core's closure",
        instead: "the two crates are siblings and neither needs the other. If a type genuinely \
                  belongs to both, it belongs in `sutura-domain`, which is where `QueryPlan` and \
                  `ParamValue` already are. A type only the renderer uses belongs in `sutura-sql`, \
                  which is where `GeneratedQuery` went",
    },
];

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
    // Every half runs even when an earlier one fails. They are independent findings, and a gate
    // that stops early makes the second violation look like it appeared after the first fix.
    let direction = dependency_direction();
    let edges = forbidden_edges();
    let surface = typed_surface();
    if direction == Verdict::Pass && edges == Verdict::Pass && surface == Verdict::Pass {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

/// Which way dependencies point, the other way round: a named crate cannot reach a named crate.
fn forbidden_edges() -> Verdict {
    // `--all-features` for the reason [`dependency_direction`] uses it, and one more that is
    // specific to this half: a dependency moved behind a feature is still a dependency, and the
    // edge this forbids would be trivial to hide behind one.
    let meta = match crate::cargo_metadata(&["--all-features"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return Verdict::Fail;
        }
    };

    let mut failed = false;
    for edge in FORBIDDEN_EDGES {
        // Walked per entry rather than once, because `from` differs per rule and a missing
        // `from` has to be an error rather than a vacuous pass: a renamed crate would
        // otherwise silently switch the rule off.
        let tree = match transitive_names(&meta, edge.from) {
            Ok(names) => names,
            Err(message) => {
                eprintln!("xtask check-boundaries: {message}");
                failed = true;
                continue;
            }
        };
        if !tree.contains(edge.forbidden) {
            println!(
                "xtask check-boundaries: ok - {} does not reach {} ({} crate(s) in its tree)",
                edge.from,
                edge.forbidden,
                tree.len()
            );
            continue;
        }
        failed = true;
        eprintln!(
            "xtask check-boundaries: FAILED - {} reaches {}, and it may not.",
            edge.from, edge.forbidden
        );
        eprintln!();
        eprintln!("  Why: {}", edge.why);
        eprintln!("  Do:  {}", edge.instead);
        eprintln!();
        eprintln!(
            "  `cargo tree -p {} -e normal --invert {}` names the edge.",
            edge.from, edge.forbidden
        );
        eprintln!("  If the edge genuinely belongs, the entry in FORBIDDEN_EDGES is what has to");
        eprintln!("  go, and that is an architecture decision: it should be a visible diff with");
        eprintln!("  the argument in it, not a dependency somebody added on the way past.");
        eprintln!();
    }
    if failed { Verdict::Fail } else { Verdict::Pass }
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

    use super::{ALLOWED_IN_DOMAIN, FORBIDDEN_EDGES, transitive_names, violations};

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

    #[test]
    fn a_forbidden_edge_is_caught_transitively() {
        // The case a manifest grep misses, and the one the second entry in `FORBIDDEN_EDGES`
        // exists for: `sutura-semantic` names `sutura-sql`, `sutura-sql` names the generator, and
        // no line anywhere in the core's manifest says `polyglot-sql`.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{
                "packages": [
                    {"id": "sem", "name": "sutura-semantic"},
                    {"id": "sql", "name": "sutura-sql"},
                    {"id": "pg", "name": "polyglot-sql"}
                ],
                "resolve": {"nodes": [
                    {"id": "sem", "deps": [{"pkg": "sql"}]},
                    {"id": "sql", "deps": [{"pkg": "pg"}]},
                    {"id": "pg", "deps": []}
                ]}
            }"#,
        )
        .expect("fixture parses");
        let tree = transitive_names(&meta, "sutura-semantic").expect("walk succeeds");
        for edge in FORBIDDEN_EDGES {
            assert!(tree.contains(edge.forbidden), "{} was not seen in the tree", edge.forbidden);
        }
    }

    #[test]
    fn every_forbidden_edge_says_what_to_do_instead() {
        // A gate that only says "no" gets worked around, so the message is part of the rule
        // rather than a courtesy. Asserted rather than reviewed: an entry added with an empty
        // `instead` prints a blank line where the fix should be.
        for edge in FORBIDDEN_EDGES {
            assert!(
                !edge.from.is_empty() && !edge.forbidden.is_empty(),
                "an edge names two crates"
            );
            assert!(!edge.why.is_empty(), "{} -> {} has no reason", edge.from, edge.forbidden);
            assert!(!edge.instead.is_empty(), "{} -> {} has no fix", edge.from, edge.forbidden);
        }
    }
}
