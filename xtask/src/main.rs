//! Repo automation. Run as `cargo xtask <task>`.
//!
//! These are gates, not conveniences: each one answers "what fails if this rule is
//! violated?" with a non-zero exit code rather than with a paragraph of guidance.

use std::process::ExitCode;

/// Crates that must never appear in `sutura-domain`'s dependency tree. The domain holds
/// types and ports; the moment it can reach an async runtime or a query engine, the
/// hexagon is decoration and every domain test starts paying for a framework build.
const FORBIDDEN_IN_DOMAIN: &[&str] = &[
    "tokio",
    "axum",
    "rmcp",
    "datafusion",
    "arrow",
    "duckdb",
    "reqwest",
    "hyper",
];

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("check-boundaries") => check_boundaries(),
        Some(other) => {
            eprintln!("xtask: unknown task `{other}`");
            eprintln!("tasks: check-boundaries");
            ExitCode::from(2)
        }
        None => {
            eprintln!("usage: cargo xtask <check-boundaries>");
            ExitCode::from(2)
        }
    }
}

/// Assert the domain crate's dependency tree contains nothing framework-shaped.
///
/// Reads `cargo metadata` rather than the manifest, so a dependency pulled in
/// *transitively* is caught too — which is the case a manifest grep would miss.
fn check_boundaries() -> ExitCode {
    let out = match std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            eprintln!(
                "xtask: cargo metadata failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("xtask: could not run cargo metadata: {e}");
            return ExitCode::FAILURE;
        }
    };

    let meta: serde_json::Value = match serde_json::from_slice(&out) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xtask: cargo metadata was not valid JSON: {e}");
            return ExitCode::FAILURE;
        }
    };

    let Some(packages) = meta.get("packages").and_then(|p| p.as_array()) else {
        eprintln!("xtask: cargo metadata had no `packages` array");
        return ExitCode::FAILURE;
    };

    let Some(domain) = packages
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("sutura-domain"))
    else {
        eprintln!("xtask: sutura-domain not found in workspace metadata");
        return ExitCode::FAILURE;
    };

    let deps: Vec<&str> = domain
        .get("dependencies")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|d| d.get("name").and_then(|n| n.as_str()))
                .collect()
        })
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
