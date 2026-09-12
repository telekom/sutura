//! The dprint exclusion list is pinned, because a formatter's blind spot grows in silence.
//!
//! THIRTEEN PATHS ARE EXCLUDED AND SEVEN OF THEM ARE ARGUED, each by a named mechanism a reformat
//! would break: `check-api-docs` byte-compares the generated pages against a fresh generation,
//! `check-skills` reads content-hash-locked imports and reported 23 as local forks the one time
//! dprint saw them, `check-gate-classification` and `check-venues` match table rows as exact lines
//! that cell padding defeats, a digest pins the catalog fixture, and a vendored tree's whole value
//! is a readable diff against upstream. Those are permanent, and `dprint.json` argues each in
//! place.
//!
//! THE OTHER SIX ARE NOT ARGUED. `dprint.json` calls them "work in flight" and "a follow-up, not a
//! policy" - which is a rule with no mechanism, and this test is the mechanism. They cover 32
//! tracked text files that no formatter inspects, and before this pin nothing in the tree would
//! have noticed a seventh being added: the gate would stay green while its coverage shrank, which
//! is the failure mode a formatter is least likely to be caught in.
//!
//! THE PIN IS THE ENTRY LIST AND NOT THE FILE COUNT, which is a measurement rather than a taste.
//! `docs/adr/**` alone is 27 of those 32 files, so a count pinned at 32 would refuse the next ADR
//! anybody writes - an unrelated lane reddened by an exclusion it never touched, and the repair a
//! reviewer reaches for first is to bump the number without reading why. The glob list moves only
//! when somebody changes what is excluded, and that is the event worth refusing.

#![cfg(test)]

/// Every path in `dprint.json`'s `excludes`, in file order. Held here rather than counted, so a
/// diff says which exclusion moved instead of only that the total did.
const EXCLUDED: &[&str] = &[
    "vendor/**",
    ".agents/**",
    "docs/api/**",
    "examples/single-player/catalog/**",
    "docs/implementation-plan-identity-and-services.md",
    "docs/where-identity-is-proven.md",
    "test-infra/README.md",
    "docs/adr/**",
    "crates/sutura-conformance/**",
    ".github/workflows/ci.yml",
    ".github/workflows/cross-link.yml",
    ".github/workflows/docs.yml",
    ".github/actionlint.yaml",
];

/// The subset excluded only because work is in flight over it, and the reason this file exists.
/// A path LEAVING this list is a blind spot closed and wants no ceremony; a path ARRIVING needs an
/// argument written beside it in `dprint.json`, which is what editing this pin makes someone do.
const IN_FLIGHT: &[&str] = &[
    "docs/adr/**",
    "crates/sutura-conformance/**",
    ".github/workflows/ci.yml",
    ".github/workflows/cross-link.yml",
    ".github/workflows/docs.yml",
    ".github/actionlint.yaml",
];

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
fn the_dprint_exclusions_are_pinned_and_the_unargued_group_has_not_grown() {
    let found = excluded(include_str!("../../dprint.json"));

    assert_eq!(
        found, EXCLUDED,
        "dprint.json's excludes moved - argue the change, then pin it"
    );

    // The reader must not be able to drift from the file: an in-flight path that is no longer
    // excluded at all would otherwise sit here forever, overstating the hole it describes.
    for path in IN_FLIGHT {
        assert!(found.contains(path), "{path} is pinned as in-flight but is not excluded");
    }

    let unargued = found.iter().filter(|path| IN_FLIGHT.contains(path)).count();
    assert_eq!(
        unargued, 6,
        "the unargued exclusions changed: {unargued} in the file, 6 pinned - a new one needs an \
         argument in dprint.json, and a removed one should shrink this pin"
    );
}
