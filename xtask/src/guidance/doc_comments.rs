//! A `///` or `//!` line in a library crate is read by the wording, task, recipe and path rules.
//!
//! The doc comments are published on the API reference, which nothing commits, so these cells hold
//! the one place a stale sentence there can still be caught. Each drives [`super::tree_problems`]
//! over a fixture tree, because the rules each have tests of their own and the CALL, which is what
//! feeds them a `.rs` file at all, had none.

const WITHDRAWN_WORDING: &str = "because a buildless database extracts the crate's own dependencies but not";

/// The fixture files as the tree builder takes them: a path and its bytes.
type Files<'a> = Vec<(&'a str, &'a [u8])>;

fn problems_over(name: &str, files: &[(&str, &str)]) -> Vec<String> {
    let bytes: Files<'_> = files.iter().map(|(path, text)| (*path, text.as_bytes())).collect();
    let tree = crate::scratch_tree::Tree::of(name, &bytes);
    let read = |rel: &str| std::fs::read_to_string(tree.root().join(rel)).ok();
    let listed: Vec<String> = files.iter().map(|(path, _)| String::from(*path)).collect();
    let (problems, ..) = super::tree_problems(tree.root(), &read, &read, &listed, &[]);
    problems
}

#[test]
fn a_withdrawn_wording_is_refused_in_a_doc_comment_and_in_no_other_line_of_rust() {
    let lib =
        format!("/// It holds {WITHDRAWN_WORDING} alloc.\n// {WITHDRAWN_WORDING}\nconst S: &str = \"{WITHDRAWN_WORDING}\";\n");
    let found = problems_over(
        "doc-comments-wording",
        &[("crates/x/src/lib.rs", &lib), ("xtask/src/rules.rs", &lib)],
    );
    let hits: Vec<&String> = found.iter().filter(|p| p.contains("was withdrawn")).collect();
    assert_eq!(hits.len(), 1, "{found:#?}");
    assert!(hits[0].starts_with("crates/x/src/lib.rs:1:"), "{hits:?}");
}

#[test]
fn a_task_and_a_recipe_that_do_not_exist_are_refused_in_a_doc_comment() {
    let found = problems_over(
        "doc-comments-citations",
        &[
            ("justfile", "build:\n    true\n"),
            (
                "dev/src/lib.rs",
                "//! Run `cargo xtask no-such-gate`, or `just no-such-recipe`.\n",
            ),
        ],
    );
    assert!(
        found.iter().any(|p| p.contains("`no-such-gate` is not an xtask task")),
        "{found:#?}"
    );
    assert!(
        found.iter().any(|p| p.contains("`just no-such-recipe` is not a just task")),
        "{found:#?}"
    );
}

#[test]
fn a_path_that_does_not_exist_is_refused_in_a_doc_comment() {
    let found = problems_over(
        "doc-comments-paths",
        &[("crates/x/src/lib.rs", "/// See `docs/does-not-exist.md`.\n")],
    );
    assert!(
        found
            .iter()
            .any(|p| p.starts_with("crates/x/src/lib.rs:1:") && p.contains("`docs/does-not-exist.md` does not exist")),
        "{found:#?}"
    );
}
