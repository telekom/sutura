//! Round 2 of #971's review: `catalog_knowledge`'s own declaration claims positions ("listed
//! below", "at the end of this document") that were only true of `render`'s document, not the
//! tool's - the tool used to order glossary, `not_defined`, declaration, examples, with caveats
//! appended last and no heading of their own, so the glossary rendered ABOVE the declaration that
//! called it "below", and worked questions were not "at the end" once caveats followed them.
//!
//! Its own file for the reason every other case in `prompt/tests/` is: `prompt/tests.rs` is at the
//! thousand-line limit `cargo xtask max-lines` enforces and cannot be exempted.

use sutura_domain::knowledge::KnowledgeCapabilities;
use sutura_domain::pinned::view::ScopedView;

use super::bundle_with;
use crate::prompt::{CatalogProse, catalog_knowledge};

/// This cell pins the order `catalog_knowledge` actually uses - declaration, glossary,
/// `not_defined`, caveats (own heading), examples last - so the two cannot drift apart again
/// without failing here.
#[test]
fn the_tool_reply_orders_its_sections_so_the_declarations_position_claims_hold() {
    let pinned = bundle_with(KnowledgeCapabilities::all());
    let view = ScopedView::everything(&pinned);
    let text = catalog_knowledge(&view, CatalogProse::Quoted);

    let declaration = text
        .find("## What this deployment records about its own definitions")
        .expect("the declaration section is present");
    let glossary = text
        .find("## The words a question may arrive in")
        .expect("the glossary section is present");
    let not_defined = text
        .find("## Terms this deployment records as NOT defined")
        .expect("the not-defined section is present");
    let caveats = text.find("## Caveats, by metric").expect("caveats have their own heading");
    let examples = text
        .find("## Worked questions")
        .expect("the worked-questions section is present");

    assert!(declaration < glossary, "declaration says the glossary is listed below it");
    assert!(declaration < not_defined, "declaration is above the not-defined section");
    assert!(declaration < caveats, "declaration is above the caveats section");
    assert!(
        examples > glossary && examples > not_defined && examples > caveats,
        "declaration says worked questions are at the end of this document, so nothing else may follow them"
    );
    // The caveat text itself must not read as a worked question or as a continuation of whatever
    // section precedes it - the round-2 finding's own failure mode, fixed by giving it a heading.
    assert!(
        text.contains("## Caveats, by metric\n\n### revenue"),
        "a caveat renders under its own heading, not appended under \"## Worked questions\": {text}"
    );
    // The tool never renders a refusal-reason table or a bounds section, so its own wording must
    // not point an agent at either.
    assert!(
        !text.contains("after the refusal section"),
        "the tool's declaration must not point at a refusal section it never renders: {text}"
    );
    assert!(
        !text.contains("MetricUnknown` above"),
        "the tool's not-defined section must not point at a refusal guide it never renders: {text}"
    );
    assert!(
        !text.contains("the bounds above"),
        "the tool's examples section must not point at a bounds section it never renders: {text}"
    );
}
