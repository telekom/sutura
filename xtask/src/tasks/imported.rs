//! The readers in `tasks` read the files the justfile imports, not the root alone.

use std::path::PathBuf;

use super::{Source, problems, recipe_body, recipe_names};
use crate::scratch_tree::Tree;

fn source(path: &str, text: &str) -> Source {
    Source {
        path: PathBuf::from(path),
        text: String::from(text),
    }
}

fn split_tree(tag: &str) -> Tree {
    Tree::of(
        &format!("tasks-imported-{tag}"),
        &[
            ("justfile", b"import \"just/docs.just\"\n\nlint:\n    echo hi\n"),
            ("just/docs.just", b"docs-list:\n    echo listed\n"),
        ],
    )
}

#[test]
fn a_recipe_declared_only_in_an_imported_file_is_a_recipe_name() {
    let names = recipe_names(split_tree("names").root()).expect("the set reads");
    assert!(names.contains("lint") && names.contains("docs-list"), "{names:?}");
}

#[test]
fn a_recipe_body_is_read_from_the_imported_file_that_declares_it() {
    let body = recipe_body(split_tree("body").root(), "docs-list").unwrap_or_default();
    assert!(body.iter().any(|line| line.contains("echo listed")), "{body:?}");
}

#[test]
fn an_unescaped_backtick_in_an_imported_file_is_reported_with_that_file_and_line() {
    let set = [
        source("justfile", "lint:\n    cargo clippy --workspace\n"),
        source("just/docs.just", "docs-list:\n    echo \"banner: `exit 7`\"\n"),
    ];
    let found = problems(&set);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found
            .iter()
            .all(|message| message.starts_with("just/docs.just:2: an unescaped backtick")),
        "{found:?}"
    );
}

#[test]
fn a_citation_of_a_recipe_in_another_file_is_not_flagged_and_a_missing_one_is() {
    let set = [
        source(
            "justfile",
            "lint:\n    cargo clippy --workspace\n    echo 'see `just docs-list`'\n",
        ),
        source("just/docs.just", "docs-list:\n    echo 'and `just gone`'\n"),
    ];
    let found = problems(&set);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found
            .iter()
            .all(|message| message.starts_with("just/docs.just:2: `just gone` names no recipe")),
        "{found:?}"
    );
}
