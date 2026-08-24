//! The dependency-boundary gate: the domain crate acquires no framework dependency.

use std::process::ExitCode;

/// Crates that must never appear in `sutura-domain`'s dependency tree. The domain holds
/// types and ports; the moment it can reach an async runtime or a query engine, the
/// hexagon is decoration and every domain test starts paying for a framework build.
const FORBIDDEN_IN_DOMAIN: &[&str] = &["tokio", "axum", "rmcp", "datafusion", "arrow", "duckdb", "reqwest", "hyper"];

/// Assert the domain crate's dependency tree contains nothing framework-shaped.
///
/// Reads `cargo metadata` rather than the manifest, so a dependency pulled in
/// *transitively* is caught too — which is the case a manifest grep would miss.
pub(crate) fn run() -> ExitCode {
    let meta = match crate::cargo_metadata(&[]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask check-boundaries: {message}");
            return ExitCode::FAILURE;
        }
    };

    let Some(packages) = meta.get("packages").and_then(|p| p.as_array()) else {
        eprintln!("xtask check-boundaries: cargo metadata had no `packages` array");
        return ExitCode::FAILURE;
    };

    let Some(domain) = packages
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("sutura-domain"))
    else {
        eprintln!("xtask check-boundaries: sutura-domain not found in workspace metadata");
        return ExitCode::FAILURE;
    };

    let deps: Vec<&str> = domain
        .get("dependencies")
        .and_then(|d| d.as_array())
        .map(|a| a.iter().filter_map(|d| d.get("name")?.as_str()).collect())
        .unwrap_or_default();

    let violations: Vec<&&str> = FORBIDDEN_IN_DOMAIN
        .iter()
        .filter(|f| deps.iter().any(|d| *d == **f))
        .collect();

    if violations.is_empty() {
        println!(
            "xtask check-boundaries: ok — sutura-domain has {} deps, none forbidden",
            deps.len()
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("xtask check-boundaries: FAILED");
        for v in violations {
            eprintln!("  sutura-domain must not depend on `{v}` — it belongs in an adapter crate");
        }
        ExitCode::FAILURE
    }
}
