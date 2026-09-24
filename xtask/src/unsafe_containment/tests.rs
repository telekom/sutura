//! The gate's own rules, each driven through [`super::check`] over a scratch tree.
//!
//! **A tree per rule and never the repository**, for the reason `falsifier`'s header gives: the one
//! tree that makes every assertion here vacuous is the one that already obeys the rules. The real
//! tree is read once, at the bottom, and only to assert that the gate and the workspace agree.

use std::path::{Path, PathBuf};

use super::{EXCEPTED_FILE, EXCEPTED_ROOT, Why, check, lowerings, member_paths, reassertion};

/// A file that lowers the lint, built from [`lowerings`] rather than written out.
///
/// **For the gate's own reason:** a fixture spelling the attribute whole would make this file one
/// of the findings - it did, measured, before the needles were assembled from parts.
fn lowers() -> String {
    format!("{}, reason = \"a fixture\")]\nfn f() {{}}\n", lowerings()[0])
}

/// The same with the `allow` spelling, which differs only in whether an unfulfilled one warns.
fn lowers_by_allowing() -> String {
    format!("{})]\nfn f() {{}}\n", lowerings()[1])
}

/// A scratch workspace root keyed on the test's own name, removed first.
///
/// Keyed and removed for `falsifier_tree`'s measured reason: a process id is reusable, so a tree
/// an earlier run left behind would seed files this test never declared.
fn tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sutura-unsafe-{}-{name}", std::process::id()));
    drop(std::fs::remove_dir_all(&root));
    std::fs::create_dir_all(&root).expect("a scratch root");
    root
}

/// Writes one file, creating its parents.
fn put(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("a seeded file has a parent")).expect("a parent directory");
    std::fs::write(path, contents).expect("a seeded file");
}

/// A one-member workspace whose single library root carries `body`.
fn one_member(name: &str, body: &str) -> PathBuf {
    let root = tree(name);
    put(&root, "Cargo.toml", "[workspace]\nmembers = [\n  \"crates/one\",\n]\n");
    put(&root, "crates/one/Cargo.toml", "[package]\nname = \"one\"\n");
    put(&root, "crates/one/src/lib.rs", body);
    root
}

#[test]
fn a_root_missing_the_reassertion_is_named() {
    let root = one_member("unguarded", "pub fn reachable() {}\n");
    let why = check(&root).expect_err("a root with no forbid is the gate's own rule");
    drop(std::fs::remove_dir_all(&root));
    assert!(
        matches!(&why, Why::Unguarded(paths) if paths == &["crates/one/src/lib.rs".to_owned()]),
        "{why:?}"
    );
}

#[test]
fn a_root_that_reasserts_the_forbid_passes() {
    let root = one_member("guarded", &format!("{}\npub fn reachable() {{}}\n", reassertion()));
    let scanned = check(&root).expect("a guarded root is the whole rule");
    drop(std::fs::remove_dir_all(&root));
    assert_eq!(scanned.roots, 1);
    assert_eq!(scanned.members, 1);
}

#[test]
fn the_attribute_named_only_in_a_comment_does_not_guard_a_root() {
    // **The reason the scan blanks comments, as a cell rather than as a sentence.** This
    // repository's prose quotes the attribute in fourteen places, so a gate that counted a
    // mention would read every one of them as a declaration - and a root whose attribute was
    // deleted in a comment trim would stay green.
    let root = one_member(
        "commented",
        &format!("//! We set {} everywhere.\npub fn reachable() {{}}\n", reassertion()),
    );
    let why = check(&root).expect_err("a comment is not a declaration");
    drop(std::fs::remove_dir_all(&root));
    assert!(matches!(why, Why::Unguarded(_)), "{why:?}");
}

#[test]
fn an_integration_test_root_is_judged_beside_the_library() {
    // Each integration target is its own crate with its own root, so a `forbid` in `src/lib.rs`
    // says nothing about `tests/`. A gate that read only library roots would leave every test
    // target at the lowerable `deny`.
    let root = one_member("integration", &format!("{}\npub fn reachable() {{}}\n", reassertion()));
    put(&root, "crates/one/tests/venue.rs", "#[test]\nfn t() {}\n");
    let why = check(&root).expect_err("a test root with no forbid is unguarded");
    drop(std::fs::remove_dir_all(&root));
    assert!(
        matches!(&why, Why::Unguarded(paths) if paths == &["crates/one/tests/venue.rs".to_owned()]),
        "{why:?}"
    );
}

#[test]
fn a_file_lowering_the_lint_outside_the_declared_one_is_named() {
    let root = one_member("lowered", &format!("{}\npub mod sneaky;\n", reassertion()));
    put(&root, "crates/one/src/sneaky.rs", &lowers());
    let why = check(&root).expect_err("an undeclared lowering is the second rule");
    drop(std::fs::remove_dir_all(&root));
    assert!(
        matches!(&why, Why::Lowered(paths) if paths == &["crates/one/src/sneaky.rs".to_owned()]),
        "{why:?}"
    );
}

#[test]
fn an_allow_lowers_the_lint_as_much_as_an_expect_does() {
    // `#[allow]` and `#[expect]` differ only in whether an unfulfilled one warns, so a gate that
    // refused the second and not the first would refuse the tidier spelling and permit the other.
    let root = one_member("allowed", &format!("{}\npub mod sneaky;\n", reassertion()));
    put(&root, "crates/one/src/sneaky.rs", &lowers_by_allowing());
    let why = check(&root).expect_err("an allow is a lowering");
    drop(std::fs::remove_dir_all(&root));
    assert!(matches!(why, Why::Lowered(_)), "{why:?}");
}

#[test]
fn a_workspace_declaring_no_member_is_refused_rather_than_swept_clean() {
    let root = tree("empty");
    put(&root, "Cargo.toml", "[workspace]\nmembers = []\n");
    let why = check(&root).expect_err("nothing to judge is not a clean bill");
    drop(std::fs::remove_dir_all(&root));
    assert!(matches!(why, Why::NoMembers), "{why:?}");
}

#[test]
fn a_member_with_no_crate_root_at_all_is_refused() {
    let root = tree("rootless");
    put(&root, "Cargo.toml", "[workspace]\nmembers = [\n  \"crates/one\",\n]\n");
    put(&root, "crates/one/Cargo.toml", "[package]\nname = \"one\"\n");
    let why = check(&root).expect_err("a scan that read nothing is not a pass");
    drop(std::fs::remove_dir_all(&root));
    assert!(matches!(why, Why::NoRoots), "{why:?}");
}

#[test]
fn the_declared_exception_may_omit_the_reassertion_and_must_use_it() {
    // BOTH halves of the exception, in one cell because they are one bargain: the excepted root is
    // allowed to go without the attribute precisely because one named file under it lowers the
    // lint. Without that file the exception is a widening nobody spends, and the gate says so.
    let root = tree("exception");
    put(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\n  \"crates/sutura-exec-bigquery\",\n]\n",
    );
    put(&root, "crates/sutura-exec-bigquery/Cargo.toml", "[package]\nname = \"b\"\n");
    put(&root, EXCEPTED_ROOT, "pub mod adbc;\n");
    let why = check(&root).expect_err("an exception that lowers nothing is stale");
    assert!(matches!(why, Why::ExceptionEmpty), "{why:?}");

    put(&root, EXCEPTED_FILE, &lowers());
    let scanned = check(&root).expect("the declared exception, spent where it was declared");
    assert_eq!(scanned.roots, 1, "the excepted root is still counted as read");
    drop(std::fs::remove_dir_all(&root));
}

#[test]
fn an_exception_the_tree_no_longer_needs_is_refused_rather_than_left_standing() {
    let root = tree("stale");
    put(
        &root,
        "Cargo.toml",
        "[workspace]\nmembers = [\n  \"crates/sutura-exec-bigquery\",\n]\n",
    );
    put(&root, "crates/sutura-exec-bigquery/Cargo.toml", "[package]\nname = \"b\"\n");
    put(&root, EXCEPTED_ROOT, &format!("{}\npub fn f() {{}}\n", reassertion()));
    let why = check(&root).expect_err("a root that re-asserts the forbid needs no exception");
    drop(std::fs::remove_dir_all(&root));
    assert!(matches!(why, Why::ExceptionUnused), "{why:?}");
}

#[test]
fn a_member_list_naming_a_crate_in_a_comment_does_not_read_it_as_a_member() {
    let manifest = "[workspace]\nmembers = [\n  # prose about \"crates/ghost\"\n  \"crates/real\",\n]\n";
    assert_eq!(member_paths(manifest), vec![String::from("crates/real")]);
}

#[test]
fn the_real_tree_agrees_with_this_gate() {
    // The gate against the tree it guards, so a refactor of the walk cannot pass its own fixtures
    // and fail the repository - `xtask/src/shipped.rs`'s own suite does the same. It also pins the
    // two declarations to real paths: an exception naming a file that moved would be an exception
    // holding nothing, and `EXCEPTION_EMPTY` would not fire if the root moved too.
    let Some(root) = crate::repo::root() else {
        return;
    };
    let scanned = check(&root).expect("the workspace this gate guards");
    assert!(scanned.members >= 20, "{} member(s) read", scanned.members);
    assert!(scanned.roots >= 60, "{} crate root(s) read", scanned.roots);
    assert!(root.join(EXCEPTED_ROOT).is_file(), "the excepted root moved");
    assert!(root.join(EXCEPTED_FILE).is_file(), "the excepted file moved");
}
