//! The diffs `super`'s assertions are about: a two-file split, and the four ways to drive it.
//!
//! **HARNESS ONLY, AND THAT IS THE RULE THIS FILE IS AN INSTANCE OF.** `super` reached the
//! unexemptable 1000-line cap, and the gate `super` implements never reverts a file that adds a
//! `#[test]` - so moving ASSERTIONS out of a file at the cap orphans them the moment the
//! declaring file is reverted, while moving the HARNESS out leaves every assertion where it was.
//! Nothing here carries a `#[test]`, deliberately: that is what keeps `super` the diff's only test
//! file and the verdict on this branch `NOT MECHANICALLY SEPARABLE` rather than an exit-3 base
//! tree that cannot build.
//!
//! **AND `super`'s OWN TRAILER COULD NOT RESCUE THIS SPLIT**, which is worth knowing before
//! reaching for it: the commit that moved this file also adds a test cell and a lexer fix, so it
//! is not a pure relocation and `Cleanup-Split:` would be refused by the very check it declares.
//! The trailer is for a commit that moves test code and changes none; this one changes some.

use super::{Broken, Changed, Claim, Images, Relocation, decide};
use crate::causality::diff::ChangedFile;
use crate::causality::fixtures::{changed, changed_removing, tree};
use crate::causality::regions::PostImage;

/// The file a split takes assertions OUT of.
///
/// Under `crates/x/tests/`, which is what `causality::regions`' dedicated-test-target rule answers
/// off the path alone - so every line of it is test code without a fixture, and an assertion about
/// the multiset is not also an assertion about region detection. The two tests that are ABOUT
/// position name a path outside it.
pub(super) const HELD: &str = "crates/x/tests/held.rs";

/// The file a split puts them INTO.
pub(super) const MOVED: &str = "crates/x/tests/held/moved.rs";

/// The four assertion lines that move, as they read in the file they leave.
pub(super) const ASSERTIONS: [&str; 4] = [
    "    assert_eq!(one(), 1);",
    "    assert_eq!(two(), 2);",
    "    assert!(three());",
    "    assert_ne!(four(), 5);",
];

/// The same four, at the indentation the file they arrive in gives them.
///
/// DEDENTED ON PURPOSE: a test leaving `mod tests { .. }` for its own file loses a level, so a raw
/// comparison would call every relocation impure. The multiset is over TRIMMED lines - except
/// inside a string literal, which `super`'s two literal tests are about.
pub(super) const RELOCATED: [&str; 4] = [
    "assert_eq!(one(), 1);",
    "assert_eq!(two(), 2);",
    "assert!(three());",
    "assert_ne!(four(), 5);",
];

/// A pure relocation: [`HELD`] loses the four assertions and gains a `mod`, [`MOVED`] gains them
/// at a different indentation.
pub(super) fn pure_move() -> Vec<ChangedFile> {
    vec![
        changed_removing(HELD, 1, &["mod moved;"], 1, &ASSERTIONS),
        changed(MOVED, 1, &RELOCATED),
    ]
}

/// The verdict for `files`, with the trailer naming [`HELD`] and no image for any file.
pub(super) fn tagged(files: &[ChangedFile]) -> Relocation {
    let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    verdict(files, &paths, &tree(&[]), &tree(&[]), true)
}

/// The same diff with no trailer at all.
pub(super) fn untagged(files: &[ChangedFile]) -> Relocation {
    let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    verdict(files, &paths, &tree(&[]), &tree(&[]), false)
}

/// The general form: the diff, what git says it touched, both images, and whether it is tagged.
pub(super) fn verdict(
    files: &[ChangedFile],
    touched: &[String],
    head: &impl Fn(&str) -> Option<String>,
    base: &impl Fn(&str) -> Option<String>,
    with_trailer: bool,
) -> Relocation {
    let head: &PostImage<'_> = head;
    let base: &PostImage<'_> = base;
    let claim = Claim::of(&format!("chore: split\n\nCleanup-Split: {HELD}\n"));
    decide(
        with_trailer.then_some(()).and(claim.as_ref()),
        &Changed { files, touched },
        &Images { head, base },
    )
}

/// A test whose expected output is a multi-line literal, as the file it LEFT holds it.
///
/// The two trailing spaces after `one` are INSIDE the string, so they are part of what the test
/// asserts on - and they are on the line that OPENS the literal, which begins outside it. That is
/// the margin a check asking only *did this line begin inside a literal* trims away.
pub(super) const OPENS_WITH_TRAILING: &str = concat!(
    "#[test]\n",
    "fn fixture() {\n",
    "    let expected = \"one  \n",
    "two\";\n",
    "    assert_eq!(render(), expected);\n",
    "}\n",
);

/// A test whose expected output is a multi-line literal continued over two lines.
///
/// `one` and `two` sit INSIDE the literal, so their leading whitespace is value on both.
pub(super) const CONTINUES_INDENTED: &str = concat!(
    "#[test]\n",
    "fn fixture() {\n",
    "    let expected = \"\\\n",
    "    one\n",
    "    two\";\n",
    "    assert_eq!(render(), expected);\n",
    "}\n",
);

/// The header a split's new module carries, so the file reads as test code in its own right.
pub(super) const ARRIVED: &str = "#![cfg(test)]\n\nuse super::*;\n\n";

/// The verdict over a two-file split given each side's whole file image.
///
/// `was` is [`HELD`]'s content at base and `now` is [`MOVED`]'s at head, so the diff is *every
/// line of one file left and every line of the other arrived*, plus the `mod` declaration that
/// puts it into the build. The IMAGES are bound to the files they belong to, because the margin
/// lexer reads a whole file rather than a line.
pub(super) fn verdict_over(was: &str, now: &str) -> Relocation {
    let left: Vec<&str> = was.lines().collect();
    let arrived: Vec<&str> = now.lines().collect();
    let files = vec![
        changed_removing(HELD, 1, &["mod moved;"], 1, &left),
        changed(MOVED, 1, &arrived),
    ];
    let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
    verdict(
        &files,
        &paths,
        &tree(&[(MOVED, now), (HELD, "mod moved;\n")]),
        &tree(&[(HELD, was)]),
        true,
    )
}

/// The reasons [`tagged`] gives, or a panic naming what it said instead.
pub(super) fn refused(files: &[ChangedFile]) -> Vec<Broken> {
    match tagged(files) {
        Relocation::Refused(broken) => broken,
        other => panic!("expected a refusal, got {other:?}"),
    }
}
