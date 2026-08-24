//! The unused-dependency gate.
//!
//! A dependency nobody uses is not free: it is compile time, a lockfile entry, a licence
//! obligation and an advisory surface, all for code that never runs. It also makes the
//! manifest a bad description of the crate, which is worse than it sounds once boundary
//! rules are read off the manifests.
//!
//! Detection is a token scan of the crate's own `.rs` files for the dependency's Rust
//! identifier. That is a heuristic - a crate reachable only through a re-exported macro
//! would look unused - but it is the same heuristic that catches the real case, and it
//! needs no nightly compiler and no extra tool in the shell.

use crate::repo;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

/// A finding: one dependency declaration that nothing refers to.
struct Unused {
    owner: String,
    dependency: String,
    reason: &'static str,
}

pub(crate) fn run(_args: &[String]) -> ExitCode {
    let Some(root) = repo::root() else {
        eprintln!("xtask unused-deps: could not locate the repo root");
        return ExitCode::FAILURE;
    };

    let metadata = match crate::cargo_metadata(&["--no-deps"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask unused-deps: {message}");
            return ExitCode::FAILURE;
        }
    };
    let Some(packages) = metadata.get("packages").and_then(|p| p.as_array()) else {
        eprintln!("xtask unused-deps: cargo metadata had no `packages` array");
        return ExitCode::FAILURE;
    };

    let mut findings: Vec<Unused> = Vec::new();
    let mut declared_anywhere: BTreeSet<String> = BTreeSet::new();
    let mut checked = 0_usize;

    for package in packages {
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>");
        let Some(manifest) = package.get("manifest_path").and_then(|p| p.as_str()) else {
            continue;
        };
        let Some(crate_dir) = Path::new(manifest).parent() else {
            continue;
        };
        let identifiers = rust_identifiers_in(&root, crate_dir);
        let dependencies = package.get("dependencies").and_then(|d| d.as_array());
        for dependency in dependencies.into_iter().flatten() {
            let Some(dep_name) = dependency.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            declared_anywhere.insert(dep_name.to_owned());
            let local_name = dependency
                .get("rename")
                .and_then(|r| r.as_str())
                .unwrap_or(dep_name)
                .replace('-', "_");
            checked = checked.saturating_add(1);
            if !identifiers.contains(&local_name) {
                findings.push(Unused {
                    owner: name.to_owned(),
                    dependency: dep_name.to_owned(),
                    reason: "declared, but its Rust identifier appears in no .rs file of that crate",
                });
            }
        }
    }

    findings.extend(orphaned_workspace_entries(&root, &declared_anywhere));
    report(checked, &findings)
}

fn report(checked: usize, findings: &[Unused]) -> ExitCode {
    if findings.is_empty() {
        println!("xtask unused-deps: ok - {checked} dependency declarations, all referenced");
        return ExitCode::SUCCESS;
    }
    eprintln!("xtask unused-deps: FAILED - {} unused declaration(s)", findings.len());
    for finding in findings {
        eprintln!("  {}: `{}` {}", finding.owner, finding.dependency, finding.reason);
    }
    eprintln!("  remove the declaration, or use it. It comes back with the code that needs it.");
    ExitCode::FAILURE
}

/// `[workspace.dependencies]` entries that no member crate inherits. The table is a
/// version-pin registry; an entry nothing inherits pins nothing.
fn orphaned_workspace_entries(root: &Path, declared: &BTreeSet<String>) -> Vec<Unused> {
    let Ok(manifest) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return Vec::new();
    };
    workspace_dependency_keys(&manifest)
        .into_iter()
        .filter(|key| !declared.contains(key))
        .map(|key| Unused {
            owner: "[workspace.dependencies]".to_owned(),
            dependency: key,
            reason: "pinned in the workspace table but inherited by no member crate",
        })
        .collect()
}

/// Keys of the `[workspace.dependencies]` table, read as text.
///
/// A hand-rolled read of one flat table beats adding a TOML parser to a crate that has no
/// other use for one; the shape it accepts is asserted by the tests below.
fn workspace_dependency_keys(manifest: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[workspace.dependencies]";
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once('=') {
            keys.push(key.trim().trim_matches('"').to_owned());
        }
    }
    keys
}

/// Every identifier-shaped token in the crate's Rust sources. Tokenising beats a substring
/// search: `alpha` must not be reported as used merely because `alpha_charlie` is.
///
/// Comments count as source, so naming a real dependency anywhere in a crate - including in
/// a doc comment like this one - is enough to make it look used. That is why the names in
/// this module's prose and fixtures are fictional.
fn rust_identifiers_in(root: &Path, crate_dir: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    repo::collect_files(root, crate_dir, &["rs"], &mut files);
    let mut identifiers = BTreeSet::new();
    for rel in &files {
        if let Ok(text) = std::fs::read_to_string(root.join(rel)) {
            identifiers.extend(tokenize(&text));
        }
    }
    identifiers
}

/// Split text into maximal runs of `[A-Za-z0-9_]`.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::{tokenize, workspace_dependency_keys};

    /// Fixture crate names are deliberately fictional. A fixture naming a real dependency
    /// would put that name into this crate's own token set and mask a genuine finding -
    /// the check would then be unable to fail on the crate that implements it.
    #[test]
    fn reads_the_workspace_table_and_stops_at_the_next_one() {
        let manifest = "\
[workspace]
members = [\"a\"]

[workspace.dependencies]
# a comment
alpha = { version = \"1\", features = [\"derive\"] }
bravo = \"2\"

[profile.dev]
opt-level = 0
";
        assert_eq!(workspace_dependency_keys(manifest), vec!["alpha", "bravo"]);
    }

    #[test]
    fn a_prefix_is_not_a_use() {
        let tokens = tokenize("use alpha_charlie::Value;");
        assert!(tokens.contains(&"alpha_charlie".to_owned()));
        assert!(!tokens.contains(&"alpha".to_owned()));
    }

    #[test]
    fn tokens_survive_punctuation_and_end_of_input() {
        assert_eq!(tokenize("a::b(c)"), vec!["a", "b", "c"]);
        assert_eq!(tokenize("trailing_ident"), vec!["trailing_ident"]);
    }
}
