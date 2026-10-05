//! Which edited lines are a comment, as the lexer reads them rather than as the text spells them.
//!
//! A `//` or empty line that opens inside a multi-line string is string content, so adding or
//! removing one inside a test - or inside a helper a test calls - is an edit of that test.

use super::{Plan, plan_with_base};
use crate::causality::edited::{Touched, edited_helper_caller};
use crate::causality::fixtures::{added_from, changed, changed_removing, tree};
use crate::causality::names::Ident;
use crate::causality::regions::TestScope;

#[test]
fn a_slash_or_blank_line_added_inside_a_string_in_a_test_is_an_edit() {
    for (post_image, line) in [
        (
            "#[test]\nfn t() {\n    let src = \"\n// a fixture line\n\";\n    assert!(!src.is_empty());\n}\n",
            "// a fixture line",
        ),
        (
            "#[test]\nfn t() {\n    let src = r#\"a\n\nb\"#;\n    assert!(src.contains(\"a\\n\\nb\"));\n}\n",
            "",
        ),
    ] {
        let files = vec![changed("crates/x/src/a.rs", 4, &[line])];
        let read = tree(&[("crates/x/src/a.rs", post_image)]);
        assert_ne!(plan_with_base(&files, &read, &read), Plan::NotRequired, "{line:?}");
    }
}

#[test]
fn a_slash_line_added_inside_a_helpers_string_names_its_caller() {
    let file = "fn fixture() -> &'static str {\n    \"\n// a fixture line\n\"\n}\n#[test]\nfn t() {\n    assert!(!fixture().is_empty());\n}\n";
    let lines: Vec<&str> = file.lines().collect();
    let added = added_from(3, &["// a fixture line"]);
    assert_eq!(
        edited_helper_caller(&lines, &added, &TestScope::WholeFile),
        vec![Touched::Runs(Ident::parse("t").unwrap())]
    );
}

#[test]
fn removing_a_slash_line_inside_a_string_in_a_test_is_a_deleted_test() {
    let base_image = "#[test]\nfn t() {\n    let src = \"\n// a fixture line\n\";\n    assert!(!src.is_empty());\n}\n";
    let post_image = "#[test]\nfn t() {\n    let src = \"\n\";\n    assert!(!src.is_empty());\n}\n";
    let files = vec![changed_removing("crates/x/src/a.rs", 4, &[], 4, &["// a fixture line"])];
    let read = tree(&[("crates/x/src/a.rs", post_image)]);
    let base = tree(&[("crates/x/src/a.rs", base_image)]);
    assert!(matches!(plan_with_base(&files, &read, &base), Plan::DeletedTests(_)));
}

#[test]
fn a_comment_added_inside_a_called_helper_names_no_caller() {
    // `github.com/telekom/sutura#1279`, the helper half: a `//` line in a helper's code is not an
    // edit of the test that calls it, the same as one in the test itself.
    let file = "fn helper() -> u8 {\n    // why it is one\n    1\n}\n#[test]\nfn t() {\n    assert_eq!(helper(), 1);\n}\n";
    let lines: Vec<&str> = file.lines().collect();
    let added = added_from(2, &["    // why it is one"]);
    assert_eq!(edited_helper_caller(&lines, &added, &TestScope::WholeFile), Vec::new());
}

#[test]
fn a_slash_line_inside_a_string_after_two_continuations_is_an_edit() {
    let post_image = concat!(
        "fn msg() -> &'static str {\n",
        "    \"a \\\n",
        "     b \\\n",
        "     c\"\n",
        "}\n",
        "#[test]\n",
        "fn t() {\n",
        "    let src = \"\n",
        "x\n",
        "// a fixture line\n",
        "\";\n",
        "    assert!(!src.is_empty());\n",
        "}\n",
    );
    let files = vec![changed("crates/x/src/a.rs", 10, &["// a fixture line"])];
    let read = tree(&[("crates/x/src/a.rs", post_image)]);
    assert_ne!(plan_with_base(&files, &read, &read), Plan::NotRequired);
}

#[test]
fn a_comment_in_a_file_with_a_continued_string_is_still_skipped() {
    let post_image = "fn msg() -> &'static str {\n    \"a \\\n     b\"\n}\n#[test]\nfn t() {\n    // a corrected comment\n    assert!(!msg().is_empty());\n}\n";
    let files = vec![changed("crates/x/src/a.rs", 7, &["    // a corrected comment"])];
    let read = tree(&[("crates/x/src/a.rs", post_image)]);
    assert_eq!(plan_with_base(&files, &read, &read), Plan::NotRequired);
}
