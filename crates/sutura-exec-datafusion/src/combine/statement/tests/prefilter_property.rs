//! `preserves` admits a needle against a redaction rule, or a sequence of them, only when no
//! alignment of the needle over any rule's replacement can be forced to match. This exhaustively
//! checks the property that admission claims: over every needle, every short sequence of
//! regex-driven redaction rules (applied in order, as a column's declared `redact` list would be),
//! and every raw text a small alphabet can spell, if `preserves` admits the needle against each
//! rule in the sequence, a LIKE match on the text the rules produce implies a LIKE match on the
//! raw text. `preserves` is deliberately exercised directly (it is `pub(super)`, reached as
//! `super::super::text_prefilter::preserves`) rather than through `condition`'s SQL-rewrite
//! surface: the property under test is the admission rule against an actual regex-driven
//! redaction, not the planner's statement rewrite, which `text_prefilter.rs`'s own tests already
//! cover.
//!
//! Every call here passes `upper = false, folded = false`: the sweep does not exercise
//! `preserves`'s uppercasing or case-folding branches. `text_prefilter.rs`'s own tests cover those
//! through the SQL-rewrite surface instead.

use regex::{NoExpand, Regex};

use super::super::text_prefilter::preserves;

/// A column's one `redact` rule: an engine-side regex and its literal replacement.
struct Rule {
    pattern: &'static str,
    replacement: &'static str,
}

/// Includes two zero-width patterns (`$`, `a*`) alongside ordinary substring-consuming ones, so a
/// sequence can reproduce the `$`/`[m]`/`ab` counterexample the admission rule must still refuse.
const RULES: &[Rule] = &[
    Rule {
        pattern: "a",
        replacement: "b",
    },
    Rule {
        pattern: "ab",
        replacement: "cd",
    },
    Rule {
        pattern: "b+",
        replacement: "x",
    },
    Rule {
        pattern: "$",
        replacement: "m",
    },
    Rule {
        pattern: "$",
        replacement: "[m]",
    },
    Rule {
        pattern: "a*",
        replacement: "y",
    },
    Rule {
        pattern: "^",
        replacement: "[m]",
    },
    Rule {
        pattern: "b",
        replacement: "aab",
    },
];

/// Every nonempty string over `{a, b}` up to `max_len`, including the empty one.
fn raw_texts(max_len: usize) -> Vec<String> {
    let mut texts = vec![String::new()];
    for _ in 0..max_len {
        texts = texts
            .iter()
            .flat_map(|text| [format!("{text}a"), format!("{text}b")])
            .chain(texts.iter().cloned())
            .collect();
    }
    texts.sort();
    texts.dedup();
    texts
}

/// Every nonempty needle over `{a, b, _}` up to `max_len`: the admitted alphabet, `_` included.
fn needles(max_len: usize) -> Vec<String> {
    let mut needles = vec![String::new()];
    for _ in 0..max_len {
        needles = needles
            .iter()
            .flat_map(|needle| [format!("{needle}a"), format!("{needle}b"), format!("{needle}_")])
            .collect();
    }
    needles.into_iter().filter(|needle| !needle.is_empty()).collect()
}

/// `%needle%` over `text`, with `_` standing for any one ASCII byte - LIKE's own wildcard, emulated
/// at the byte granularity `preserves` reasons about (every candidate here is single-byte ASCII).
fn like_contains(text: &str, needle: &str) -> bool {
    let text = text.as_bytes();
    let needle = needle.as_bytes();
    if needle.len() > text.len() {
        return false;
    }
    (0..=text.len() - needle.len()).any(|start| {
        text[start..start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(&byte, &pattern)| pattern == b'_' || byte == pattern)
    })
}

/// Every rule in `sequence`, applied in declared order, exactly as a column's redactions run.
fn redact(text: &str, sequence: &[&Rule]) -> String {
    let mut text = String::from(text);
    for rule in sequence {
        let regex = Regex::new(rule.pattern).expect("a fixed test pattern compiles");
        text = regex.replace_all(&text, NoExpand(rule.replacement)).into_owned();
    }
    text
}

#[test]
fn preserves_admitting_every_rule_in_a_sequence_implies_redacted_match_implies_raw_match() {
    let raws = raw_texts(5);
    let candidates = needles(3);
    let mut sequences: Vec<Vec<&Rule>> = RULES.iter().map(|rule| vec![rule]).collect();
    for first in RULES {
        for second in RULES {
            sequences.push(vec![first, second]);
        }
    }
    let mut checked_zero_width = false;
    let mut checked_two_rules = false;
    for needle in &candidates {
        for sequence in &sequences {
            if !sequence.iter().all(|rule| preserves(needle, rule.replacement, false, false)) {
                continue;
            }
            checked_zero_width |= sequence.iter().any(|rule| rule.pattern == "$" || rule.pattern == "a*");
            checked_two_rules |= sequence.len() == 2;
            for raw in &raws {
                let redacted = redact(raw, sequence);
                if like_contains(&redacted, needle) {
                    assert!(
                        like_contains(raw, needle),
                        "needle {needle:?} admitted against {:?} but matched only after redaction: \
                         raw {raw:?} -> {redacted:?}",
                        sequence
                            .iter()
                            .map(|rule| (rule.pattern, rule.replacement))
                            .collect::<Vec<_>>(),
                    );
                }
            }
        }
    }
    assert!(
        checked_zero_width,
        "the sweep must exercise at least one zero-width rule's admitted needles"
    );
    assert!(
        checked_two_rules,
        "the sweep must exercise at least one two-rule sequence's admitted needles"
    );
}

/// The concrete counterexample the admission rule must refuse: pattern `$` (zero-width) with
/// replacement `[m]` against raw `ab` redacts to `ab[m]`, which matches `%ab_%` where `ab` itself
/// does not - so `preserves` must not admit needle `ab_` against replacement `[m]`.
#[test]
fn preserves_refuses_a_lone_underscore_overlap_against_a_zero_width_replacement() {
    assert!(!preserves("ab_", "[m]", false, false));
    assert!(like_contains(
        &redact(
            "ab",
            &[&Rule {
                pattern: "$",
                replacement: "[m]"
            }]
        ),
        "ab_"
    ));
    assert!(!like_contains("ab", "ab_"));
}

// The widening counterexample from the review - a pure-literal needle the pre-`_` rule refused but
// `preserves` admits, soundly - is already measured through the SQL-rewrite surface by
// `a_pure_literal_needle_sharing_a_boundary_byte_is_admitted_when_every_alignment_still_mismatches`
// in `tests/text_prefilter.rs`; it is not repeated here.
