//! The dprint exclusion list is pinned, because a formatter's blind spot grows in silence.
//!
//! EIGHT PATHS ARE EXCLUDED AND EACH NAMES A MECHANISM a reformat would break: `check-api-docs`
//! byte-compares the generated pages against a fresh generation, `check-skills` reads
//! content-hash-locked imports and reported 23 as local forks the one time dprint saw them,
//! `check-gate-classification` and `check-venues` match table rows as exact lines that cell padding
//! defeats, a digest pins each of the two catalog fixtures, and a vendored tree's whole value is a
//! readable diff against upstream. `dprint.json` argues each in place.
//!
//! THE SIX THAT WERE DEFERRED WORK ARE GONE, by the owner's decision. `dprint.json` called them
//! "work in flight" and "a follow-up, not a policy" - a rule with no mechanism - and they hid 32
//! tracked files from the formatter, 28 ADRs among them. Coverage is now 154 of 276 candidates,
//! where it was 120 of 274, and the deferred group is EMPTY.
//!
//! THE PIN IS THE ENTRY LIST, so an exclusion cannot be added, removed or reworded without editing
//! it, which is the moment somebody has to say which mechanism it protects. [`RETIRED`] names the
//! six separately, so re-adding one fails with its own reason rather than passing as a new entry.
//!
//! MEASURED BEFORE THE SIX WERE DROPPED, because the risk was real and specific: dprint pads table
//! cells, and ADR tables do get padded - ADR 0012's impersonation row now carries 100+ spaces. No
//! gate matches an ADR row. The one gate-matched needle in an ADR is
//! `guidance::claims::contradicted`'s "3016 sysroot files" in ADR 0025, which is PROSE, and
//! `textWrap: maintain` does not move a prose line break.

#![cfg(test)]

use std::path::{Path, PathBuf};

/// Every path in `dprint.json`'s `excludes`, in file order. Held here rather than counted, so a
/// diff says which exclusion moved instead of only that the total did.
const EXCLUDED: &[&str] = &[
    "vendor/**",
    ".agents/**",
    "docs/api/**",
    "examples/single-player/catalog/**",
    "examples/authored-sql/catalog/**",
    "docs/implementation-plan-identity-and-services.md",
    "docs/where-identity-is-proven.md",
    "test-infra/README.md",
];

/// The six exclusions that were deferred work rather than a mechanism, dropped when the owner
/// decided them and the formatter was pointed at what they hid. Named here rather than simply
/// deleted, so re-adding one fails with its own reason instead of passing as a new entry.
const RETIRED: &[&str] = &[
    "docs/adr/**",
    "crates/sutura-conformance/**",
    ".github/workflows/ci.yml",
    ".github/workflows/cross-link.yml",
    ".github/workflows/docs.yml",
    ".github/actionlint.yaml",
];

/// The repository root, from this test binary's own manifest directory - the idiom
/// `xtask/tests/cache_witness.rs` uses, and READ AT RUN TIME for the reason `flake.nix`'s source
/// filter states in its own header: `include_str!` resolves against the FILTERED copy in a nix
/// build, and `checks.clippy` compiles every test target against it. `dprint.json` is not a Cargo
/// input, so a compile-time include of it failed with `couldn't read xtask/tests/../../dprint.json`
/// inside the sandbox while passing under cargo and under the commit hooks - the one bug class a
/// local gate cannot see, and the fourth time that filter has bitten. `checks.nextest` runs this
/// on `wholeTree` and has the file. A filter arm would have worked too and costs every
/// filtered-src check a rehash; this costs nothing.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

/// `dprint.json` is JSONC - the comments carrying those arguments are the point of the file - so
/// this reads the array rather than deserialising it. Quoted lines are entries and `//` lines are
/// not, which is the whole grammar involved.
fn excluded(config: &str) -> Vec<&str> {
    let (_, rest) = config
        .split_once("\"excludes\": [")
        .expect("dprint.json declares an excludes array");
    let (region, _) = rest.split_once("\n  ],").expect("the excludes array closes");
    region
        .lines()
        .filter_map(|line| line.trim().strip_prefix('"'))
        .filter_map(|entry| entry.split_once('"').map(|(path, _)| path))
        .collect()
}

#[test]
fn the_dprint_exclusions_are_pinned_and_no_deferred_exclusion_came_back() {
    // `expect` and not a skip: a pin that silently inspects nothing is worse than no pin.
    let config = std::fs::read_to_string(root().join("dprint.json")).expect("dprint.json is readable");
    let found = excluded(&config);

    assert_eq!(
        found, EXCLUDED,
        "dprint.json's excludes moved - argue the change, then pin it"
    );

    // The deferred group is empty and must not grow back. Re-adding one of the six reads as a
    // normal new entry to the assertion above; here it reads as the thing it is.
    for path in RETIRED {
        assert!(
            !found.contains(path),
            "{path} was retired as deferred work - excluding it again needs a mechanism, not a follow-up"
        );
    }
}
