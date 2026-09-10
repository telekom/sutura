//! The fragment the pinned parser did not return from, and the bound that refuses it.
//!
//! A submodule for the same mechanical reason as [`super::dialect_resolution`]: `cargo xtask
//! max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and the parent is
//! within twenty of it.
//!
//! **The two cells here divide one claim on purpose.** The first asserts only that the compile
//! RETURNS, and names no new refusal - so it builds against a tree that has no bound and fails
//! there by running out of time rather than by failing to compile, which is the difference between
//! a test that proves the behaviour and a test that proves the enum changed. The second pins which
//! refusal it is and where, and that one does name the variant.

use std::thread;
use std::time::{Duration, Instant};

use super::super::refusal::ExpressionError;
use super::portable;

/// The exact bytes the `sql_expression` fuzz target recorded as
/// `timeout-472fb665086f6dab29dfbd1510afbfea172e94ee`.
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
/// parser loops forever and no deadline would be long enough. Ten seconds keeps the red run short.
const MUST_RETURN_WITHIN: Duration = Duration::from_secs(10);

#[test]
fn the_recorded_fuzz_timeout_returns_from_the_compile_at_all() {
    // On a tree without the bound this call NEVER RETURNS - `Parser::parse_data_type`'s argument
    // loop scans for a `)` that is not in the token stream, and `advance()` past the end does not
    // move the cursor - so the assertion has to be about returning. A bare `matches!` on the result
    // would hang the suite rather than fail it, and a hung suite is not a red.
    //
    // Delivered through `String::from_utf8_lossy` the way the harness delivers it. That is the
    // identity here, since every byte is ASCII, and it is written this way so the input is the
    // artifact rather than a transcription of it.
    let fragment = String::from_utf8_lossy(RECORDED_TIMEOUT).into_owned();
    let worker = thread::spawn(move || portable(&fragment).is_err());
    let deadline = Instant::now() + MUST_RETURN_WITHIN;
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        worker.is_finished(),
        "the compile did not return within {MUST_RETURN_WITHIN:?} on the recorded fuzz input: the pinned \
         parser is looping, which under `panic = \"abort\"` is the process never coming back"
    );
    assert!(
        worker.join().expect("the compile thread must not panic"),
        "the recorded fuzz input must be refused, not accepted as a measure"
    );
}

/// The column a fragment's unclosed parenthesis was reported at, or a panic naming what happened
/// instead. The shape [`super::refusal`] uses, for the same reason: an assertion that only says
/// *not that* sends the next reader to run it themselves.
fn unclosed(sql: &str) -> usize {
    match portable(sql) {
        Err(ExpressionError::UnclosedParenthesis { column, .. }) => column,
        Err(other) => panic!("{sql:?} was refused, but not for an unclosed parenthesis: {other}"),
        Ok(compiled) => panic!("{sql:?} was ACCEPTED: {:?}", compiled.renderings()),
    }
}

#[test]
fn a_parenthesis_with_no_closer_is_refused_where_it_was_opened() {
    // The reduction of the input above, and the whole trigger: `.:` reads `S1` as a custom data
    // type and the `(` has no closer. The column is the author's own and 1-based.
    assert_eq!(unclosed("mrr_eur.:S1("), 12);

    // The same fragment with the closer present is NOT this refusal - it reaches the parse and is
    // judged for what it is. Asserted as an absence, so a guard that refused every `(` would be red
    // here rather than reading as a stricter version of the same rule.
    assert!(
        !matches!(portable("mrr_eur.:S1()"), Err(ExpressionError::UnclosedParenthesis { .. })),
        "a closed parenthesis is not an unclosed one"
    );

    // The case that decides the bound is over TOKENS and not over the text: the two characters
    // balance, one `(` and one `)`, so a count reads this as closed - while the `)` is a string
    // literal the tokenizer hands over as a single token and never as an `RParen`, leaving the
    // parser a parenthesis with no closer. That is `holds_comment_delimiter`'s fail-open one
    // character class over, and the reason this guard asks the tokenizer instead of counting.
    assert_eq!(unclosed("SUM(mrr_eur.:S1(')'"), 4);

    // The outermost unclosed parenthesis is the one reported, not the innermost, so an author is
    // pointed at the one whose closer is missing rather than at the deepest nesting.
    assert_eq!(unclosed("SUM(mrr_eur"), 4);
}
