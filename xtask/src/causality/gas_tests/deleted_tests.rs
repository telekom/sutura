//! `causality::run` over a diff that deletes a test file whole. The deleted file has no post-image,
//! so the diff lists nothing for it, and the rule for a test removed from a file that stays
//! (a `Weakens-Test:` trailer naming it) has to hold for the lines it held all the same. Both
//! rules hold under a diff attribute too, because the gate reads the diff as text, and the diff
//! readers' own cells sit here beside them.

use std::path::PathBuf;

use super::{committed_tree, git, git_output};
use crate::Verdict;
use crate::causality::diff::{ChangedFile, changes, commit_additions};
use crate::causality::provenance::Commit;

const LIB: &str = "pub fn f() -> u8 { 1 }\n";
const PINS: &str = "#[test]\nfn pins_f() {\n    assert_eq!(wired::f(), 1);\n}\n\n#[test]\nfn pins_f_again() {\n    assert_ne!(wired::f(), 0);\n}\n";

/// The verdict over a head that deletes `deleted` (holding the two tests in [`PINS`]) and adds the
/// `added` files, with one `Weakens-Test:` trailer per name in `waived`.
fn deleting(deleted: &str, added: &[(&str, &str)], waived: &[&str]) -> Verdict {
    let trailers: String = waived
        .iter()
        .map(|name| ["Weakens-Test: ", name, " - superseded\n"].concat())
        .collect();
    committed_tree(
        &format!("test: drop {deleted}\n\n{trailers}"),
        &[("src/lib.rs", LIB), (deleted, PINS)],
        &[&[("src/lib.rs", LIB)][..], added].concat(),
    )
}

/// The verdicts with no trailer, with one naming only `pins_f`, and with one per test.
fn trailer_ladder(deleted: &str, added: &[(&str, &str)]) -> (Verdict, Verdict, Verdict) {
    (
        deleting(deleted, added, &[]),
        deleting(deleted, added, &["pins_f"]),
        deleting(deleted, added, &["pins_f", "pins_f_again"]),
    )
}

/// Named, not blanket: a trailer for one of the two deleted tests leaves the other refused.
#[test]
fn a_test_file_deleted_whole_needs_a_trailer_naming_each_test() {
    assert_eq!(
        trailer_ladder("tests/pins.rs", &[]),
        (Verdict::Fail, Verdict::Fail, Verdict::Pass)
    );
}

/// Git lists `tests/a.rs`, deleted, ahead of the added `tests/zz.rs`: the next `diff --git` line
/// ends the deletion, not the end of the diff.
#[test]
fn a_test_file_deleted_ahead_of_another_changed_path_is_held_the_same() {
    assert_eq!(
        trailer_ladder("tests/a.rs", &[("tests/zz.rs", "pub fn aux() {}\n")]),
        (Verdict::Fail, Verdict::Fail, Verdict::Pass)
    );
}

/// Marks every Rust file `-diff`, so a plain `git diff` prints no hunk for any of them.
const NO_DIFF: (&str, &str) = (".gitattributes", "*.rs -diff\n");

/// The verdicts with no trailer and with one per name in `waived`, over a head that turns the
/// base's `tests/pins.rs` (holding [`PINS`]) into `head` - `None` deletes it - under [`NO_DIFF`].
fn under_no_diff(head: Option<&str>, waived: &[&str]) -> (Verdict, Verdict) {
    let base = [NO_DIFF, ("src/lib.rs", LIB), ("tests/pins.rs", PINS)];
    let kept = head.map(|content| ("tests/pins.rs", content));
    let after: Vec<(&str, &str)> = [NO_DIFF, ("src/lib.rs", LIB)].into_iter().chain(kept).collect();
    let trailers: String = waived
        .iter()
        .map(|name| ["Weakens-Test: ", name, " - superseded\n"].concat())
        .collect();
    (
        committed_tree("test: drop a pin\n", &base, &after),
        committed_tree(&format!("test: drop a pin\n\n{trailers}"), &base, &after),
    )
}

#[test]
fn a_test_file_deleted_whole_under_a_diff_attribute_is_still_held() {
    assert_eq!(
        under_no_diff(None, &["pins_f", "pins_f_again"]),
        (Verdict::Fail, Verdict::Pass)
    );
}

#[test]
fn a_test_removed_from_a_kept_file_under_a_diff_attribute_is_still_held() {
    let kept = "#[test]\nfn pins_f() {\n    assert_eq!(wired::f(), 1);\n}\n";
    assert_eq!(under_no_diff(Some(kept), &["pins_f_again"]), (Verdict::Fail, Verdict::Pass));
}

#[test]
fn a_deleted_test_file_with_an_unpinned_header_is_refused() {
    assert_eq!(
        trailer_ladder("tests/q\"s.rs", &[]),
        (Verdict::Fail, Verdict::Fail, Verdict::Fail)
    );
}

#[test]
fn a_deleted_test_file_with_another_unpinned_header_is_refused() {
    assert_eq!(
        trailer_ladder("tests/my pins.rs", &[]),
        (Verdict::Fail, Verdict::Fail, Verdict::Fail)
    );
}

/// The added tests are red on base, so only the refusal of the `+++` header fails the head.
#[test]
fn an_added_test_file_with_an_unpinned_header_is_refused() {
    let base = [("src/lib.rs", "pub fn f() -> u8 { 0 }\n")];
    assert_eq!(
        committed_tree("fix: f is 1\n", &base, &[("src/lib.rs", LIB), ("tests/q\"s.rs", PINS)]),
        Verdict::Fail
    );
}

#[test]
fn a_test_file_renamed_out_of_the_build_needs_a_trailer_naming_each_test() {
    assert_eq!(
        trailer_ladder("tests/pins.rs", &[("tests/pins.off", PINS)]),
        (Verdict::Fail, Verdict::Fail, Verdict::Pass)
    );
}

/// A diff prints a rename as a deletion and an addition, and a test whose name and body are the
/// same in both is neither: it needs no trailer and no base result.
#[test]
fn a_test_file_renamed_between_compiled_paths_adds_and_deletes_no_test() {
    assert_eq!(
        committed_tree(
            "test: rename the pins\n",
            &[("src/lib.rs", LIB), ("tests/pins.rs", PINS)],
            &[("src/lib.rs", LIB), ("tests/moved.rs", PINS)],
        ),
        Verdict::Pass
    );
}

/// What the renamed file adds is still a test added: it needs a result on base like any other.
#[test]
fn a_renamed_test_file_that_also_adds_a_test_is_refused() {
    let added = format!("{PINS}\n#[test]\nfn pins_g() {{\n    assert_eq!(wired::f(), 1);\n}}\n");
    assert_eq!(
        committed_tree(
            "test: rename the pins and add one\n",
            &[("src/lib.rs", LIB), ("tests/pins.rs", PINS)],
            &[("src/lib.rs", LIB), ("tests/moved.rs", &added)],
        ),
        Verdict::Fail
    );
}

/// The new file's first four lines are the span the moved test holds in `tests/moved.rs`: a line
/// number that coincides is not the same test.
#[test]
fn a_new_test_file_beside_a_rename_is_not_taken_for_the_moved_test() {
    let other = "#[test]\nfn other_new() {\n    assert_eq!(wired::f(), 1);\n}\n";
    assert_eq!(
        committed_tree(
            "test: rename the pins and add a file\n",
            &[("src/lib.rs", LIB), ("tests/pins.rs", PINS)],
            &[("src/lib.rs", LIB), ("tests/moved.rs", PINS), ("tests/other_new.rs", other)],
        ),
        Verdict::Inconclusive
    );
}

/// `tests/sub/pins.rs` is no target and nothing declares it, so cargo never runs its tests.
#[test]
fn a_test_file_renamed_where_no_target_builds_it_is_refused() {
    assert_eq!(
        committed_tree(
            "test: move the pins\n",
            &[("src/lib.rs", LIB), ("tests/pins.rs", PINS)],
            &[("src/lib.rs", LIB), ("tests/sub/pins.rs", PINS)],
        ),
        Verdict::Fail
    );
}

/// The verdict over a head that moves `tests/pins.rs` to `to`, beside `support` files both sides hold.
fn pins_moved_to(to: &str, support: &[(&str, &str)]) -> Verdict {
    let base = [&[("src/lib.rs", LIB), ("tests/pins.rs", PINS)][..], support].concat();
    let head = [&[("src/lib.rs", LIB), (to, PINS)][..], support].concat();
    committed_tree("test: move the pins\n", &base, &head)
}

/// `src/bin/foo/util.rs` is declared, but by a module of a bin target, which is not walked from.
#[test]
fn a_test_file_renamed_into_a_bin_target_module_is_refused() {
    let main = ("src/bin/foo/main.rs", "#[cfg(test)]\nmod util;\nfn main() {}\n");
    assert_eq!(pins_moved_to("src/bin/foo/util.rs", &[main]), Verdict::Fail);
}

/// `src/other.rs` declares `orphan`, but no crate root declares `other`.
#[test]
fn a_test_file_renamed_under_a_module_no_root_reaches_is_refused() {
    let other = ("src/other.rs", "#[cfg(test)]\nmod orphan;\n");
    assert_eq!(pins_moved_to("src/other/orphan.rs", &[other]), Verdict::Fail);
}

/// A declaration inside a block comment is not a declaration.
#[test]
fn a_test_file_renamed_under_a_commented_out_declaration_is_refused() {
    let main = ("tests/sub/main.rs", "/*\nmod pins;\n*/\nfn main() {}\n");
    assert_eq!(pins_moved_to("tests/sub/pins.rs", &[main]), Verdict::Fail);
}

/// The comment closes at the inner `*/` only if nesting is not counted.
#[test]
fn a_test_file_renamed_under_a_nested_commented_out_declaration_is_refused() {
    let main = ("tests/sub/main.rs", "/* /* */\nmod pins;\n*/\nfn main() {}\n");
    assert_eq!(pins_moved_to("tests/sub/pins.rs", &[main]), Verdict::Fail);
}

/// `/*` inside a `//` comment opens no block, so the declaration on the next line is real.
#[test]
fn a_test_file_renamed_under_a_declaration_after_a_line_comment_is_carried() {
    let main = ("tests/sub/main.rs", "// /*\nmod pins;\nfn main() {}\n");
    assert_eq!(pins_moved_to("tests/sub/pins.rs", &[main]), Verdict::Pass);
}

/// A block comment that closes on its own line hides nothing after it.
#[test]
fn a_test_file_renamed_under_a_declaration_after_a_block_comment_is_carried() {
    let main = ("tests/sub/main.rs", "/* note */\nmod pins;\nfn main() {}\n");
    assert_eq!(pins_moved_to("tests/sub/pins.rs", &[main]), Verdict::Pass);
}

/// A fresh repository with a `tests/` directory and a commit identity.
fn scratch_repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sutura-causality-{tag}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    git(&dir, &["init", "-q"]);
    git(&dir, &["config", "user.email", "user@example.com"]);
    git(&dir, &["config", "user.name", "test"]);
    dir
}

/// Each listed file's path and how many lines it added; nothing when the reader refuses.
fn listed(files: impl IntoIterator<Item = ChangedFile>) -> Vec<(String, usize)> {
    files.into_iter().map(|file| (file.path, file.added.len())).collect()
}

#[test]
fn a_commit_under_a_diff_attribute_still_lists_its_added_lines() {
    let dir = scratch_repo("diff-text");
    std::fs::write(dir.join(".gitattributes"), "*.rs -diff\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "base"]);
    std::fs::write(dir.join("tests/t.rs"), "#[test]\nfn t() {}\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "head"]);
    let added = listed(commit_additions(&dir, "HEAD").into_iter().flatten());
    drop(std::fs::remove_dir_all(&dir));
    assert_eq!(added, [(String::from("tests/t.rs"), 2)]);
}

#[test]
fn both_diff_readers_list_a_commit_the_same_under_a_user_git_config() {
    assert!(
        std::env::var_os("NEXTEST").is_some(),
        "this cell changes the process directory; run it under `just test`"
    );
    let dir = scratch_repo("diff-config");
    for (key, value) in [
        ("diff.external", "true"),
        ("diff.conv.textconv", "false"),
        ("diff.noprefix", "true"),
        ("core.quotePath", "true"),
        ("color.diff", "always"),
    ] {
        git(&dir, &["config", key, value]);
    }
    std::fs::create_dir_all(dir.join(".git/info")).unwrap();
    std::fs::write(dir.join(".git/info/attributes"), "*.rs diff=conv\n").unwrap();
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let base = String::from_utf8(git_output(&dir, &["rev-parse", "HEAD"]).stdout).unwrap();
    let base = Commit::parse(base.trim()).unwrap();
    std::fs::write(dir.join("tests/p\u{ef}ns.rs"), "#[test]\nfn t() {}\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-qm", "head"]);
    let commit = listed(commit_additions(&dir, "HEAD").into_iter().flatten());
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let range = listed(changes(&base).into_iter().flat_map(|changes| changes.files));
    std::env::set_current_dir(original).unwrap();
    drop(std::fs::remove_dir_all(&dir));
    let want = [(String::from("tests/p\u{ef}ns.rs"), 2)];
    assert_eq!((range, commit), (want.to_vec(), want.to_vec()));
}
