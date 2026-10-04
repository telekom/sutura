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

/// What [`Scan::of`] answers when line `added` of the test target `file` is the diff's only line.
///
/// [`Scan::Unreadable`] when that line names no test: neither a declaration nor a caller.
fn scan_with_one_added_line(file: &str, added: usize) -> Scan {
    let path = "crates/x/tests/t.rs";
    let line = file.lines().nth(added.saturating_sub(1)).unwrap_or_default();
    let files = vec![changed(path, added, &[line])];
    let read = tree(&[(path, file), ("crates/x/Cargo.toml", &manifest("x"))]);
    Scan::of(&files, &[String::from(path)], &read)
}

#[test]
fn a_doc_comment_naming_fn_is_not_a_declaration() {
    // `github.com/telekom/sutura#1270`, and the same shape on #1264: a `///` line reading
    // `this fn reads ..` declared a helper named `reads`, and a test whose comment says `reads`
    // was named as its caller.
    let file = concat!(
        "#[test]\n",                                      // 1
        "fn uses() {\n",                                  // 2
        "    // the plan reads the second fact\n",        // 3
        "}\n",                                            // 4
        "/// an edit inside this fn reads to the gate\n", // 5
        "fn disagreement() -> u8 { 1 }\n",                // 6
    );
    let scan = scan_with_one_added_line(file, 5);
    assert!(
        matches!(scan, Scan::Unreadable(_)),
        "a doc comment is not a declaration: {scan:?}"
    );
}

#[test]
fn a_string_literal_naming_fn_is_not_a_declaration() {
    // `github.com/telekom/sutura#1270`: a one-line literal spelling a standalone `fn foo` is
    // not a declaration, whatever its position.
    let file = concat!(
        "#[test]\n",                               // 1
        "fn mentions() {\n",                       // 2
        "    let _ = \"uses foo\";\n",             // 3
        "}\n",                                     // 4
        "const S: &str = \"call fn foo here\";\n", // 5
    );
    let scan = scan_with_one_added_line(file, 5);
    assert!(
        matches!(scan, Scan::Unreadable(_)),
        "a string literal is not a declaration: {scan:?}"
    );
}

#[test]
fn a_line_inside_a_multi_line_comment_is_not_a_declaration() {
    // `github.com/telekom/sutura#1270`'s review: lexing one line at a time read the interior of a
    // block comment as code, so `fn helper` there named the test that calls `helper()`.
    let file = concat!(
        "#[test]\n",               // 1
        "fn calls() {\n",          // 2
        "    helper();\n",         // 3
        "}\n",                     // 4
        "/*\n",                    // 5
        " * names no fn helper\n", // 6
        " */\n",                   // 7
    );
    let scan = scan_with_one_added_line(file, 6);
    assert!(
        matches!(scan, Scan::Unreadable(_)),
        "a block comment is not a declaration: {scan:?}"
    );
}
