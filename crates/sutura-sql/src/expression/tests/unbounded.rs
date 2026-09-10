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
//! cells build against base and fail there: two of them by running out of the deadline, because the
//! parse loops, and the third by naming a different refusal. Nothing but this bound produces the
//! words asserted below, and the compile is what holds it: `check` returns that variant or one of
//! the others, and there is no third outcome.

use std::thread;
use std::time::{Duration, Instant};

use super::portable;

/// The exact bytes the `sql_expression` fuzz target recorded as
/// `timeout-472fb665086f6dab29dfbd1510afbfea172e94ee`, quarantined as
/// `fuzz/seeds/sql_expression/unclosed-paren-timeout`.
///
/// **Every byte is ASCII**, which is why the `NonAscii` bound cannot see it: that one closes a
/// generator panic on multi-byte text, and this input never reaches the generator. Two characters
/// carry it - the `.:` that reads the next word as a custom data type, and the `(` at character 20
/// that is never closed.
const RECORDED_TIMEOUT: &[u8] = b"$a^-a61.c.:S1a.:#S1(^cAUAU";

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

/// The body of [`super::the_sql_fuzz_timeout_replays_as_a_refusal_not_an_unbounded_parse`].
///
/// Here rather than there because the parent is three lines from the line cap, and the `#[test]`
/// is there rather than here because it and the `mod` line have to be in one file for
/// `just causality` to compile this module against base at all.
///
/// The artifact is delivered the way the harness delivers it. `from_utf8_lossy` is the identity
/// here - every byte is ASCII - and it is written that way so this is the artifact rather than a
/// transcription of it. Red before the bound by RUNNING OUT OF TIME, which is what
/// [`verdict_within_the_deadline`] exists to turn into a failure instead of a hang.
pub(super) fn the_recorded_timeout_is_refused_at_the_parenthesis_it_opened() {
    let fragment = String::from_utf8_lossy(RECORDED_TIMEOUT).into_owned();
    let verdict = verdict_within_the_deadline(&fragment);
    assert!(
        verdict.contains(&format!("opens a parenthesis at character 20 {UNCLOSED}")),
        "the recorded fuzz timeout must be refused at its own unclosed parenthesis, got: {verdict}"
    );
}

#[test]
fn the_bound_is_over_tokens_so_a_closer_inside_a_string_literal_closes_nothing() {
    // The first is the reduction of the recorded artifact and the whole trigger - `cargo fuzz tmin`
    // took it to five bytes, `a.:a(`, on the base tree. The second is the case that decides the
    // bound is asked of the TOKENS: its two characters balance, one `(` and one `)`, so a count
    // reads it as closed, while the `)` is a string literal the tokenizer hands over as a single
    // token and never as an `RParen`. That is `holds_comment_delimiter`'s fail-open one character
    // class over, and the reason this guard asks the tokenizer instead of counting. The third has
    // no `.:`, so the parser DOES return on it - it is `Unparsable` without the bound - and it is
    // here because the bound must name the outermost open parenthesis rather than the innermost.
    for (fragment, column) in [("mrr_eur.:S1(", 12), ("SUM(mrr_eur.:S1(')'", 4), ("SUM(mrr_eur", 4)] {
        let verdict = verdict_within_the_deadline(fragment);
        assert!(
            verdict.contains(&format!("opens a parenthesis at character {column} {UNCLOSED}")),
            "{fragment:?} must be refused at character {column}, got: {verdict}"
        );
    }

    // The same fragment with the closer present is NOT this refusal - it reaches the parse and is
    // judged for what it is. Asserted as an absence, so a guard that refused every `(` would be red
    // here rather than reading as a stricter version of the same rule.
    assert!(
        !verdict_within_the_deadline("mrr_eur.:S1()").contains(UNCLOSED),
        "a closed parenthesis is not an unclosed one"
    );
}
