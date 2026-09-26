//! One claim, held against the VENUE TABLE rather than against a second sentence: *leg 2 is
//! proven*.
//!
//! **Why this rule is not a `CONTRADICTED` row.** That table is the right machinery for a claim the
//! TREE refutes - a path that exists, a literal a file holds. This claim is refuted by a STATE the
//! venue page records, and `Evidence` can only ask whether a file holds a needle. A needle in
//! `docs/where-identity-is-proven.md` would have been a second piece of prose standing in for the
//! cell, which is the shape `claims.rs`'s own header argues against. So the trigger is parsed:
//! [`crate::venues::leg_two_citable`] reads the claims matrix with the parser that gate already
//! owns, and this one contributes the prose walk `guidance::run` already performs.
//!
//! **Measured, which is why it exists at all.** On this tree four surfaces said leg 2 was proven
//! for `BigQuery` by a hosted run - `AGENTS.md`, `identity/SKILL.md` twice, and
//! `docs/implementation-plan.md` - describing an exchange and a workload-identity pool the ADBC
//! adoption deleted. `check-guidance` printed `ok` over all four. A planted bad path in a skill
//! file reddens the same gate on the same tree, so the gate was armed and this claim was simply
//! outside every rule it had.
//!
//! # What it holds, and the two limits beside it
//!
//! While no leg-2 row in the matrix carries a citable verdict, no page in scope may state one of
//! [`WORDINGS`]. When a row moves to `yes` or `can`, the claim becomes sayable and this rule stops
//! applying - the same self-retiring shape a `Contradicted` row has, derived from the cell instead
//! of from evidence files. **Those two words and not three:** this sentence used to name `only
//! here` as well, and `venues::page::CITABLE` is `["yes", "can"]` - its own doc says `only here`
//! and `redundant` are deliberately absent, because they say WHICH venue owns a claim rather than
//! that one has answered it. So the sentence described a wider retirement than the code performs,
//! which for a self-retiring rule is the direction that matters.
//!
//! **It matches a literal, so a paraphrase escapes.** That is the limit the whole `claims` module
//! records, and it is why [`WORDINGS`] carries every wording that was found rather than one: the
//! failure this class has here is a correction landing in one file and not being carried to its
//! siblings. A new wording is added the moment it is found.
//!
//! **And it reads a CELL, never a run.** `check-venues` states the same limit about its own
//! anchors: a `yes` this rule believes is a word somebody wrote, and whether the run behind it is
//! real stays review's. What is closed is the direction that failed silently - a page claiming
//! proof while the table records none.
//!
//! # The scope, which is the limit that matters most here
//!
//! `guidance::tree_problems` passes `text_files` plus every `.rs` file, so this rule reads `md`,
//! `nix`, `yml`, `yaml`, `toml` and `sh` as written, and `.rs` through
//! [`absences::prose`](super::absences::prose), which keeps `///` and `//!` doc comments only. A rule table written in Rust holds the very phrases it
//! forbids, which is why `guidance::in_scope` excludes `.rs` for everything else that judges a
//! sentence; here the WORDINGS are string literals, not doc comments, so the rule does not report
//! itself. A `//` line comment, a `/* */` block comment and a string literal in `.rs` are not read,
//! and that is the limit: an overstatement there rests on review. Five of the sites this rule's own wordings were harvested from were doc comments
//! (`sutura-exec-bigquery`'s `principal.rs` and `transport.rs`, `sutura-cli`'s `serve/broker.rs`,
//! `sutura-config`'s `sources/workload_identity.rs` and `raw.rs`), and widening to doc comments
//! holds them where it held nothing before. Widening `contradicted_claims` to `files` was priced
//! and declined for `in_scope`'s reason; what is added instead is a wording per sentence found,
//! which at least holds the SIBLINGS of a corrected page.
//!
//! It reaches no `.py` and no commit message either - nothing in `check-guidance` reads git
//! history.

use std::path::Path;

use super::absences::prose;
use super::claims::flatten;

/// Every wording of *leg 2 is proven* that has been found in this repository.
///
/// Whitespace-collapsed, matched against [`flatten`]'s view, for the reason that view exists:
/// prose wraps and a claim does not. Each of the first four was live and false on the tree that
/// this rule was written against.
const WORDINGS: &[&str] = &[
    "Leg 2 (a source executing AS them) is proven",
    "Leg 2 - a source executing AS the asker - is proven",
    "leg 2 is proven here for BigQuery only",
    "Proven live for one source, hosted",
    "a hosted run whose job holds both principals' keys",
    "a hosted run whose job held both principals' keys",
    // Round 8 of telekom/sutura#929, all three off `.agents/skills/sutura/identity/SKILL.md` - the
    // file that routes other agents, offering a run of the deleted `wire` transport as current
    // evidence and then retracting it fifteen lines later. A heading is a needle like any other.
    "An exchange HAS run against real STS and `iamcredentials`",
    "What proved leg 2 for BigQuery, hosted",
    "a hosted `workflow_dispatch` run of it concluded `success`, each principal resolved to its own account",
];

/// Pages that may state a wording in order to correct it.
///
/// The venue page is the authority on the state and has to be able to name what it is not; the
/// record that withdrew the claim has to be able to quote it. Everything else in scope is held.
const EXCEPT: &[&str] = &[
    "docs/where-identity-is-proven.md",
    "docs/adr/0018-what-the-bigquery-wire-is-built-from.md",
];

/// Everything wrong with how this tree states leg 2's state.
///
/// `files` is the prose scope plus every `.rs` file, read through a text-backed `read` closure
/// exactly as [`super::claims::contradicted_claims`] reads it, and each one through [`prose`] - a
/// `.rs` file's doc comments, anything else as written - so an unreadable page reports itself once,
/// through the one reader every check here shares.
pub(super) fn problems(root: &Path, read: &crate::causality::regions::PostImage<'_>, files: &[String]) -> Vec<String> {
    let Some(citable) = crate::venues::leg_two_citable(root) else {
        return vec![String::from(
            "the leg-2 rows of `docs/where-identity-is-proven.md`'s claims matrix could not be \
             read, so *is leg 2 proven* has no answer to hold a page to. That is this rule's own \
             input rather than a page's mistake - fix the matrix, or the anchors in \
             `xtask/src/venues/leg_two_row.rs`",
        )];
    };
    if citable {
        return Vec::new();
    }
    let mut problems = Vec::new();
    for path in files {
        if crate::repo::matches_any(EXCEPT, path) {
            continue;
        }
        let Some(text) = read(path) else {
            continue;
        };
        problems.extend(stated_in(path, &prose(path, &text)));
    }
    problems
}

/// Every wording one page states, with the line each is on.
///
/// Split from [`problems`] so the matching is assertable without a tree: the IO half above reads
/// the scope and the condition, and this half is the whole of what a page is held to.
fn stated_in(path: &str, text: &str) -> Vec<String> {
    let (flat, lines) = flatten(text);
    WORDINGS
        .iter()
        .filter_map(|wording| flat.find(wording).map(|at| (wording, at)))
        .map(|(wording, at)| {
            let line = lines.get(at).copied().unwrap_or(0);
            format!(
                "{path}:{line} states `{wording}`, and no leg-2 row in \
                 `docs/where-identity-is-proven.md`'s claims matrix carries a citable verdict - \
                 every one of them is `no`, `-`, `unrun` or `wired`, and the page's own header says \
                 neither of the last two counts. What is true instead: the mechanism is built and \
                 the venue is `wired`, so nobody has run it. Say that, or dispatch the venue and \
                 move its cell"
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{EXCEPT, WORDINGS, stated_in};

    #[test]
    fn a_page_claiming_leg_two_is_proven_is_named_with_its_line() {
        // The four surfaces this rule was written against said it in four different wordings, which
        // is why the table has more than one - and why a fixture here uses the WRAPPED form: prose
        // wraps and a claim does not, so the flattened view is the only one that finds it.
        let page = "Some prose first.\n\nLeg 1 is built. Leg 2 (a source executing AS\nthem) is proven for BigQuery.\n";
        let found = stated_in("AGENTS.md", page);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("AGENTS.md:3 states"), "{found:?}");
        assert!(found[0].contains("no leg-2 row"), "{found:?}");
    }

    #[test]
    fn a_page_that_states_no_wording_is_not_named() {
        // The corrected sentence, and it has to pass: a rule that reddened the fix as well as the
        // defect is one nobody can satisfy. This is the wording all six surfaces now carry.
        let corrected = "Leg 2 - a source executing AS the asker - is built and unproven: the hosted venue is `wired`.\n";
        assert_eq!(stated_in("AGENTS.md", corrected), Vec::<String>::new());
    }

    #[test]
    fn every_wording_can_survive_the_view_this_matcher_reads() {
        // **Two of the three ways a registered wording goes quiet, and the third named as unheld.**
        // The previous version of this cell claimed all three and held one: it built the page FROM
        // the wording, so a needle always matched itself unless `flatten` changed it. Review
        // measured exactly that - a curly-apostrophe wording passed, a `\n` one failed - so the
        // comment was wider than the check. Corrected rather than deleted, because an overstated
        // self-check is worse than an honest narrow one.
        //
        // HELD, mode 1 - WHITESPACE the flattened view collapses. `flatten` reduces every run of
        // whitespace to one space, so a needle carrying a newline, a tab or a double space can
        // never appear in it. The round trip below is what catches that.
        for wording in WORDINGS {
            assert_eq!(
                stated_in("docs/somewhere.md", &format!("Prose. {wording} More prose.\n")).len(),
                1,
                "this rule cannot match its own registered wording: {wording}"
            );
        }
        // HELD, mode 2 - a CHARACTER a page's prose does not carry. The corrected sentences and the
        // false ones are both ASCII; a wording typed with a curly apostrophe or a non-breaking
        // space is a row that can match nothing, and the round trip above cannot see it because the
        // fixture page is built from the wording itself. This reads the wording alone.
        for wording in WORDINGS {
            assert!(
                wording.is_ascii(),
                "a registered wording carries a non-ASCII character, so no page written in ASCII \
                 prose can match it: {wording:?}"
            );
            assert!(
                !wording.contains("  ") && wording.trim() == *wording,
                "a registered wording carries whitespace the flattened view cannot hold: {wording:?}"
            );
        }
        // NOT HELD, mode 3 - a TYPO. A misspelled wording is a well-formed needle for a sentence
        // nobody wrote, and nothing here has an independent copy of the true sentence to compare it
        // against, so there is no oracle for it. What stands in its place is that a wording is
        // added the moment it is FOUND in a file - the table is a record of matches that happened,
        // not of sentences somebody expected.
    }

    #[test]
    fn the_wordings_round_eight_harvested_are_refused_where_they_were_found() {
        // **RED against base, and that is the point of registering a wording at all.** Each of
        // these three stood in `.agents/skills/sutura/identity/SKILL.md` while `check-guidance`
        // printed `ok`, because the table held six other sentences and not these. A skill file
        // routes other agents, so a false sentence there propagates; this cell is what keeps the
        // three from coming back once the page is corrected.
        //
        // The LIMIT, because this cell proves less than it looks like it does: it holds the
        // literals, in prose files. The same three claims in a `//` line comment or a block comment
        // are still refused by nothing - see this module's own header - and a paraphrase of any of
        // them escapes. A `///` or `//!` doc comment IS refused, which
        // `a_doc_comment_overstatement_in_rust_is_refused_but_a_string_literal_is_not` holds.
        for wording in [
            "An exchange HAS run against real STS and `iamcredentials`",
            "What proved leg 2 for BigQuery, hosted",
            "a hosted `workflow_dispatch` run of it concluded `success`, each principal resolved to its own account",
        ] {
            let page = format!("Prose before.\n\n{wording} - and prose after.\n");
            let found = stated_in(".agents/skills/sutura/identity/SKILL.md", &page);
            assert_eq!(found.len(), 1, "an unregistered wording: {wording}\n{found:?}");
            assert!(found[0].contains("no leg-2 row"), "{found:?}");
        }
    }

    #[test]
    fn a_doc_comment_overstatement_in_rust_is_refused_but_a_string_literal_is_not() {
        // **RED against base by assertion, not by missing-fn.** Before the scope was widened,
        // `problems` read a `.rs` file as raw text, so a wording in a `///` doc comment and one
        // in a string literal were both found - two problems. After the widening, `problems`
        // routes `.rs` through `absences::prose`, which reads `///` and `//!` only and blanks a
        // string literal to an empty line - one problem, the doc comment. This cell drives the
        // rule's public entry (`problems`) against a fixture tree, so the assertion is what
        // fails on base, not a missing `use super::prose`.
        let root = std::env::temp_dir().join(format!("sutura-leg-two-doc-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(root.join("docs")).expect("fixture root");
        // A venue page whose two leg-2 rows carry no citable verdict, which is the condition.
        std::fs::write(
            root.join("docs/where-identity-is-proven.md"),
            "| Claim | Fake at the port |\n| --- | --- |\n\
             | Whether two distinct subjects resolve to two distinct principals | **wired** |\n\
             | A served binary executes as a verified human caller through the declared per-source map | - |\n",
        )
        .expect("fixture venue page");
        // A `.rs` file with one registered wording in a `///` doc comment and a different one
        // in a string literal. On base both are found (raw text); after the widening only the
        // doc comment survives `prose`.
        let rel = "crates/sutura-exec-bigquery/src/principal.rs";
        std::fs::create_dir_all(root.join("crates/sutura-exec-bigquery/src")).expect("fixture crate dir");
        std::fs::write(
            root.join(rel),
            concat!(
                "//! Module doc.\n",
                "/// Leg 2 (a source executing AS them) is proven for BigQuery.\n",
                "fn f() -> &'static str {\n",
                "    \"leg 2 is proven here for BigQuery only\"\n",
                "}\n",
            ),
        )
        .expect("fixture claimant");
        let files = vec![String::from(rel)];
        let read = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
        let found = super::problems(&root, &read, &files);
        drop(std::fs::remove_dir_all(&root));
        // One problem: the `///` doc comment. The string literal is blanked by `prose`, so on
        // base this is 2 and the assertion fails by its own count.
        assert_eq!(
            found.len(),
            1,
            "expected one finding (the doc comment), the string literal must be blanked: {found:?}"
        );
        assert!(
            found[0].contains("no leg-2 row"),
            "the doc comment overstatement was not reported: {found:?}"
        );
        assert!(
            found[0].contains("Leg 2 (a source executing AS them) is proven"),
            "the reported finding should name the doc comment wording, not the string literal: {found:?}"
        );
    }

    #[test]
    fn the_exempt_pages_are_the_two_that_have_to_quote_the_claim() {
        // The exemption is a list rather than a predicate, so it is worth asserting it is the list
        // it is meant to be: the venue page decides the state and the record dates the withdrawal.
        // A third entry here is how this rule would be turned off one page at a time.
        assert_eq!(EXCEPT.len(), 2);
        assert!(EXCEPT.contains(&"docs/where-identity-is-proven.md"));
    }
}
