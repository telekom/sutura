//! One Arrow major per lock file, because two are two incompatible sets of types.
//!
//! WHY THIS GATE EXISTS AND `deny.toml` IS NOT IT. `deny.toml` sets `multiple-versions = "warn"`,
//! deliberately - denying it would need a skip entry per duplicated crate, and on this workspace
//! that is twenty-seven of them, churning on every `just update`. A gate that fails on correct
//! code gets disabled, which is the same reasoning `allow-wildcard-paths` already carries there.
//! So the general duplicate check stays a warning and this gate is narrow.
//!
//! WHAT MAKES ARROW DIFFERENT from the other twenty-six duplicates. Arrow is a TYPE vocabulary
//! rather than a library: `RecordBatch` and `Schema` from one major are not the ones from the
//! next, so the moment first-party code holds a value from one and hands it to something
//! expecting the other, it does not compile - or, forced across the C data interface, it is
//! undefined behaviour. A duplicated hashing crate costs build time. A duplicated Arrow costs
//! correctness the day anything crosses between them.
//!
//! AND WHY IT IS NOT ALREADY BROKEN. As of writing, this workspace holds two Arrow majors and it
//! is a duplicate rather than a type boundary: the engine pulls one, the `DuckDB` adapter's crate
//! pulls the other, and `sutura-exec-duckdb` declares no `arrow` dependency and names no Arrow
//! type - it converts to a neutral row type instead. `differential.rs` compares rows rather than
//! batches for the same reason. **The test for which one you have is not the lock file, it is
//! whether any first-party crate names the type.** This gate reads the lock file, so it cannot
//! make that distinction - which is exactly why an accepted split needs a written reason rather
//! than being silently tolerated.
//!
//! The allowlist is `devco/arrow-majors-allow`, one entry per line as `<major> <ISO date> <reason>`.
//! An entry is a decision somebody wrote down, and the date is what makes a stale one visible.

use std::collections::BTreeMap;

use crate::Verdict;
use crate::repo;

/// The allowlist, relative to the repo root.
const ALLOWLIST: &str = "devco/arrow-majors-allow";

/// Crates whose major must agree. The whole family moves together upstream, so a split in any one
/// of them is the same defect as a split in `arrow` itself - and checking only `arrow` would miss
/// a transitive dependant pinning `arrow-schema` alone.
const FAMILY_PREFIX: &str = "arrow";

/// One `[[package]]` stanza's name and version, as read from `Cargo.lock`.
struct Package<'a> {
    name: &'a str,
    version: &'a str,
}

/// Parse `name` / `version` pairs out of a lock file without a TOML dependency.
///
/// `xtask` has one dependency and adding a TOML parser to read two fields would be a poor trade -
/// the same argument `changes.rs` makes for keeping its area table in Rust. The shape this relies
/// on is `cargo`'s own output: within a `[[package]]` stanza, `name` precedes `version`, and both
/// are `key = "value"` on their own line.
fn packages(lock: &str) -> Vec<Package<'_>> {
    let mut found = Vec::new();
    let mut name: Option<&str> = None;
    for line in lock.lines() {
        if let Some(value) = line.strip_prefix("name = ") {
            name = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ")
            && let Some(n) = name.take()
            && let Some(v) = unquote(value)
        {
            found.push(Package { name: n, version: v });
        }
    }
    found
}

/// Strip the surrounding quotes from a lock-file value, or `None` if it is not quoted.
fn unquote(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

/// The major component of a semver string, as text so a four-digit major cannot overflow.
fn major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

/// Majors that an allowlist entry accepts, mapped to the reason written beside them.
fn allowed(contents: &str) -> BTreeMap<String, String> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.splitn(3, char::is_whitespace);
            let major = parts.next()?.to_owned();
            let rest = parts.next().map(str::to_owned).unwrap_or_default();
            let reason = parts.next().unwrap_or("").trim().to_owned();
            Some((major, format!("{rest} {reason}").trim().to_owned()))
        })
        .collect()
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-arrow: could not determine the repo root");
        return Verdict::Fail;
    };

    let Ok(lock) = std::fs::read_to_string(root.join("Cargo.lock")) else {
        eprintln!("xtask check-arrow: could not read Cargo.lock");
        return Verdict::Fail;
    };

    // Which majors each family member resolves to, and which members carry each major. The second
    // is what the failure message needs: naming one crate would send somebody to the wrong place.
    let mut majors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for package in packages(&lock) {
        if package.name == FAMILY_PREFIX || package.name.starts_with("arrow-") {
            majors
                .entry(major(package.version).to_owned())
                .or_default()
                .push(format!("{}@{}", package.name, package.version));
        }
    }

    if majors.len() <= 1 {
        let which = majors
            .keys()
            .next()
            .map_or_else(|| "none present".to_owned(), |m| format!("major {m}"));
        println!("xtask check-arrow: ok - one Arrow major in Cargo.lock ({which})");
        return Verdict::Pass;
    }

    // More than one major. An allowlist entry per major is what turns this from a defect into a
    // decision, and the entry has to name every major present - accepting one and not the other
    // would be an allowlist that permits a split it never described.
    let allowlist = std::fs::read_to_string(root.join(ALLOWLIST)).unwrap_or_default();
    let permitted = allowed(&allowlist);
    let all_explained = majors.keys().all(|m| permitted.contains_key(m));

    if all_explained {
        println!(
            "xtask check-arrow: ok - {} Arrow majors, every one explained in {ALLOWLIST}",
            majors.len()
        );
        for (major, members) in &majors {
            let reason = permitted.get(major).map_or("no reason recorded", String::as_str);
            println!("  major {major}: {} crate(s) - {reason}", members.len());
        }
        return Verdict::Pass;
    }

    eprintln!("xtask check-arrow: FAILED - Cargo.lock holds {} Arrow majors", majors.len());
    for (major, members) in &majors {
        let note = permitted.get(major).map_or("NOT EXPLAINED", String::as_str);
        eprintln!("  major {major}: {} crate(s) [{note}]", members.len());
        for member in members.iter().take(3) {
            eprintln!("    {member}");
        }
        if members.len() > 3 {
            eprintln!("    ... and {} more", members.len() - 3);
        }
    }
    eprintln!();
    eprintln!("  The engine's Arrow major is the workspace's Arrow major: a data source is");
    eprintln!("  replaceable and the engine is not, so every other Arrow-consuming dependency");
    eprintln!("  conforms to it. Downgrading the engine to match an adapter is not the fix.");
    eprintln!();
    eprintln!("  In preference order, each reached only because the one above failed:");
    eprintln!("    1. bump the conforming crate to a release that already agrees");
    eprintln!("    2. disable the feature that pulls the conflicting version, if unused");
    eprintln!("    3. `[patch.crates-io]` on the DEPENDENT crate - NOT on arrow itself, which");
    eprintln!("       cannot widen a requirement and is silently ignored across disjoint majors");
    eprintln!("    4. vendor, last, and never silently: VENDOR.md, and it makes us the security");
    eprintln!("       response for that copy permanently");
    eprintln!();
    eprintln!("  Do not vendor, inline, fork or pin back on your own judgement - raise it, saying");
    eprintln!("  which crates conflict, whether it is a duplicate or a TYPE BOUNDARY, what each");
    eprintln!("  option costs, and what breaks if nothing is done.");
    eprintln!();
    eprintln!("  To accept a split deliberately, add to {ALLOWLIST}:");
    for major in majors.keys().filter(|m| !permitted.contains_key(*m)) {
        eprintln!("    {major} <ISO date> <why this split is acceptable, and what would end it>");
    }
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_major_is_read_out_of_a_lock_file() {
        let lock =
            "[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n\n[[package]]\nname = \"arrow-schema\"\nversion = \"59.2.0\"\n";
        let found = packages(lock);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "arrow");
        assert_eq!(major(found[1].version), "59");
    }

    #[test]
    fn a_version_line_without_a_preceding_name_is_not_a_package() {
        // A lock file's `[metadata]` and `[[patch]]` tables carry bare values, and reading one as
        // a package would invent a dependency - the defect class this gate exists to prevent.
        let found = packages("version = \"3\"\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version, "59.2.0");
    }

    #[test]
    fn the_whole_family_counts_not_only_the_arrow_crate() {
        // A transitive dependant may pin `arrow-schema` alone, which is the same defect and is
        // what checking only `arrow` would miss.
        let lock = "[[package]]\nname = \"arrow-schema\"\nversion = \"58.4.0\"\n";
        let found: Vec<&str> = packages(lock)
            .iter()
            .filter(|p| p.name == FAMILY_PREFIX || p.name.starts_with("arrow-"))
            .map(|p| p.name)
            .collect();
        assert_eq!(found, vec!["arrow-schema"]);
    }

    #[test]
    fn a_crate_merely_starting_with_arrow_is_not_family() {
        // `arrowhead` would be a different project. The family is `arrow` or `arrow-*`.
        let lock = "[[package]]\nname = \"arrowhead\"\nversion = \"1.0.0\"\n";
        let any_family = packages(lock)
            .iter()
            .any(|p| p.name == FAMILY_PREFIX || p.name.starts_with("arrow-"));
        assert!(!any_family, "arrowhead is not the Arrow family");
    }

    #[test]
    fn an_allowlist_entry_carries_its_reason() {
        let permitted = allowed("# a comment\n58 2026-08-28 the duckdb crate lags; dev-dependency only\n\n");
        assert_eq!(permitted.len(), 1);
        assert!(permitted["58"].contains("2026-08-28"), "the date is part of the reason");
        assert!(permitted["58"].contains("duckdb"));
    }

    #[test]
    fn a_blank_or_commented_allowlist_permits_nothing() {
        assert!(allowed("\n# nothing here\n\n").is_empty());
    }
}
