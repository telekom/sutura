//! The correlation a fuzz target's bound crates make with the two hook surfaces.
//!
//! `cargo xtask check-fuzz` gates five obligations for a fuzz target: a `[[bin]]` entry, a seed
//! directory, a workflow-matrix entry, a pre-commit `fuzz` hook `files:` regex entry, and a
//! `hook_coverage/surfaces.rs` `"fuzzed tree"` row entry. The first three are read off
//! `fuzz/Cargo.toml`, `fuzz/seeds/` and `.github/workflows/fuzz.yml`; these last two are the ones
//! this module decides, and the reason they had to be gated read on a live instance:
//! `bigquery_answer` (#864) landed everywhere but the two hook files, so editing the exact parser
//! the target drives fired no local `fuzz` hook and no coverage row - a green `check-fuzz` and a
//! green commit that read as "the target's parser was replayed" when it was not.
//!
//! **Where a target's crates come from, and why this is reliable enough.** A target's header names
//! its crates in prose, which a gate cannot read. What it CAN read is the `use sutura_<crate>::…`
//! and `sutura_<crate>::…` paths in the target's own source - the identifiers the file is bound to
//! by construction. Each `sutura_(crate)` identifier maps `_` to `-` and is checked as
//! `crates/<crate>` against BOTH surfaces. The one text seam: an identifier the source spells but
//! a surface omits is exactly the failure we are here to catch, so a NEW target whose bound crate
//! reaches neither hook file is a red `check-fuzz` rather than a silent gap.
//!
//! The two consumers live in this module and nowhere else: [`hook_regex`] reads the `fuzz` hook's
//! `files:` value (via [`crate::hooks`], the one parser of `.pre-commit-config.yaml`), and
//! [`fuzzed_tree_row`] reads `crate::hook_coverage::surfaces::SURFACES`'s `"fuzzed tree"` row. A
//! second parser of either file would violate the single-parser rule each module's own header
//! states, so this module opens both surfaces but introduces no new reader. The decision half -
//! [`missing_from_regex`] and [`missing_from_row`] - lives here too, so `crate::fuzz`'s `report`
//! only formats the gaps this module has already derived, and the red/green case is exercised here
//! where the rule is, not in a file that also carries the production wiring.

use std::collections::{BTreeMap, BTreeSet};

/// Every crate a target's source names, as `crates/<crate>` path prefixes.
///
/// `sutura_config` becomes `crates/sutura-config`, the same `_` -> `-` mapping the target's
/// `use sutura_config::…` makes when it resolves to the crate directory. No trailing slash, so
/// both consumers match on a prefix: the hook regex spells `crates/sutura-domain/src/query.rs`
/// rather than the bare directory.
pub(crate) fn crates_of(source: &str) -> BTreeSet<String> {
    let mut crates = BTreeSet::new();
    for word in source.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
        if word.len() > "sutura_".len() && word.starts_with("sutura_") {
            let kebab = word.replace('_', "-");
            crates.insert(format!("crates/{kebab}"));
        }
    }
    crates
}

/// The `fuzz` hook's `files:` regex value, from the one parser of `.pre-commit-config.yaml`.
///
/// `None` when the hook declares no `files:` - a missing filter is the other spelling of the
/// hazard (`always_run`), which this module's sibling rules already hold.
pub(crate) fn hook_regex(declared: &[crate::hooks::Hook]) -> Option<String> {
    declared
        .iter()
        .find(|hook| hook.id == "fuzz")
        .map(|hook| hook.files.clone())
        .filter(|files| !files.is_empty())
}

/// The `"fuzzed tree"` row's path globs, from `crate::hook_coverage::surfaces::SURFACES`.
pub(crate) fn fuzzed_tree_row() -> Option<&'static [&'static str]> {
    crate::hook_coverage::surfaces::SURFACES
        .iter()
        .find(|surface| surface.label == "fuzzed tree")
        .map(|surface| surface.paths)
}

/// Does `regex` reach any path under a `crates/<crate>` directory?
///
/// A prefix match on the bare directory: the hook's alternation lists files and subdirectories
/// (`crates/sutura-domain/src/…`) rather than the crate root, so "the crate is reachable" is "one
/// of those entries starts with `crates/<crate>`".
pub(crate) fn regex_reaches(regex: &str, crate_prefix: &str) -> bool {
    regex.contains(crate_prefix)
}

/// Does the `"fuzzed tree"` row reach any path under a `crates/<crate>` directory?
pub(crate) fn row_reaches(paths: &[&str], crate_prefix: &str) -> bool {
    paths.iter().any(|glob| glob.starts_with(crate_prefix))
}

/// The target/crate pairs the hook's `files:` regex fails to reach.
///
/// The red case for the pre-commit surface: each bound crate absent from the regex is named so a
/// `check-fuzz` run can say exactly which target and which crate no local replay fires for.
pub(crate) fn missing_from_regex(bound: &BTreeMap<String, BTreeSet<String>>, regex: Option<&str>) -> Vec<(String, String)> {
    let Some(regex) = regex else { return Vec::new() };
    let mut missing = Vec::new();
    for (target, crates) in bound {
        for prefix in crates {
            if !regex_reaches(regex, prefix) {
                missing.push((target.clone(), prefix.clone()));
            }
        }
    }
    missing
}

/// The target/crate pairs the hook-coverage `"fuzzed tree"` row fails to reach.
///
/// With `None` for the row the caller has already refused the whole run - a coverage table with
/// no fuzzed-tree row cannot report a gap - so this returns everything, which is the loudest
/// honest answer to "nothing reaches these".
pub(crate) fn missing_from_row(bound: &BTreeMap<String, BTreeSet<String>>, row: Option<&[&str]>) -> Vec<(String, String)> {
    let Some(row) = row else {
        return bound
            .iter()
            .flat_map(|(target, crates)| crates.iter().map(move |c| (target.clone(), c.clone())))
            .collect();
    };
    let mut missing = Vec::new();
    for (target, crates) in bound {
        for prefix in crates {
            if !row_reaches(row, prefix) {
                missing.push((target.clone(), prefix.clone()));
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests;
