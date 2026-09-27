//! `edited::moved` at the level a caller sees: what `plan_with_base` answers over a range that
//! moves tests across files. Every cell moves at least one test unchanged, so each is red on a tree
//! that reads every move as a deletion.

use crate::causality::diff::ChangedFile;
use crate::causality::fixtures::{changed_removing, tree};
use crate::causality::plan::{DeletedFrom, Plan, plan_with_base};

const FOO: &str = "#[test]\nfn foo() {\n    assert_eq!(2 + 2, 4);\n}\n";
const BAR: &str = "#[test]\nfn bar() {\n    assert_eq!(3 + 3, 6);\n}\n";

/// `before` to `after` as one git hunk reads it: the common head and tail kept, the middle replaced.
fn diffed(path: &str, before: &str, after: &str) -> ChangedFile {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    changed_removing(
        path,
        head + 1,
        &new[head..new.len() - tail],
        head + 1,
        &old[head..old.len() - tail],
    )
}

/// The plan over `[path, base image, post image]`; an empty base image is a new file.
fn plan_of(files: &[[&str; 3]]) -> Plan {
    let changed: Vec<ChangedFile> = files
        .iter()
        .map(|[path, before, after]| diffed(path, before, after))
        .collect();
    let bases: Vec<(&str, &str)> = files
        .iter()
        .filter(|[_, before, _]| !before.is_empty())
        .map(|[path, before, _]| (*path, *before))
        .collect();
    let posts: Vec<(&str, &str)> = files.iter().map(|[path, _, after]| (*path, *after)).collect();
    plan_with_base(&changed, &tree(&posts), &tree(&bases))
}

fn deleted(path: &str, tests: &[&str]) -> DeletedFrom {
    DeletedFrom {
        path: String::from(path),
        tests: tests.iter().map(|name| String::from(*name)).collect(),
    }
}

#[test]
fn a_test_moved_unchanged_to_another_file_is_not_a_deletion() {
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{FOO}{BAR}"), BAR],
        ["crates/x/tests/c.rs", "", FOO],
    ]);
    assert!(
        matches!(plan, Plan::Separable(_)),
        "a pure move must reach the proof, got {plan:?}"
    );
}

/// The smuggling shape a name-keyed match lets through: `a.rs` weakens its `foo` in place while
/// `b.rs` moves an identical `foo` to `c.rs`. The one copy excuses the one move, and `a.rs` sorts
/// first, so a match that did not refuse a rewritten name would spend the copy on `a.rs` instead.
#[test]
fn a_move_does_not_excuse_a_same_named_test_weakened_elsewhere() {
    let weakened = "#[test]\nfn foo() {\n}\n";
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", FOO, weakened],
        ["crates/x/tests/b.rs", FOO, ""],
        ["crates/x/tests/c.rs", "", FOO],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["foo"])]));
}

#[test]
fn one_moved_copy_excuses_only_one_of_two_identical_deletions() {
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", FOO, ""],
        ["crates/x/tests/b.rs", FOO, ""],
        ["crates/x/tests/c.rs", "", FOO],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/b.rs", &["foo"])]));
}

#[test]
fn a_moved_test_with_one_changed_literal_is_still_a_deletion() {
    let changed = BAR.replace('6', "7");
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{FOO}{BAR}"), ""],
        ["crates/x/tests/c.rs", "", &format!("{FOO}{changed}")],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["bar"])]));
}

#[test]
fn a_test_moved_and_renamed_is_still_a_deletion() {
    let renamed = BAR.replace("bar", "baz");
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{FOO}{BAR}"), ""],
        ["crates/x/tests/c.rs", "", &format!("{FOO}{renamed}")],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["bar"])]));
}

/// `foo` moves unchanged, but the helper it calls is weakened at its new home: the test's own
/// tokens match, so only comparing the helper as an item of its own keeps `foo` named.
#[test]
fn a_test_moved_unchanged_whose_helper_changed_on_the_way_is_still_a_deletion() {
    let calls = "#[test]\nfn foo() {\n    assert!(helper());\n}\n";
    let helper = "fn helper() -> bool {\n    2 + 2 == 4\n}\n";
    let weakened = "fn helper() -> bool {\n    true\n}\n";
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{helper}{calls}{BAR}"), ""],
        ["crates/x/tests/c.rs", "", &format!("{weakened}{calls}{BAR}")],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["foo"])]));
}

/// `reflowed` at base: a comment and a method chain rustfmt broke over three lines.
const REFLOWED: &str = "#[test]\nfn reflowed() {\n    // the sum\n    let total = [1, 2]\n        .iter()\n        .sum::<i32>();\n    assert_eq!(total, 3);\n}\n";
/// The same test moved: its comment edited and the chain joined onto one line.
const REJOINED: &str = "#[test]\nfn reflowed() {\n    // the sum of both\n    let total = [1, 2].iter().sum::<i32>();\n    assert_eq!(total, 3);\n}\n";

#[test]
fn an_unchanged_move_with_an_edited_comment_and_a_reflow_is_not_a_deletion() {
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{REFLOWED}{BAR}"), BAR],
        ["crates/x/tests/c.rs", "", REJOINED],
    ]);
    assert!(
        matches!(plan, Plan::Separable(_)),
        "comments and layout are not the key, got {plan:?}"
    );
}

/// A helper moved into a sibling module gains `pub(super)` so its callers still reach it.
#[test]
fn a_moved_helper_that_gains_pub_super_is_not_a_deletion() {
    let calls = "#[test]\nfn foo() {\n    assert!(helper());\n}\n";
    let helper = "fn helper() -> bool {\n    2 + 2 == 4\n}\n";
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{helper}{calls}{BAR}"), BAR],
        ["crates/x/tests/c.rs", "", &format!("pub(super) {helper}{calls}")],
    ]);
    assert!(
        matches!(plan, Plan::Separable(_)),
        "the item's own visibility is not the key, got {plan:?}"
    );
}

/// Beside a move only the token key recognises, so the refusal is the literal's alone.
#[test]
fn a_moved_test_with_one_changed_literal_is_refused_beside_a_reflowed_move() {
    let changed = BAR.replace('6', "7");
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{REFLOWED}{BAR}"), ""],
        ["crates/x/tests/c.rs", "", &format!("{REJOINED}{changed}")],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["bar"])]));
}

/// The same name reached by another path may be another function.
#[test]
fn a_moved_test_whose_call_path_changed_is_refused_beside_a_reflowed_move() {
    let before = "#[test]\nfn looks_up() {\n    assert!(lookup_source().is_some());\n}\n";
    let after = before.replace("lookup_source", "super::corpus::lookup_source");
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{REFLOWED}{before}"), ""],
        ["crates/x/tests/c.rs", "", &format!("{REJOINED}{after}")],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["looks_up"])]));
}

/// A test moved out of an inline module is dedented, and so is a `\`-continued string in it:
/// its source changes, its value does not. The edited comment is what the whitespace key refused.
#[test]
fn a_moved_test_whose_continued_string_was_reindented_is_not_a_deletion() {
    let inline = "mod inner {\n    #[test]\n    fn says() {\n        // one line\n        assert_eq!(\"a \\\n                    b\", \"a b\");\n    }\n}\n";
    let file = "#[test]\nfn says() {\n    // one line, continued\n    assert_eq!(\"a \\\n                b\", \"a b\");\n}\n";
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{BAR}{inline}"), BAR],
        ["crates/x/tests/c.rs", "", file],
    ]);
    assert!(
        matches!(plan, Plan::Separable(_)),
        "a continuation is keyed by value, got {plan:?}"
    );
}

/// `#[doc(hidden)]` is an attribute, not a comment: a copy that gains it is not the same item.
#[test]
fn a_moved_test_that_gains_doc_hidden_is_refused_beside_a_reflowed_move() {
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{REFLOWED}{FOO}"), ""],
        ["crates/x/tests/c.rs", "", &format!("{REJOINED}#[doc(hidden)]\n{FOO}")],
    ]);
    assert_eq!(plan, Plan::DeletedTests(vec![deleted("crates/x/tests/a.rs", &["foo"])]));
}

/// `github.com/telekom/sutura#1103`, the shape a split at the line cap takes: the tests move to one
/// new module and the helpers they share to another that names no test. Reverting that one deletes
/// a file the kept target still declares, so the base does not build and no moved test is measured.
#[test]
fn a_helper_moved_into_a_new_path_module_stays_declared_at_base() {
    let helper = "fn four() -> i32 {\n    2 + 2\n}\n";
    let head = "#[path = \"a/harness.rs\"]\nmod harness;\n#[cfg(test)]\n#[path = \"a/moved.rs\"]\nmod moved;\n";
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{helper}{FOO}{BAR}"), &format!("{head}{BAR}")],
        ["crates/x/tests/a/harness.rs", "", helper],
        ["crates/x/tests/a/moved.rs", "", FOO],
    ]);
    let Plan::Separable(separable) = plan else {
        panic!("a pure move must reach the proof, got {plan:?}");
    };
    assert_eq!(separable.test_files, ["crates/x/tests/a.rs", "crates/x/tests/a/moved.rs"]);
    assert!(
        !separable.revert.contains(&String::from("crates/x/tests/a/harness.rs")),
        "a declared module must not be deleted from the base tree: {separable:?}"
    );
}

/// The same split's other half: a sibling test module that existed at base follows the helpers to
/// their new module, changing only its `use` line. Restoring it puts base imports under a parent
/// kept at HEAD, which no longer has them, and the base fails to build the same way.
#[test]
fn a_sibling_test_module_that_follows_a_moved_helper_stays_at_head() {
    let helper = "pub fn four() -> i32 {\n    2 + 2\n}\n";
    let sibling = "#[cfg(test)]\nmod kinds;\n";
    let head = format!("#[path = \"a/harness.rs\"]\nmod harness;\n{sibling}#[cfg(test)]\n#[path = \"a/moved.rs\"]\nmod moved;\n");
    let plan = plan_of(&[
        ["crates/x/tests/a.rs", &format!("{helper}{sibling}{FOO}"), &head],
        ["crates/x/tests/a/harness.rs", "", helper],
        ["crates/x/tests/a/moved.rs", "", FOO],
        [
            "crates/x/tests/a/kinds.rs",
            &format!("use super::four;\n{BAR}"),
            &format!("use super::harness::four;\n{BAR}"),
        ],
    ]);
    let Plan::Separable(separable) = plan else {
        panic!("a pure move must reach the proof, got {plan:?}");
    };
    assert!(
        !separable.revert.contains(&String::from("crates/x/tests/a/kinds.rs")),
        "a test module declared by a kept test file must stay beside it: {separable:?}"
    );
}

/// The added-file rule on its own: the new module sits under `src/`, so it is not whole-file test
/// code, and its declarer is held rather than a test file, so the test-code rule cannot fire.
#[test]
fn an_added_src_module_a_held_file_declares_stays_at_head() {
    let before = "pub fn one() -> i32 {\n    1\n}\n";
    let after = format!("{before}#[cfg(test)]\npub(crate) fn helper() {{}}\nmod extra;\n");
    let plan = plan_of(&[
        ["crates/x/src/support.rs", before, &after],
        ["crates/x/src/support/extra.rs", "", "pub fn two() -> i32 {\n    2\n}\n"],
        ["crates/x/tests/a.rs", "", FOO],
    ]);
    let Plan::Separable(separable) = plan else {
        panic!("an added test must reach the proof, got {plan:?}");
    };
    assert_eq!(separable.test_only, ["crates/x/src/support.rs"]);
    assert!(
        !separable.revert.contains(&String::from("crates/x/src/support/extra.rs")),
        "an added module a held file declares must not be deleted from the base tree: {separable:?}"
    );
}
