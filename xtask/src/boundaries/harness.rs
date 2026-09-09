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
//! A denylist catches only named adapters. The all-feature closure admits the domain plus the
//! compiler and renderer; the latter two must be optional behind the default-off `compile` feature.
//! This checks Cargo's declaration, not an individual consumer's resolved feature set or build cost.
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
/// ADR0012's compile family needs the compiler and renderer, but no port implementor. Their
/// declaration is checked below; merely adding them to this all-feature allowlist is insufficient.
const PERMITTED: &[&str] = &["sutura-domain", "sutura-semantic", "sutura-sql"];

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

    let mut problems = compile_feature(meta)?;
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

    Ok(Report {
        closure: closure.len(),
        first_party,
        problems,
    })
}

/// The one feature shape this harness declares. Cargo parses the manifest; no TOML or feature
/// expression parser lives here. A new feature is a deliberate extension of this closed contract.
fn compile_feature(meta: &serde_json::Value) -> Result<Vec<String>, String> {
    let packages = meta["packages"].as_array().ok_or("metadata has no packages")?;
    let mut harnesses = packages.iter().filter(|package| package["name"].as_str() == Some(HARNESS));
    let harness = harnesses.next().ok_or("metadata has no conformance harness")?;
    if harnesses.next().is_some() {
        return Err(String::from("metadata names more than one conformance harness"));
    }
    let dependencies = harness["dependencies"]
        .as_array()
        .ok_or("harness has no dependency declarations")?;
    let features = harness["features"].as_object().ok_or("harness has no feature declarations")?;
    let expected = ["dep:sutura-semantic", "dep:sutura-sql", "dep:serde_json"];
    let compile = features.get("compile").and_then(serde_json::Value::as_array);
    let mut problems = Vec::new();
    if features.len() != 2
        || features.get("default") != Some(&serde_json::json!([]))
        || !compile.is_some_and(|tokens| {
            tokens.len() == expected.len()
                && expected
                    .iter()
                    .all(|expected| tokens.iter().filter(|token| token.as_str() == Some(expected)).count() == 1)
        })
    {
        problems.push(String::from(
            "sutura-conformance must declare only empty default and the compile dependency feature",
        ));
    }
    for name in ["sutura-semantic", "sutura-sql", "serde_json"] {
        let mut declarations = dependencies
            .iter()
            .filter(|dependency| dependency["name"].as_str() == Some(name));
        let optional = declarations.next().is_some_and(|dependency| {
            dependency.get("kind") == Some(&serde_json::Value::Null)
                && dependency["optional"].as_bool() == Some(true)
                && dependency.get("rename") == Some(&serde_json::Value::Null)
                && dependency.get("target") == Some(&serde_json::Value::Null)
        });
        if !optional || declarations.next().is_some() {
            problems.push(format!(
                "{HARNESS} must declare {name} once as an unrenamed optional normal dependency"
            ));
        }
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
    eprintln!("  If it genuinely belongs, `PERMITTED` and the compile feature contract must change;");
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
                serde_json::json!({
                    "id": format!("id-{name}"), "name": name,
                    "features": {"default": [], "compile": ["dep:sutura-semantic", "dep:sutura-sql", "dep:serde_json"]},
                    "dependencies": (["sutura-semantic", "sutura-sql", "serde_json"].map(|dependency| serde_json::json!({
                        "name": dependency, "kind": null, "optional": true, "rename": null, "target": null,
                    }))),
                })
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

    /// All features admit compiler and renderer, but still no implementor.
    #[test]
    fn a_compile_harness_reaches_domain_compiler_and_renderer_only() {
        let meta = metadata(&[
            (HARNESS, &["sutura-domain", "sutura-semantic", "sutura-sql", "thiserror"]),
            ("sutura-domain", &[]),
            ("sutura-semantic", &["sutura-domain"]),
            ("sutura-sql", &["sutura-domain"]),
            ("thiserror", &[]),
        ]);
        let report = check(&meta).expect("the fixture resolves");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.first_party, ["sutura-domain", "sutura-semantic", "sutura-sql"]);
    }

    /// **The mutation this half exists for**, and the one every other gate in this repository
    /// passed: an adapter under `[dependencies]`.
    #[test]
    fn a_harness_that_reaches_an_adapter_fails() {
        let meta = metadata(&[
            (
                HARNESS,
                &["sutura-domain", "sutura-semantic", "sutura-sql", "sutura-exec-duckdb"],
            ),
            ("sutura-domain", &[]),
            ("sutura-semantic", &[]),
            ("sutura-sql", &[]),
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
            (HARNESS, &["sutura-domain", "sutura-semantic", "sutura-sql"]),
            ("sutura-domain", &[]),
            ("sutura-semantic", &[]),
            ("sutura-sql", &["sutura-exec-duckdb"]),
            ("sutura-exec-duckdb", &[]),
        ]);
        let report = check(&meta).expect("the fixture resolves");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("sutura-exec-duckdb"));
    }

    /// It fails CLOSED: a harness that no longer reaches the interior is a rule that has stopped
    /// checking anything, not a clean tree.
    #[test]
    fn a_harness_that_reaches_nothing_first_party_fails_rather_than_passing() {
        let meta = metadata(&[(HARNESS, &["thiserror"]), ("thiserror", &[])]);
        let report = check(&meta).expect("the fixture resolves");
        assert_eq!(report.problems.len(), 3, "{:?}", report.problems);
        assert!(report.problems[0].contains("no longer reaches"), "{:?}", report.problems);
    }

    /// And a tree with no harness in it is an error rather than a pass.
    #[test]
    fn a_missing_harness_is_an_error() {
        let meta = metadata(&[("sutura-domain", &[])]);
        assert!(check(&meta).is_err());
    }

    /// This reaches the same check as the real all-features metadata. The base allowlist refuses
    /// the clean graph, so that positive row is a base-compatible behavioral RED; the declaration
    /// refusals additionally need mutants once the new edges are allowed.
    #[test]
    fn compile_dependencies_are_optional_and_default_off_not_merely_allowlisted() {
        let valid = metadata(&[
            (HARNESS, &["sutura-domain", "sutura-semantic", "sutura-sql"]),
            ("sutura-domain", &[]),
            ("sutura-semantic", &[]),
            ("sutura-sql", &[]),
        ]);
        assert!(check(&valid).expect("valid metadata").problems.is_empty());
        for (field, value) in [
            ("default", serde_json::json!(["compile"])),
            ("compile", serde_json::json!(["dep:sutura-semantic", "dep:sutura-sql"])),
            (
                "compile",
                serde_json::json!(["dep:sutura-semantic", "dep:sutura-sql", "dep:sutura-sql"]),
            ),
            ("extra", serde_json::json!(["dep:sutura-semantic"])),
        ] {
            let mut meta = valid.clone();
            meta["packages"][0]["features"][field] = value;
            let report = check(&meta).expect("metadata resolves");
            assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
            assert!(report.problems[0].contains("feature"));
        }
        for index in 0..3 {
            for (field, value) in [
                ("optional", serde_json::json!(false)),
                ("kind", serde_json::json!("dev")),
                ("rename", serde_json::json!("alias")),
                ("target", serde_json::json!("cfg(unix)")),
            ] {
                let mut meta = valid.clone();
                meta["packages"][0]["dependencies"][index][field] = value;
                let report = check(&meta).expect("metadata resolves");
                assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
                assert!(report.problems[0].contains("optional normal dependency"));
            }
        }
        for duplicate in [false, true] {
            let mut meta = valid.clone();
            let declarations = meta["packages"][0]["dependencies"].as_array_mut().expect("dependencies");
            let last = declarations.pop().expect("declaration");
            if duplicate {
                declarations.push(last.clone());
                declarations.push(last);
            }
            assert_eq!(check(&meta).expect("metadata resolves").problems.len(), 1);
        }
        for field in ["dependencies", "features"] {
            let mut meta = valid.clone();
            meta["packages"][0].as_object_mut().expect("package").remove(field);
            assert!(check(&meta).is_err());
        }
    }
}
