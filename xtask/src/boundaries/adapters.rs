//! An adapter never calls another adapter, for the adapters that do the same job.
//!
//! A hexagonal rule that was *review* in `.agents/skills/engineering/rust/SKILL.md`, and the input
//! it needs is the one `check-boundaries` already reads. **The DEFINITION is the whole of the work
//! here, and it is deliberately narrower than the sentence:** `sutura-sql` is a renderer that data
//! adapters legitimately depend on, `sutura-runtime` is process-global machinery both transports
//! take, and a composition root names every adapter it opens - so a rule against "an adapter
//! depending on an adapter" would forbid three shapes this workspace is built out of.
//!
//! What is forbidden is an edge **inside one class**: two crates that do the same job at the same
//! boundary, one of which becomes the other's library. That is the direction with no honest
//! reading, because each is one way to reach one kind of thing: they are selected between rather
//! than composed, and the seam they share is the port and not each other.
//!
//! **Dev-dependencies are exempt, and that is not a loophole.** A dev-dependency between two data
//! adapters is how a corpus reaches a real system: `sutura-exec-bigquery` dev-depends on
//! `sutura-exec-datafusion` precisely so a question's rows can be compared against the engine's,
//! and `sutura-app`'s golden suite dev-depends on four adapters for the same reason. Forbidding
//! that would forbid the differential suite, which is the strongest evidence in this repository.
//! So the walk here follows [`Edges::Normal`] where the two halves next door follow every edge -
//! and the reason each is right is written where the enum is.
//!
//! **The classes are NAMED rather than guessed**, and a class that loses every member fails: a
//! renamed prefix would otherwise switch the rule off silently, which is this gate's own worst
//! failure mode. A class with ONE member cannot be violated, and the verdict prints the member
//! count per class rather than hiding that - read those counts rather than a sentence here, which
//! named the one-member class until a second metadata provider arrived and made it wrong.
//!
//! Two limits. It reads the dependency GRAPH, so an adapter reaching another through a re-export
//! it does not declare is not a thing cargo permits and not a thing this checks. And a crate joins
//! a class by NAME: an adapter called something else is outside every class until somebody adds it
//! here, which is the same cost `FORBIDDEN_EDGES` pays for being a denylist.

use std::collections::BTreeSet;

use super::{Edges, transitive_names};

/// How a crate joins a class.
enum Membership {
    /// Every workspace member whose name starts with this.
    Prefix(&'static str),
    /// Exactly these workspace members.
    Named(&'static [&'static str]),
}

/// One class of adapter: crates doing the same job at the same boundary.
struct AdapterClass {
    /// Human name, for the message.
    name: &'static str,
    /// What puts a crate in it.
    members: Membership,
    /// Why an edge inside the class is wrong. Printed, because a rule whose reason is unstated
    /// gets reverted by the next person who needs the edge for twenty minutes.
    why: &'static str,
    /// What to do instead. Printed, because a gate that only says "no" gets worked around.
    instead: &'static str,
}

/// The classes. **Adding, widening or removing one is an architecture decision. That is the
/// point** - the same sentence `ALLOWED_IN_DOMAIN` and `FORBIDDEN_EDGES` carry, for the same
/// reason: the diff is where the argument happens.
const CLASSES: &[AdapterClass] = &[
    AdapterClass {
        name: "data systems",
        members: Membership::Prefix("sutura-exec-"),
        why: "each of these is one way to reach one data system, and a composition root SELECTS \
              between them rather than composing them. An edge here makes one adapter the other's \
              library, so a build that wanted one links both - and the engine that ships is the \
              one that must not acquire a SQL renderer or a network client it cannot use",
        instead: "the shared rendering is `sutura-sql`, which every SQL adapter depends on and the \
                  engine does not. A type both genuinely need belongs in `sutura-domain`, where \
                  `QueryPlan` and `RowSet` already are. If what you want is to COMPARE two \
                  adapters, that is a dev-dependency and this rule does not touch it",
    },
    AdapterClass {
        name: "metadata providers",
        members: Membership::Prefix("sutura-catalog-"),
        why: "the same reason as the data systems above: a catalog adapter supplies the model from \
              one kind of source, and `docs/adr/0016` makes which tests it faces a property of its \
              own declaration rather than of another adapter's",
        instead: "the `SemanticCatalog` port is the seam. A declaring adapter that needs part of \
                  another's model is a composition decision, which belongs in a composition root",
    },
    AdapterClass {
        name: "transports",
        members: Membership::Named(&["sutura-http", "sutura-mcp"]),
        why: "the workspace manifest already states this one as the reason `sutura-mcp` exists as \
              a second transport rather than a layer on the first: it depends on `sutura-app`'s \
              driving port and on nothing in `sutura-http`, because an adapter reaching into \
              another adapter is the one direction this layout forbids",
        instead: "a wire type per transport, and the cost is paid deliberately - \
                  `crates/sutura-mcp/src/wire.rs` says so at length. What keeps the two \
                  descriptions equal is `sutura_app::Capability`, one declaration both read",
    },
];

/// What the check looked at, and what it found.
pub(super) struct Report {
    /// Class name and how many workspace members it has. Printed on success, so a class that has
    /// stopped matching anything is visible in the verdict rather than only in a diff.
    pub(super) sizes: Vec<(&'static str, usize)>,
    /// One line per violation, already formatted for stderr.
    pub(super) problems: Vec<String>,
}

/// Every class, walked.
pub(super) fn check(meta: &serde_json::Value) -> Result<Report, String> {
    let members = workspace_members(meta)?;
    let mut sizes = Vec::new();
    let mut problems = Vec::new();
    for class in CLASSES {
        let in_class = class_members(&members, class);
        if in_class.is_empty() {
            // The failure this gate is most exposed to: a renamed prefix leaves the rule green and
            // checking nothing. An empty class is therefore an error, not a pass.
            problems.push(format!(
                "the `{}` class matches no workspace member - the names moved and this rule went quiet",
                class.name
            ));
            continue;
        }
        sizes.push((class.name, in_class.len()));
        problems.extend(edges_inside(meta, class, &in_class)?);
    }
    Ok(Report { sizes, problems })
}

/// Normal-dependency edges from one member of `class` to another.
fn edges_inside(meta: &serde_json::Value, class: &AdapterClass, in_class: &[String]) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    for from in in_class {
        let tree = transitive_names(meta, from, Edges::Normal)?;
        for to in in_class {
            if to != from && tree.contains(to) {
                problems.push(format!(
                    "{from} -> {to}: two `{}` adapters, and a normal dependency between them",
                    class.name
                ));
            }
        }
    }
    Ok(problems)
}

/// Workspace member names.
///
/// Members ONLY, so a third-party crate that happened to share a prefix could not join a class,
/// and so the walk starts somewhere `transitive_names` can find.
pub(super) fn workspace_members(meta: &serde_json::Value) -> Result<BTreeSet<String>, String> {
    let packages = meta
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;
    let ids: BTreeSet<&str> = meta
        .get("workspace_members")
        .and_then(|m| m.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `workspace_members` array"))?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let names: BTreeSet<String> = packages
        .iter()
        .filter(|package| package.get("id").and_then(|i| i.as_str()).is_some_and(|id| ids.contains(id)))
        .filter_map(|package| package.get("name")?.as_str().map(String::from))
        .collect();
    if names.is_empty() {
        return Err(String::from(
            "cargo metadata named no workspace member - this rule would check nothing",
        ));
    }
    Ok(names)
}

/// The members of one class, in a stable order.
fn class_members(members: &BTreeSet<String>, class: &AdapterClass) -> Vec<String> {
    match class.members {
        Membership::Prefix(prefix) => members.iter().filter(|name| name.starts_with(prefix)).cloned().collect(),
        Membership::Named(named) => members
            .iter()
            .filter(|name| named.contains(&name.as_str()))
            .cloned()
            .collect(),
    }
}

/// Printed when this half fails, because a rule whose reason is unstated gets reverted.
pub(super) fn explain(problems: &[String]) {
    eprintln!("An adapter never calls another adapter, and what that means here is an edge INSIDE");
    eprintln!("one class - two crates doing the same job at the same boundary, one of which becomes");
    eprintln!("the other's library. `sutura-sql`, `sutura-runtime` and a composition root are all");
    eprintln!("legitimate cross-adapter edges and none of them is in a class with the crate it");
    eprintln!("serves. Dev-dependencies are exempt: that is how a corpus reaches a real system.");
    eprintln!();
    for class in CLASSES {
        if !problems.iter().any(|problem| problem.contains(class.name)) {
            continue;
        }
        eprintln!("  {}:", class.name);
        eprintln!("    Why: {}", class.why);
        eprintln!("    Do:  {}", class.instead);
        eprintln!();
    }
    eprintln!("If a class here is wrong, the entry in boundaries/adapters.rs is what has to change,");
    eprintln!("and that is an architecture decision: it should be a visible diff with the argument");
    eprintln!("in it, not a dependency somebody added on the way past.");
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{AdapterClass, CLASSES, Membership, check, class_members, workspace_members};

    /// Two data adapters, two transports, one catalog and the crates that legitimately sit between
    /// them - shaped like `cargo metadata --all-features`.
    fn metadata(edges: &str) -> serde_json::Value {
        let packages = [
            "sutura-domain",
            "sutura-sql",
            "sutura-app",
            "sutura-catalog-local",
            "sutura-exec-duckdb",
            "sutura-exec-bigquery",
            "sutura-http",
            "sutura-mcp",
        ]
        .iter()
        .map(|name| format!(r#"{{"id": "{name}", "name": "{name}"}}"#))
        .collect::<Vec<String>>()
        .join(",");
        let members = [
            "sutura-domain",
            "sutura-sql",
            "sutura-app",
            "sutura-catalog-local",
            "sutura-exec-duckdb",
            "sutura-exec-bigquery",
            "sutura-http",
            "sutura-mcp",
        ]
        .iter()
        .map(|name| format!(r#""{name}""#))
        .collect::<Vec<String>>()
        .join(",");
        serde_json::from_str(&format!(
            r#"{{"packages": [{packages}], "workspace_members": [{members}], "resolve": {{"nodes": [{edges}]}}}}"#
        ))
        .expect("fixture parses")
    }

    /// One resolve node, with every named dependency as a normal edge.
    fn normal(id: &str, to: &[&str]) -> String {
        let deps = to
            .iter()
            .map(|name| format!(r#"{{"pkg": "{name}", "dep_kinds": [{{"kind": null, "target": null}}]}}"#))
            .collect::<Vec<String>>()
            .join(",");
        format!(r#"{{"id": "{id}", "deps": [{deps}]}}"#)
    }

    /// One resolve node, with every named dependency as a DEV edge.
    fn dev(id: &str, to: &[&str]) -> String {
        let deps = to
            .iter()
            .map(|name| format!(r#"{{"pkg": "{name}", "dep_kinds": [{{"kind": "dev", "target": null}}]}}"#))
            .collect::<Vec<String>>()
            .join(",");
        format!(r#"{{"id": "{id}", "deps": [{deps}]}}"#)
    }

    /// The tree as it actually is: every adapter on the domain, the SQL ones on the renderer, the
    /// transports on the application.
    fn as_it_is() -> Vec<String> {
        vec![
            normal("sutura-domain", &[]),
            normal("sutura-sql", &["sutura-domain"]),
            normal("sutura-app", &["sutura-domain"]),
            normal("sutura-catalog-local", &["sutura-domain"]),
            normal("sutura-exec-duckdb", &["sutura-domain", "sutura-sql"]),
            normal("sutura-exec-bigquery", &["sutura-domain", "sutura-sql"]),
            normal("sutura-http", &["sutura-app", "sutura-domain"]),
            normal("sutura-mcp", &["sutura-app", "sutura-domain"]),
        ]
    }

    fn report(nodes: &[String]) -> Vec<String> {
        check(&metadata(&nodes.join(","))).expect("the fixture walks").problems
    }

    #[test]
    fn the_shape_this_workspace_actually_has_is_clean() {
        assert!(report(&as_it_is()).is_empty(), "the real workspace shape has no adapter-boundary problems");
    }

    #[test]
    fn an_adapter_depending_on_an_adapter_fails_the_boundary_gate() {
        let mut nodes = as_it_is();
        nodes.retain(|node| !node.contains(r#""id": "sutura-exec-bigquery""#));
        nodes.push(normal(
            "sutura-exec-bigquery",
            &["sutura-domain", "sutura-sql", "sutura-exec-duckdb"],
        ));
        let found = report(&nodes);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found.first().is_some_and(|problem| problem.contains("data systems")),
            "{found:?}"
        );
    }

    #[test]
    fn a_transport_depending_on_a_transport_fails_too() {
        let mut nodes = as_it_is();
        nodes.retain(|node| !node.contains(r#""id": "sutura-mcp""#));
        nodes.push(normal("sutura-mcp", &["sutura-app", "sutura-domain", "sutura-http"]));
        let found = report(&nodes);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found.first().is_some_and(|problem| problem.contains("transports")),
            "{found:?}"
        );
    }

    #[test]
    fn the_edge_is_caught_transitively() {
        // The case a manifest grep misses: bigquery names the renderer, the renderer names duckdb,
        // and no line in bigquery's own manifest says `sutura-exec-duckdb`.
        let mut nodes = as_it_is();
        nodes.retain(|node| !node.contains(r#""id": "sutura-sql""#));
        nodes.push(normal("sutura-sql", &["sutura-domain", "sutura-exec-duckdb"]));
        assert_eq!(report(&nodes).len(), 1);
    }

    #[test]
    fn a_dev_dependency_between_two_adapters_is_how_a_corpus_reaches_a_real_system() {
        // The real edge in this workspace, and the reason the walk is normal-only: bigquery
        // dev-depends on the engine so a question's rows can be compared against it.
        let mut nodes = as_it_is();
        nodes.retain(|node| !node.contains(r#""id": "sutura-exec-bigquery""#));
        nodes.push(normal("sutura-exec-bigquery", &["sutura-domain", "sutura-sql"]));
        nodes.push(dev("sutura-exec-bigquery", &["sutura-exec-duckdb"]));
        assert!(report(&nodes).is_empty(), "{:?}", report(&nodes));
    }

    #[test]
    fn a_shared_renderer_is_not_an_adapter_edge() {
        // Both SQL adapters depend on `sutura-sql` and it is in no class, which is the whole
        // reason the rule is per class rather than "an adapter depending on an adapter".
        assert!(report(&as_it_is()).is_empty(), "the real workspace shape has no adapter-boundary problems");
    }

    #[test]
    fn a_class_that_matches_nothing_is_a_failure_and_not_a_pass() {
        // A renamed prefix would otherwise switch the rule off with a green run.
        let meta: serde_json::Value = serde_json::from_str(
            r#"{"packages": [{"id": "a", "name": "renamed-domain"}], "workspace_members": ["a"], "resolve": {"nodes": [{"id": "a", "deps": []}]}}"#,
        )
        .expect("fixture parses");
        let found = check(&meta).expect("the fixture walks").problems;
        assert_eq!(found.len(), CLASSES.len(), "{found:?}");
        assert!(found.iter().all(|problem| problem.contains("went quiet")), "{found:?}");
    }

    #[test]
    fn a_workspace_with_no_members_is_an_error_not_a_pass() {
        let meta: serde_json::Value =
            serde_json::from_str(r#"{"packages": [], "workspace_members": [], "resolve": {"nodes": []}}"#)
                .expect("fixture parses");
        drop(workspace_members(&meta).expect_err("must not pass vacuously"));
    }

    #[test]
    fn a_third_party_crate_sharing_a_prefix_does_not_join_a_class() {
        // Membership is workspace members only, so a registry crate called `sutura-exec-anything`
        // could not be dragged into a rule about this repository's own layout.
        let members: BTreeSet<String> = std::iter::once(String::from("sutura-exec-duckdb")).collect();
        let class = AdapterClass {
            name: "data systems",
            members: Membership::Prefix("sutura-exec-"),
            why: "x",
            instead: "y",
        };
        assert_eq!(class_members(&members, &class), vec![String::from("sutura-exec-duckdb")]);
    }

    #[test]
    fn every_class_says_what_to_do_instead() {
        // A gate that only says "no" gets worked around, so the message is part of the rule rather
        // than a courtesy - the `FORBIDDEN_EDGES` precedent next door.
        for class in CLASSES {
            assert!(!class.name.is_empty(), "a class has a name");
            assert!(!class.why.is_empty(), "{} has no reason", class.name);
            assert!(!class.instead.is_empty(), "{} has no fix", class.name);
        }
    }
}
