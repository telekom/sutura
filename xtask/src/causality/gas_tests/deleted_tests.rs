//! `causality::run` over a diff that deletes a test file whole. The deleted file has no post-image,
//! so the diff lists nothing for it, and the rule for a test removed from a file that stays
//! (a `Weakens-Test:` trailer naming it) has to hold for the lines it held all the same.

use super::committed_tree;
use crate::Verdict;

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
