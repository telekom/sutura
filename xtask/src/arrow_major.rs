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
//!
//! # BOTH DIRECTIONS, because only one of them used to be asked
//!
//! This gate returned on `majors.len() <= 1` before it read the allowlist at all, so on the day a
//! split closes it printed `ok - one Arrow major` and the entry that permitted the split stayed in
//! the file permitting nothing, its dated paragraph intact. What rots is not the entry, it is the
//! reasoning beside it - and that paragraph is the only thing a reviewer reads to decide whether
//! the exception is still earned. `max-lines` and `check-refusal-coverage` each hold their own
//! annotated allowlist to that rule already; this is the third list of the same shape.
//!
//! So the allowlist is read on every run, and compared both ways: a major in the lock with no entry
//! fails, and an entry naming a major the lock does not hold fails. See [`assess`].
//!
//! **THE CHOICE, and there were two.** `59` is the engine's own major and is not an exception to
//! anything - it is in the file so the failure message can name a reason for every major present.
//! Exempting it needs the file to mark which row is the engine's, which is a second thing to keep
//! true and a marker nothing checks. So: **every row is a row like any other and must name a major
//! the lock file holds.** The consequence, stated rather than left to be discovered: were Arrow to
//! leave the dependency graph entirely, every row goes inert at once and the gate fails until the
//! file is deleted along with them. That is the intended reading - a list of tolerated Arrow majors
//! on a tree with no Arrow is a rule that lost its subject.
//!
//! **WHAT THIS STILL DOES NOT ANSWER.** Not the date: an entry nobody has revisited in a year reads
//! exactly like one written this morning, which the allowlist's own header already discloses. Not
//! the shape either - a bare `<major>` with no date and no reason parses, permits the split, and
//! prints an empty reason beside it. Both are about an entry that is present and current; this gate
//! answers only whether the major it names is.

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
///
/// `pub(crate)` rather than private because `attribution` reads the same pairs from the same
/// file for the same reason: the crate list, no TOML parser.
pub(crate) struct Package<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
}

/// Parse `name` / `version` pairs out of a lock file without a TOML dependency.
///
/// `xtask` has one dependency and adding a TOML parser to read two fields would be a poor trade -
/// the same argument `changes.rs` makes for keeping its area table in Rust. The shape this relies
/// on is `cargo`'s own output: within a `[[package]]` stanza, `name` precedes `version`, and both
/// are `key = "value"` on their own line.
pub(crate) fn packages(lock: &str) -> Vec<Package<'_>> {
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

/// Which majors the Arrow family resolves to, and which members carry each one.
///
/// The members are what a failure message needs: naming one crate would send somebody to the
/// wrong place.
fn family_majors(lock: &str) -> BTreeMap<String, Vec<String>> {
    let mut majors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for package in packages(lock) {
        if package.name == FAMILY_PREFIX || package.name.starts_with("arrow-") {
            majors
                .entry(major(package.version).to_owned())
                .or_default()
                .push(format!("{}@{}", package.name, package.version));
        }
    }
    majors
}

/// What the lock file and the allowlist say about each other, in both directions.
struct Assessment {
    /// Majors present in the lock file that no entry explains.
    unexplained: Vec<String>,
    /// Entries naming a major the lock file does not hold - the split closed, the paragraph stayed.
    inert: Vec<String>,
}

/// Compare the two lists both ways.
///
/// **`unexplained` is asked only when there is a split**, and that is a rule rather than an
/// oversight: one Arrow major is the state this workspace wants, so demanding an entry for it would
/// fail a tree that is exactly right - and a gate that fails on correct code gets disabled, which is
/// the argument `deny.toml` already carries. **`inert` is asked on every run**, that one included,
/// because the day a split closes is precisely the day its entry stops being earned, and until now
/// nothing looked.
fn assess(majors: &BTreeMap<String, Vec<String>>, permitted: &BTreeMap<String, String>) -> Assessment {
    let unexplained = if majors.len() > 1 {
        majors.keys().filter(|m| !permitted.contains_key(*m)).cloned().collect()
    } else {
        Vec::new()
    };
    let inert = permitted.keys().filter(|m| !majors.contains_key(*m)).cloned().collect();
    Assessment { unexplained, inert }
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

    let majors = family_majors(&lock);
    // Read BEFORE any verdict, so a single-major tree is still told what its allowlist claims.
    let allowlist = std::fs::read_to_string(root.join(ALLOWLIST)).unwrap_or_default();
    let permitted = allowed(&allowlist);
    let found = assess(&majors, &permitted);

    // BOTH reports, never the first one alone: a gate that knows two numbers and prints one sends
    // its reader back for a second run to find the half it withheld.
    if !found.inert.is_empty() {
        report_inert(&found.inert, &majors);
    }
    if !found.unexplained.is_empty() {
        report_split(&majors, &permitted);
    }
    if !found.inert.is_empty() || !found.unexplained.is_empty() {
        return Verdict::Fail;
    }

    if majors.len() > 1 {
        println!(
            "xtask check-arrow: ok - {} Arrow majors, every one explained in {ALLOWLIST}",
            majors.len()
        );
        for (major, members) in &majors {
            let reason = permitted.get(major).map_or("no reason recorded", String::as_str);
            println!("  major {major}: {} crate(s) - {reason}", members.len());
        }
    } else {
        let which = majors
            .keys()
            .next()
            .map_or_else(|| "none present".to_owned(), |m| format!("major {m}"));
        println!(
            "xtask check-arrow: ok - one Arrow major in Cargo.lock ({which}), and no entry in {ALLOWLIST} names a major it does not hold"
        );
    }
    Verdict::Pass
}

/// An entry naming a major the lock file no longer holds.
fn report_inert(inert: &[String], majors: &BTreeMap<String, Vec<String>>) {
    eprintln!(
        "xtask check-arrow: FAILED - {ALLOWLIST} names {} major(s) Cargo.lock does not hold",
        inert.len()
    );
    for major in inert {
        eprintln!("  major {major}: no arrow crate in the lock file resolves to it");
    }
    if majors.is_empty() {
        eprintln!("  the lock file holds no arrow crate at all");
    } else {
        let present: Vec<&str> = majors.keys().map(String::as_str).collect();
        eprintln!("  present: {}", present.join(", "));
    }
    eprintln!();
    eprintln!("  The split that entry permitted has closed. An exception nothing needs is a claim");
    eprintln!("  nobody checks, and this file is a list of claims: the paragraph beside an entry is");
    eprintln!("  the only thing a reviewer reads to decide whether it is still earned. Delete the");
    eprintln!("  entry and its argument together, in the change that closed the split.");
    eprintln!();
}

/// A major in the lock file that no entry explains.
fn report_split(majors: &BTreeMap<String, Vec<String>>, permitted: &BTreeMap<String, String>) {
    eprintln!("xtask check-arrow: FAILED - Cargo.lock holds {} Arrow majors", majors.len());
    for (major, members) in majors {
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

    /// The lock file this workspace has today: an engine major and an adapter's, both allowed.
    fn split_lock() -> &'static str {
        "[[package]]\nname = \"arrow\"\nversion = \"58.4.0\"\n\n[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n"
    }

    #[test]
    fn an_entry_for_a_major_the_lock_no_longer_holds_is_inert() {
        // The day the split closes. Before this gate looked, the run printed `ok - one Arrow
        // major` and the 58 entry stayed in the file with its dated paragraph, permitting nothing.
        let majors = family_majors("[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        let permitted = allowed("58 2026-08-28 the duckdb crate lags\n59 2026-08-28 the engine\n");
        let found = assess(&majors, &permitted);
        assert_eq!(found.inert, vec!["58".to_owned()], "the closed split's entry is named");
        assert!(found.unexplained.is_empty(), "one major needs no entry");
    }

    #[test]
    fn a_single_major_with_an_allowlist_that_matches_it_is_clean() {
        // The other half of the same rule: reading the allowlist on a single-major tree must not
        // turn a correct tree red, or the gate is one somebody disables.
        let majors = family_majors("[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        let permitted = allowed("59 2026-08-28 the engine's major\n");
        let found = assess(&majors, &permitted);
        assert!(found.inert.is_empty());
        assert!(found.unexplained.is_empty());
    }

    #[test]
    fn a_single_major_needs_no_entry_at_all() {
        // One major is the state this workspace wants, not an exception to anything.
        let majors = family_majors("[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        let found = assess(&majors, &allowed(""));
        assert!(found.unexplained.is_empty());
        assert!(found.inert.is_empty());
    }

    #[test]
    fn every_entry_goes_inert_when_arrow_leaves_the_graph_entirely() {
        // The stated consequence of treating the engine's own row as a row like any other: a list
        // of tolerated Arrow majors on a tree with no Arrow is a rule that lost its subject.
        let majors = family_majors("[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n");
        let permitted = allowed("58 2026-08-28 the adapter\n59 2026-08-28 the engine\n");
        let found = assess(&majors, &permitted);
        assert_eq!(found.inert, vec!["58".to_owned(), "59".to_owned()]);
    }

    #[test]
    fn a_split_every_entry_explains_is_clean_in_both_directions() {
        let permitted = allowed("58 2026-08-28 the adapter\n59 2026-08-28 the engine\n");
        let found = assess(&family_majors(split_lock()), &permitted);
        assert!(found.unexplained.is_empty());
        assert!(found.inert.is_empty());
    }

    #[test]
    fn a_split_still_fails_on_the_major_no_entry_names() {
        // Today's failure, unchanged - and the entry for a major that IS absent is reported beside
        // it rather than instead of it.
        let permitted = allowed("57 2026-08-28 long gone\n59 2026-08-28 the engine\n");
        let found = assess(&family_majors(split_lock()), &permitted);
        assert_eq!(found.unexplained, vec!["58".to_owned()]);
        assert_eq!(found.inert, vec!["57".to_owned()]);
    }
}
