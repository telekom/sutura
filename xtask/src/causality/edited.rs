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
//! **THE TWO SHAPES AN ADDED LINE CANNOT NAME ARE CLOSED NOW, not stated.** Both move the file
//! out of `Plan::NotRequired` by naming the role an ADDED line plays: [`edited_helper_caller`]
//! names a `#[test]` whose helper a changed line reached, and [`removed_in`] names a pre-existing
//! `#[test]` a REMOVED line landed inside - the pure DELETION of an assertion, with no line added
//! in its place. A removal exists only in the PRE-image, so that shape reads it there
//! (`super::diff::RemovedLine`'s own doc records why the post-image approximation was falsified);
//! `super::plan` threads the base image in for exactly this. What still slips through, stated
//! next to the claims now that the two shapes that were loudest are closed:
//!
//! - **A moved assertion** - delete it in one test and re-add it in another - re-adds a line, so
//!   `touched_in` names the re-adding test; only the rename-to-nothing deletion refuses, which is
//!   the loud/safe direction this gate biases toward.
//! - **A helper called by a test in a DIFFERENT file** is reached only when the caller is in the
//!   SAME file - the reach this walks is the file's own `#[cfg(test)]` region. A cross-file
//!   helper needs the module-path graph and is the limit that remains.
//! - **A helper no test calls** names nothing - the "reached from a test" requirement excludes
//!   it, so adding a dead helper stays a `Plan::NotRequired`.
//! - **A helper whose edit is a PURE REFACTOR** (no behaviour change) still names its caller and
//!   routes into proof - the gate's syntactic bias costs a `Claim-Cell:` ask rather than a
//!   wrong verdict, which is the direction [`super::attributes`] records.

use crate::causality::attributes::{declares_a_test, item_below};
use crate::causality::diff::RemovedLine;
use crate::causality::names::Ident;
use crate::causality::regions::{AddedLine, PostImage, carries_no_behaviour, cfg_test_regions, item_end};
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
    !touched_in(&lines, added).is_empty() || !edited_helper_caller(&lines, added).is_empty()
}

/// Does `path`'s BASE image have a pre-existing test one of `removed` landed inside?
///
/// The convenience [`super::plan`] needs for SHAPE A, in the same shape as [`touches`]: `plan`
/// classifies a file from its path and the diff, and this asks the base-image span question a
/// pure deletion answers - a removed assertion exists only in the pre-image, so the reader this
/// takes is the BASE one, never the post-image.
pub(super) fn deletes_from_test(removed: &[RemovedLine], path: &str, base: &PostImage<'_>) -> bool {
    let text = base(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    !removed_in(&lines, removed).is_empty()
}

/// A pre-existing test one of this diff's REMOVED lines sat inside at the base commit.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Deleted {
    /// A run in this venue reaches it.
    Runs(Ident),
    /// `#[ignore]`d - named, but no run here measures it, so a deletion there is a fact to state
    /// rather than a proof to re-run.
    Ignored(Ident),
}

/// The pre-existing tests whose item a REMOVED line landed inside at the base commit.
///
/// The mirror of [`touched_in`] over the BASE image: an added line names a test from the
/// post-image it sits inside, and a removed line can only name one from the pre-image that
/// contained it. A line this removes is the whole reason the image has to be threaded in at
/// all - `super::plan` reads the post-image only today, so this walks the pre-image its caller
/// reads instead, asking each pre-existing `#[test]`'s span whether a removed line carrying
/// behaviour fell inside it. Blank/comment/attribute removals are filtered by
/// [`carries_no_behaviour`] (the converse: what is left is behaviour), so a deletion of a comment
/// or a blank line names nothing. `RemovedLine.before` is the PRE-image's own line number
/// ([`RemovedLine`]'s doc carries why no other image may name it), which is what lets this ask
/// exactly the question a pure deletion answers.
pub(super) fn removed_in(base_lines: &[&str], removed: &[RemovedLine]) -> Vec<Deleted> {
    let mut out = Vec::new();
    for (index, line) in base_lines.iter().enumerate() {
        if !declares_a_test(line.trim()) {
            continue;
        }
        let declared_at = index.saturating_add(1);
        // A removed `#[test]` is not a removed assertion - the test may be GONE. Naming it as
        // touched would claim this diff weakened a test it removed, which is a whole-test change
        // for the "changed tests" refusal rather than a deletion-of-evidence one.
        if removed.iter().any(|one| one.before == declared_at) {
            continue;
        }
        let Some((fn_index, declaration)) = item_below(base_lines, declared_at) else {
            continue;
        };
        let Some(name) = function_name(declaration) else {
            continue;
        };
        let last = item_end(base_lines, index);
        if !removed
            .iter()
            .any(|one| !carries_no_behaviour(&one.text) && (declared_at..=last.saturating_add(1)).contains(&one.before))
        {
            continue;
        }
        out.push(if is_ignored(base_lines, fn_index) {
            Deleted::Ignored(name)
        } else {
            Deleted::Runs(name)
        });
    }
    out
}

/// A `#[test]` in the same file whose item an added line lands inside, reached because that line
/// is inside a `#[cfg(test)]` HELPER fn the test calls.
///
/// SHAPE B. [`touched_in`] walks DOWN from a `#[test]`-declaring attribute through that item's
/// own span, so an added line inside a SIBLING helper - a `#[cfg(test)]` fn a test calls but whose
/// own attributed item is not the test's - names nothing there. This walks the other direction:
/// find the helper fn, check an added line lands in its span, and only then name a `#[test]` in
/// the same file whose body TEXT references the helper's bare name. An added line means the diff
/// reached the helper; a calling test is what makes the edit proof's to run rather than a helper
/// nothing tests. A helper no `#[test]` calls names nothing.
///
/// POST-IMAGE ONLY, unlike [`removed_in`]: a helper edit ADDS a line, and the added number names
/// itself - no pre-image is threaded in for this shape.
pub(super) fn edited_helper_caller(lines: &[&str], added: &[AddedLine]) -> Vec<Ident> {
    let mut out = Vec::new();
    for region in cfg_test_regions(&lines.join("\n")) {
        // `cfg_test_regions` returns 1-BASED half-open ranges spelled `start+1..last+2`, where
        // `start` is the `#[cfg(test)]` line's 0-based index. Those integer VALUES coincide with
        // the 0-based indices this loop wants (`start+1` is both the 1-based number of the first
        // ITEM line and that line's 0-based index), so `region.start`/`region.end` are used
        // directly as 0-based bounds here. Walk only `fn` decls - the `#[test]` items themselves
        // are reached by [`touched_in`], and non-fn lines (a `mod tests {`) are skipped.
        let mut index = region.start;
        while index < region.end {
            let trimmed = lines.get(index).map_or("", |one| one.trim());
            if !trimmed.starts_with("fn ") {
                index += 1;
                continue;
            }
            let Some(_) = function_name(trimmed) else {
                index += 1;
                continue;
            };
            let last = item_end(lines, index).saturating_add(1);
            let span = index.saturating_add(1)..=last;
            if !added.iter().any(|one| span.contains(&one.number)) {
                index = last;
                continue;
            }
            if let Some(test) = calling_test(lines, trimmed) {
                out.push(test);
            }
            index = last;
        }
    }
    out
}

/// A `#[test]` in `lines` whose item references `helper`'s bare function name.
///
/// `helper` is the whole `fn NAME(..)` signature line; the name is split off the first token
/// after `fn`, matching [`function_name`] on the signature. The call check scans each `#[test]`'s
/// own item span (the same [`touched_in`] walk) for the name: a `use` of the helper reaches the
/// same kept-at-HEAD tree the test runs in, and a bare call names the reach that matters.
fn calling_test(lines: &[&str], helper: &str) -> Option<Ident> {
    let name = function_name(helper)?.as_str().to_owned();
    for (index, line) in lines.iter().enumerate() {
        if !declares_a_test(line.trim()) {
            continue;
        }
        let declared_at = index.saturating_add(1);
        let Some((fn_index, declaration)) = item_below(lines, declared_at) else {
            continue;
        };
        let Some(test_name) = function_name(declaration) else {
            continue;
        };
        if test_name.as_str() == name {
            continue;
        }
        let last = item_end(lines, index);
        let Some(span) = lines.get(fn_index..=last) else {
            continue;
        };
        let body: String = span.join("\n");
        if body.contains(&name) {
            return Some(test_name);
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::{Deleted, Touched, edited_helper_caller, removed_in, touched_in, touches};
    use crate::causality::diff::RemovedLine;
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
    fn a_pure_deletion_is_named_from_the_base_image_not_the_post_image() {
        // `touched_in` reads ADDED lines, so a pure deletion - removed assertion, nothing added
        // in its place - has nothing for it to find. `removed_in` reads the PRE-image instead
        // (`RemovedLine.before` numbers the base), so it names the very deletion `touched_in`
        // cannot see. Both halves of the same file.
        let file = concat!(
            "#[test]\n",            // 1
            "fn t() {\n",           // 2
            "    assert!(true);\n", // 3 - removed at base line 3
            "}\n",                  // 4
        );
        let lines = existing_test(file);
        assert_eq!(touched_in(&lines, &[]), Vec::new(), "no added line, nothing to touch");
        let removed = vec![RemovedLine {
            before: 3,
            text: String::from("    assert!(true);"),
        }];
        assert_eq!(removed_in(&lines, &removed), vec![Deleted::Runs(Ident::parse("t").unwrap())]);
    }

    #[test]
    fn a_removed_blank_or_comment_is_not_a_deleted_assertion() {
        // The negative twin: a deletion of a non-behaviour line carries no evidence to state,
        // so it must not be named. `carries_no_behaviour` filters blank/comment/attribute.
        let file = concat!(
            "#[test]\n",            // base line 1
            "fn t() {\n",           // base line 2
            "    // a comment\n",   // base line 3 - removed
            "\n",                   // base line 4 - removed (blank)
            "    assert!(true);\n", // base line 5
            "}\n",                  // base line 6
        );
        let lines = existing_test(file);
        let removed = vec![
            RemovedLine {
                before: 3,
                text: String::from("    // a comment"),
            },
            RemovedLine {
                before: 4,
                text: String::new(),
            },
        ];
        assert_eq!(removed_in(&lines, &removed), Vec::new());
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

    #[test]
    fn an_edit_inside_a_helper_a_test_calls_names_that_test() {
        // SHAPE B: the added line sits in a `#[cfg(test)]` helper fn - a SIBLING item, not the
        // test's own - and `touched_in` names it on its own; this must name the CALLING test.
        let file = concat!(
            "#[cfg(test)]\n",                     // 1
            "mod tests {\n",                      // 2
            "    fn helper() -> u8 { 1 }\n",      // 3
            "    #[test]\n",                      // 4
            "    fn t() {\n",                     // 5
            "        assert_eq!(helper(), 1);\n", // 6
            "    }\n",                            // 7
            "}\n",                                // 8
        );
        let lines = existing_test(file);
        // The added line is a change to the helper's own body (post-image line 3, the `fn
        // helper` line). The diff reached no `#[test]`-declaring attribute, so `touched_in`
        // alone names nothing and `edited_helper_caller` is what does.
        let added = added_from(3, &["    fn helper() -> u8 { 2 }"]);
        assert_eq!(edited_helper_caller(&lines, &added), vec![Ident::parse("t").unwrap()]);
    }

    #[test]
    fn an_edit_inside_a_helper_no_test_calls_names_nothing() {
        // The negative twin: the helper is never reached from a `#[test]`, so the edit has no
        // caller to prove it against.
        let file = concat!(
            "#[cfg(test)]\n",                       // 1
            "mod tests {\n",                        // 2
            "    fn unused_helper() -> u8 { 1 }\n", // 3
            "    #[test]\n",                        // 4
            "    fn t() {\n",                       // 5
            "        assert!(true);\n",             // 6
            "    }\n",                              // 7
            "}\n",                                  // 8
        );
        let lines = existing_test(file);
        // Post-image line 3 is the helper's own `fn` line again.
        let added = added_from(3, &["    fn unused_helper() -> u8 { 2 }"]);
        assert_eq!(edited_helper_caller(&lines, &added), Vec::new());
    }
}
