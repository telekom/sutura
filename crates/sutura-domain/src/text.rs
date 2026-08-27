//! The one character-level rule every piece of authored text in this crate is held to.
//!
//! **A module for a single predicate, because the predicate used to be two and the two had already
//! drifted.** The same decision - which code points make the text a reviewer reads differ from the
//! text that runs - was written down once for authored SQL and once for a glossary phrase, and the
//! second copy was missing `U+00AD`, `U+2060..2064` and `U+FFF9..FFFB`. Nothing was wrong with
//! either list on the day it was written; what was wrong was that there were two, so a range added
//! to one was absent from the other and no check anywhere could notice. A load-bearing refusal is
//! not a thing to keep two copies of, and the fix for a duplicated decision is not a comment asking
//! the next author to update both.
//!
//! So this module owns the set, every parser in this crate calls [`is_invisible`], and the tests at
//! the bottom of this file walk **every** Unicode scalar value to assert that what
//! [`crate::expression::SqlFragment::parse`] refuses is exactly what this predicate names - which is
//! the check the two copies could not have, and which fails if either list is edited alone.
//!
//! # Why it is not in `model`, where the other text rules are
//!
//! [`crate::model`]'s parser is the *identifier* rule: lower-case, digits, underscore, and a length.
//! It refuses these code points already, by refusing everything that is not one of a few dozen ASCII
//! bytes, and it has nothing to say about the types this module exists for. A phrase, a note body, a
//! version label and an authored SQL fragment each accept a different, much wider character set -
//! spaces, non-ASCII letters, quotes, parentheses, newlines - and the ONE thing all four agree on is
//! this set. That agreement is what the module is: the rule that survives when every other rule
//! about the shape of the text is different.
//!
//! # `pub(crate)`, and why it stops here
//!
//! Every caller is a parser in this crate. The renderer that puts this text into an agent-facing
//! prompt - `sutura_app::prompt` - deliberately does **not** call it: the rule in this repository is
//! *refuse at load, never alter at render*, because a render that quietly dropped a character would
//! make the rendered document differ from the text the definition digest certifies, and would do so
//! invisibly. Publishing this predicate would mainly make that easy to write. A caller outside the
//! crate that thinks it needs this needs a parse boundary instead.

/// A character a terminal, a diff or a browser does not show, or shows in the wrong order.
///
/// Refused wherever authored text is parsed, and the reason is the whole of it: **the text a
/// reviewer reads has to be the text that runs.** These are all general category `Cf`, so
/// `char::is_control` is **false** for every one of them - a fact the tests below pin rather than
/// assume - and a control-character check therefore cannot see a single one. That is what makes this
/// a second refusal beside the control-character one in every type that has both, rather than a
/// wider version of it.
///
/// This is Trojan Source, CVE-2021-42574, pointed at reviewed content. A right-to-left override
/// inside a string literal reads in a terminal, in a diff and in a pull request as
/// `status = 'active'` and compiles to a comparison against something else. A soft hyphen inside a
/// phrase draws nothing until a line happens to break there, so `mrr` and `m\u{00AD}rr` are one word
/// to every reader and two keys to every index. A note body carrying either reaches an agent's
/// context under a digest taken over text nobody read.
///
/// Enumerated rather than taken as a whole category. `Cf` also holds the language-tag and
/// variation-selector blocks, and a category test would move with the Unicode table under a
/// dependency bump - which for a load-bearing refusal is a set that changes without a diff. The
/// ranges here are the ones that reorder or erase rendered text:
///
/// - `U+00AD` soft hyphen, and `U+FEFF` byte order mark - invisible, and legal mid-string;
/// - `U+200B..200F` the zero-width set, ending in the two directional marks;
/// - `U+202A..202E` the bidirectional embeddings and overrides - the Trojan Source characters;
/// - `U+2060..2064` word joiner and the invisible operators;
/// - `U+2066..2069` the bidirectional isolates, which do the same job as the overrides;
/// - `U+FFF9..FFFB` interlinear annotation, which hides one run of text behind another.
///
/// The cost is stated rather than hidden: `U+200C` and `U+200D` are inside the second range and
/// carry meaning in Persian, in several Indic scripts and inside an emoji sequence, so text needing
/// one cannot be written in a phrase or a note body here. That is the price of a glossary whose two
/// entries cannot look identical to a reader, and it is one line to revisit.
pub(crate) const fn is_invisible(character: char) -> bool {
    matches!(
        character,
        '\u{00AD}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// The first such character in some text, for the refusal that has to name one.
///
/// A function rather than the `chars().find(..)` written out at each parser, so that "which one is
/// reported" - the first in reading order - is one decision rather than three that agree by
/// accident. The character and not a `bool`, because an error saying only that something invisible
/// is in there leaves an author looking at prose that appears correct.
pub(crate) fn first_invisible(text: &str) -> Option<char> {
    text.chars().find(|character| is_invisible(*character))
}

#[cfg(test)]
mod tests {
    use super::{first_invisible, is_invisible};
    use crate::expression::{InvalidFragment, SqlFragment};

    /// One fragment holding exactly one candidate character, in the middle so that no parser trims
    /// it away.
    fn fragment(character: char) -> String {
        format!("SUM({character}mrr_eur)")
    }

    #[test]
    fn every_range_is_named_at_both_ends_and_no_neighbour_of_one_is() {
        // Both ends of all seven ranges, so a typo in a bound fails here rather than at a review a
        // year from now.
        for code in [
            0x00AD_u32, 0x200B, 0x200F, 0x202A, 0x202E, 0x2060, 0x2064, 0x2066, 0x2069, 0xFEFF, 0xFFF9, 0xFFFB,
        ] {
            let offending = char::from_u32(code).expect("a listed code point is a character");
            assert!(is_invisible(offending), "{code:#06x} is one of the invisible ones");
            // The premise the whole refusal rests on: general category `Cf`, so a control-character
            // check sees none of these. If a future Rust widens `char::is_control` this fails, and
            // that is the right place to find out.
            assert!(!offending.is_control(), "{code:#06x} is not a control character");
        }
        // And the code point on each side of each range is NOT named, so this is the set written
        // down rather than a wider sweep that would reject ordinary text.
        for code in [
            0x00AC_u32, 0x00AE, 0x200A, 0x2010, 0x2029, 0x202F, 0x205F, 0x2065, 0x206A, 0xFEFE, 0xFF00, 0xFFFC,
        ] {
            let benign = char::from_u32(code).expect("a listed code point is a character");
            assert!(!is_invisible(benign), "{code:#06x} is not one of the invisible ones");
        }
    }

    #[test]
    fn the_first_one_in_reading_order_is_the_one_reported() {
        assert_eq!(first_invisible("plain prose"), None);
        assert_eq!(first_invisible("act\u{200B}ive\u{202E}"), Some('\u{200B}'));
        assert_eq!(first_invisible("\u{FEFF}leading"), Some('\u{FEFF}'));
    }

    /// The test the two drifted copies could not have had.
    ///
    /// It walks the whole Unicode scalar range and asserts that the set
    /// [`SqlFragment::parse`] refuses as invisible is the same set [`is_invisible`] names - not a
    /// sample of it, the same set. `expression` still holds a private copy of the list, pending the
    /// one-line change that makes it call this function; until then this is what keeps the two from
    /// disagreeing again, and it fails whichever of the two is edited alone.
    #[test]
    fn the_authored_expression_refusal_and_this_predicate_are_one_set() {
        let mut refused = 0_usize;
        for code in 0..=0x0010_FFFF_u32 {
            let Some(character) = char::from_u32(code) else {
                continue;
            };
            let named = is_invisible(character);
            let refused_as_invisible = matches!(
                SqlFragment::parse(fragment(character)),
                Err(InvalidFragment::InvisibleCharacter { code: reported }) if reported == code
            );
            assert_eq!(refused_as_invisible, named, "{code:#06x}");
            refused = refused.saturating_add(usize::from(named));
        }
        // The size of the set, so that a range collapsing to nothing - or the walk itself silently
        // skipping - is a failure rather than a test that passes over an empty set.
        assert_eq!(refused, 24);
    }
}
