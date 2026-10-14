//! `causality::run` over a test that compiles only WITHOUT a feature. `--all-features` leaves it out, so
//! it is measured in its package's default build, under the same red-on-base rules as every other test.

use super::changed_tree;
use crate::Verdict;

const MANIFEST: &str = "[package]\nname = \"wired\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[features]\noff = []\n\n[profile.ci]\ninherits = \"dev\"\n";
const ON: &str = "#[test]\nfn the_on_one() {\n    assert_eq!(wired::on(), 2);\n}\n";
const OFF: &str = "#[test]\n#[cfg(not(feature = \"off\"))]\nfn the_off_one() {\n    assert_eq!(wired::off(), 2);\n}\n";

/// The verdict over `cells` added in `tests/t.rs` while `on()` and `off()` go from `base` to two.
fn built_off(base: (u8, u8), cells: &str) -> Verdict {
    let lib = |(on, off): (u8, u8)| format!("pub fn on() -> u8 {{ {on} }}\npub fn off() -> u8 {{ {off} }}\n");
    changed_tree(
        &[("Cargo.toml", MANIFEST), ("src/lib.rs", &lib(base))],
        &[("Cargo.toml", MANIFEST), ("src/lib.rs", &lib((2, 2))), ("tests/t.rs", cells)],
    )
}

#[test]
fn a_test_compiled_only_without_a_feature_is_measured_in_the_default_build() {
    assert_eq!(built_off((2, 1), OFF), Verdict::Pass);
}

/// A pair, because a refusal alone is also what matching nothing under `--all-features` answers.
#[test]
fn a_test_compiled_only_without_a_feature_and_green_on_base_is_refused_as_a_pin() {
    assert_eq!(
        (built_off((2, 1), OFF), built_off((1, 2), OFF)),
        (Verdict::Pass, Verdict::Fail)
    );
}

#[test]
fn a_scope_spanning_both_builds_measures_each_test_in_its_own() {
    let both = format!("{ON}{OFF}");
    assert_eq!(
        (built_off((1, 1), &both), built_off((1, 2), &both), built_off((2, 1), &both)),
        (Verdict::Pass, Verdict::Fail, Verdict::Fail)
    );
}
