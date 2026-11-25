//! Whether a raw-text needle may be pushed down as a necessary condition against a column's own
//! [`TextPolicy`], decided from the needle and the policy alone - never a row, an adapter's SQL
//! shape, or how long anything took. A pure function of query and config belongs in the hexagon's
//! interior: an adapter calls [`eligible`] to decide whether to push a condition down, and the
//! application calls it again, over the same needle and policy, to name why a search it could not
//! push down ran long or wide instead of leaving that unnamed.

use std::fmt;

use super::text::TextPolicy;

/// Why `needle` may not be pushed down against a column's [`TextPolicy`].
///
/// Closed, and carries nothing from a row or a clock: every variant is decidable from the needle's
/// own bytes and the policy's own configured rules. **Full detail (this type's own [`fmt::Display`]
/// and the [`Self::AgreesWithReplacement`] index) is for a server-side log, never a caller.**
/// [`Self::caller_category`] is the coarse, three-way grouping a caller's own refusal may carry -
/// see that method for what a sweep of needles can still learn from it.
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
    /// For a server-side log only; callers get [`Self::caller_category`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDeclared => write!(f, "the column declares no raw prefilter"),
            Self::NeedleShape => write!(f, "the needle is outside the alphanumeric-or-underscore alphabet this allows"),
            Self::AgreesWithReplacement { replacement_index } => {
                write!(
                    f,
                    "the needle could match redaction rule #{replacement_index}'s own replacement text, \
                     which a raw search over the unredacted column would never have matched"
                )
            }
            Self::AgreesWithTruncationMarker => write!(f, "the needle agrees with the truncation marker"),
        }
    }
}

impl PrefilterIneligible {
    /// The coarse, three-way category a caller's own refusal may carry - the only detail that
    /// reaches outside this process. [`Self::AgreesWithReplacement`]'s rule index and
    /// [`Self::AgreesWithTruncationMarker`] collapse into one category here: a caller still learns
    /// that the needle overlapped SOME redaction marker, never which rule or its index, but a sweep
    /// of needles against the same column still reveals, needle by needle, which characters a
    /// marker's own shape contains.
    #[must_use]
    pub const fn caller_category(&self) -> &'static str {
        match self {
            Self::NotDeclared => "no raw prefilter is declared for this column",
            Self::NeedleShape => {
                "the search term's shape is not eligible (empty, longer than 128 bytes, or a \
                 character outside letters, digits or `_`)"
            }
            Self::AgreesWithReplacement { .. } | Self::AgreesWithTruncationMarker => {
                "the search term overlaps the column's redaction marker"
            }
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
/// sufficient but not necessary: needle `ac` against replacement `ab` is refused by the old rule
/// (`ac` contains `a`, the replacement's first byte) but admitted here, because the only overlaps -
/// `ac`/`b`, `ac`/`ab`, and `c`/`ab` - each still carry a disagreeing byte (`a` vs `b`, `c` vs `b`,
/// and `c` vs `a`). This is sound by the same argument as above; it was never implied by "this
/// reduces to the old check," which does not hold.
///
/// Not maximal: an alignment whose overlap is a lone `_` still refuses here even though the `_`
/// itself cannot force a mismatch, because leaving it admitted would require checking that every
/// *redaction rule* replaces exactly one raw character with exactly one character. A rule whose
/// pattern matches an empty string (`$`, `^`, `a*`, a lookaround) inserts its replacement without
/// consuming any raw text, and a rule whose pattern consumes more than one character (`b+` against
/// `abba`, replaced with `x`: raw `abba` becomes `axa`, and `%a_a%` matches `axa` but not `abba`) is
/// just as unsound - either shifts what the needle lines up against on either side of the overlap,
/// so the same needle can be unsound against such a rule even where every overlap carries only `_`.
/// Refusing every lone-`_` overlap, rather than proving each redaction rule's pattern always matches
/// exactly one character, is the cheaper and safe choice.
///
/// `folded` compares ASCII case-insensitively, so the proof also holds under a case-insensitive collation.
///
/// `pub`, not `pub(super)`: `combine::statement::tests::prefilter_property` in `sutura-exec-datafusion`
/// is the only caller outside this crate, and it reaches in at this visibility rather than through
/// the planner's SQL-rewrite surface, because what it checks is this function's own admission rule
/// against an actual regex-driven redaction, not a statement rewrite.
pub fn preserves(needle: &str, replacement: &str, upper: bool, folded: bool) -> bool {
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

    /// The widening counterexample's sibling: a needle naming the policy's own truncation marker is
    /// refused the same way, only reachable where `max_text_bytes` is set and the column is not a LOB.
    #[test]
    fn names_agreement_with_the_truncation_marker_only_where_truncation_applies() {
        let policy: TextPolicy = serde_json::from_value(serde_json::json!({
            "redact": [{"pattern": "x", "replace": "safe"}],
            "raw_prefilter": {"acknowledged_because": "accepted secret-dependent timing"},
            "max_text_bytes": 64,
        }))
        .expect("a valid policy");
        assert_eq!(
            eligible("truncated", &policy, false, false, false),
            Err(PrefilterIneligible::AgreesWithTruncationMarker)
        );
        assert_eq!(eligible("truncated", &policy, false, false, true), Ok(()));
    }
}
