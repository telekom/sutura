//! `sutura-http-client` reaches no adapter, no composition root, no settings crate and no
//! transport over a NORMAL edge - the same third shape [`super::application`] holds for
//! `sutura-app`, one named crate against a class rather than a [`super::FORBIDDEN_EDGES`] denylist
//! row or an [`super::adapters`] intra-class comparison.
//!
//! **Why this crate needed its own row rather than inheriting `sutura-tls`'s.** The crate-map
//! skill's `sutura-tls` paragraph licenses an unprefixed crate with "no crypto provider, no
//! network client and no `sutura-config`" - and `sutura-http-client` carries `ureq`, which that
//! paragraph's own wording excludes. It joins none of `adapters::CLASSES` (no
//! `sutura-catalog-`/`sutura-exec-` prefix, and it is not a transport), so
//! [`super::adapter_classes`] cannot see an edge FROM it either - the identical gap
//! `github.com/telekom/sutura#929` found for `sutura-app` and closed with `application.rs`. This
//! module closes the same gap for the crate two catalog HTTP readers now share.
//!
//! **`Edges::Normal`, not `Edges::Every`.** The claim is about what a SHIPPED BINARY links; a
//! dev-only edge (a differential fixture comparing this crate against a catalog adapter's own
//! reader, say) would not be the edge this rule exists to forbid, the same reasoning
//! [`super::application`]'s own header gives for the identical choice.

use super::{Edges, transitive_names};

/// The crate this rule watches.
pub(crate) const SHARED_CLIENT: &str = "sutura-http-client";

/// Prefixes that join the forbidden class - the same two `super::adapters::CLASSES` names as
/// "data systems" and "metadata providers". Kept as its own copy for the reason
/// `application::ADAPTER_PREFIX` is: two rules asking different questions each keep the
/// fact they depend on, so a reviewer changing one sees the whole of what it means.
const FORBIDDEN_PREFIXES: &[&str] = &["sutura-exec-", "sutura-catalog-"];

/// Exact names that join the forbidden set beside the two prefixes: the two composition
/// roots/settings crates (`sutura-app`, `sutura-config`) neither adapter class prefix names, and
/// the two named transports (`super::adapters::CLASSES`'s "transports" class,
/// `Membership::Named(&["sutura-http", "sutura-mcp"])`) - round-2 review's finding that the class
/// this crate must not reach back into is bigger than the two prefixes alone cover.
const FORBIDDEN_NAMES: &[&str] = &["sutura-app", "sutura-config", "sutura-http", "sutura-mcp"];

/// What the check found.
pub(crate) struct Report {
    /// How many crates were in `sutura-http-client`'s normal tree - printed on success, the same
    /// reason [`super::application::Report::tree_size`] is.
    pub(crate) tree_size: usize,
    /// One line per forbidden crate reached, already formatted for stderr.
    pub(crate) problems: Vec<String>,
}

/// Walks `sutura-http-client`'s normal-dependency tree and refuses any adapter-class prefix or
/// named crate in it.
pub(crate) fn check(meta: &serde_json::Value) -> Result<Report, String> {
    let tree = transitive_names(meta, SHARED_CLIENT, Edges::Normal)?;
    let problems = tree
        .iter()
        .filter(|name| {
            FORBIDDEN_PREFIXES.iter().any(|prefix| name.starts_with(prefix)) || FORBIDDEN_NAMES.contains(&name.as_str())
        })
        .map(|name| {
            format!(
                "{SHARED_CLIENT} -> {name}: a normal dependency from the shared HTTP client onto an adapter or a composition root"
            )
        })
        .collect();
    Ok(Report {
        tree_size: tree.len(),
        problems,
    })
}

/// The argument, printed once, the same shape every sibling half in this gate prints.
pub(crate) fn explain() {
    eprintln!("  Why: this crate exists so two catalog HTTP readers of the SAME adapter class share");
    eprintln!("  one Endpoint/TLS implementation without becoming each other's library - the class");
    eprintln!("  rule `adapter_classes` already holds moves one layer out if this crate could reach");
    eprintln!("  back into an adapter, a composition root, or the settings crate that describes one.");
    eprintln!("  Do:  a type both a reader and this crate genuinely need belongs in `sutura-domain`,");
    eprintln!("  the same answer `sutura-tls`'s own header gives.");
}

#[cfg(test)]
mod tests {
    use super::{check, explain};

    fn meta(edges: &str) -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{
                "packages": [
                    {{"id": "client", "name": "sutura-http-client"}},
                    {{"id": "exec", "name": "sutura-exec-datafusion"}},
                    {{"id": "catalog", "name": "sutura-catalog-datahub"}},
                    {{"id": "app", "name": "sutura-app"}},
                    {{"id": "config", "name": "sutura-config"}},
                    {{"id": "http", "name": "sutura-http"}},
                    {{"id": "mcp", "name": "sutura-mcp"}},
                    {{"id": "tls", "name": "sutura-tls"}}
                ],
                "resolve": {{"nodes": [
                    {{"id": "client", "deps": [{{"pkg": "tls"}}{edges}]}},
                    {{"id": "exec", "deps": []}},
                    {{"id": "catalog", "deps": []}},
                    {{"id": "app", "deps": []}},
                    {{"id": "config", "deps": []}},
                    {{"id": "http", "deps": []}},
                    {{"id": "mcp", "deps": []}},
                    {{"id": "tls", "deps": []}}
                ]}}
            }}"#
        ))
        .expect("fixture parses")
    }

    #[test]
    fn a_tree_reaching_only_sutura_tls_passes() {
        let report = check(&meta("")).expect("walk succeeds");
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.tree_size, 1, "sutura-tls alone");
    }

    #[test]
    fn a_normal_edge_onto_a_data_system_adapter_is_a_violation() {
        let report = check(&meta(r#", {"pkg": "exec"}"#)).expect("walk succeeds");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("sutura-exec-datafusion"), "{:?}", report.problems);
    }

    #[test]
    fn a_normal_edge_onto_a_metadata_provider_is_a_violation() {
        let report = check(&meta(r#", {"pkg": "catalog"}"#)).expect("walk succeeds");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(report.problems[0].contains("sutura-catalog-datahub"), "{:?}", report.problems);
    }

    #[test]
    fn a_normal_edge_onto_the_application_or_the_settings_crate_is_a_violation() {
        let report = check(&meta(r#", {"pkg": "app"}, {"pkg": "config"}"#)).expect("walk succeeds");
        assert_eq!(report.problems.len(), 2, "{:?}", report.problems);
    }

    #[test]
    fn a_normal_edge_onto_a_transport_is_a_violation() {
        let report = check(&meta(r#", {"pkg": "http"}, {"pkg": "mcp"}"#)).expect("walk succeeds");
        assert_eq!(report.problems.len(), 2, "{:?}", report.problems);
    }

    #[test]
    fn a_dev_only_edge_onto_an_adapter_is_not_a_violation() {
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
        assert!(report.tree_size > 0, "sutura-http-client's own tree is not empty");
    }

    #[test]
    fn explain_does_not_panic() {
        explain();
    }
}
