//! Which of a line's MARGINS are part of a string literal's value.
//!
//! Split out of `super` because that file reached the unexemptable 1000-line cap when this
//! arrived - the same cap `super::super::relocation` exists to make survivable, reached by the
//! change that added it. The seam is real rather than a line count: everything here is one
//! question asked of a whole file image, while its parent's subject is which lines are TEST code.
//!
//! **IMPLEMENTATION ONLY, AND DELIBERATELY.** A file that adds a `#[test]` is never reverted by
//! `super::super`'s own gate, so moving assertions into a new module orphans them when the
//! declaring file is reverted. The assertions for this live in `super::super::relocation`, where
//! assertions already were, and they drive it end to end with a control one line away in each
//! direction - which is this repository's rule for a file at the cap: move the harness, leave
//! every assertion where it is.

use std::collections::BTreeMap;

use super::Nesting;

/// Which of a line's margins are a string literal's value rather than the source's own layout.
///
/// **TWO INDEPENDENT ANSWERS, and the first version of this carried only the first.** A line that
/// BEGAN inside a literal has leading whitespace that is value; a line that ENDED inside one has
/// trailing bytes that are value. They are different lines: the line that OPENS a multi-line
/// literal begins outside it, so a check that asked only *did this line begin inside* trimmed the
/// opening line's trailing bytes away - measured at exit 0 over a fixture whose two trailing
/// spaces were dropped, under a verdict saying the diff changed nothing.
///
/// Keeping them apart is what lets a relocation still DEDENT the line that opens a literal, which
/// is ordinary when a test leaves `mod tests { .. }`: its leading margin is code, so it is trimmed,
/// while its trailing margin is value, so it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::causality) struct Margins {
    /// The line BEGAN inside a literal, so its leading whitespace is part of the value.
    pub(in crate::causality) leading: bool,
    /// The line ENDED inside a literal, so its trailing bytes are part of the value.
    pub(in crate::causality) trailing: bool,
}

/// Every 1-based line of `text` with a margin inside a string literal, and which margin.
///
/// A line in neither state gets no entry, so the common case allocates nothing per line and the
/// caller's default - trim both ends - needs no special case.
///
/// The same [`Nesting`] lexer the region scan uses, so a `"` inside a comment opens nothing and a
/// raw string closes on its own hash count. A file this cannot read contributes no lines, which is
/// the direction that compares MORE strictly rather than less: an unread image trims nothing away.
pub(in crate::causality) fn literal_margins(text: &str) -> BTreeMap<usize, Margins> {
    let mut scanner = Nesting::braces();
    let mut found = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let leading = scanner.in_literal();
        scanner.feed(line);
        let trailing = scanner.in_literal();
        if leading || trailing {
            found.insert(index + 1, Margins { leading, trailing });
        }
    }
    found
}
