//! The two shipped-feature-set gates clean first-party artifacts before building (#1047).
//!
//! `check-default-features` and `check-default-feature-tests` share `target/causality-target`
//! with the causality gate, which gets a profile-scoped `cargo clean --workspace` from
//! [`crate::causality::isolation::Isolated::of`] before every run. Without the same clean the other
//! two can compile and pass over first-party artifacts another tree left there.
//!
//! POSITION, NOT PRESENCE: a gate that moved the clean after its build would still contain the
//! call. So this reads the byte offset of the CALL - `Isolated::of(`, with the parenthesis - against
//! the first BUILD invocation: the first `Command::new("cargo")` whose statement's first string
//! literal is not `"tree"`. `cargo tree` is resolve-only and writes no artifacts, so it may precede
//! the clean.
//!
//! Comments and MULTI-LINE string interiors come out first, through
//! [`crate::serde_parse::scan::code_lines`] as in `boot_order`, so prose or a literal spanning lines
//! that names the call cannot satisfy it. **A single-line string literal is NOT removed** - the shared
//! lexer keeps its content by contract - so a one-line literal spelling `Isolated::of(` above the
//! build is a live anchor whether the real call moved or was deleted. Neither gate file contains one
//! today, checked over every occurrence of `Isolated::of`, so this is a limit and not an open hole.
//!
//! WHAT IT DOES NOT REACH: it reads TEXT order, not execution order. A build moved into a helper
//! defined above `run`, or a clean moved behind a condition, is outside it; a gate with no build
//! invocation left to find is refused rather than passed. The behaviour itself - that the clean
//! runs before the first compile - is held by `xtask/tests/default_features.rs` for
//! `check-default-features` only; `check-default-feature-tests` is held by this textual check alone.

use std::path::Path;

use crate::serde_parse::scan::code_lines;

/// The gates that share `target/causality-target` with `causality` and must clean first.
const ISOLATED_GATES: &[(&str, &str)] = &[
    ("check-default-features", "xtask/src/default_features.rs"),
    ("check-default-feature-tests", "xtask/src/default_feature_tests.rs"),
];

/// The call whose position this reads, as a call rather than as a name in a comment.
const ISOLATED_CALL: &str = "Isolated::of(";

/// How a cargo invocation begins in both gates.
const CARGO_COMMAND: &str = "Command::new(\"cargo\")";

/// The only cargo subcommand that is resolve-only and may precede the clean.
const RESOLVE_ONLY: &str = "tree";

/// How many warmed gates call `Isolated::of(` before their first build invocation.
pub(super) fn holds(root: &Path) -> Result<usize, String> {
    for (gate, source) in ISOLATED_GATES {
        let text = std::fs::read_to_string(root.join(source)).map_err(|error| format!("could not read {source}: {error}"))?;
        clean_precedes_build(&text).map_err(|why| {
            format!(
                "{source} ({gate}) {why}.\n  It shares `target/causality-target` with the causality gate, so without that clean\n  \
                 it can compile and pass over another tree's first-party artifacts - #1047."
            )
        })?;
    }
    Ok(ISOLATED_GATES.len())
}

/// Whether a gate's source calls `Isolated::of(` before its first build invocation, read over its
/// [`code_lines`] image joined back with `\n` so a multi-line statement still reads its subcommand.
fn clean_precedes_build(text: &str) -> Result<(), &'static str> {
    let code = code_lines(text).join("\n");
    let call = code.find(ISOLATED_CALL).ok_or("no longer calls `Isolated::of`")?;
    let build = first_build_invocation(&code).ok_or("has no build invocation for `Isolated::of` to precede")?;
    if call > build {
        return Err("calls `Isolated::of` after its first build invocation");
    }
    Ok(())
}

/// The byte offset of the first `Command::new("cargo")` whose statement is not `cargo tree`.
///
/// The statement ends at the next `;`, so an unrelated literal on a later line is not read as the
/// subcommand. `.args(invocation(...))` names no literal, so it is a build.
fn first_build_invocation(text: &str) -> Option<usize> {
    text.match_indices(CARGO_COMMAND).map(|(at, _)| at).find(|&at| {
        let rest = text.get(at + CARGO_COMMAND.len()..).unwrap_or_default();
        let statement = rest.split(';').next().unwrap_or_default();
        statement.split('"').nth(1) != Some(RESOLVE_ONLY)
    })
}

#[cfg(test)]
mod tests {
    use super::clean_precedes_build;

    const TREE: &str =
        "let out = Command::new(\"cargo\")\n    .args([\n        \"tree\",\n        \"--offline\",\n    ])\n    .output();\n";
    const CLEAN: &str = "match crate::causality::isolation::Isolated::of(&root, target) {}\n";
    const BUILD: &str = "let mut command = Command::new(\"cargo\");\ncommand.args(invocation(pass, package, profile));\n";

    #[test]
    fn a_resolve_only_tree_may_precede_the_clean_and_the_build_may_not() {
        assert_eq!(clean_precedes_build(&[TREE, CLEAN, BUILD].concat()), Ok(()));
        assert!(clean_precedes_build(&[TREE, BUILD, CLEAN].concat()).is_err());
    }

    #[test]
    fn a_comment_left_behind_by_a_moved_clean_does_not_hold_it() {
        for comment in [
            "// the clean `Isolated::of` performs\n",
            "// the clean `Isolated::of(..)` performs\n",
        ] {
            assert!(clean_precedes_build(&[comment, BUILD, CLEAN].concat()).is_err(), "{comment}");
            assert!(clean_precedes_build(&[comment, BUILD].concat()).is_err(), "{comment}");
        }
    }

    #[test]
    fn a_multiline_string_spelling_the_call_does_not_hold_it() {
        let note = "let note = \"the clean Isolated::of(\nperforms the removal\";\n";
        assert!(clean_precedes_build(&[note, BUILD, CLEAN].concat()).is_err());
        assert!(clean_precedes_build(&[note, BUILD].concat()).is_err());
    }

    #[test]
    fn a_tree_literal_past_the_statement_is_not_the_subcommand() {
        let src = [CLEAN, "let command = Command::new(\"cargo\");\nlet unrelated = \"tree\";\n"].concat();
        assert_eq!(clean_precedes_build(&src), Ok(()));
        let src = ["let command = Command::new(\"cargo\");\nlet unrelated = \"tree\";\n", CLEAN].concat();
        assert!(clean_precedes_build(&src).is_err());
    }

    #[test]
    fn a_gate_without_the_call_or_without_a_build_is_refused() {
        assert!(clean_precedes_build(&[TREE, BUILD].concat()).is_err());
        assert!(clean_precedes_build(&[TREE, CLEAN].concat()).is_err());
    }

    #[test]
    fn both_warmed_gates_isolate_before_their_first_build() {
        let root = crate::repo::root().expect("the repo root");
        assert_eq!(super::holds(&root), Ok(super::ISOLATED_GATES.len()));
    }
}
