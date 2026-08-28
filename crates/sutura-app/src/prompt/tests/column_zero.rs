//! The one property that needed a whole hostile bundle: catalog text and column zero.
//!
//! A submodule of [`super`] rather than one more test in it, and the reason is mechanical:
//! `cargo xtask max-lines` fails at a thousand lines under `crates/` and cannot be exempted, and
//! that file plus this case is over it. The seam is a real one - everything here is about ONE
//! rendering decision - the `indent` argument of [`super::super::wrap`] - and about the fixture
//! [`super::POSITIONED_VALUE`] that exists to attack it.

use super::super::{WIDTH, wrap};
use super::{POSITIONED_VALUE, everything};

#[test]
fn a_declared_value_cannot_put_a_word_of_its_own_at_column_zero() {
    // THE case a review named, asserted in both directions.
    //
    // `quote` promises that no sequence a catalog author writes reaches the output at column zero,
    // and it keeps that promise by prefixing every line itself. `wrap` keeps the same promise only
    // when its `indent` is non-empty, and `knowledge::caveats_about` was the one call in this crate
    // that passed author-controlled text with an empty one. So the first half of this test is that
    // the finding was REAL - the old call shape does put the author's word at column zero - and the
    // second is that the document no longer does.
    let line = format!("**Caveat `value_positioned_at_zero`** - about `{POSITIONED_VALUE}` of its `region`:");
    assert!(
        line.chars().count() > WIDTH,
        "the fixture has to reach the wrap boundary, and it is {} characters",
        line.chars().count()
    );
    // Asserted as "some word OF THE VALUE begins a line" rather than against one hard-coded token,
    // so it stays a statement about the mechanism if the wording of the line around it ever moves.
    let words: Vec<&str> = POSITIONED_VALUE.split(' ').collect();
    let at_column_zero = wrap("", &line, "");
    assert!(
        at_column_zero
            .lines()
            .skip(1)
            .any(|broken| words.contains(&broken.split(' ').next().unwrap_or_default())),
        "the finding was real: with an empty indent the author owns column zero\n{at_column_zero}"
    );

    // And the rendered document, which is what an agent reads.
    let text = everything();
    // Every word of it, rather than the whole string: `wrap` re-flows on whitespace, so a value long
    // enough to cross the boundary is rendered across two lines. Nothing is dropped and nothing is
    // altered - which is the rule this repository holds to, and the reason the guard is a parse and
    // not a filter at render.
    for word in &words {
        assert!(text.contains(word), "the value is rendered, not censored: {word}");
    }
    // Every line of the document, not only the caveat's, and against the two tokens of the value
    // that are distinctive enough for `starts_with` to be a real assertion: `##` would be a heading
    // and `zzsystem` is a word nothing else in this document writes. The value reaches two
    // renderings - the dimension list and this scope line - and this covers both.
    for rendered_line in text.lines() {
        for forged in ["## zzsystem", "zzsystem"] {
            assert!(
                !rendered_line.starts_with(forged),
                "a declared value reached column zero: {rendered_line}"
            );
        }
    }
    // The positive form of the same fact, and the structural one: whatever word the wrap boundary
    // lands on, our own two spaces are in front of it.
    assert!(
        text.contains(&wrap("", &line, "  ")),
        "the document carries the indented form"
    );
    let scope = text
        .find("**Caveat `value_positioned_at_zero`**")
        .expect("the caveat about the value is rendered");
    let broken = text
        .get(scope..)
        .and_then(|rest| rest.split_once('\n'))
        .map(|(_, rest)| rest)
        .expect("the scope line wrapped");
    assert!(broken.starts_with("  "), "the continuation is ours: {broken:?}");
}
