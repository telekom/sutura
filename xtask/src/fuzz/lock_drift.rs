//! Does the fuzz lock pin every crates.io package it shares with the root lock at a version the
//! root lock pins?
//!
//! `fuzz/` resolves on its own, so nothing else stops its lock drifting ahead of or behind the
//! root's. Measured: `fuzz/Cargo.lock` pinned `yoke-derive` 0.8.3 while the root pinned 0.8.2, and
//! when crates.io yanked 0.8.3 the fuzz-scoped `cargo deny` failed every pull request for a
//! version the shipped graph never used. The same sweep refused 33 more shared packages.
//!
//! **Subset, not equality.** The fuzz graph is smaller, so a name the root pins at two versions
//! (`syn` 2 and 3) may appear in it at one. A package only the fuzz graph needs (`libfuzzer-sys`)
//! has nothing to agree with and passes. A drifted first-party `sutura*` pin is refused here and
//! by [`super::version_gaps`] alike.
//!
//! **The cost: a Dependabot PR that moves one lock is refused.** Dependabot resolves each directory
//! on its own even when one PR covers both: #1187, grouped by `group-by: dependency-name`, carried
//! both locks yet took a newer `rustix` patch in `fuzz/` than the root kept. This gate reds such a PR
//! whenever it moves a crate both locks pin. Fix it on the PR: run the same
//! `cargo update -p <name> --precise <version>` in the other directory. `.github/dependabot.yml`'s
//! groups without `group-by` (`patch-level`, `arrow`, `utoipa`) also span both directories in one
//! PR - read from dependabot-core's source, the limit being that no grouped PR covering two
//! directories is observed yet.
//!
//! **Source, not only version.** A `[patch.crates-io]` path entry or a path dependency resolves
//! with no `source =` line; the unpatched registry crate resolves with one. Two locks can agree on
//! name and version while one holds the patched crate and the other the registry one it replaces -
//! [`source_gaps`] refuses that pairing. The limit: it compares resolved sources only, so it cannot
//! see a feature selection that differs from the shipped one.

use std::collections::{BTreeMap, BTreeSet};

use super::{LOCK, pins, unquote};
use crate::workflows::sources::in_scope_text;

/// The root workspace's lock, the one the shipped graph resolves against.
const ROOT_LOCK: &str = "Cargo.lock";

/// Every [`LOCK`] pin at a version [`ROOT_LOCK`] does not pin, each formatted as a failure line
/// naming the package and both versions.
pub(super) fn gaps(root_lock: &str, fuzz_lock: &str) -> Vec<String> {
    let mut root: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, version) in pins(root_lock) {
        root.entry(name).or_default().insert(version);
    }
    pins(fuzz_lock)
        .into_iter()
        .filter_map(|(name, pinned)| {
            let versions = root.get(name)?;
            (!versions.contains(pinned)).then(|| {
                let at = versions.iter().copied().collect::<Vec<_>>().join("\", \"");
                format!(
                    "{LOCK} pins {name} at \"{pinned}\", but {ROOT_LOCK} pins it at \"{at}\" - run \
                     `cargo update -p {name}@{pinned} --precise <root version>` in fuzz/"
                )
            })
        })
        .collect()
}

/// A pinned package's `(name, version, source)` - `source` is `None` for a path dependency or a
/// `[patch.crates-io]` entry, which resolve with no `source = ` line.
type SourcedPin<'a> = (&'a str, &'a str, Option<&'a str>);

/// [`SourcedPin`] keyed by `(name, version)`, for a lookup of the source the other lock resolved
/// the same name and version from.
type SourceByNameVersion<'a> = BTreeMap<(&'a str, &'a str), Option<&'a str>>;

/// Every `[[package]]` stanza in `lock`, as a [`SourcedPin`]. Stanza-scoped, unlike [`pins`]'s
/// single-pass scan, so a `source` line is never attributed to the wrong package regardless of
/// key order within the stanza.
fn pins_with_source(lock: &str) -> Vec<SourcedPin<'_>> {
    lock.split("[[package]]")
        .filter_map(|stanza| {
            let mut name = None;
            let mut version = None;
            let mut source = None;
            for line in stanza.lines() {
                if let Some(value) = line.strip_prefix("name = ") {
                    name = unquote(value);
                } else if let Some(value) = line.strip_prefix("version = ") {
                    version = unquote(value);
                } else if let Some(value) = line.strip_prefix("source = ") {
                    source = unquote(value);
                }
            }
            Some((name?, version?, source))
        })
        .collect()
}

/// `source` formatted for a failure line - a path dependency or a `[patch.crates-io]` entry
/// resolves with no `source = ` line, so `None` reads as "a path".
fn describe_source(source: Option<&str>) -> &str {
    source.unwrap_or("a path")
}

/// Every package both [`ROOT_LOCK`] and [`LOCK`] pin at the SAME name and version but resolve
/// from different sources - a path against a registry, a registry against a path, or a git source
/// against anything else. A different version at the same name is [`gaps`]'s job, not this one's.
pub(super) fn source_gaps(root_lock: &str, fuzz_lock: &str) -> Vec<String> {
    let root: SourceByNameVersion<'_> = pins_with_source(root_lock)
        .into_iter()
        .map(|(name, version, source)| ((name, version), source))
        .collect();
    pins_with_source(fuzz_lock)
        .into_iter()
        .filter_map(|(name, version, fuzz_source)| {
            let root_source = *root.get(&(name, version))?;
            (root_source != fuzz_source).then(|| {
                format!(
                    "{LOCK} resolves {name} {version} from {}, but {ROOT_LOCK} resolves it from {} - mirror the root's \
                     `[patch.crates-io]` entry, or its path dependency, for {name} in fuzz/Cargo.toml",
                    describe_source(fuzz_source),
                    describe_source(root_source),
                )
            })
        })
        .collect()
}

/// Reads [`ROOT_LOCK`] under `root` and compares it to `fuzz_lock`.
///
/// An absent or unreadable root lock is an `Err`: it would read as no pins, share nothing and pass.
pub(super) fn gaps_in(root: &std::path::Path, fuzz_lock: &str) -> Result<Vec<String>, String> {
    let path = root.join(ROOT_LOCK);
    let text = in_scope_text(&path)?.ok_or_else(|| format!("{}: not found", path.display()))?;
    let mut found = gaps(&text, fuzz_lock);
    found.extend(source_gaps(&text, fuzz_lock));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::gaps_in;
    use crate::scratch_tree::Tree;

    #[test]
    fn a_root_lock_that_cannot_be_read_refuses_the_drift_check() {
        let unreadable = Tree::of("drift-unreadable", &[("Cargo.lock/keep", b"")]);
        let why = gaps_in(unreadable.root(), "").expect_err("a directory is not a lock");
        assert!(why.contains("Cargo.lock"), "the refusal has to name the file: {why}");

        let absent = Tree::of("drift-absent", &[]);
        let why = gaps_in(absent.root(), "").expect_err("a tree without its root lock is not this repository");
        assert!(why.contains("Cargo.lock"), "the refusal has to name the file: {why}");
    }
}
