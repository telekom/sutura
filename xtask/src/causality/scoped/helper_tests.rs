use super::Scan;
use crate::causality::fixtures::{changed, manifest, tree};

#[test]
fn a_changed_helper_called_by_a_test_is_named_without_an_added_test_attribute() {
    let path = "crates/x/tests/helper.rs";
    let file = "fn helper() -> u8 { 2 }\n#[test]\nfn observes_helper() { assert_eq!(helper(), 2); }\n";
    let files = vec![changed(path, 1, &["fn helper() -> u8 { 2 }"])];
    let read = tree(&[(path, file), ("crates/x/Cargo.toml", &manifest("x"))]);
    let result = Scan::of(&files, &[String::from(path)], &read);
    let Scan::Runnable(scoped) = result else {
        panic!("a called helper's test must be in the runnable set: {result:?}");
    };
    assert_eq!(scoped.tests().len(), 1);
    assert_eq!(scoped.tests()[0].name(), "observes_helper");
}

#[test]
fn an_ignored_test_calling_an_added_helper_does_not_become_runnable() {
    let file = "fn helper() {}\n#[test]\n#[ignore]\nfn acceptance() { helper(); }\n";
    let files = vec![changed("crates/x/tests/t.rs", 1, &file.lines().collect::<Vec<_>>())];
    let read = tree(&[("crates/x/tests/t.rs", file), ("crates/x/Cargo.toml", &manifest("x"))]);
    assert!(matches!(
        Scan::of(&files, &[String::from("crates/x/tests/t.rs")], &read),
        Scan::OnlyIgnored(_)
    ));
}

/// An EDITED test carries its attached build condition, like an added one: a partial base run
/// then names the gate as the likely cause, not the generic "may be cfg-gated" sentence.
#[test]
fn an_edited_cfg_gated_test_carries_its_gate_and_is_marked_edited() {
    let path = "pa/src/lib.rs";
    let file = "#[cfg(not(feature = \"rdbms\"))]\n#[test]\nfn omitted() {\n    let _kept = 1;\n}\n";
    let files = vec![changed(path, 4, &["    let _kept = 1;"])];
    let read = tree(&[(path, file), ("pa/Cargo.toml", &manifest("pa"))]);
    let Scan::Runnable(scoped) = Scan::of(&files, &[String::from(path)], &read) else {
        panic!("the edited test must be named");
    };
    assert_eq!(scoped.tests()[0].gate(), Some("#[cfg(not(feature = \"rdbms\"))]"));
    assert!(scoped.tests()[0].is_edited());
}
