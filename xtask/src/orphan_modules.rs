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
use std::path::Path;

/// A finding: one `pub` module that no first-party crate references.
struct Orphan {
    owner: String,
    module: String,
}

/// `(package name, the `pub mod` idents it declares)` for the first-party library crates.
type LibraryCrates = Vec<(String, BTreeSet<String>)>;

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

    // The corpus is every first-party `.rs` file concatenated, so a module is reachable when any
    // crate names it - including the owner's own `crate::…` references. Built in the SAME pass
    // that finds each crate's `pub mod` declarations now: a second walk over the same directory
    // used to re-read every file to find them, and both walks held a plain `Vec` a `.take(n)`
    // could narrow with nothing here to notice.
    let mut corpus = String::new();
    // The crates that declare library modules, with what each one declares.
    let mut library_crates: LibraryCrates = Vec::new();
    for package in packages {
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or("<unnamed>").to_owned();
        let Some(manifest) = package.get("manifest_path").and_then(|p| p.as_str()) else {
            continue;
        };
        let Some(crate_dir) = Path::new(manifest).parent() else {
            continue;
        };
        match crate_pass(&root, crate_dir) {
            Ok(Some(pass)) => {
                corpus.push('\n');
                corpus.push_str(&pass.text);
                library_crates.push((name, pass.declared));
            }
            // a non-library crate with no `.rs`; nothing to hold reachability for
            Ok(None) => {}
            Err(why) => {
                eprintln!("xtask unreachable-public-modules: FAILED - {name}: {why}");
                return Verdict::Fail;
            }
        }
    }

    let mut findings: Vec<Orphan> = Vec::new();
    let mut checked = 0_usize;
    for (name, declared) in &library_crates {
        for module in declared {
            checked = checked.saturating_add(1);
            if !reached_as_segment(&corpus, module) {
                findings.push(Orphan {
                    owner: name.clone(),
                    module: module.clone(),
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

/// One crate's contribution: its `.rs` text, concatenated, and every `pub mod <ident>` it
/// declares. `None` when the crate has no `.rs` at all - a build-script-only crate, say - which is
/// not a failure, just nothing to hold reachability for.
struct CratePass {
    /// The crate's `.rs` text, concatenated, for the reachability corpus.
    text: String,
    /// Every `pub mod <ident>` this crate declares.
    declared: BTreeSet<String>,
}

/// Walk one crate's `.rs` tree once, building the corpus text and the `pub mod` declarations
/// together.
///
/// This used to be two walks over the same directory - one to build the corpus, a second inside
/// [`public_mods_in`] (since deleted) to re-read every file looking for declarations - and both
/// held the transitional door's plain `Vec` in caller space. [`repo::Census::inspect`] performs
/// the read once now; a `pub mod x;` or inline `pub mod x { ... }` declares a public module,
/// `pub(crate) mod` excluded because after the first `pub ` the next non-space token is `(`, not
/// `mod`. Textual, with `unused_deps`'s trade: no compiler needed.
fn crate_pass(root: &Path, crate_dir: &Path) -> Result<Option<CratePass>, String> {
    let mut text = String::new();
    let mut declared = BTreeSet::new();
    let mut error: Option<String> = None;
    let census = repo::collect_files(root, crate_dir, &["rs"]);
    let outcome = census.inspect(&[], everything, |rel, bytes| {
        if error.is_some() {
            return;
        }
        let Ok(file_text) = std::str::from_utf8(bytes) else {
            error = Some(format!("{rel} is not valid UTF-8"));
            return;
        };
        text.push_str(file_text);
        for line in file_text.lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix("pub ") else { continue };
            let Some(body) = rest.strip_prefix("mod ") else { continue };
            let Some(ident) = body.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next() else {
                continue;
            };
            if !ident.is_empty() {
                declared.insert(ident.to_owned());
            }
        }
    });
    if let Some(why) = error {
        return Err(why);
    }
    match outcome {
        Ok(_inspected) => Ok(Some(CratePass { text, declared })),
        Err(repo::Refusal::Empty) => Ok(None),
        Err(why) => Err(why.describe()),
    }
}

/// This gate's [`repo::Scope`] for [`crate_pass`]: `collect_files` already filtered to `.rs`, so
/// every subject the census offers is in scope.
const fn everything(_rel: &str) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{crate_pass, reached_as_segment};

    #[test]
    fn an_unreachable_subtree_refuses_the_crate_pass_instead_of_a_smaller_corpus() {
        // `github.com/telekom/sutura#414`: the corpus used to be built by re-reading a `Vec`
        // handed back by the transitional door, in a separate loop from the one that found each
        // `pub mod` - both held a plain `Vec` a narrowing had nothing here to notice.
        // `crate_pass` walks once through `Census::inspect`, so an unreachable subtree refuses
        // the whole pass rather than shrinking the corpus.
        use std::os::unix::fs::PermissionsExt as _;
        let root = std::env::temp_dir().join(format!("sutura-orphan-modules-unreachable-{}", std::process::id()));
        let crate_dir = root.join("crates/thing");
        std::fs::create_dir_all(crate_dir.join("src/blocked")).expect("the scratch tree");
        std::fs::write(crate_dir.join("src/lib.rs"), "pub mod blocked;\n").expect("a readable file");
        std::fs::set_permissions(crate_dir.join("src/blocked"), std::fs::Permissions::from_mode(0o000))
            .expect("chmod 000 on the subtree");

        let result = crate_pass(&root, &crate_dir);

        std::fs::set_permissions(crate_dir.join("src/blocked"), std::fs::Permissions::from_mode(0o700))
            .expect("restore permissions so cleanup can remove the tree");
        std::fs::remove_dir_all(&root).expect("remove the owned fixture");

        assert!(
            result.is_err(),
            "an unreachable subtree must refuse rather than a smaller corpus"
        );
    }

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
