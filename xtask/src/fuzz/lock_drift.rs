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
//! **Not a hole: an absent root lock.** It reads as empty here, shares nothing and passes, but
//! `check-boundaries`' `cargo metadata --locked` refuses it in the same hygiene sweep.

use std::collections::{BTreeMap, BTreeSet};

use super::{LOCK, pins};

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

/// Reads [`ROOT_LOCK`] under `root` and compares it to `fuzz_lock`.
pub(super) fn gaps_in(root: &std::path::Path, fuzz_lock: &str) -> Vec<String> {
    gaps(&std::fs::read_to_string(root.join(ROOT_LOCK)).unwrap_or_default(), fuzz_lock)
}
