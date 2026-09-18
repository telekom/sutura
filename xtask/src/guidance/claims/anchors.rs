//! The one decidable shape of a stale `Evidence::holds`: a bare count.
//!
//! `github.com/telekom/sutura#603`. General anchor *truth* is not decidable from the tree alone -
//! `Evidence::stands` proves a comment is PRESENT, never that it is TRUE - so this does not try to
//! gate truth. It gates the one class that actually rotted: a row whose `holds` is a standalone
//! integer or English number word, describing something that can change count without the anchor
//! ever going absent. A count belongs in a [`super::counts::Counted`] row, which derives its
//! number from the tree instead of pinning one by hand.

/// English number words a `holds` literal might spell out, standalone.
///
/// Deliberately short - the failure this replaces was a hand count of refusal arms ("four", then
/// "five", then "six"), not a large number ever written in prose.
const WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
];

/// Is `holds`, trimmed, nothing but a number - digits or an English number word?
///
/// Standalone is the whole test: `"mcp-e2e:"` carries a digit and stays evidence, `"LIMIT
/// 10001"` carries one inside a longer literal and stays evidence. Only a `holds` that IS the
/// number, with nothing else, is what a `Counted` row exists to replace.
pub(super) fn is_a_bare_count(holds: &str) -> bool {
    let trimmed = holds.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    WORDS.iter().any(|word| trimmed.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests;
