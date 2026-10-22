//! A page under `docs/api/` selects no gate: nothing commits one, so it is a local preview.

use super::classify;

#[test]
fn a_locally_generated_page_is_prose_and_starts_no_gate() {
    // Nothing commits a page, so a path under `docs/api/` is the ignored output of `just api`.
    // It must not select the nightly gate: it cannot reach a diff, and the gate reads sources.
    let r = classify(&[String::from("docs/api/sutura-domain.md")]);
    assert!(!r.needs("api"), "{:?}", r.reasons);
    assert!(!r.docs_only.is_empty(), "it is docs-only like every page under docs/");
}
