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

#[test]
fn a_doc_comment_naming_fn_is_not_a_declaration() {
    // `github.com/telekom/sutura#1270`: `//! no fn in the tree` was read as a helper named
    // `in`, and any test containing the word was then marked an edited caller of it.
    assert!(
        super::function_name("//! this document names no fn in the tree").is_none(),
        "a doc comment is not a declaration"
    );
    assert_eq!(
        super::function_name("fn real() -> u8 { 1 }"),
        Some(crate::causality::names::Ident::parse("real").unwrap())
    );
}

#[test]
fn a_string_literal_naming_fn_is_not_a_declaration() {
    // `github.com/telekom/sutura#1270`: a one-line literal spelling a standalone `fn foo` is
    // not a declaration, whatever its position.
    assert!(
        super::function_name(r#"let s = "call fn foo here";"#).is_none(),
        "a string literal is not a declaration"
    );
}
