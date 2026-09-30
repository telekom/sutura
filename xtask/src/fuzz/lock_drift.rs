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
//! **The cost: a one-lock Dependabot PR is refused.** `.github/dependabot.yml`'s `patch-level` and
//! `arrow` groups have no `group-by`, and Dependabot opens a grouped update once per directory
//! (its options reference, quoted there; no run observed), so each of the two PRs moves a shared
//! crate in one lock only and this gate reds both. Land them as one: run the same `cargo update
//! -p <name> --precise <version>` in the other directory on either PR. `group-by:
//! dependency-name` would pair the directories, but splits a group into one PR per dependency -
//! the grouping those two exist for.
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
