//! Whether a raw-text needle may be pushed down as a necessary condition against a column's own
//! [`TextPolicy`], decided from the needle and the policy alone - never a row, an adapter's SQL
//! shape, or how long anything took. A pure function of query and config belongs in the hexagon's
//! interior: an adapter calls it to decide whether to push a condition down, and the application
//! calls the same function, over the same needle and policy, to name why a search it could not push
//! down ran long or wide instead of leaving that unnamed.

use std::fmt;

use super::text::TextPolicy;

/// Why `needle` may not be pushed down against a column's [`TextPolicy`].
///
/// Closed, and carries nothing from a row or a clock: every variant is decidable from the needle's
/// own bytes and the policy's own configured rules, so it is safe to show a caller or log without
/// disclosing anything beyond what their own query and the column's already-public policy shape
/// already say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum PrefilterIneligible {
    /// The column's policy declares no `raw_prefilter`, or no redaction rule at all - `raw_prefilter`
    /// only ever narrows an already-redacted search, never replaces checking it.
    NotDeclared,
    /// The needle is empty, over the 128-byte cap, or contains a byte that is neither ASCII
    /// alphanumeric nor `_` (LIKE's own single-char wildcard) - outside the alphabet a raw condition
    /// is built from.
    NeedleShape,
    /// Some alignment of the needle, read as a `_`-wildcard pattern, over one redaction rule's own
    /// replacement carries no literal disagreement - admitting the needle could let a match the
    /// replacement alone produced stand in for one the raw text never had. Carries that rule's
    /// index into [`TextPolicy::redactions`].
    AgreesWithReplacement { replacement_index: usize },
    /// Same shape as [`Self::AgreesWithReplacement`], against the policy's own truncation marker
    /// rather than a configured rule - reachable only where `max_text_bytes` is set.
    AgreesWithTruncationMarker,
}

impl fmt::Display for PrefilterIneligible {
    /// Safe to show a caller or log: every word comes from the enum's own name and the already-public
    /// policy shape, never a row or a clock.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDeclared => write!(f, "the column declares no raw prefilter"),
            Self::NeedleShape => write!(f, "the needle is outside the alphanumeric-or-underscore alphabet this allows"),
            Self::AgreesWithReplacement { replacement_index } => {
                write!(f, "the needle agrees with replacement #{replacement_index}")
            }
            Self::AgreesWithTruncationMarker => write!(f, "the needle agrees with the truncation marker"),
        }
    }
}

/// Whether `needle` (already extracted from a `LIKE '%needle%'` pattern) may be pushed down against
/// `policy`.
///
/// Folded upper-case first when `upper` (an Oracle `UPPER(column)` comparison folds the stored
/// value, so the replacement must be judged after the same fold) and compared ASCII
/// case-insensitively throughout when `folded` (a case-insensitive collation). `lob` skips the
/// truncation-marker check: Oracle's LOB columns are read and redacted without the fixed-width
/// truncation this policy's `max_text_bytes` otherwise applies.
///
/// # Errors
///
/// [`PrefilterIneligible`] naming why, decidable from `needle` and `policy` alone.
pub fn eligible(needle: &str, policy: &TextPolicy, upper: bool, folded: bool, lob: bool) -> Result<(), PrefilterIneligible> {
    if needle.is_empty() || needle.len() > 128 || !needle.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
        return Err(PrefilterIneligible::NeedleShape);
    }
    if policy.raw_prefilter().is_none() || policy.redactions().is_empty() {
        return Err(PrefilterIneligible::NotDeclared);
    }
    for (replacement_index, rule) in policy.redactions().iter().enumerate() {
        if !preserves(needle, rule.replacement(), upper, folded) {
            return Err(PrefilterIneligible::AgreesWithReplacement { replacement_index });
        }
    }
    if !lob && policy.max_text_bytes().is_some() && !preserves(needle, "...[truncated]", upper, folded) {
        return Err(PrefilterIneligible::AgreesWithTruncationMarker);
    }
    Ok(())
}

/// No needle occurrence may be *forced to exist* by the replacement alone.
///
/// Slide the needle (as a `_`-wildcard pattern) over the replacement at every alignment that shares
/// at least one byte with it - call that shared span the overlap. Outside the overlap sits text this
/// function knows nothing about (real, unredacted bytes to either side), so soundness needs every
/// alignment where the *overlap itself* cannot already force a mismatch to be refused: otherwise an
/// adjacent, unconstrained byte could complete a match that the replacement's own fixed characters
/// did not. A `_` at an overlap position never forces a mismatch (it is LIKE's own wildcard, so it
/// agrees with anything), but a literal needle byte that differs from the replacement's byte there
/// still blocks that whole alignment.
///
/// **This widens eligibility beyond the pre-`_` rule even for a needle with no `_` in it.** The old
/// rule refused whenever the needle contained the replacement's first or last byte, which is
/// sufficient but not necessary: needle `ac` against replacement `ab` shares no substring with it and
/// is refused by the old rule (`ac` contains `a`, the replacement's first byte) but admitted here,
/// because the only overlaps - `ac`/`a` and `c`/`ab` - each still carry a disagreeing byte (`c` vs
/// `a`, and `c` vs `a`). This is sound by the same argument as above; it was never implied by "this
/// reduces to the old check," which does not hold.
///
/// Not maximal: an alignment whose overlap is a lone `_` still refuses here even though the `_`
/// itself cannot force a mismatch, because leaving it admitted would require checking that no
/// *redaction rule* can itself produce a zero-width match that shifts the alignment - a replacement
/// rule built from a regex that matches an empty string (`$`, `^`, `a*`, a lookaround) inserts its
/// replacement without consuming any raw text, so the same needle can be unsound against such a rule
/// even where every overlap carries only `_`. Refusing every lone-`_` overlap, rather than proving
/// each redaction rule's pattern cannot match empty, is the cheaper and safe choice.
///
/// `folded` compares ASCII case-insensitively, so the proof also holds under a case-insensitive collation.
fn preserves(needle: &str, replacement: &str, upper: bool, folded: bool) -> bool {
    if replacement.is_empty() || !replacement.is_ascii() {
        return false;
    }
    let normalized = if upper {
        replacement.to_ascii_uppercase()
    } else {
        String::from(replacement)
    };
    let (needle, normalized) = if folded {
        (needle.to_ascii_lowercase(), normalized.to_ascii_lowercase())
    } else {
        (String::from(needle), normalized)
    };
    let needle = needle.as_bytes();
    let replacement = normalized.as_bytes();
    // Every alignment with a nonempty overlap, as a pair of tails zipped together (the shorter one
    // bounds the overlap): either the replacement starts `skip` bytes before needle[0], so the
    // overlap is `replacement[skip..]` against the whole needle, or it starts `skip` bytes into the
    // needle, so the overlap is the whole replacement against `needle[skip..]`. `.get` turns every
    // index into a bounds check rather than a potential panic, though `skip` never runs past either
    // slice's length here.
    let leading = (1..replacement.len()).filter_map(|skip| Some((needle, replacement.get(skip..)?)));
    let trailing = (0..needle.len()).filter_map(|skip| Some((needle.get(skip..)?, replacement)));
    leading.chain(trailing).all(|(needle_tail, replacement_tail)| {
        needle_tail
            .iter()
            .zip(replacement_tail)
            .any(|(&needle_byte, &replacement_byte)| needle_byte != b'_' && needle_byte != replacement_byte)
    })
}

#[cfg(test)]
mod tests {
    use regex::{NoExpand, Regex};

    use super::{PrefilterIneligible, eligible, preserves};
    use crate::catalog::TextPolicy;

    fn policy(rules: &[(&str, &str)]) -> TextPolicy {
        let redact: Vec<_> = rules
            .iter()
            .map(|(pattern, replace)| serde_json::json!({"pattern": pattern, "replace": replace}))
            .collect();
        serde_json::from_value(serde_json::json!({
            "redact": redact,
            "raw_prefilter": {"acknowledged_because": "accepted secret-dependent timing"},
        }))
        .expect("a valid policy")
    }

    #[test]
    fn refuses_a_needle_with_no_declared_prefilter_or_no_redaction() {
        assert_eq!(
            eligible("ab", &TextPolicy::empty(), false, false, false),
            Err(PrefilterIneligible::NotDeclared)
        );
    }

    #[test]
    fn refuses_a_needle_outside_the_alphanumeric_or_underscore_alphabet() {
        assert_eq!(
            eligible("ab-c", &policy(&[("a", "masked")]), false, false, false),
            Err(PrefilterIneligible::NeedleShape)
        );
        assert_eq!(
            eligible("", &policy(&[("a", "masked")]), false, false, false),
            Err(PrefilterIneligible::NeedleShape)
        );
    }

    #[test]
    fn names_the_first_replacement_a_needle_agrees_with() {
        assert_eq!(
            eligible("abc", &policy(&[("x", "safe"), ("y", "abc")]), false, false, false),
            Err(PrefilterIneligible::AgreesWithReplacement { replacement_index: 1 })
        );
    }

    /// The widening counterexample from the review: a pure-literal needle the pre-`_` rule refused
    /// (it shares the replacement's first byte) but `preserves` admits, soundly.
    #[test]
    fn preserves_admits_a_pure_literal_needle_the_old_boundary_byte_rule_refused() {
        assert!(preserves("ac", "ab", false, false));
        assert!(preserves("MXY", "MASKED", false, false));
    }

    /// The concrete counterexample the admission rule must refuse: pattern `$` (zero-width) with
    /// replacement `[m]` against raw `ab` redacts to `ab[m]`, which matches `%ab_%` where `ab` itself
    /// does not - so `preserves` must not admit needle `ab_` against replacement `[m]`.
    #[test]
    fn preserves_refuses_a_lone_underscore_overlap_against_a_zero_width_replacement() {
        assert!(!preserves("ab_", "[m]", false, false));
    }

    /// `preserves` admits a needle against a redaction rule, or a sequence of them, only when no
    /// alignment of the needle over any rule's replacement can be forced to match. This exhaustively
    /// checks the property that admission claims: over every needle, every short sequence of
    /// regex-driven redaction rules (applied in order, as a column's declared `redact` list would
    /// be), and every raw text a small alphabet can spell, if `preserves` admits the needle against
    /// each rule in the sequence, a LIKE match on the text the rules produce implies a LIKE match on
    /// the raw text.
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
}
