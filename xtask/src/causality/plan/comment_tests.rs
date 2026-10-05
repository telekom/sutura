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
