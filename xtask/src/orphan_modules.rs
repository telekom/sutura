//! The unreachable-public-module gate (issue #131, the "either way" slice).
//!
//! `github.com/telekom/sutura#131` names the missing gate class: *"an `xtask` gate that fails a
//! `pub` module in a library crate which no first-party crate calls - the missing gate class -
//! `unused-deps` for manifests, this for calls - and it belongs in the diff whichever path is
//! taken."* A `pub` module nobody reaches is a second thing to keep true: it compiles, it carries
//! tests, and the argument for keeping its exception narrow is read by nobody. `unused-deps`
//! catches a declared dependency with no use; this catches a declared module with no reference.
//!
//! # The mechanism, mirroring `unused_deps`
//!
//! [`unused_deps`](crate::unused_deps) is a token scan needing no nightly compiler and no extra
//! tool in the shell. This gate is the same scan turned inside out:
//!
//! * For every first-party crate, read its `pub mod <name>` declarations.
//! * Scan **every** first-party crate's `.rs` files (the whole workspace, so a consumer in another
//!   crate is seen, and intra-crate `crate::…` references count too) for each module name **used as
//!   a path segment**.
//!
//! A module name used as a path segment appears in one of the shapes a real reference takes: `m::`
//! (the module as a path prefix, as in `banner::print`, `crate::inbound::caller::VerifiedCaller` or
//! `base_paths::CATALOG`) or `::m` (the module as the tail of a `use` / re-export, as in `pub use
//! crate::panics::install_panic_hook` or `use crate::inbound::caller`). The declaration itself,
//! `pub mod m;` or `pub mod m { … }`, has `m` followed by `;`/`{`/whitespace and preceded by
//! whitespace - neither shape matches, so a module's declaration cannot count as its own reference.
//!
//! A public module whose name appears as a path segment in **no** first-party source is
//! unreachable and refuses the gate. The scan is over the whole workspace so a consumer in any
//! crate is seen, and an intra-crate `crate::…` reference legitimately satisfies the module.
//!
//! # What it does NOT hold
//!
//! The trade is the one `unused_deps` states: this is a token scan, text over semantics. A module
//! reached only through a macro that expands to the path, or a module whose name also appears as
//! an unrelated identifier elsewhere, can read as reachable when it is not - the safe direction for
//! an orphan gate (under-reports, never falsely blocks a live crate). A module referenced through a
//! differently-spelled re-export can look unreachable the same way a macro-only dependency looks
//! unused to `unused_deps`; a full module-graph walk would need a nightly compiler or a build
//! inside the sandbox, which this does not.
//!
//! It does not deliberate what is wired. `sutura_sql::expression` is NOT flagged here - `sutura_sql`'s
//! `expression.rs` and its `tests/adversarial_findings.rs` reference `expression` as a path segment -
//! because it is unwired (nothing published calls `compile`; `docs/adr/0004`'s amendment says why
//! the load does not), not orphaned (no first-party crate names it). That distinction is the "either
//! way" scope: this gate holds *no reference at all*.

use crate::Verdict;
use crate::repo;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A finding: one `pub` module that no first-party crate references.
struct Orphan {
    owner: String,
    module: String,
}

/// `(package name, its repo-relative `.rs` files)` for the first-party packages.
type CrateSources = Vec<(String, Vec<String>)>;

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask unreachable-public-modules: could not locate the repo root");
        return Verdict::Fail;
    };

    let metadata = match crate::cargo_metadata(&["--no-deps"]) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("xtask unreachable-public-modules: {message}");
            return Verdict::Fail;
        }
    };
    let Some(packages) = metadata.get("packages").and_then(|p| p.as_array()) else {
        eprintln!("xtask unreachable-public-modules: cargo metadata had no `packages` array");
        return Verdict::Fail;
    };

    // Every first-party package, keyed by package name, with its `.rs` files (repo-relative).
    let mut crate_sources: CrateSources = Vec::new();
    // The crates that declare library modules: `(package name, crate dir)`.
    let mut library_crates: Vec<(String, PathBuf)> = Vec::new();
    for package in packages {
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>").to_owned();
        let Some(manifest) = package.get("manifest_path").and_then(|p| p.as_str()) else {
            continue;
        };
        let Some(crate_dir) = Path::new(manifest).parent() else {
            continue;
        };
        let files = match listing(&root, crate_dir) {
            Ok(files) => files,
            Err(why) => {
                eprintln!("xtask unreachable-public-modules: FAILED - {name}: {why}");
                return Verdict::Fail;
            }
        };
        if files.is_empty() {
            continue; // a non-library crate with no `.rs`; nothing to hold reachability for
        }
        crate_sources.push((name.clone(), files));
        library_crates.push((name, crate_dir.to_path_buf()));
    }

    // The corpus is every first-party `.rs` file concatenated, so a module is reachable when any
    // crate names it - including the owner's own `crate::…` references.
    let mut corpus = String::new();
    for (_ident, files) in &crate_sources {
        corpus.push('\n');
        for rel in files {
            match std::fs::read_to_string(root.join(rel)) {
                Ok(text) => corpus.push_str(&text),
                Err(why) => {
                    eprintln!("xtask unreachable-public-modules: FAILED - {rel}: {why}");
                    return Verdict::Fail;
                }
            }
        }
    }

    let mut findings: Vec<Orphan> = Vec::new();
    let mut checked = 0_usize;
    for (name, crate_dir) in &library_crates {
        let declared = match public_mods_in(&root, crate_dir) {
            Ok(v) => v,
            Err(why) => {
                eprintln!("xtask unreachable-public-modules: FAILED - {name}: {why}");
                return Verdict::Fail;
            }
        };
        for module in declared {
            checked = checked.saturating_add(1);
            if !reached_as_segment(&corpus, &module) {
                findings.push(Orphan {
                    owner: name.clone(),
                    module,
                });
            }
        }
    }

    if checked == 0 {
        // The falsifier (issue #371) demands a gate refuse a tree it cannot attest. A tree with
        // no library crate enumerating any `pub mod` is one the scan read nothing from, so the
        // "all referenced" verdict would be about an empty tree rather than about this one. Refuse
        // rather than report a pass over nothing.
        eprintln!("xtask unreachable-public-modules: FAILED - no public module to check in any first-party crate");
        return Verdict::Fail;
    }
    if findings.is_empty() {
        println!("xtask unreachable-public-modules: ok - {checked} public module(s) checked, all referenced");
        Verdict::Pass
    } else {
        for f in &findings {
            eprintln!(
                "xtask unreachable-public-modules: {}::{} has no first-party reference",
                f.owner, f.module
            );
        }
        eprintln!(
            "xtask unreachable-public-modules: {n} unreachable public module(s) of {checked} checked",
            n = findings.len()
        );
        Verdict::Fail
    }
}

/// The `.rs` files under `dir`, as repo-relative paths, via the transitional census door.
fn listing(root: &Path, dir: &Path) -> Result<Vec<String>, String> {
    repo::collect_files(root, dir, &["rs"])
        .into_listing(repo::Unmigrated::OrphanModules)
        .map(|(_, files)| files)
        .map_err(|why| why.describe())
}

/// Whether the identifier `name` appears in `text` used as a path segment.
///
/// Two reference shapes count, both of which a real module reference takes and neither of which a
/// `pub mod name;` declaration does:
///
/// * `name::` - the module name as a path prefix: `banner::print`, `caller::VerifiedCaller`,
///   `base_paths::CATALOG`.
/// * `::name` - the module name as the tail of a `use`/re-export: `pub use crate::panics::…` names
///   `panics` on its own line, `use crate::inbound::caller` ends in `::caller`.
///
/// A longer identifier (`nameX`) never counts, because here the character after `name` is a letter.
/// The declaration `pub mod name;` has `name` followed by `;`/`{`/space and preceded by space, so
/// neither `name::` nor `::name` matches it.
fn reached_as_segment(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let n = name.len();
    if n == 0 || bytes.len() < n {
        return false;
    }
    let mut i = 0_usize;
    while i + n <= bytes.len() {
        if bytes.get(i..i + n) == Some(name.as_bytes()) {
            // Identifier boundary on both sides: neither adjacent byte continues the name.
            let before_ok = bytes.get(i.wrapping_sub(1)).is_none_or(|b| !is_ident_char(*b));
            let after = i + n;
            let after_ok = bytes.get(after).is_none_or(|b| !is_ident_char(*b));
            if !before_ok || !after_ok {
                i += 1;
                continue;
            }
            // A path segment: `name::` (followed by `:`) or `::name` (preceded by `:`), either of
            // which is the shape a real module reference takes.
            if bytes.get(after) == Some(&b':') || bytes.get(i.wrapping_sub(1)) == Some(&b':') {
                return true;
            }
        }
        i += 1;
    }
    false
}

#[inline]
const fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Every `pub mod <ident>` declaration in a crate's `.rs` tree.
///
/// A `pub mod x;` or inline `pub mod x { ... }` declares a public module. `pub(crate) mod` is
/// excluded because after the first `pub ` the next non-space token is `(`, not `mod`. Textual,
/// with `unused_deps`'s trade: no compiler needed.
fn public_mods_in(root: &Path, crate_dir: &Path) -> Result<BTreeSet<String>, String> {
    let files = listing(root, crate_dir)?;
    let mut out = BTreeSet::new();
    for rel in &files {
        let file = root.join(rel);
        let text = std::fs::read_to_string(&file).map_err(|why| format!("could not read {}: {why}", file.display()))?;
        for line in text.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("pub ") else { continue };
            // `pub(crate)` / `pub(super)` are excluded; only a bare identifier `mod` follows `pub `.
            let Some(body) = rest.strip_prefix("mod ") else { continue };
            let Some(ident) = body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next() else {
                continue;
            };
            if ident.is_empty() {
                continue;
            }
            out.insert(ident.to_owned());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::reached_as_segment;

    #[test]
    fn a_module_used_as_a_path_prefix_is_referenced() {
        assert!(reached_as_segment("banner::print", "banner"));
        assert!(reached_as_segment("use crate::inbound::caller::VerifiedCaller;", "caller"));
        assert!(reached_as_segment("base_paths::CATALOG", "base_paths"));
    }

    #[test]
    fn a_module_named_as_a_use_tail_is_referenced() {
        assert!(reached_as_segment("pub use crate::panics::install_panic_hook;", "panics"));
        assert!(reached_as_segment("use crate::inbound::caller;", "caller"));
    }

    #[test]
    fn a_qualified_cross_crate_reference_is_referenced() {
        assert!(reached_as_segment("sutura_http::inbound::gate::InboundGate", "gate"));
    }

    #[test]
    fn the_declaration_itself_is_not_a_reference() {
        assert!(!reached_as_segment("pub mod expression;", "expression"));
        assert!(!reached_as_segment("pub mod banner { }", "banner"));
    }

    #[test]
    fn a_longer_identifier_is_not_a_reference() {
        assert!(!reached_as_segment("use alpha_charlie::Value;", "alpha"));
        assert!(!reached_as_segment("a::alphaX", "alpha"));
    }

    #[test]
    fn end_of_input_counts() {
        assert!(reached_as_segment("a::b", "b"));
        assert!(reached_as_segment("trailing::ident", "ident"));
    }
}
