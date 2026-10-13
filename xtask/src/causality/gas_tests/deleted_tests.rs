//! `causality::run` over a diff that deletes a test file whole. The deleted file has no post-image,
//! so the diff lists nothing for it, and the rule for a test removed from a file that stays
//! (a `Weakens-Test:` trailer naming it) has to hold for the lines it held all the same.

use super::committed_tree;
use crate::Verdict;

const LIB: &str = "pub fn f() -> u8 { 1 }\n";
const PINS: &str = "#[test]\nfn pins_f() {\n    assert_eq!(wired::f(), 1);\n}\n\n#[test]\nfn pins_f_again() {\n    assert_ne!(wired::f(), 0);\n}\n";

/// The verdict over a head that deletes `tests/pins.rs` and changes nothing else, with one
/// `Weakens-Test:` trailer per name in `waived`.
fn deleting(waived: &[&str]) -> Verdict {
    let trailers: String = waived
        .iter()
        .map(|name| ["Weakens-Test: ", name, " - superseded\n"].concat())
        .collect();
    committed_tree(
        &format!("test: drop pins.rs\n\n{trailers}"),
        &[("src/lib.rs", LIB), ("tests/pins.rs", PINS)],
        &[("src/lib.rs", LIB)],
    )
}

/// Named, not blanket: a trailer for one of the two deleted tests leaves the other refused.
#[test]
fn a_test_file_deleted_whole_needs_a_trailer_naming_each_test() {
    assert_eq!(
        (deleting(&[]), deleting(&["pins_f"]), deleting(&["pins_f", "pins_f_again"])),
        (Verdict::Fail, Verdict::Fail, Verdict::Pass)
    );
}
