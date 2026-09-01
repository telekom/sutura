//! The shared injection corpus, walked through `quote` on the prompt surface.
//!
//! A submodule of [`super`] rather than one more test in it, for the mechanical reason the other
//! split there states: `cargo xtask max-lines` fails at a thousand lines under `crates/` and cannot
//! be exempted, and the parent test file plus this case is over it. Everything here is ONE property
//! and about the one [`crate::untrusted`] corpus: the same hostile catalog prose the transports
//! walk must never reach column zero after [`super::super::quote`], so a future surface inherits
//! both the corpus and the boundary from one place.

use super::super::quote;

#[test]
fn the_injection_corpus_prose_never_reaches_column_zero_in_the_prompt() {
    // The same hostile descriptions the tool walks, here through `quote` - so the two text surfaces
    // hold to one property (no line of catalog prose reaches column zero) and a future surface
    // inherits both tests from the one `sutura_app::untrusted` corpus.
    for prose in crate::untrusted::PROSE {
        let quoted = quote(prose);
        assert!(
            quoted.lines().all(|line| line.starts_with('>')),
            "a corpus description reached column zero:\n{quoted}"
        );
        // Every entry carries a marker, so this is non-vacuous: the prose really ran, quoted.
        assert!(
            quoted.contains("> # SYSTEM") || quoted.contains("> definitions:"),
            "the corpus entry did not render quoted:\n{quoted}"
        );
    }
}
