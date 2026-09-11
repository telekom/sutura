//! The fragment the pinned parser did not return from, and the bound that refuses it.
//!
//! A submodule for the same mechanical reason as [`super::dialect_resolution`]: `cargo xtask
//! max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and the parent is
//! within twenty of it. The harness is what moved here; the cell that reads the recorded artifact
//! stays in the parent, because the `mod` line below has to live in a file `just causality` holds
//! at HEAD or the whole module is never compiled against base and nothing is measured.
//!
//! **Every assertion here is over the SENTENCE and not over the variant**, which is a deliberate
//! trade rather than a weaker test by accident. `ExpressionError::UnclosedParenthesis` is new on
//! this change, so a `matches!` on it would not COMPILE against base - and `just causality` reads a
//! base tree that does not build as inconclusive, which is not a red. Over the sentence, these
//! cells build against base and fail there: all but one of them by running out of the deadline,
//! because the parse loops, and that one by naming a different refusal. Nothing but this bound
//! produces the words asserted below, and the compile is what holds it: `check` returns that
//! variant or one of the others, and there is no third outcome.
//!
//! **One assertion here is NOT red against base, and is written down rather than counted as
//! coverage:** the over-refusal case at the end of
//! `the_bound_is_over_tokens_so_a_closer_inside_a_string_literal_closes_nothing` passes on a tree
//! with no guard at all, because such a tree accepts that fragment too. What it is red against is a
//! guard that counts `(` against `)` in the text - measured. It rides inside a `#[test]` that IS
//! red on base, so the gate's verdict on this module does not come from it.

use std::thread;
use std::time::{Duration, Instant};

use super::portable;

/// One recorded artifact: its bytes, and the 1-based character its unclosed parenthesis sits at. A
/// named pair because `-D clippy::type-complexity` refuses the tuple written inline.
type Artifact = (&'static [u8], usize);

/// Every artifact the `sql_expression` target recorded for this defect, with the character its own
/// unclosed parenthesis sits at. Each is quarantined as the committed seed named beside it, so
/// `just fuzz-smoke` replays exactly these bytes.
///
/// **Three artifacts, ONE upstream defect, and the report shape is whatever the run happened to
/// notice first** - two timeouts while `format!` copies a growing string, one out-of-memory once
/// the copying has churned enough of it. Reproduced individually against the pinned parser: none of
/// the three returns, and each returns as soon as its parentheses are balanced.
///
/// **Every byte is ASCII**, which is why the `NonAscii` bound cannot see any of them: that one
/// closes a generator panic on multi-byte text, and this input never reaches the generator.
///
/// All three carry `.:`, which invited a bound on the construct instead - and the construct is the
/// wrong axis, measured: `mrr_eur.:S1(9)` parses and returns, while `CAST(mrr_eur AS S1(9` loops
/// with no `.:` in it. See `super::super::unclosed_parenthesis`.
const RECORDED: &[Artifact] = &[
    // `unclosed-paren-timeout`, `timeout-472fb665086f6dab29dfbd1510afbfea172e94ee`.
    (b"$a^-a61.c.:S1a.:#S1(^cAUAU", 20),
    // `unclosed-paren-timeout-in-subscript`, `timeout-10dad32e8a6573c813df7e7f1c2038f0174b18a9`.
    // The one whose recorded stack reaches `parse_data_type` through `maybe_parse_subscript`, and
    // the reason the seed is named for the route rather than for the hash.
    (b"SSE%LE.:E.E.:SEIF(~~~$R_ta", 18),
    // `unclosed-paren-oom`, `oom-00f6892aed5dac930e8926cca65df177d3fdaeb1`. Reported as an
    // out-of-memory whose LIVE heap was ~30 MB, every top live context libFuzzer's own, against
    // ~1M allocation churn plus the sanitizer's quarantine - which is what a `format!` per turn of
    // a loop that does not terminate looks like from outside, and not one large allocation.
    (b"IF~F((NU .:rv ((NU .:r~>", 5),
];

/// How long the compile is given to come back at all.
///
/// Not a bound on the compile and not a performance assertion: with the guard the call returns in
/// microseconds, so this deadline is only ever reached by a build that has no guard, where the
/// parse loops forever and no deadline would be long enough. Ten seconds keeps the red run short.
const MUST_RETURN_WITHIN: Duration = Duration::from_secs(10);

/// What the compile said about one fragment, within the deadline, as the sentence a caller reads.
///
/// **On a tree without the bound this call NEVER RETURNS** for a fragment carrying the trigger -
/// `Parser::parse_data_type`'s argument loop scans for a `)` that is not in the token stream, and
/// `advance()` past the end does not move the cursor - so the assertion has to be about returning
/// at all. A bare `matches!` on the result would hang the suite rather than fail it, and a hung
/// suite is not a red.
///
/// A `String` and not the `Result`: the value crosses a thread boundary, and `Display` is what
/// `super::super::refusal` states a caller may read.
fn verdict_within_the_deadline(sql: &str) -> String {
    let owned = String::from(sql);
    let worker = thread::spawn(move || match portable(&owned) {
        Err(refusal) => refusal.to_string(),
        Ok(compiled) => format!("ACCEPTED: {:?}", compiled.renderings()),
    });
    let deadline = Instant::now() + MUST_RETURN_WITHIN;
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        worker.is_finished(),
        "the compile did not return within {MUST_RETURN_WITHIN:?} on {sql:?}: the pinned parser is looping, \
         which under `panic = \"abort\"` is the process never coming back"
    );
    worker.join().expect("the compile thread must not panic")
}

/// The sentence this bound produces, without the character it names.
const UNCLOSED: &str = "and never closes it";

/// The body of [`super::the_sql_fuzz_artifacts_replay_as_a_refusal_not_an_unbounded_parse`].
///
/// Here rather than there because the parent is ten lines from the line cap, and the `#[test]`
/// is there rather than here because it and the `mod` line have to be in one file for
/// `just causality` to compile this module against base at all.
///
/// Each artifact is delivered the way the harness delivers it. `from_utf8_lossy` is the identity
/// here - every byte is ASCII - and it is written that way so these are the artifacts rather than
/// transcriptions of them. Red before the bound by RUNNING OUT OF TIME, which is what
/// [`verdict_within_the_deadline`] exists to turn into a failure instead of a hang.
pub(super) fn the_recorded_artifacts_are_refused_at_the_parenthesis_they_opened() {
    for &(artifact, column) in RECORDED {
        let fragment = String::from_utf8_lossy(artifact).into_owned();
        let verdict = verdict_within_the_deadline(&fragment);
        assert!(
            verdict.contains(&format!("opens a parenthesis at character {column} {UNCLOSED}")),
            "the recorded fuzz artifact {fragment:?} must be refused at character {column}, got: {verdict}"
        );
    }
}

#[test]
fn the_bound_is_over_tokens_so_a_closer_inside_a_string_literal_closes_nothing() {
    // The first is the reduction of the recorded artifact and the whole trigger - `cargo fuzz tmin`
    // took it to five bytes, `a.:a(`, on the base tree.
    //
    // The second is the case that decides the bound is asked of the TOKENS, and it is TEXT-BALANCED:
    // one `(`, one `)`, so a count reads it as closed - while the `)` is a string literal the
    // tokenizer hands over as a single token and never as an `RParen`, leaving the parser a
    // parenthesis with no closer. That is `holds_comment_delimiter`'s fail-open one character class
    // over, and the reason this guard asks the tokenizer instead of counting. **The first spelling
    // of this cell used `SUM(mrr_eur.:S1(')'`, which a count WOULD have caught - two `(` against
    // one `)` - so it was red against a naive bound for the wrong reason and proved nothing about
    // the tokenizer.** Dropping the `SUM(` is the whole repair.
    //
    // The third has no `.:` at all and still does not return, which is what makes the axis the
    // parenthesis and not the construct - see `super::super::unclosed_parenthesis`.
    //
    // The fourth has no `.:` either and the parser DOES return on it: it is `Unparsable` without
    // the bound, and it is here because the bound must name the outermost open parenthesis rather
    // than the innermost.
    let bounded = [
        ("mrr_eur.:S1(", 12),
        ("mrr_eur.:S1(')'", 12),
        ("CAST(mrr_eur AS S1(9", 5),
        ("SUM(mrr_eur", 4),
    ];
    for (fragment, column) in bounded {
        let verdict = verdict_within_the_deadline(fragment);
        assert!(
            verdict.contains(&format!("opens a parenthesis at character {column} {UNCLOSED}")),
            "{fragment:?} must be refused at character {column}, got: {verdict}"
        );
    }

    // The same fragments with the closer present are NOT this refusal - each reaches the parse and
    // is judged for what it is. Asserted as an absence, so a guard that refused every `(`, or one
    // that refused the `.:` construct, would be red here rather than reading as a stricter version
    // of the same rule. The second is the measurement that decided against the construct axis: it
    // parses and returns on the base tree, so refusing `.:` would refuse a harmless fragment.
    for closed in ["mrr_eur.:S1()", "mrr_eur.:S1(9)"] {
        assert!(
            !verdict_within_the_deadline(closed).contains(UNCLOSED),
            "{closed:?} closes its parenthesis, so it is not an unclosed one"
        );
    }

    // The OTHER direction: the naive count also OVER-refuses, and this is the fragment that says
    // the cost of that is not paid. Text-unbalanced - two `(` against one `)`, because one `(` sits
    // inside a string literal - and token-balanced, so a count refuses an **ordinary conditional
    // sum** while the tokenizer accepts it. Asserted as ACCEPTED rather than as an absence, because
    // "not this refusal" would also hold for `Refused` or `Unparsable`, and the claim is that the
    // fragment compiles.
    //
    // The over-refusal direction is not entirely uncovered and the difference is the point.
    // `super::a_fragment_that_escapes_its_own_parentheses_is_refused_naming_a_position` catches a
    // count too - measured, on `SUM(mrr_eur))`, an extra CLOSER - but what it notices is one
    // refusal arriving instead of another, which a stricter-but-correct guard could also produce.
    // Nothing held a fragment a count refuses while it is a fragment a catalog would really write.
    //
    // It is the cheap half of the pair as well: a naive count fails this in microseconds, where the
    // under-refusal case above only fails by running out of the deadline. And it is the cell
    // `docs/adr/0004` now argues from - the guard has no accepted cost of the comment refusal's
    // kind, and `SUM(CASE WHEN status = '--' THEN mrr_eur END)` being refused as a comment (in the
    // parent module's own cells) is the contrast that makes the point.
    let accepted = verdict_within_the_deadline("SUM(CASE WHEN status = '(' THEN mrr_eur END)");
    assert!(
        accepted.starts_with("ACCEPTED"),
        "a parenthesis inside a string literal is balanced in the TOKENS and must compile, got: {accepted}"
    );
}
