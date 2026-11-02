//! The declared pool's screening, asserted where a source composes it.

use super::{UnusablePool, WorkloadPool};

#[test]
fn a_declared_pool_value_that_could_reshape_the_document_is_refused_at_composition() {
    // The audience lands in a JSON document and a URL, so it is screened - and the screening is at
    // COMPOSITION, so an unusable declaration fails to start rather than failing every question.
    for hostile in ["has space", "has\"quote", "has{brace}", ""] {
        assert!(
            WorkloadPool::parse(hostile).is_err(),
            "`{hostile}` is not an audience this transport may send"
        );
    }
    assert!(matches!(WorkloadPool::parse(""), Err(UnusablePool::Empty)));
    // And the bound is a bound rather than a comment: one character past it is refused, and the
    // refusal carries the position and never the text.
    let over = "a".repeat(257);
    assert!(matches!(
        WorkloadPool::parse(&over),
        Err(UnusablePool::TooLong { found: 257, most: 256 })
    ));
}
