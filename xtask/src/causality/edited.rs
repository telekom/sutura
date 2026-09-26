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
//! `super::plan` threads the base image in for exactly this, and [`deletion_in`] is the composed
//! answer it calls, over EVERY compiled file rather than only one with no other test-side change -
//! `github.com/telekom/sutura#1031`'s own review found that the first cut of this only ran the
//! check in the `Adds::Nothing` arm, so a file that ALSO added a test or edited a different one
//! never asked the deletion question at all. What still slips through, stated next to the claims:
//!
//! - **A moved assertion** - delete it in one test and re-add it in another - re-adds a line, so
//!   `touched_in` names the re-adding test; only the rename-to-nothing deletion refuses, which is
//!   the loud/safe direction this gate biases toward. [`deletion_in`] subtracts every name
//!   `touched_in` already claims on the POST-image before naming a deletion, so the SAME test
//!   being both edited and (elsewhere) evidenced as deleted is routed to proof once, not refused
//!   twice.
//! - **A helper called by a test in a DIFFERENT file** is reached only when the caller is in the
//!   SAME file - the reach this walks is the file's own test SCOPE (`super::regions::scope`):
//!   every `#[cfg(test)]` region for an ordinary file, or the WHOLE file when `scope` says so (an
//!   out-of-line `#[cfg(test)] mod x;` file, a `tests/*.rs` target, or one carrying
//!   `#![cfg(test)]`). A cross-file helper needs the module-path graph and is the limit that
//!   remains.
//! - **A helper no test calls** names nothing - the "reached from a test" requirement excludes
//!   it, so adding a dead helper stays a `Plan::NotRequired`.
//! - **A helper whose edit is a PURE REFACTOR** (no behaviour change) still names its caller and
//!   routes into proof - the gate's syntactic bias costs a `Claim-Cell:` ask rather than a
//!   wrong verdict, which is the direction [`super::attributes`] records.
//! - **An unreadable BASE image with a non-empty removed set** fails CLOSED
//!   ([`Deletion::BaseUnreadable`]) rather than silently answering "no deletion" - the direction
//!   every other unreadable-input arm in this gate already takes.

use crate::causality::attributes::{attached, declares_a_test, item_below};
use crate::causality::diff::RemovedLine;
use crate::causality::names::Ident;
use crate::causality::regions::{AddedLine, PostImage, TestScope, carries_no_behaviour, item_end};
use crate::causality::scoped::{function_name, is_ignored};

/// A pre-existing test whose item an added line lands inside.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Touched {
    /// A run in this venue reaches it.
    Runs(Ident),
    /// `#[ignore]`d - named, but no run here can measure it.
    Ignored(Ident),
}

impl Touched {
    /// The name either variant carries, for [`deletion_in`]'s subtraction: a test #1025 already
    /// routes to proof (an ADDED line in its span) is not ALSO named as a Shape A deletion, even
    /// when the same diff's removed lines carry behaviour inside that same test's BASE span - that
    /// is one edit, not a deletion with nothing added in its place.
    const fn name(&self) -> &Ident {
        match self {
            Self::Runs(name) | Self::Ignored(name) => name,
        }
    }
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
    let scope = crate::causality::regions::scope(path, read);
    !touched_in(&lines, added).is_empty() || !edited_helper_caller(&lines, added, &scope).is_empty()
}

/// One file's SHAPE A verdict.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Deletion {
    /// No removed line carried behaviour inside a pre-existing test - or every one that did is
    /// also a test #1025's edited-assertion shape already routes to proof.
    None,
    /// `removed` is non-empty and the BASE image could not be read at all: fail closed rather than
    /// silently answering [`Self::None`] about a file this cannot actually read.
    BaseUnreadable,
    /// These pre-existing tests lost evidence at the base commit.
    Named(Vec<Ident>),
}

/// SHAPE A over `path`, for [`super::plan::plan`]'s use over EVERY compiled file - not only one
/// with no other test-side change, which is the gap `github.com/telekom/sutura#1031`'s own review
/// found: the first cut of this ran only inside the `Adds::Nothing` arm, so a file that also added
/// a new test, a helper, or edited a DIFFERENT existing test never asked the deletion question for
/// the one it silently weakened.
///
/// Reads the BASE image for [`removed_in`]'s question and the POST-image for [`touched_in`]'s, to
/// subtract a name #1025 already claims - the same test edited (an added line in its span) AND
/// evidenced by a removed line carrying behaviour in its base span is one EDIT, not a deletion
/// with nothing added in its place, and stays routed to proof rather than being named twice.
pub(super) fn deletion_in(
    file_added: &[AddedLine],
    file_removed: &[RemovedLine],
    path: &str,
    base: &PostImage<'_>,
    read: &PostImage<'_>,
) -> Deletion {
    if file_removed.is_empty() {
        return Deletion::None;
    }
    let Some(base_text) = base(path) else {
        return Deletion::BaseUnreadable;
    };
    let base_lines: Vec<&str> = base_text.lines().collect();
    let deleted = removed_in(&base_lines, file_removed);
    if deleted.is_empty() {
        return Deletion::None;
    }
    let post_text = read(path).unwrap_or_default();
    let post_lines: Vec<&str> = post_text.lines().collect();
    let touched = touched_in(&post_lines, file_added);
    let names: Vec<Ident> = deleted
        .into_iter()
        .map(|one| one.name())
        .filter(|name| !touched.iter().any(|one| one.name() == name))
        .collect();
    if names.is_empty() {
        Deletion::None
    } else {
        Deletion::Named(names)
    }
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

impl Deleted {
    fn name(&self) -> Ident {
        match self {
            Self::Runs(name) | Self::Ignored(name) => name.clone(),
        }
    }
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
        // A removed `#[test]` ATTRIBUTE line is itself inside the item's own span
        // (`declared_at..=last+1` below), so a WHOLE test deleted - attribute, signature and
        // body all gone - is caught the same way a deleted assertion is: `item_below`/
        // `function_name` still resolve the name off the BASE image, which has every line this
        // diff removed. `github.com/telekom/sutura#1031`'s own review named the earlier special
        // case here a perverse incentive - deleting one assert refused while deleting the whole
        // test passed - so there is no special case left to skip past.
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
/// is inside a HELPER fn the test calls.
///
/// SHAPE B. [`touched_in`] walks DOWN from a `#[test]`-declaring attribute through that item's
/// own span, so an added line inside a SIBLING helper - a fn a test calls but whose own
/// attributed item is not the test's - names nothing there. This walks the other direction: find
/// the helper fn, check an added line lands in its span, and only then name every `#[test]` in the
/// same file whose body TEXT references the helper's bare name. An added line means the diff
/// reached the helper; a calling test is what makes the edit proof's to run rather than a helper
/// nothing tests. A helper no `#[test]` calls names nothing.
///
/// `scope` decides WHICH lines count as test code to walk at all - `super::regions::scope`'s own
/// two answers: every `#[cfg(test)]` region for an ordinary file, or the WHOLE file when the file
/// itself is entirely test code (an out-of-line `#[cfg(test)] mod x;` file, a `tests/*.rs` target,
/// `#![cfg(test)]`). Walking `scope.covers(number)` PER LINE rather than a raw `Range` is what
/// closes the off-by-one `github.com/telekom/sutura#1031`'s own review found: the earlier version
/// read `cfg_test_regions`'s 1-based range as 0-based bounds directly, which walked one line PAST
/// each region's own end - a production `fn` placed right after a one-line `#[cfg(test)]` item was
/// read as a helper.
///
/// A `#[test]`-declared item is skipped as a "helper" - [`touched_in`] already reaches it
/// directly, and asking `calling_tests` about it would scan OTHER tests' bodies for ITS name,
/// which is a second, wrong-direction reach for the same test.
///
/// POST-IMAGE ONLY, unlike [`removed_in`]: a helper edit ADDS a line, and the added number names
/// itself - no pre-image is threaded in for this shape.
pub(super) fn edited_helper_caller(lines: &[&str], added: &[AddedLine], scope: &TestScope) -> Vec<Ident> {
    let mut out = Vec::new();
    let mut index = 0_usize;
    while index < lines.len() {
        let number = index.saturating_add(1);
        if !scope.covers(number) {
            index += 1;
            continue;
        }
        let trimmed = lines.get(index).map_or("", |line| line.trim());
        if function_name(trimmed).is_none() {
            index += 1;
            continue;
        }
        let last = item_end(lines, index);
        if attached(lines, index).iter().any(|(_, opening)| declares_a_test(opening)) {
            index = last.saturating_add(1);
            continue;
        }
        let span = number..=last.saturating_add(1);
        if added.iter().any(|one| span.contains(&one.number)) {
            for test in calling_tests(lines, trimmed) {
                if !out.contains(&test) {
                    out.push(test);
                }
            }
        }
        index = last.saturating_add(1);
    }
    out
}

/// Every `#[test]` in `lines` whose item references `helper`'s bare function name as a whole
/// identifier - never as a SUBSTRING of a longer one.
///
/// `helper` is the whole `fn NAME(..)` signature line; the name is split off the first token
/// after `fn`, matching [`function_name`] on the signature. The call check scans each `#[test]`'s
/// own item span (the same [`touched_in`] walk) for the name: a `use` of the helper reaches the
/// same kept-at-HEAD tree the test runs in, and a bare call names the reach that matters. Returns
/// EVERY caller rather than the first - `github.com/telekom/sutura#1031`'s own review found the
/// first-match version silently dropped a real caller behind an earlier, spurious one.
fn calling_tests(lines: &[&str], helper: &str) -> Vec<Ident> {
    let Some(name) = function_name(helper) else {
        return Vec::new();
    };
    let name = name.as_str();
    let mut out = Vec::new();
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
        if references(&body, name) && !out.contains(&test_name) {
            out.push(test_name);
        }
    }
    out
}

/// Does `body` reference `name` as a whole identifier - a call, a `use`, a bare mention - and
/// never as part of a LONGER identifier that merely contains it?
///
/// A substring check alone matched `read` inside a test that only names `thread`, and `parse`
/// inside `fn parse_works()` whether or not that test calls it - `github.com/telekom/sutura#1031`'s
/// own review. An identifier boundary on both sides is enough: Rust identifiers are `[A-Za-z0-9_]`,
/// so a match whose neighbours (if any) are outside that set is `name` on its own, whatever
/// follows - deliberately still permissive of a bare mention, not only a `name(` call, because a
/// `use` of the helper is the reach this module's own header already counts.
fn references(body: &str, name: &str) -> bool {
    let bytes = body.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut start = 0_usize;
    while let Some(found) = body.get(start..).and_then(|rest| rest.find(name)) {
        let at = start + found;
        let before_ok = at == 0 || !bytes.get(at - 1).is_some_and(|b| is_ident(*b));
        let after = at + name.len();
        let after_ok = bytes.get(after).is_none_or(|b| !is_ident(*b));
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}
#[cfg(test)]
mod tests {
    use super::{Deleted, Deletion, Touched, deletion_in, edited_helper_caller, removed_in, touched_in, touches};
    use crate::causality::diff::RemovedLine;
    use crate::causality::fixtures::{added_from, tree};
    use crate::causality::names::Ident;
    use crate::causality::regions::TestScope;
    use std::ops::Range;

    /// One `#[cfg(test)]` region covering `r`, the shape every fixture below needs.
    fn region(r: Range<usize>) -> TestScope {
        TestScope::Regions(vec![r])
    }

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

    /// `touched_in` reads added lines only; `removed_in` covers this shape for the gate.
    #[test]
    fn a_pure_deletion_names_nothing_the_stated_limit() {
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
    fn deleting_a_whole_test_is_named_the_same_as_deleting_one_of_its_asserts() {
        // N3 (`github.com/telekom/sutura#1031`'s review): the earlier version special-cased a
        // removed `#[test]` ATTRIBUTE line as "the test is gone, not weakened" and skipped it -
        // a perverse incentive, since deleting the whole test then passed while deleting one of
        // its asserts refused. `item_below`/`function_name` still resolve the name off the BASE
        // image, which has every line whether or not this diff removed it.
        let file = concat!(
            "#[test]\n",            // base line 1 - removed
            "fn whole_test() {\n",  // base line 2 - removed
            "    assert!(true);\n", // base line 3 - removed
            "}\n",                  // base line 4 - removed
        );
        let lines = existing_test(file);
        let removed = vec![
            RemovedLine {
                before: 1,
                text: String::from("#[test]"),
            },
            RemovedLine {
                before: 2,
                text: String::from("fn whole_test() {"),
            },
            RemovedLine {
                before: 3,
                text: String::from("    assert!(true);"),
            },
            RemovedLine {
                before: 4,
                text: String::from("}"),
            },
        ];
        assert_eq!(
            removed_in(&lines, &removed),
            vec![Deleted::Runs(Ident::parse("whole_test").unwrap())]
        );
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
        let scope = region(1..8);
        assert_eq!(edited_helper_caller(&lines, &added, &scope), vec![Ident::parse("t").unwrap()]);
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
        let scope = region(1..8);
        assert_eq!(edited_helper_caller(&lines, &added, &scope), Vec::new());
    }

    #[test]
    fn a_helper_edit_names_every_caller_not_only_the_first() {
        // N1 (`github.com/telekom/sutura#1031`'s review): the first-match version silently
        // dropped a real caller behind an earlier one. Two `#[test]`s call the same helper.
        let file = concat!(
            "#[cfg(test)]\n",                     // 1
            "mod tests {\n",                      // 2
            "    fn helper() -> u8 { 1 }\n",      // 3
            "    #[test]\n",                      // 4
            "    fn first() {\n",                 // 5
            "        assert_eq!(helper(), 1);\n", // 6
            "    }\n",                            // 7
            "    #[test]\n",                      // 8
            "    fn second() {\n",                // 9
            "        assert_eq!(helper(), 1);\n", // 10
            "    }\n",                            // 11
            "}\n",                                // 12
        );
        let lines = existing_test(file);
        let added = added_from(3, &["    fn helper() -> u8 { 2 }"]);
        let scope = region(1..12);
        assert_eq!(
            edited_helper_caller(&lines, &added, &scope),
            vec![Ident::parse("first").unwrap(), Ident::parse("second").unwrap()]
        );
    }

    #[test]
    fn a_helper_name_that_is_a_substring_of_another_identifier_is_not_a_false_match() {
        // N1: a bare substring check matched `read` inside a test that only names `thread`.
        // `references` requires an identifier boundary on both sides.
        let file = concat!(
            "#[cfg(test)]\n",               // 1
            "mod tests {\n",                // 2
            "    fn read() -> u8 { 1 }\n",  // 3
            "    #[test]\n",                // 4
            "    fn spawns_a_thread() {\n", // 5
            "        assert!(true);\n",     // 6
            "    }\n",                      // 7
            "}\n",                          // 8
        );
        let lines = existing_test(file);
        let added = added_from(3, &["    fn read() -> u8 { 2 }"]);
        let scope = region(1..8);
        assert_eq!(edited_helper_caller(&lines, &added, &scope), Vec::new());
    }

    #[test]
    fn a_test_item_is_never_walked_as_its_own_helper() {
        // The header's own rule: a `#[test]`-declared item is skipped by the helper walk -
        // `touched_in` already reaches it directly, and asking `calling_tests` about it would
        // scan every OTHER test for its bare name, a second and wrong-direction reach.
        let file = concat!(
            "#[cfg(test)]\n",           // 1
            "mod tests {\n",            // 2
            "    #[test]\n",            // 3
            "    fn edited() {\n",      // 4
            "        assert!(true);\n", // 5
            "    }\n",                  // 6
            "}\n",                      // 7
        );
        let lines = existing_test(file);
        // Line 5 is inside `edited`'s OWN item, so `touched_in` names it directly; the helper
        // walk over the same span must add nothing on top of that.
        let added = added_from(5, &["        assert!(true);"]);
        let scope = region(1..7);
        assert_eq!(edited_helper_caller(&lines, &added, &scope), Vec::new());
    }

    #[test]
    fn a_whole_file_scope_reaches_a_helper_outside_any_cfg_test_block() {
        // B2 (`github.com/telekom/sutura#1031`'s review): the earlier version walked only
        // `cfg_test_regions`, so an out-of-line `#[cfg(test)] mod x;` file, a `tests/*.rs`
        // target or a `#![cfg(test)]` file - none of which write an INLINE `#[cfg(test)]` marker
        // at all - never reached the helper walk. `TestScope::WholeFile` is what `regions::scope`
        // answers for all three.
        let file = concat!(
            "fn helper() -> u8 { 1 }\n",      // 1 - no #[cfg(test)] anywhere in this file
            "#[test]\n",                      // 2
            "fn t() {\n",                     // 3
            "    assert_eq!(helper(), 1);\n", // 4
            "}\n",                            // 5
        );
        let lines = existing_test(file);
        let added = added_from(1, &["fn helper() -> u8 { 2 }"]);
        assert_eq!(
            edited_helper_caller(&lines, &added, &TestScope::WholeFile),
            vec![Ident::parse("t").unwrap()]
        );
    }

    #[test]
    fn a_qualified_helper_is_still_a_helper() {
        // B2 (`github.com/telekom/sutura#1031`'s review): the earlier version's own line-prefix
        // check (`starts_with("fn ")`) skipped a helper written `pub(super) fn` or `async fn` -
        // `function_name` already reads past a visibility or `async` qualifier, so this walk asks
        // it directly instead of re-checking the prefix itself.
        let file = concat!(
            "#[cfg(test)]\n",                                 // 1
            "mod tests {\n",                                  // 2
            "    pub(super) async fn helper() -> u8 { 1 }\n", // 3
            "    #[test]\n",                                  // 4
            "    fn t() {\n",                                 // 5
            "        assert_eq!(helper(), 1);\n",             // 6
            "    }\n",                                        // 7
            "}\n",                                            // 8
        );
        let lines = existing_test(file);
        let added = added_from(3, &["    pub(super) async fn helper() -> u8 { 2 }"]);
        let scope = region(1..8);
        assert_eq!(edited_helper_caller(&lines, &added, &scope), vec![Ident::parse("t").unwrap()]);
    }

    #[test]
    fn deletion_in_subtracts_a_name_the_post_image_already_touches() {
        // B1's own caveat: the SAME test edited (an added line in its span) and evidenced by a
        // removed line in its BASE span is one edit (`github.com/telekom/sutura#1025`'s shape),
        // not a deletion with nothing added in its place - `touched_in` already routes it.
        let base_image = "#[test]\nfn t() {\n    assert!(true);\n    assert!(false);\n}\n";
        let post_image = "#[test]\nfn t() {\n    assert!(false);\n    assert!(1 == 1);\n}\n";
        let added = added_from(4, &["    assert!(1 == 1);"]);
        let removed = vec![RemovedLine {
            before: 3,
            text: String::from("    assert!(true);"),
        }];
        let base = tree(&[("crates/x/src/a.rs", base_image)]);
        let read = tree(&[("crates/x/src/a.rs", post_image)]);
        assert_eq!(
            deletion_in(&added, &removed, "crates/x/src/a.rs", &base, &read),
            Deletion::None,
            "the same test's own edit, not a separate deletion"
        );
    }

    #[test]
    fn deletion_in_fails_closed_on_an_unreadable_base_image() {
        // N5: `removed` is non-empty and the base cannot be read at all - answering `None` would
        // silently pass a deletion this gate cannot actually see.
        let removed = vec![RemovedLine {
            before: 3,
            text: String::from("    assert!(true);"),
        }];
        let read = tree(&[("crates/x/src/a.rs", "#[test]\nfn t() {}\n")]);
        let unreadable_base = tree(&[]);
        assert_eq!(
            deletion_in(&[], &removed, "crates/x/src/a.rs", &unreadable_base, &read),
            Deletion::BaseUnreadable
        );
    }
}
