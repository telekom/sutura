//! `docs/adr/0013`'s raw tool, and how the prompt talks about it - or does not.
//!
//! A submodule of [`super`] for the mechanical reason the other splits there state:
//! `cargo xtask max-lines` fails at a thousand lines under `crates/` and the parent file plus this
//! case is over it. Everything here is one property: the rendered document names `run_sql` only
//! where a deployment turned it on, and where it does, the framing is `docs/adr/0022`'s - ungoverned,
//! the deployment's own role, no provenance of the certified kind - and never claims the word
//! "certified" for the raw tool itself.
//!
//! Framing assertions read [`Tool::RunSql::summary`](Tool::summary) directly rather than the
//! rendered document: [`super::wrap`] re-flows every summary onto lines at most [`super::WIDTH`]
//! columns wide, so a multi-word phrase asserted against the WRAPPED text could straddle a line
//! break by accident of length and fail for a reason that has nothing to do with the framing. The
//! unwrapped string is what the framing actually is; presence-in-the-document is its own, separate
//! assertion below, over short substrings a wrap point cannot land inside.

use super::{CatalogProse, Tool, rendered};

#[test]
fn run_sql_is_absent_from_the_operations_list_unless_the_tool_list_carries_it() {
    let without = rendered(Tool::ALL, CatalogProse::Quoted, None);
    assert!(
        !without.contains("run_sql"),
        "a deployment that never turned run_sql on must not name it anywhere:\n{without}"
    );
    assert!(
        !without.contains("stated exception"),
        "the no-such-field caveat must not appear unless run_sql is exposed:\n{without}"
    );

    let with = rendered(&[Tool::Catalog, Tool::Query, Tool::RunSql], CatalogProse::Quoted, None);
    assert!(
        with.contains("run_sql"),
        "run_sql must be named in the document once a deployment turns it on:\n{with}"
    );
}

#[test]
fn the_run_sql_summary_names_the_ungoverned_boundary_and_never_calls_it_certified() {
    let summary = Tool::RunSql.summary();
    // `docs/adr/0022`'s framing, in an agent's own words: ungoverned, the deployment's own role
    // rather than the caller's, and no provenance of the kind a certified answer carries.
    assert!(summary.contains("ungoverned"), "the ungoverned framing is missing: {summary}");
    assert!(
        summary.contains("DEPLOYMENT's own role"),
        "the identity boundary is missing: {summary}"
    );
    assert!(
        summary.contains("no definition version, no digest and no provenance"),
        "the no-provenance framing is missing: {summary}"
    );
    // The word "certified" appears elsewhere in this document (the fixed preamble, the certified
    // `query` summary) - what must never happen is the raw tool's OWN summary claiming it. Asserted
    // against the summary in isolation rather than the whole document, so this is not a
    // whole-document ban the fixed sections would fail.
    assert!(
        !summary.contains("certified"),
        "the raw tool's own summary must never use the word \"certified\": {summary}"
    );
}

#[test]
fn a_deployment_that_turns_run_sql_on_does_not_contradict_its_own_fixed_sections() {
    // Before this module existed, three fixed sections stated claims that are only true of
    // `query`, unconditionally: "What this surface has no field for" said "do not ask a human to
    // enable it for you", the opening frame said sutura itself "does not take SQL", and the closing
    // "Provenance" section said "Every answer carries the version and the digest". A document that
    // states any of those AND separately advertises `run_sql` under "The operations you have" is
    // internally inconsistent, which is worse than the fixed sentence alone - review's own finding:
    // the PR that added `no_such_field` fixed one of the three and left the other two standing.
    let text = rendered(&[Tool::Catalog, Tool::Query, Tool::RunSql], CatalogProse::Quoted, None);
    // The constant's own literal breaks the line here (`to\nenable`), not with a space - checked
    // as two substrings rather than one so this assertion does not depend on that line break.
    assert!(
        text.contains("do not ask a human to") && text.contains("enable it for you"),
        "the fixed sentence about query's own fields must stay - it is still true of query:\n{text}"
    );
    assert!(
        text.contains("stated exception"),
        "the no-such-field caveat pointing at the exception is missing:\n{text}"
    );
    assert!(
        text.contains("That is `query`, this deployment's certified operation."),
        "the opening frame's own scoping caveat is missing:\n{text}"
    );
    assert!(
        text.contains("A `run_sql` result carries none of this"),
        "the provenance section's own scoping caveat is missing:\n{text}"
    );

    // And the other direction: a deployment that never turned run_sql on gets none of the three
    // caveats, because the fixed sentences are already true without qualification there.
    let without = rendered(Tool::ALL, CatalogProse::Quoted, None);
    assert!(
        !without.contains("That is `query`, this deployment's certified operation."),
        "the opening frame must stay unqualified when run_sql is absent:\n{without}"
    );
    assert!(
        !without.contains("A `run_sql` result carries none of this"),
        "the provenance section must stay unqualified when run_sql is absent:\n{without}"
    );
}
