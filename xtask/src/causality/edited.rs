//! Naming a test whose ITEM this diff's added lines land inside, without that test's own
//! declaring attribute being one of them.
//!
//! `github.com/telekom/sutura#1025`. `super::attributes::adds` and `super::scoped::declared_under`
//! both read an ADDED line's own text - a `#[test]` or `mod tests` marker - so a diff whose only
//! change is a body edit inside a test that already existed adds no marker either of them reads.
//! `super::plan` then called such a file `Adds::Nothing`, the same answer any implementation
//! change gets, and reverted it; absent any OTHER test file in the diff, `test_files` stayed
//! empty and the whole diff answered `Plan::NotRequired` - *no changed tests* over a diff that
//! changed one, unconditionally, with no `Claim-Cell:` asked of it.
//!
//! THE QUESTION IS THE SAME SHAPE AS [`super::regions::cfg_test_regions`], walked from a
//! `#[test]`-DECLARING line instead of a `#[cfg(test)]` one: find the attribute, find its item's
//! span by balancing braces from the attribute's own line ([`super::regions::item_end`]), and ask
//! whether an added line falls inside it. **EXCLUDES an attribute that is itself among the added
//! lines** - that shape is `Adds::NamedTest`, a genuinely NEW test, and `declared_under` already
//! names it; asking about it here would just be a second, redundant name for the same test.
//!
//! **THE SPAN IS THE WHOLE ITEM, ATTRIBUTE THROUGH CLOSING BRACE - not only the body.** The issue
//! that opened this file asks about "a hunk whose enclosing item is a `#[test]` fn", not about the
//! braces after its signature, and one span for both closes a second stated limit for free:
//! `scoped::function_name`'s doc used to record that an added line inside a WRAPPED signature -
//! a def-interior edit above the body's own `{` - named nothing, because nothing walked UP from
//! it. This walks DOWN from the attribute instead, so the span covers it too.
//!
//! **THE LIMIT.** A pure DELETION - removing an assertion with no line added in its place - has no
//! added line for this to find, so it names nothing here either.
//! `super::diff::RemovedLine`'s own doc records why that is not casually fixed: an earlier version
//! of this gate approximated a removed line's POST-image neighbourhood instead of reading the
//! PRE-image, and review falsified the approximation end to end. Reading the PRE-image correctly
//! would need it threaded into `super::plan`, which does not have it today - a diff that only
//! deletes an assertion still reaches `Plan::NotRequired`, tracked here rather than guessed at.
//!
//! **A second shape this does not reach: an edit inside a `#[cfg(test)]` helper fn that a
//! `#[test]` calls, but whose own attributed item is not the test's.** `touched_in` walks only
//! the `#[test]`-declaring item's own brace span, never a sibling item a test calls, so an added
//! line inside the helper reaches `Plan::NotRequired` the same way a pure deletion does. A
//! syntactic walker that followed the call graph would be a much bigger mechanism and is outside
//! the issue's stated scope ("a hunk whose enclosing item is a `#[test]` fn"); the gap is stated
//! here rather than closed.

use crate::causality::attributes::{declares_a_test, item_below};
use crate::causality::names::Ident;
use crate::causality::regions::{AddedLine, PostImage, item_end};
use crate::causality::scoped::{function_name, is_ignored};

/// A pre-existing test whose item an added line lands inside.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Touched {
    /// A run in this venue reaches it.
    Runs(Ident),
    /// `#[ignore]`d - named, but no run here can measure it.
    Ignored(Ident),
}

/// The pre-existing tests in `lines` whose item an added line in `added` lands inside.
///
/// This module's own header carries the shape, the exclusion and the limit.
pub(super) fn touched_in(lines: &[&str], added: &[AddedLine]) -> Vec<Touched> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !declares_a_test(line.trim()) {
            continue;
        }
        let declared_at = index.saturating_add(1);
        if added.iter().any(|one| one.number == declared_at) {
            // A genuinely NEW test - `declared_under` already names it from the added side.
            continue;
        }
        let Some((fn_index, declaration)) = item_below(lines, declared_at) else {
            continue;
        };
        let Some(name) = function_name(declaration) else {
            continue;
        };
        let last = item_end(lines, index);
        if !added
            .iter()
            .any(|one| (declared_at..=last.saturating_add(1)).contains(&one.number))
        {
            continue;
        }
        out.push(if is_ignored(lines, fn_index) {
            Touched::Ignored(name)
        } else {
            Touched::Runs(name)
        });
    }
    out
}

/// Does `path`'s post-image touch a pre-existing test with any of `added`?
///
/// The convenience `super::plan` needs: it classifies a file from its path alone, unlike
/// `super::scoped::Scan::of`, which has already read the file's lines for its own reasons and
/// calls [`touched_in`] directly rather than through this.
pub(super) fn touches(added: &[AddedLine], path: &str, read: &PostImage<'_>) -> bool {
    let text = read(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    !touched_in(&lines, added).is_empty()
}

#[cfg(test)]
mod tests {
    use super::{Touched, touched_in, touches};
    use crate::causality::fixtures::{added_from, tree};
    use crate::causality::names::Ident;

    /// The core shape: an assertion inside an EXISTING test changes, nothing else does.
    fn existing_test(text: &str) -> Vec<&str> {
        text.lines().collect()
    }

    #[test]
    fn a_changed_assertion_inside_an_existing_test_is_named() {
        let file = concat!(
            "pub fn f() -> u8 {\n",          // 1
            "    2\n",                       // 2
            "}\n",                           // 3
            "#[cfg(test)]\n",                // 4
            "mod tests {\n",                 // 5
            "    use super::f;\n",           // 6
            "    #[test]\n",                 // 7
            "    fn existing() {\n",         // 8
            "        assert_eq!(f(), 2);\n", // 9
            "    }\n",                       // 10
            "}\n",                           // 11
        );
        let lines = existing_test(file);
        let added = added_from(9, &["        assert_eq!(f(), 2);"]);
        assert_eq!(
            touched_in(&lines, &added),
            vec![Touched::Runs(Ident::parse("existing").unwrap())]
        );
    }

    #[test]
    fn a_new_test_s_own_attribute_is_not_double_counted_as_touched() {
        // The exclusion: a genuinely ADDED test's attribute line is itself in `added`, so this
        // must find nothing - `declared_under` on the scoped side already names it.
        let file = "#[test]\nfn brand_new() {}\n";
        let lines = existing_test(file);
        let added = added_from(1, &["#[test]", "fn brand_new() {}"]);
        assert_eq!(touched_in(&lines, &added), Vec::new());
    }

    #[test]
    fn an_untouched_neighbour_is_not_named() {
        // Two tests, one file: editing one must not sweep in the other.
        let file = concat!(
            "#[cfg(test)]\n",              // 1
            "mod tests {\n",               // 2
            "    #[test]\n",               // 3
            "    fn untouched() {\n",      // 4
            "        assert!(true);\n",    // 5
            "    }\n",                     // 6
            "    #[test]\n",               // 7
            "    fn edited() {\n",         // 8
            "        assert_eq!(1, 1);\n", // 9
            "    }\n",                     // 10
            "}\n",                         // 11
        );
        let lines = existing_test(file);
        let added = added_from(9, &["        assert_eq!(1, 1);"]);
        assert_eq!(
            touched_in(&lines, &added),
            vec![Touched::Runs(Ident::parse("edited").unwrap())]
        );
    }

    #[test]
    fn an_edited_ignored_test_is_named_and_marked_ignored() {
        let file = concat!(
            "#[test]\n",             // 1
            "#[ignore]\n",           // 2
            "fn parked() {\n",       // 3
            "    assert!(false);\n", // 4
            "}\n",                   // 5
        );
        let lines = existing_test(file);
        let added = added_from(4, &["    assert!(false);"]);
        assert_eq!(
            touched_in(&lines, &added),
            vec![Touched::Ignored(Ident::parse("parked").unwrap())]
        );
    }

    #[test]
    fn a_pure_deletion_names_nothing_the_stated_limit() {
        // A removed assertion with no replacement adds nothing, so there is no added line for
        // this to find - the limit this module's own header states.
        let file = concat!(
            "#[test]\n",            // 1
            "fn t() {\n",           // 2
            "    assert!(true);\n", // 3
            "}\n",                  // 4
        );
        let lines = existing_test(file);
        assert_eq!(touched_in(&lines, &[]), Vec::new());
    }

    #[test]
    fn an_added_line_outside_every_item_touches_nothing() {
        let file = "pub fn f() -> u8 { 1 }\n#[test]\nfn t() {}\n";
        let lines = existing_test(file);
        let added = added_from(1, &["pub fn f() -> u8 { 1 }"]);
        assert_eq!(touched_in(&lines, &added), Vec::new());
    }

    #[test]
    fn touches_reads_the_file_through_the_post_image_reader() {
        let read = tree(&[("crates/x/src/a.rs", "#[test]\nfn t() {\n    assert!(true);\n}\n")]);
        let added = added_from(3, &["    assert!(true);"]);
        assert!(touches(&added, "crates/x/src/a.rs", &read));
        assert!(!touches(&[], "crates/x/src/a.rs", &read));
        assert!(!touches(&added, "crates/x/src/absent.rs", &read));
    }
}
