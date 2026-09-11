//! Does `just api` hand rustdoc the same flags this gate does?
//!
//! The parent module's header states that the gate and the writer are ONE CODE PATH, so the fix a
//! failure names - `just api` - produces what the gate compares against. Two things carried that
//! claim and neither was a mechanism: the `cargo rustdoc` line here and the one in
//! `nix/api-docs.nix` are two hand-written copies of one invocation, in two languages.
//!
//! **A flag on one side only is invisible, and the two directions fail differently.** A flag the
//! gate passes and the writer does not makes `just api` unable to see what turned the gate red;
//! a flag the writer passes and the gate does not makes the pages regenerate from JSON the gate
//! never judged. `check-api-links` holds one list of URL schemes across the same two languages
//! for the same reason, and this is that shape over the invocation.
//!
//! **Why it matters more than a style rule.** Without `--document-private-items` rustdoc never
//! runs the link-resolution pass over a private item, so `broken_intra_doc_links` - `forbid` in
//! the root manifest, and armed on every member per [`lints`](crate::api_docs::lints) - reports **nothing** about a
//! private module's doc comments. Measured on a two-file crate: one unresolvable link inside a
//! private module is exit 0 without the flag and exit 101 with it, same tree, same lint level.
//! `github.com/telekom/sutura#327` is that hole; the flag is what closes it, and this module is
//! what keeps it closed in both venues.
//!
//! **The limits, next to the claim.**
//!
//! * It compares the flags AFTER `--`, the ones rustdoc itself reads. The cargo arguments before
//!   it legitimately differ - the writer selects `"$lib"` from a shell loop and takes the profile
//!   literally, the gate selects a package name and reads the profile from an environment
//!   variable - so requiring those to match would refuse a correct tree.
//! * It reads the writer's TEXT, not a run of it. That the flags then reached rustdoc is what the
//!   rustdoc child in the parent module proves by exiting non-zero on a link it cannot resolve.
//! * It says nothing about `flake.nix`'s `checks.api-docs`, which invokes this gate rather than
//!   rustdoc, so it has no third copy to drift.

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
const INVOCATION: &str = "cargo rustdoc";

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

/// The writer's one `cargo rustdoc` line, with its shell continuations joined.
fn invocation(text: &str) -> Result<String, String> {
    let joined = text.replace("\\\n", " ");
    let lines: Vec<&str> = joined
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with(INVOCATION))
        .collect();
    match lines.as_slice() {
        [] => Err(format!(
            "{WRITER} has no line starting with `{INVOCATION}` - the two invocations cannot be compared"
        )),
        [only] => Ok(String::from(*only)),
        many => Err(format!(
            "{WRITER} starts {} lines with `{INVOCATION}` - which one does `just api` run?",
            many.len()
        )),
    }
}

/// What the writer hands rustdoc: the tokens after the `--` separator.
fn flags(text: &str) -> Result<Vec<String>, String> {
    let line = invocation(text)?;
    let mut tokens = line.split_whitespace().skip_while(|token| *token != "--");
    if tokens.next().is_none() {
        return Err(format!(
            "{WRITER}'s `{INVOCATION}` line has no `--` separator, so it passes rustdoc nothing"
        ));
    }
    let found: Vec<String> = tokens.map(String::from).collect();
    if found.is_empty() {
        return Err(format!(
            "{WRITER}'s `{INVOCATION}` line ends at `--` and names no rustdoc flag"
        ));
    }
    Ok(found)
}

/// `Ok` with the flags both venues pass, or why the two cannot be shown to agree.
///
/// A precondition rather than a finding, for [`super::lints`]' reason: pages regenerated from a
/// rustdoc run whose flags nobody checked are pages compared against an unknown question.
pub(super) fn check(root: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(root.join(WRITER)).map_err(|error| format!("could not read {WRITER}: {error}"))?;
    let theirs = flags(&text)?;
    let mine: Vec<String> = RUSTDOC_ARGS.iter().map(|flag| String::from(*flag)).collect();
    if theirs == mine {
        return Ok(mine);
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
    use super::{RUSTDOC_ARGS, WRITER, check, flags};
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
    fn the_flags_are_read_from_a_continued_shell_line() {
        let text = concat!(
            "  # cargo rustdoc is named in this comment\n",
            "  cargo rustdoc -q -p \"$lib\" --profile ci -- \\\n",
            "    -Z unstable-options --output-format json\n"
        );
        assert_eq!(
            flags(text).expect("one invocation"),
            ["-Z", "unstable-options", "--output-format", "json"]
        );
    }

    /// FAIL CLOSED on every shape that leaves nothing to compare. A comparison with an empty or
    /// unfound left side is the way this kind of gate goes green over two lists that disagree.
    #[test]
    fn a_writer_this_cannot_read_is_a_refusal_rather_than_agreement() {
        for (text, why) in [
            ("pkgs.writeShellApplication { }\n", "no invocation"),
            ("cargo rustdoc -p a -- --x\ncargo rustdoc -p b -- --y\n", "two invocations"),
            ("cargo rustdoc -q -p x --all-features\n", "no separator"),
            ("cargo rustdoc -q -p x --\n", "nothing after the separator"),
        ] {
            assert!(flags(text).is_err(), "{why} must refuse");
        }
    }
}
