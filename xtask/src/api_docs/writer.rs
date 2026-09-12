//! Does `just api` ask rustdoc the same question this gate does?
//!
//! The parent module's header states that the gate and the writer are ONE CODE PATH, so the fix a
//! failure names - `just api` - produces what the gate compares against. Two things carried that
//! claim and neither was a mechanism: the `cargo doc` line here and the one in
//! `nix/api-docs.nix` are two hand-written copies of one invocation, in two languages.
//!
//! **Two halves, because the question has two halves.** The rustdoc FLAGS decide what rustdoc
//! looks at; the cargo SELECTION decides which units it runs over and - through feature
//! unification - what those units contain. A copy that agrees on one and not the other still
//! produces pages the gate never judged.
//!
//! **A flag on one side only is invisible, and the two directions fail differently.** A flag the
//! gate passes and the writer does not makes `just api` unable to see what turned the gate red;
//! a flag the writer passes and the gate does not makes the pages regenerate from JSON the gate
//! never judged.
//!
//! **Why the flags matter more than a style rule.** Without `--document-private-items` rustdoc
//! never runs the link-resolution pass over a private item, so `broken_intra_doc_links` -
//! `forbid` in the root manifest, and armed on every member per
//! [`lints`](crate::api_docs::lints) - reports **nothing** about a private module's doc comments.
//! Measured on a two-file crate: one unresolvable link inside a private module is exit 0 without
//! the flag and exit 101 with it, same tree, same lint level. `github.com/telekom/sutura#327` is
//! that hole; the flag is what closes it, and this module is what keeps it closed in both venues.
//!
//! **Why the selection matters just as much.** `--workspace --all-features` resolves features
//! once over every member. `-p <one> --all-features` resolves them for one, which is a different
//! set for every shared dependency - so a writer that documented packages one at a time could
//! hand the generator JSON the gate's own run would never produce, and the byte comparison would
//! be against the wrong build. It is also the resolution `sutura-deps` is built under, which is
//! what makes the warm artifacts reusable; that is a cost argument rather than a correctness one
//! and is recorded in `flake.nix`, not here.
//!
//! **The limits, next to the claim.**
//!
//! * It compares the rustdoc flags exactly, and the cargo line only for the three arguments that
//!   decide scope and features. The rest legitimately differs - the writer takes the profile
//!   literally, the gate reads it from an environment variable - so requiring the whole line to
//!   match would refuse a correct tree.
//! * It reads the writer's TEXT, not a run of it. That the flags then reached rustdoc is what the
//!   rustdoc child in the parent module proves by exiting non-zero on a link it cannot resolve.
//! * It says nothing about `flake.nix`'s `checks.api-docs`, which invokes this gate rather than
//!   cargo, so it has no third copy to drift.

use std::path::Path;

/// The writer, relative to the repo root.
///
/// `nix/api-docs.nix` and not `flake.nix`: the app declaration stays in the flake for the two
/// text-scanning gates that need it there, and the package it points at is this file.
const WRITER: &str = "nix/api-docs.nix";

/// How the writer's invocation begins, at the start of a line.
///
/// Anchored, because the same two words appear in that file's own header comment - a substring
/// search finds two "invocations" and has to pick one, which is the class of defect
/// `../gates` records against reading `flake.nix` as text.
const INVOCATION: &str = "cargo doc";

/// How the writer hands rustdoc its flags, at the start of a line.
///
/// `cargo doc` documents many units, so it takes no trailing rustdoc arguments the way
/// `cargo rustdoc` did - there is no single unit for them to belong to. The environment is the
/// only channel left, which is why this is a second anchor rather than a suffix of the first.
const ASSIGNMENT: &str = "export RUSTDOCFLAGS=";

/// The environment variable both venues pass the rustdoc flags through.
///
/// Named once and read by the parent module's child process, so the gate cannot set a variable
/// this parser is not looking for.
pub(super) const RUSTDOCFLAGS: &str = "RUSTDOCFLAGS";

/// The cargo arguments that decide which units are documented, and under which features.
///
/// Not the whole line: these three are the ones a difference in would change the JSON itself.
/// `--no-deps` is in the list because without it `--workspace` documents the entire dependency
/// closure, which is a different job at a different cost.
const SELECTION: &[&str] = &["--workspace", "--no-deps", "--all-features"];

/// Everything rustdoc is handed, in order, by BOTH venues.
///
/// `--document-private-items` is here for link resolution and not for the pages: the generator
/// keeps only `public` and `default` visibility, so a private item entering the JSON renders
/// nothing. Measured on the nightly pin - a private module, a `pub(crate)` item and a private
/// field of a public struct report `crate` or `restricted`, never `default`.
pub(super) const RUSTDOC_ARGS: &[&str] = &[
    "-Z",
    "unstable-options",
    "--output-format",
    "json",
    "--document-private-items",
];

/// The writer's one line starting with `prefix`, with its shell continuations joined.
///
/// Exactly one, or a refusal. Zero means the shape this parser was written for is gone; more than
/// one means the parser has to guess which line `just api` runs, and a gate that guesses is a
/// gate that can guess wrong quietly.
fn one_line(text: &str, prefix: &str) -> Result<String, String> {
    let joined = text.replace("\\\n", " ");
    let lines: Vec<String> = joined
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with(prefix))
        .map(String::from)
        .collect();
    match lines.as_slice() {
        [] => Err(format!(
            "{WRITER} has no line starting with `{prefix}` - the two invocations cannot be compared"
        )),
        [only] => Ok(only.clone()),
        many => Err(format!(
            "{WRITER} starts {} lines with `{prefix}` - which one does `just api` run?",
            many.len()
        )),
    }
}

/// What the writer hands rustdoc: the tokens of its `RUSTDOCFLAGS` assignment.
fn flags(text: &str) -> Result<Vec<String>, String> {
    let line = one_line(text, ASSIGNMENT)?;
    // `strip_prefix` and not a byte range: `clippy::string_slice` is denied, because indexing a
    // `str` by a length panics on a multi-byte character - unreachable here only while
    // `one_line` keeps filtering on this exact prefix, which is why the fallback is the line.
    let value = line.strip_prefix(ASSIGNMENT).unwrap_or(line.as_str()).trim();
    let quoted = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| format!("{WRITER}'s `{ASSIGNMENT}` line is not a double-quoted value: {value}"))?;
    let found: Vec<String> = quoted.split_whitespace().map(String::from).collect();
    if found.is_empty() {
        return Err(format!(
            "{WRITER} assigns {RUSTDOCFLAGS} an empty value, so it passes rustdoc nothing"
        ));
    }
    Ok(found)
}

/// Does the writer document the same units, under the same features, as this gate?
fn selection(text: &str) -> Result<(), String> {
    let line = one_line(text, INVOCATION)?;
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let missing: Vec<&str> = SELECTION.iter().copied().filter(|want| !tokens.contains(want)).collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{WRITER}'s `{INVOCATION}` line is missing {missing:?}, so `just api` documents a \
         different set of units than this gate - or resolves their features differently, which \
         changes the JSON the pages are rendered from. Change both, or neither."
    ))
}

/// `Ok` with the flags both venues pass, or why the two cannot be shown to agree.
///
/// A precondition rather than a finding, for [`super::lints`]' reason: pages regenerated from a
/// rustdoc run whose flags nobody checked are pages compared against an unknown question.
pub(super) fn check(root: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(root.join(WRITER)).map_err(|error| format!("could not read {WRITER}: {error}"))?;
    selection(&text)?;
    let theirs = flags(&text)?;
    let mine: Vec<String> = RUSTDOC_ARGS.iter().map(|flag| String::from(*flag)).collect();
    if theirs == mine {
        // `theirs` and NOT `mine`: the caller asserts the return against `RUSTDOC_ARGS`, so
        // handing back the gate's own list makes that assertion compare `mine` to `mine` and the
        // whole test tautological - measured, a neutered comparator here survived `just test`.
        return Ok(theirs);
    }
    Err(format!(
        "`just api` and this gate hand rustdoc different flags, so one of them judges what the \
         other cannot.\n  this gate: {mine:?}\n  {WRITER}: {theirs:?}\n  \
         Without `--document-private-items` rustdoc skips link resolution inside a private module \
         entirely, so `broken_intra_doc_links` reports nothing there. Change both, or neither."
    ))
}

#[cfg(test)]
mod tests {
    use super::{RUSTDOC_ARGS, WRITER, check, flags, selection};
    use crate::repo;

    /// THE PARITY ASSERTION, over the real file rather than a fixture.
    ///
    /// A fixture would only prove the parser parses. Reverting either copy of the invocation -
    /// `nix/api-docs.nix` or [`RUSTDOC_ARGS`] - has to be what turns this red, which is only true
    /// of a test that reads the writer that ships.
    #[test]
    fn the_writer_hands_rustdoc_exactly_the_flags_this_gate_does() {
        let root = repo::root().expect("the tests run inside the repo");
        let agreed = check(&root).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(agreed, RUSTDOC_ARGS, "{WRITER} disagrees with this gate");
    }

    /// The flag the whole module exists for, named rather than merely present in a list.
    ///
    /// Parity alone is satisfied by removing the flag from BOTH sides, which is exactly the
    /// fail-open state `#327` reports: rustdoc runs, the lint is armed, and a private module's
    /// unresolvable links are never looked at.
    #[test]
    fn private_items_are_documented_or_their_links_are_never_resolved() {
        assert!(
            RUSTDOC_ARGS.contains(&"--document-private-items"),
            "without it a broken link inside a private module is exit 0"
        );
    }

    #[test]
    fn the_flags_are_read_from_the_assignment_and_not_from_the_cargo_line() {
        let text = concat!(
            "  # RUSTDOCFLAGS is named in this comment\n",
            "  export RUSTDOCFLAGS=\"-Z unstable-options --output-format json\"\n",
            "  cargo doc -q --no-deps --workspace --all-features --profile ci\n"
        );
        assert_eq!(
            flags(text).expect("one assignment"),
            ["-Z", "unstable-options", "--output-format", "json"]
        );
    }

    /// A writer that documents one package at a time resolves features differently, so its pages
    /// are not the pages this gate compares - even with identical rustdoc flags.
    #[test]
    fn a_writer_that_documents_one_package_at_a_time_is_a_refusal() {
        let per_package = "  cargo doc -q --no-deps -p sutura-domain --all-features --profile ci\n";
        assert!(selection(per_package).is_err(), "a per-package selection must refuse");
        let whole = "  cargo doc -q --no-deps --workspace --all-features --profile ci\n";
        assert!(selection(whole).is_ok(), "the shipped selection must pass");
    }

    /// FAIL CLOSED on every shape that leaves nothing to compare. A comparison with an empty or
    /// unfound left side is the way this kind of gate goes green over two lists that disagree.
    #[test]
    fn a_writer_this_cannot_read_is_a_refusal_rather_than_agreement() {
        for (text, why) in [
            ("pkgs.writeShellApplication { }\n", "no assignment"),
            (
                "export RUSTDOCFLAGS=\"--a\"\nexport RUSTDOCFLAGS=\"--b\"\n",
                "two assignments",
            ),
            ("export RUSTDOCFLAGS=--output-format\n", "unquoted"),
            ("export RUSTDOCFLAGS=\"\"\n", "empty"),
        ] {
            assert!(flags(text).is_err(), "{why} must refuse");
        }
        for (text, why) in [
            ("pkgs.writeShellApplication { }\n", "no invocation"),
            ("cargo doc --workspace\ncargo doc --no-deps\n", "two invocations"),
        ] {
            assert!(selection(text).is_err(), "{why} must refuse");
        }
    }
}
