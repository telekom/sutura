//! The attribution module's test HARNESS, in its own file because `xtask/` is an UNEXEMPTABLE
//! prefix for `cargo xtask max-lines` - the module's assertions and harness together crossed the
//! 1000-line cap, and a generated-or-vendored exemption is the only kind that file accepts.
//!
//! This file keeps only the harness: the fixtures, builders and resolvers the module's
//! characterisation tests call. The `#[test]` assertions themselves live inline in
//! `attribution.rs`, beside the behaviour they pin, so the causality gate reads one file that
//! changes behaviour and carries its tests along with it.

use core::fmt::Write as _;

use std::collections::{BTreeMap, BTreeSet};

use super::{COMMITTED, Generated, Package, RELEASE, header};

/// The workspace's own crate names, as the two real files would yield them.
pub(super) fn ours(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| String::from(*n)).collect()
}

/// A lock stanza, so the fixtures read like the file they parse.
pub(super) fn stanza(name: &str, version: &str, source: Option<&str>) -> String {
    let mut out = format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n");
    if let Some(source) = source {
        writeln!(out, "source = \"{source}\"").expect("writing into a String cannot fail");
    }
    out
}

pub(super) const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

pub(super) fn package(name: &str, version: &str) -> Package {
    Package {
        name: String::from(name),
        version: String::from(version),
    }
}

/// A `Generated` as the real one would arrive: a table rendered from `licences`, the same
/// `wanted` set, and `undeclared` derived the way [`super::generate`] derives it.
///
/// Built through the real `header` and the real row format on purpose - a fixture that wrote
/// its own table shape would pass while `row` and the generator disagreed, which is the one
/// thing these refusals cannot afford to be blind to.
pub(super) fn generated(wanted: &[(&str, &str)], licences: &BTreeMap<Package, String>) -> Generated {
    let wanted: BTreeSet<Package> = wanted.iter().map(|(n, v)| package(n, v)).collect();
    let mut lines = Vec::new();
    let mut undeclared = BTreeSet::new();
    for entry in &wanted {
        let licence = licences.get(entry).map_or("", String::as_str);
        if licence.is_empty() {
            undeclared.insert(entry.clone());
        }
        let declared = if licence.is_empty() { "NOT DECLARED" } else { licence };
        lines.push(format!("| `{}` | `{}` | {declared} |", entry.name, entry.version));
    }
    Generated {
        text: format!("{}{}\n", header(lines.len()), lines.join("\n")),
        wanted,
        undeclared,
    }
}

/// One fixture declaration: crate name, resolved version, and the SPDX expression its manifest
/// declares. Named because `clippy::type_complexity` is denied and the triple is the whole point.
pub(super) type Declaration = (&'static str, &'static str, &'static str);

/// The licences `cargo metadata` would supply, as the fixtures declare them.
pub(super) fn declared(pairs: &[Declaration]) -> BTreeMap<Package, String> {
    pairs
        .iter()
        .map(|(name, version, licence)| (package(name, version), String::from(*licence)))
        .collect()
}

/// A fixture repo root: a `release.yml` carrying `workflow`, and optionally a committed
/// document. Under the process temp dir with a unique name, because two of these run at once
/// under `nextest` and a shared path would make one test read the other's tree.
pub(super) fn fixture(name: &str, workflow: &str, committed: bool) -> std::path::PathBuf {
    // The name is the discriminator and the pid keeps two concurrent runs apart; nothing here
    // needs a counter, because each caller passes its own name.
    let root = std::env::temp_dir().join(format!("sutura-attribution-{}-{name}", std::process::id()));
    // Removed and rebuilt rather than written into: the name is deterministic, so a previous run
    // leaving a committed copy behind would decide this run's verdict.
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("a temp directory is removable");
    }
    let workflows = root.join(".github").join("workflows");
    std::fs::create_dir_all(&workflows).expect("a temp directory is writable");
    std::fs::write(root.join(RELEASE), workflow).expect("a temp file is writable");
    if committed {
        std::fs::write(root.join(COMMITTED), "# a stale copy\n").expect("a temp file is writable");
    }
    root
}

/// A release workflow in the shape the gate wants: it generates and it copies nothing.
pub(super) const GENERATING_RELEASE: &str = "      - run: nix run .#xtask -- attribution dist/sutura-attribution.md\n";
