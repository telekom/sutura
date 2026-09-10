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
//!
//! **Two bounds live here now, and they are different defects with the same cause.**
//! `UnclosedParenthesis` refuses a parse that does not RETURN; `UncalledIf` refuses one that
//! returns having done work no bound on the fragment can see. The second half's accepted-spelling
//! assertions are NOT red against base - base accepts them too - and are written down rather than
//! counted as coverage: what they are red against is a bound that refuses every `IF`, which is the
//! over-refusal a reader of this file would reach for first. They ride inside a `#[test]` whose
//! other assertions ARE red on base, so the gate's verdict on this module does not come from them.

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

/// The artifact the `sql_expression` target recorded for the SPECULATIVE-IF defect, quarantined
/// byte-identical as `fuzz/seeds/sql_expression/uncalled-if-oom` so `just fuzz-smoke` replays it.
///
/// **A `&str` rather than a byte literal, and every byte here is ASCII** - so this IS the artifact
/// and not a transcription of one, and `from_utf8_lossy` would be the identity on it.
///
/// **`include_bytes!` on the seed was tried first and `just causality` refuses it**, which is worth
/// knowing before someone reaches for it again: the gate reconstructs a base tree by reverting the
/// non-test files in the diff, an ADDED file is simply absent from that tree, and a test module that
/// cannot compile without it makes the gate report *the tests this diff added did not run on base*
/// rather than a red. So the bytes are duplicated on purpose, and the cost of that is the cost
/// [`RECORDED`] already pays: editing the seed without editing this leaves the cell green over the
/// copy. Nothing holds the two equal.
///
/// **One artifact and no smaller one, and `cargo fuzz tmin` is why.** It was reported as an
/// out-of-memory whose live heap was ~50 MB behind 4.3M quarantined chunks - churn from re-parsing,
/// not one large allocation - so replayed alone it never crosses the resident-set limit and the
/// minimisation answers "did not crash". The reduction is parametric instead: the doubling family in
/// the cell below is what isolates the trigger. **There is no parenthesis anywhere in these bytes**,
/// which is why the sibling bound cannot see it.
const RECORDED_UNCALLED_IF: &str = concat!(
    "L2[[[[ IF~ [$1>N$D* IF~ L2[[[L2[[[[[N[[[[NN[[[[NIF~IF = IF~IF~ [IF~I~S[L%SW<=IF~IF~S[L%SW<=NL4",
    "[%%L%SW<=%%L%EIF~IF =IF = IF~IF~ [IF~IFDISNL4[%%LIF~IF~S[L%SW<=NL4[%%L%SW<=%%L%EIF~IF =IF = IF",
    "~IF~ [IF~IFDISTINT[ IF~ [[[N!z*[$2>2[[[[ IF~ [$2>N|I[[[NN!!~~*le",
);

/// The character the recorded artifact's first keyword `IF` sits at, 1-based.
const UNCALLED_IF_COLUMN: usize = 8;

/// The sentence this bound produces, without the character it names.
const UNCALLED: &str = "as a keyword at character";

#[test]
fn a_keyword_if_is_refused_where_it_is_written_and_a_called_one_is_not() {
    // **FIRST, and the order is load-bearing for `just causality` rather than for a reader.** This
    // is the assertion that is red against base CLEANLY: base accepts this spelling and answers in
    // microseconds, so the cell fails here with a message naming the test. The two assertions below
    // it are red against base as well and neither is USABLE as the proof - the recorded artifact
    // kills the base process with `ABORT SIG 10` in 0.16 s under the unoptimized `ci` profile,
    // because `parse_primary` -> `parse_if` -> the whole precedence ladder recurses once per keyword
    // `IF` and exhausts the thread's stack, and the gate reads a process that died without naming a
    // test as INCONCLUSIVE rather than as a red. Measured: putting the artifact first reported
    // *the base run named no failure*, exit 3, proving nothing. Do not reorder these.
    //
    // It is also the COST on the record rather than a surprise: this spelling compiled before this
    // bound and rendered as `IF(..)` in all four dialects, and it does not compile now.
    let keyword_form = verdict_within_the_deadline("SUM(IF status = 'active' THEN mrr_eur ELSE 0 END)");
    assert!(
        keyword_form.contains(&format!("{UNCALLED} 5")),
        "the keyword spelling is the accepted cost of this bound and must be refused, got: {keyword_form}"
    );

    // The recorded artifact. On base this does not return a verdict at all - see above - so what it
    // proves is that the bound refuses the exact bytes the fuzzer recorded, at the character it
    // names, which is the half a seed replay cannot assert.
    let verdict = verdict_within_the_deadline(RECORDED_UNCALLED_IF);
    assert!(
        verdict.contains(&format!("{UNCALLED} {UNCALLED_IF_COLUMN}")),
        "the recorded fuzz artifact must be refused at character {UNCALLED_IF_COLUMN}, got: {verdict}"
    );

    // **The axis, and the reason the bound is on the keyword rather than on the artifact's shape.**
    // Each `IF~` doubles the parse: measured in the harness that found it, this family costs 1.2 s
    // at k=16, 22.5 s at k=20, 91.1 s at k=22 and 389.1 s at k=24, so 75 bytes crosses libFuzzer's
    // own 1200 s timeout two doublings later while `MAX_FRAGMENT_LEN` still allows 1024. It never
    // runs on base, because the assertion above it has already failed the cell there; what it holds
    // is that the bound is on the KEYWORD and so refuses the whole family rather than one recorded
    // shape. The first keyword `IF` is at character 1.
    let mut doubling = "IF~".repeat(24);
    doubling.push_str("I?{");
    let verdict = verdict_within_the_deadline(&doubling);
    assert!(
        verdict.contains(&format!("{UNCALLED} 1")),
        "a chain of keyword IFs must be refused at the first of them, got: {verdict}"
    );

    // What the bound must NOT touch, and the half that is not red on base - see the module comment.
    // `IF(..)` is a call, so the parser never takes the speculative branch for it; `CASE WHEN` is
    // the spelling every other conditional in this repository uses. A guard that refused the `IF`
    // TOKEN rather than an uncalled one would be red here, which is the over-refusal worth pinning.
    //
    // The four allowlisted names that CONTAIN the letters are here for the same reason, and this is
    // the half `docs/adr/0004` previously carried as a measurement rather than an assertion: none of
    // them tokenizes to an `If` token, so a guard that matched the TEXT `IF` would refuse four
    // ordinary aggregates. `NULLIF` is a fifth, inside a ratio, because the guard rides on the same
    // token walk the parenthesis one does and a ratio is the shape that walk exists for.
    for accepted in [
        "SUM(IF(status = 'active', mrr_eur, 0))",
        "SUM(CASE WHEN status = 'active' THEN mrr_eur END)",
        "COUNTIF(status = 'active')",
        "COUNT_IF(status = 'active')",
        "SUMIF(mrr_eur, status = 'active')",
        "SUM_IF(mrr_eur, status = 'active')",
        "SUM(mrr_eur) / NULLIF(COUNT(status), 0)",
    ] {
        let verdict = verdict_within_the_deadline(accepted);
        assert!(
            verdict.starts_with("ACCEPTED"),
            "{accepted:?} calls IF or does not write it, so this bound must not reach it, got: {verdict}"
        );
    }
}
