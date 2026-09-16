//! The application crate reaches no data-system adapter over a NORMAL edge.
//!
//! **A gap this repository's own convention never closed.** `AGENTS.md`'s layout table, the
//! `crate-map` skill and `sutura-app`'s own `warehouses` module all say `-app` composes ports and
//! holds no adapter - "which adapters a process holds is a property of the BUILD" - but neither
//! [`super::FORBIDDEN_EDGES`] (a denylist naming two crates) nor [`super::adapters`] (an
//! intra-class rule comparing two members of the SAME class) can express it: `FORBIDDEN_EDGES` has
//! no row with `sutura-app` as `from`, and `sutura-app` belongs to none of `adapters`'s three
//! classes, so an edge from it into any `sutura-exec-*` crate passes both checks silently.
//! Measured at `telekom/sutura#112`'s own review: `sutura-app -> sutura-exec-*` reaches nothing
//! today, and this module is what keeps that a fact rather than a convention.
//!
//! **A third shape, and it is why this is its own module rather than a row in either sibling.**
//! [`super::FORBIDDEN_EDGES`] names two crates; [`super::adapters`] compares members of one named
//! class against each other. This names ONE crate against a class defined by PREFIX - closer to
//! `super::adapters`'s shape than to a denylist entry, but asymmetric where that rule is
//! symmetric: `sutura-app` is not itself a data-system adapter, so it never joins the class being
//! walked, only the crate the walk must not reach.
//!
//! **Dev-dependencies are exempt, for [`super::adapters`]'s own reason.** `sutura-app`'s
//! differential suite dev-depends on four adapters to compare a two-source answer against a
//! one-source one - the strongest evidence this repository has for federation - and forbidding
//! that would forbid exactly the tests `telekom/sutura#112`'s own differential adds. [`Edges::Normal`]
//! is the walk that leaves those alone while still catching a NORMAL dependency, which is the one
//! that ships: `sutura-app`'s own `[dependencies]` names `sutura-domain`, `sutura-semantic` and
//! `thiserror`, and nothing else, so this check is provable false today and stays that way only by
//! staying a gate rather than a sentence.

use super::{Edges, transitive_names};

/// The crate this rule watches. Named once, so a rename of the crate is one line here rather than
/// a silent hole - the same failure mode [`super::adapters`]'s own header names for a class prefix.
pub(crate) const APPLICATION: &str = "sutura-app";

/// What joins the forbidden class - every data-system adapter, by the SAME prefix
/// `super::adapters::CLASSES` uses for its "data systems" class. Not shared as one constant: the
/// two rules ask different questions (a class against itself; one crate against a class) and each
/// keeps its own copy of the fact it depends on, so a reviewer changing one sees the whole of what
/// it means rather than a name that also moves a sibling rule.
const ADAPTER_PREFIX: &str = "sutura-exec-";

/// What the check found.
pub(crate) struct Report {
    /// How many crates were in `sutura-app`'s normal tree - printed on success, so a walk that
    /// suddenly reads zero (the crate renamed, or metadata stopped resolving) is visible rather
    /// than silently passing because it checked nothing.
    pub(crate) tree_size: usize,
    /// One line per adapter reached, already formatted for stderr.
    pub(crate) problems: Vec<String>,
}

/// Walks `sutura-app`'s normal-dependency tree and refuses any `sutura-exec-*` name in it.
pub(crate) fn check(meta: &serde_json::Value) -> Result<Report, String> {
    let tree = transitive_names(meta, APPLICATION, Edges::Normal)?;
    let problems = tree
        .iter()
        .filter(|name| name.starts_with(ADAPTER_PREFIX))
        .map(|name| format!("{APPLICATION} -> {name}: a normal dependency from the application onto a data-system adapter"))
        .collect();
    Ok(Report {
        tree_size: tree.len(),
        problems,
    })
}

/// The argument, printed once, the same shape every sibling half in this gate prints.
pub(crate) fn explain() {
    eprintln!("  Why: which adapters a process holds is a property of the BUILD, decided at a composition");
    eprintln!("  root (crates/sutura-cli, crates/sutura-http) - not by the application crate importing");
    eprintln!("  one. `sutura-app` composes the driving port over whichever adapter a root hands it;");
    eprintln!("  a normal edge onto one adapter would be the first this crate ever took.");
    eprintln!("  Do:  build the concrete, per-source registry at the composition root (one call per source,");
    eprintln!("  against that source's own `Warehouse::IMPERSONATION`), then erase it with");
    eprintln!("  `sutura_app::Warehouses::into_mapped` - generic in the adapter type, so this crate");
    eprintln!("  never has to name one to offer it.");
}

#[cfg(test)]
mod tests {
    use super::{check, explain};

    fn meta(edges: &str) -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{
                "packages": [
                    {{"id": "app", "name": "sutura-app"}},
                    {{"id": "exec", "name": "sutura-exec-datafusion"}},
                    {{"id": "domain", "name": "sutura-domain"}}
                ],
                "resolve": {{"nodes": [
                    {{"id": "app", "deps": [{{"pkg": "domain"}}{edges}]}},
                    {{"id": "exec", "deps": []}},
                    {{"id": "domain", "deps": []}}
                ]}}
            }}"#
        ))
        .expect("fixture parses")
    }

    #[test]
    fn a_tree_with_no_adapter_passes() {
        let report = check(&meta("")).expect("walk succeeds");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.tree_size, 1, "domain alone");
    }

    #[test]
    fn a_normal_edge_onto_an_adapter_is_a_violation() {
        let report = check(&meta(r#", {"pkg": "exec"}"#)).expect("walk succeeds");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("sutura-exec-datafusion"), "{:?}", report.problems);
    }

    #[test]
    fn a_dev_only_edge_onto_an_adapter_is_not_a_violation() {
        // The differential suite's own shape: `sutura-app` dev-depends on an adapter to compare
        // answers, and that must not trip this rule - `Edges::Normal` is what exempts it.
        let report = check(&meta(r#", {"pkg": "exec", "dep_kinds": [{"kind": "dev"}]}"#)).expect("walk succeeds");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }

    #[test]
    fn the_real_tree_has_no_such_edge() {
        // Non-vacuous over the actual workspace: if this ever finds zero crates in the tree, the
        // fixture tests above are the only evidence this rule does anything.
        let Ok(meta) = crate::cargo_metadata(&["--all-features"]) else {
            return;
        };
        let report = check(&meta).expect("the real tree resolves");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(report.tree_size > 0, "sutura-app's own tree is not empty");
    }

    #[test]
    fn explain_does_not_panic() {
        explain();
    }
}
