//! Which lines of a file BEGIN inside a string literal.
//!
//! Split out of `super` because that file reached the unexemptable 1000-line cap when this
//! arrived - which is the same cap `super::super::relocation` exists to make survivable, reached
//! by the change that added it. The seam is real rather than a line count: everything here is one
//! question asked of a whole file image, while its parent's subject is which lines of a file are
//! TEST code.
//!
//! **IMPLEMENTATION ONLY, AND DELIBERATELY.** A file that adds a `#[test]` is never reverted by
//! `super::super`'s own gate, so moving assertions into a new module orphans them when the
//! declaring file is reverted. The assertions for this function therefore stay where they already
//! were - `super::super::relocation`'s
//! `an_indentation_change_inside_a_string_literal_is_refused` drives it end to end, with a control
//! one line away whose fixture indentation is preserved - which is this repository's own rule for
//! a file at the cap: move the harness, leave every assertion where it is.

use std::collections::BTreeSet;

use super::Nesting;

/// The 1-based lines of `text` that BEGIN inside a string literal.
///
/// **LEADING WHITESPACE ON SUCH A LINE IS PART OF THE VALUE**, so a comparison that trims it reads
/// two different strings as equal - and an expected-output fixture is exactly what a test asserts
/// on. `super::super::relocation` keys those lines on their RAW text for that reason, which is the
/// one place in this tree where trimming is a false negative rather than a convenience.
///
/// The same [`Nesting`] lexer the region scan uses, so a `"` inside a comment opens nothing and a
/// raw string closes on its own hash count. A file this cannot read contributes no lines, which is
/// the direction that compares MORE strictly rather than less: an unread image trims nothing away.
pub(in crate::causality) fn inside_a_literal(text: &str) -> BTreeSet<usize> {
    let mut scanner = Nesting::braces();
    let mut found = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        if scanner.in_literal() {
            found.insert(index + 1);
        }
        scanner.feed(line);
    }
    found
}
