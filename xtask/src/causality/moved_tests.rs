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
