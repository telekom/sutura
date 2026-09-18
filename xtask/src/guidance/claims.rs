//! The checks that judge a CLAIM rather than a line: `claims` here, `counts` beside it.
//!
//! Split out of `guidance.rs` mechanically - see the `mod claims` comment there - and nothing
//! moved here was changed on the way. What makes the seam a real one rather than a line count:
//! the checks that stayed judge one LINE at a time, and these cannot. Prose wraps, so a claim
//! is found in a flattened view of a file and a count is derived by walking the tree. [`flatten`]
//! is that view, and it is the only thing the two share.
//!
//! `claims` and `counts` exist because of one review, and because of one CAUSE rather than
//! nineteen mistakes: nineteen false sentences across `README.md`, `docs/` and `AGENTS.md`, four of
//! them saying there is no HTTP surface while two crates and a published page ship one, and in
//! almost every case the corrected sentence already existed in a sibling document. The correction
//! had landed in one file and had not been carried to the others. So what is checked here is the
//! CLAIM rather than the file: one [`Contradicted`] entry carries every wording of one claim, which
//! is what makes a sibling that was missed a failure rather than a survivor.
//!
//! **What these two cannot do, stated before a reader trusts them.** They match a literal, so a
//! paraphrase escapes - the same limit `AGENTS.md` records for the leak guard. They are a ratchet
//! on a sentence somebody has already written once, not a reader.

use std::path::Path;

// All three split off under the 1000-line cap, on the seam the causality gate forces: the
// mechanism moves and every assertion stays in `tests` below, in the file that DECLARES the module
// - a file adding no `#[test]` is one that gate may revert, which would take the declaration with
// it and orphan the tests. `remedies` judges the sentence a claim hands a reader; `counts` is the
// whole second check named in the header above.
//
// **`contradicted` went LAST, and the note it replaces is why it had to.** That note said `counts`
// moved rather than the table because [`CONTRADICTED`] is what grows by ENTRY - one claim is about
// twenty lines - "so this is the file that has to have room". The room it bought was spent, and it
// ran out on the entry registering a stale hook comment: a reserve measured in one file's spare
// lines is a reserve that expires. The table is data and reads nothing, so it is the half that can
// leave without taking a check with it, and what is left here grows by MECHANISM instead. The
// entry about the discovery document this branch carries joins it there on the same seam, and the
// merge of the two brought both - a reserve measured in one file's spare lines expires, whichever
// entry spends it.
//
// **Each is a CHILD rather than a sibling of `claims`, and that is deliberate.** `guidance.rs`
// lists the checks as peers, which argues for a sibling; against that, `flatten` below is the one
// thing they share, and a child reads it while private. A peer would need it exported to all of
// `guidance` to borrow it, which is a wider change than the one the placement buys.
mod contradicted;
mod counts;
mod remedies;

pub(super) use contradicted::{CONTRADICTED, WITHDRAWN};
pub(super) use counts::{COUNTS, count_mismatches};
pub(super) use remedies::remedy_problems;

/// What makes a claim false: a path that is there, optionally holding a literal.
///
/// A path rather than a sentence, because the point of this table is that the prose is checked
/// against the tree and not against a second piece of prose.
pub(super) struct Evidence {
    /// Repo-relative path.
    path: &'static str,
    /// A literal the file must hold. Empty means the path existing is the whole evidence.
    holds: &'static str,
}

impl Evidence {
    /// Is the evidence there?
    ///
    /// **An unreadable evidence file retires the rule rather than refusing, and the mechanism that
    /// catches it is a test rather than this gate**: `tests::every_live_rule_still_has_its_evidence`
    /// asserts `stands` for every row, so a row whose file cannot be opened is a red `just test`
    /// while `check-guidance` itself stays green. Deliberately not migrated to
    /// `repo::read_subject`: the evidence set is DECLARED and small, and a retired rule forbids
    /// nothing rather than mis-measuring the tree. The limit is that `just hygiene` alone sees
    /// neither - only `just validate` runs the gate and this test together.
    fn stands(&self, root: &Path) -> bool {
        let full = root.join(self.path);
        if self.holds.is_empty() {
            return full.exists();
        }
        std::fs::read_to_string(&full).is_ok_and(|text| text.contains(self.holds))
    }
}

/// A claim the tree contradicts.
///
/// The `versions` check above REFUSES a version a comment states; this holds a STATEMENT against
/// its source, and
/// the difference that matters is the evidence. A rule is live only while every `Evidence` stands,
/// so a rule about a crate that is later deleted retires itself rather than forbidding a sentence
/// that has become true again - which a `Forbidden` entry cannot do, being unconditional.
pub(super) struct Contradicted {
    /// Human name, for the message.
    name: &'static str,
    /// Every wording of the same claim, whitespace-collapsed. One entry, N sibling documents: a
    /// wording is added here the moment it is found, not merely fixed where it was found.
    wordings: &'static [&'static str],
    /// What refutes it. ALL of these must stand for the rule to be live.
    ///
    /// Doubly load-bearing, and the second job is why `github.com/telekom/sutura#241` was filed:
    /// the same rows underwrite [`Contradicted::instead`]. A remedy that rests on a row here
    /// retires with the rule instead of outliving it, and
    /// `tests::every_live_rule_still_has_its_evidence` is where a row that has gone reports itself.
    evidence: &'static [Evidence],
    /// What is true instead, and where the correct sentence already is.
    ///
    /// **Held by [`remedies`], and not by review** - which it was, for as long as it took a person
    /// to read this field: it said the agent surface was absent from the day `sutura mcp` shipped,
    /// invisible because `run`'s scope is prose files and this gate never reads its own source.
    ///
    /// **What that does not reach is the sentence.** A remedy is prose and nothing derives it, so a
    /// wording nobody has registered, a number, or a claim about behaviour is held here by review -
    /// the same limit `AGENTS.md` records for this whole module. What is closed is the class that
    /// rotted: a remedy asserting an absence the tree contradicts, once that absence is a wording.
    instead: &'static str,
    /// Paths this applies to. Empty means everywhere in scope.
    only: &'static [&'static str],
    /// Paths exempt - typically a page that quotes the wrong sentence in order to correct it.
    except: &'static [&'static str],
}

impl Contradicted {
    /// Is what refutes this claim still in the tree?
    ///
    /// A rule that kept forbidding a sentence after it became true again is the same bug as a
    /// stale doc, one layer up. Read by both checks over this table, so *live* means one thing.
    fn is_live(&self, root: &Path) -> bool {
        self.evidence.iter().all(|e| e.stands(root))
    }
}

/// A `CONTRADICTED` claim whose SUBJECT was deleted rather than re-anchored, kept for the one
/// thing deletion loses.
///
/// `github.com/telekom/sutura#772`: a `Contradicted` row does two jobs - it asserts its evidence
/// still `stands`, and it carries the `wordings` ratchet that refuses the claim's re-paraphrase.
/// Deleting the row, with only a tombstone COMMENT in its place, kept the first job honest and
/// silently dropped the second - nothing re-enforced the retired wording, and no gate noticed
/// the comment's absence either.
///
/// **Deliberately a different name from `Contradicted::is_live`'s "retires".** That word already
/// means the EVIDENCE went away and the row stopped enforcing - the row is still there, waiting
/// for evidence to return. This is the opposite: there never was a live counterpart to derive
/// evidence from (a number from one external run, see #772's own instance below), so there is
/// nothing to wait for and the row enforces unconditionally, forever, with no evidence field to
/// go stale in the first place.
///
/// **What this does NOT hold.** `stands`-style evidence, `instead`'s corrected sentence and
/// `only`'s scoping have no counterpart here: a `Contradicted` row is deleted for exactly one
/// reason - it stopped being re-derivable from the tree - and inventing evidence for it would be
/// the mechanism this type exists to avoid needing. What is true instead is a question for
/// whoever raises the issue named below, not a sentence this table asserts.
pub(super) struct Withdrawn {
    /// Human name, matching the deleted row's.
    name: &'static str,
    /// The issue that withdrew the claim, for the message.
    issue: &'static str,
    /// Every wording of the withdrawn claim, whitespace-collapsed - the same matching
    /// [`Contradicted::wordings`] uses.
    wordings: &'static [&'static str],
    /// Paths exempt - a record quoting the withdrawn sentence in order to correct it.
    except: &'static [&'static str],
}

/// A file's text with every run of whitespace collapsed to one space, plus the line each byte
/// came from.
///
/// **The reason this is not line-based.** Three of the false sentences this gate now holds are
/// WRAPPED in their source file: `no logging\ndependency at all` and `the only data\nsystem
/// adapter is DuckDB` are both invisible to a line-by-line search, and a check that missed the
/// exact copies it was written for would have been worse than no check. Prose wraps; a claim does
/// not. `stale_phrases` above stays line-based on purpose - its needles are command lines, where a
/// newline is a real difference.
///
/// **The limit, because it cost a wording in the table above.** A comment marker is not stripped, so
/// a claim wrapping inside a `#` comment block flattens with the `#` mid-sentence and is not found.
/// Stripping markers was rejected: `#` also starts a markdown heading, and joining a heading to the
/// paragraph before it could match a "claim" spanning two sections. A claim written across two
/// comment lines needs a needle that fits on one of them.
pub(in crate::guidance) fn flatten(text: &str) -> (String, Vec<usize>) {
    let mut flat = String::with_capacity(text.len());
    let mut lines = Vec::with_capacity(text.len());
    let mut line = 1_usize;
    let mut pending = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if character == '\n' {
                line = line.saturating_add(1);
            }
            pending = !flat.is_empty();
            continue;
        }
        if pending {
            flat.push(' ');
            // The separator is attributed to the line the NEXT word is on, so a wrapped match is
            // reported where a reader would start reading it.
            lines.push(line);
            pending = false;
        }
        let before = flat.len();
        flat.push(character);
        for _ in before..flat.len() {
            lines.push(line);
        }
    }
    (flat, lines)
}

/// Every line, one-based, on which `wording` appears in the flattened `text`.
fn wording_lines(text: &str, wording: &str) -> Vec<usize> {
    let (flat, lines) = flatten(text);
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(at) = flat.get(from..).and_then(|rest| rest.find(wording)) {
        let offset = from.saturating_add(at);
        found.push(lines.get(offset).copied().unwrap_or(1));
        from = offset.saturating_add(wording.len().max(1));
    }
    found
}

pub(super) fn contradicted_claims(root: &Path, files: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for rule in CONTRADICTED {
        if !rule.is_live(root) {
            continue;
        }
        for rel in files {
            if !rule.only.is_empty() && !crate::repo::matches_any(rule.only, rel) {
                continue;
            }
            if crate::repo::matches_any(rule.except, rel) {
                continue;
            }
            let Some(text) = crate::repo::read_subject(root, rel, &mut problems) else {
                continue;
            };
            for wording in rule.wordings {
                for line in wording_lines(&text, wording) {
                    problems.push(format!(
                        "{rel}:{line}: \"{wording}\" - {} is not true of this repo\n      \
                         what is true: {}",
                        rule.name, rule.instead
                    ));
                }
            }
        }
    }
    // #772's ratchet: no `is_live` gate, because a withdrawn claim has no evidence to go stale -
    // it enforces unconditionally, which is the whole reason it outlives the row `Contradicted`
    // deletion used to leave to a comment nothing here read.
    for rule in WITHDRAWN {
        for rel in files {
            if crate::repo::matches_any(rule.except, rel) {
                continue;
            }
            let Some(text) = crate::repo::read_subject(root, rel, &mut problems) else {
                continue;
            };
            for wording in rule.wordings {
                for line in wording_lines(&text, wording) {
                    problems.push(format!(
                        "{rel}:{line}: \"{wording}\" - {} was withdrawn ({}); its own evidence \
                         had no live counterpart to re-derive, which is why the row was deleted \
                         rather than re-anchored - reintroducing the wording is the regression",
                        rule.name, rule.issue
                    ));
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::remedies::{path_shaped, remedies_hold, remedy_scan_broke};
    use super::{CONTRADICTED, COUNTS, Contradicted, Evidence, contradicted_claims, remedy_problems};

    /// The entry as it stood before `github.com/telekom/sutura#241`, reduced to the two fields
    /// that made it wrong: evidence that the surface SHIPS, and a remedy saying it does not.
    const STALE: Contradicted = Contradicted {
        name: "a transport surface is absent",
        wordings: &["the MCP surface is absent"],
        evidence: &[Evidence {
            path: "crates/sutura-mcp/Cargo.toml",
            holds: "",
        }],
        instead: "the MCP surface is absent and the HTTP one is not: `sutura-http` and \
                  `sutura-serve` ship",
        only: &[],
        except: &[],
    };

    #[test]
    fn an_instead_sentence_that_names_an_absent_surface_fails_when_the_surface_lands() {
        let root = crate::repo::root().expect("the repo root");
        // The surface landed: the evidence row stands, so the rule is live and its remedy is read.
        assert!(STALE.is_live(&root), "the fixture's premise is that the crate is there");
        let found = remedies_hold(&root, &[&STALE]);
        assert!(
            found.iter().any(|problem| problem.contains("the MCP surface is absent")),
            "a remedy repeating a wording this table forbids must be reported: {found:?}"
        );
    }

    /// Four citations: two that resolve - a numbered record by its PREFIX, which is how this repo
    /// cites one, and a recipe - and two that do not.
    const CITES: Contradicted = Contradicted {
        name: "citations",
        wordings: &["a phrase nothing in this repo writes"],
        evidence: &[],
        instead: "see `docs/adr/0002` and `just mcp-e2e`, not `docs/no-such-page.md` or \
                  `just no-such-recipe`",
        only: &[],
        except: &[],
    };

    #[test]
    fn a_remedy_citation_that_resolves_to_nothing_is_reported() {
        let root = crate::repo::root().expect("the repo root");
        let found = remedies_hold(&root, &[&CITES]);
        assert!(
            found.iter().any(|problem| problem.contains("docs/no-such-page.md")),
            "a path that is not in the tree must be reported: {found:?}"
        );
        assert!(
            found.iter().any(|problem| problem.contains("no-such-recipe")),
            "a recipe that does not exist must be reported: {found:?}"
        );
        assert_eq!(found.len(), 2, "the two that resolve must not be reported: {found:?}");
    }

    #[test]
    fn a_settings_key_or_a_route_is_not_a_citation() {
        // The three spans in the live table that hold a slash or look like they might, and none of
        // them is something to open. Under-claiming is the safe direction here.
        assert!(!path_shaped("sources.<alias>.kind"));
        assert!(!path_shaped("GET /v1/catalog"));
        assert!(!path_shaped("ci.yml"));
        assert!(path_shaped("docs/serving.md"));
        assert!(path_shaped(".agents/skills/sutura/query-surface/SKILL.md"));
    }

    #[test]
    fn a_remedy_scan_that_reads_no_path_reports_itself() {
        // Same argument as `a_count_entry_measures_something`: a span walk that stopped working
        // would make the check pass on every remedy. Nothing to read is the tell.
        assert!(!remedy_scan_broke(&[]).is_empty(), "an empty table must not read as clean");
        let live: Vec<&Contradicted> = CONTRADICTED.iter().collect();
        assert!(remedy_scan_broke(&live).is_empty(), "the live table cites paths");
    }

    #[test]
    fn every_live_remedy_is_held_to_the_prose_it_corrects() {
        // The live half of the two fixtures above. Red against the tree this was written on: the
        // transport entry's remedy said the agent surface was absent while its own evidence rows
        // prove it ships.
        let (found, _live) = remedy_problems(&crate::repo::root().expect("the repo root"));
        assert!(found.is_empty(), "a remedy in CONTRADICTED does not hold: {found:?}");
    }

    #[test]
    fn a_retired_evidence_drops_the_live_remedy_count_by_one() {
        // `github.com/telekom/sutura#414` item 2: the verdict printed `CONTRADICTED.len()` as how
        // many claims it CHECKED, when the loop skips a row the moment its evidence goes - so a
        // retired row was invisible in a green run. `remedy_problems`'s second field is the live
        // count now, and this is that count moving without `CONTRADICTED.len()` moving with it.
        //
        // A bare scratch tree, not the repo: over the real tree every row bar the one under test
        // is live already, so a paraphrase there would be masked by
        // `every_live_rule_still_has_its_evidence` turning `just test` red by name instead of
        // this count moving - the warning this issue's brief gives. Here nothing stands except
        // the one evidence file planted, so the live count is exactly one row until it is not.
        let tree = crate::scratch_tree::Tree::of("remedy-live-count", &[("crates/sutura-http/Cargo.toml", b"[features]\n")]);
        let total = CONTRADICTED.len();
        let (_, live_before) = remedy_problems(tree.root());
        assert_eq!(
            live_before, 1,
            "only \"no crate declares a feature\" should stand in this tree"
        );
        // The paraphrase: the same claim, the evidence gone.
        std::fs::write(tree.root().join("crates/sutura-http/Cargo.toml"), b"[dependencies]\n")
            .expect("overwrite the fixture manifest");
        let (_, live_after) = remedy_problems(tree.root());
        assert_eq!(live_after, 0, "the retired row must stop counting as live");
        assert_eq!(CONTRADICTED.len(), total, "the declared row count never reads the tree");
    }

    #[test]
    fn a_withdrawn_wording_is_refused_with_no_evidence_to_go_stale() {
        // #772. A `Contradicted` row this scratch tree plants no evidence for goes quiet - see
        // `a_retired_evidence_drops_the_live_remedy_count_by_one` above. The `#603`/`#770` row is
        // `Withdrawn` instead, so an empty tree with no evidence file anywhere must still refuse
        // the wording the deleted row used to hold.
        let tree = crate::scratch_tree::Tree::of(
            "withdrawn-ratchet",
            &[(
                "docs/example.md",
                b"the cause: because a buildless database extracts the crate's own dependencies \
                  but not the standard library, which is fixed now\n",
            )],
        );
        let files = vec![String::from("docs/example.md")];
        let found = contradicted_claims(tree.root(), &files);
        assert!(
            found.iter().any(|problem| problem.contains("was withdrawn")),
            "a withdrawn wording must be refused with no evidence anywhere in the tree: {found:?}"
        );
    }

    #[test]
    fn a_wrapped_sentence_is_still_one_claim() {
        use super::wording_lines;
        // THE case this check exists for: two sentences it was written to hold wrap in their file,
        // so a line-based search finds neither. Reported on the line a reader starts them on.
        let wrapped = "There is no audit sink in the workspace and no logging\ndependency at all.";
        assert_eq!(wording_lines(wrapped, "no logging dependency at all"), vec![1]);

        let later = "line one\nline two\nbroker, no audit sink, and the only data\nsystem adapter is DuckDB.";
        assert_eq!(wording_lines(later, "the only data system adapter is DuckDB"), vec![3]);
    }

    #[test]
    fn a_wording_that_is_absent_is_not_reported() {
        use super::wording_lines;
        assert!(
            wording_lines(
                "There IS an HTTP surface, and its token authenticates the deployment.",
                "no MCP server and no HTTP surface"
            )
            .is_empty(),
            "an absent wording is not reported"
        );
        // Every occurrence, not just the first: two siblings in one file is the shape this
        // whole table exists for.
        let twice = "no logging dependency at all\nand again, no logging dependency at all\n";
        assert_eq!(wording_lines(twice, "no logging dependency at all"), vec![1, 2]);
    }

    #[test]
    fn tabs_and_runs_of_spaces_collapse_the_same_way_a_newline_does() {
        use super::wording_lines;
        // A markdown table cell pads with spaces and a code block indents with tabs. Neither is
        // a difference in the claim, and treating either as one would leave a hole in a file
        // format this repo writes most of its prose in.
        assert_eq!(wording_lines("| x |  four   exact\tsets | y |", "four exact sets"), vec![1]);
    }

    #[test]
    fn only_the_two_false_scoped_view_claims_are_registered() {
        use super::wording_lines;

        const FALSE: &[&str] = &[
            "dimension validation reads the pinned definitions, not the scoped view",
            "The scoped view BORROWS the pinned definitions",
        ];
        let rule = CONTRADICTED
            .iter()
            .find(|rule| rule.name == "the request path reads a scoped view of the definitions")
            .expect("the two false claims stay registered");
        assert_eq!(rule.wordings, FALSE, "the ratchet holds the exact prior overclaims");
        for sentence in FALSE {
            assert_eq!(wording_lines(sentence, sentence), vec![1]);
        }

        let valid_design = "A later scoped view may borrow the pinned bundle without claiming one exists today.";
        assert!(
            rule.wordings
                .iter()
                .all(|wording| wording_lines(valid_design, wording).is_empty()),
            "the ratchet must not forbid a future design merely for naming a scoped view"
        );
    }

    #[test]
    fn evidence_is_the_path_and_not_a_second_opinion() {
        let root = crate::repo::root().expect("the repo root");
        // Existence alone.
        assert!(
            Evidence {
                path: "AGENTS.md",
                holds: ""
            }
            .stands(&root)
        );
        // The absent side of the same check. **Not the manifest of a PLANNED crate**, which is what
        // this used to be: `crates/sutura-mcp/Cargo.toml` was the fixture until the agent surface
        // landed, and then a test about path existence started failing because a crate got written.
        // A path with a name nothing will ever take cannot go the same way.
        assert!(
            !Evidence {
                path: "crates/no-such-crate-exists/Cargo.toml",
                holds: ""
            }
            .stands(&root)
        );
        // And a literal inside it.
        assert!(
            Evidence {
                path: "AGENTS.md",
                holds: "identity-aware semantic data runtime"
            }
            .stands(&root)
        );
        assert!(
            !Evidence {
                path: "AGENTS.md",
                holds: "a phrase nothing in this repo writes"
            }
            .stands(&root)
        );
    }

    #[test]
    fn every_live_rule_still_has_its_evidence() {
        // A rule whose evidence has gone is SILENT - right behaviour, and a failure mode nobody
        // notices. This is the tell: either the claim became true, in which case delete the row
        // the way the invariants table says, or its anchor was renamed and needs replacing.
        let root = crate::repo::root().expect("the repo root");
        for rule in CONTRADICTED {
            for evidence in rule.evidence {
                assert!(
                    evidence.stands(&root),
                    "`{}` no longer has its evidence: {} (holds `{}`)",
                    rule.name,
                    evidence.path,
                    evidence.holds
                );
            }
            assert!(!rule.wordings.is_empty(), "`{}` forbids nothing", rule.name);
            // An empty list is vacuously true, so an entry with no evidence can never retire and
            // the loop above would check nothing about it.
            assert!(!rule.evidence.is_empty(), "`{}` rests on nothing", rule.name);
        }
    }

    /// `#603`, `#670`, `#676`: three hand-corrections of the same claim (four refusals, then
    /// five, then six arms while every site still said five) each left at least one prose site
    /// stale, because the number was registered as a `CONTRADICTED` wording rather than read off
    /// the arms. That entry is gone now - superseded by the `Counted` row below - so nothing here
    /// pins its old wording against the current true sentence; there is no longer a row that
    /// could forbid it. The row's own tally against the real file is pinned in
    /// `a_count_entry_measures_something` below, rather than a fresh listing here: `census.rs`'s
    /// own `every_transitional_door_in_the_tree_is_declared` counts `into_listing(` call sites, so
    /// a second one here would be exactly the kind of diff that test asks to be justified.
    #[test]
    fn a_stop_before_boundary_excludes_a_repeated_literal_past_it() {
        use super::counts::{Counted, Granularity, tally};
        let tree = crate::scratch_tree::Tree::of(
            "counts-stop-before",
            &[(
                "src/wire.rs",
                b"fn f() {\n    let a = MARK;\n    let b = MARK;\n    let c = MARK;\n}\n\n\
                  #[cfg(test)]\nmod tests {\n    fn t() { let d = MARK; }\n}\n",
            )],
        );
        let files = vec![String::from("src/wire.rs")];
        let counted = Counted {
            name: "fixture",
            over: &["src/wire.rs"],
            holds: "MARK",
            stop_before: "#[cfg(test)]",
            granularity: Granularity::Occurrences,
            mentioned_in: &[],
            also_stated_in: &[],
            marker: "",
        };
        let mut unreadable = Vec::new();
        // Three before the boundary, a fourth after it that must not count.
        assert_eq!(tally(tree.root(), &files, &counted, &mut unreadable), 3);
    }

    #[test]
    fn a_number_may_be_stated_in_an_allow_listed_rust_file_and_a_mismatch_there_is_caught() {
        use super::counts::{Counted, Granularity, mismatches_for};
        // `holds` and `marker` are deliberately disjoint strings, the way the real entry's
        // `StatusCode::UNPROCESSABLE_ENTITY` and `refusal reasons land on \`422\`` are - a marker
        // sentence that repeated the counted literal would make the tally see its own statement.
        let all: Vec<String> = vec![String::from("src/wire.rs"), String::from("docs/page.md")];
        let counted = Counted {
            name: "fixture",
            over: &["src/wire.rs"],
            holds: "TAG(",
            stop_before: "",
            granularity: Granularity::Occurrences,
            mentioned_in: &["docs/page.md"],
            also_stated_in: &["src/wire.rs"],
            marker: "call sites are tagged",
        };

        // Agreement: the doc, the allow-listed Rust comment, and the tree all say 3.
        let agreeing = crate::scratch_tree::Tree::of(
            "counts-also-stated-in",
            &[
                (
                    "src/wire.rs",
                    b"let a = TAG();\nlet b = TAG();\nlet c = TAG();\n// 3 call sites are tagged\n",
                ),
                ("docs/page.md", b"3 call sites are tagged\n"),
            ],
        );
        let mut agree = Vec::new();
        mismatches_for(agreeing.root(), &all, &all, &counted, &mut agree);
        assert!(agree.is_empty(), "{agree:?}");

        // Wrong in the comment alone - `mentioned_in` (the doc) still agrees, so this is caught
        // only because `also_stated_in` is read too.
        let wrong = crate::scratch_tree::Tree::of(
            "counts-also-stated-in-wrong",
            &[
                (
                    "src/wire.rs",
                    b"let a = TAG();\nlet b = TAG();\nlet c = TAG();\n// 4 call sites are tagged\n",
                ),
                ("docs/page.md", b"3 call sites are tagged\n"),
            ],
        );
        let mut found = Vec::new();
        mismatches_for(wrong.root(), &all, &all, &counted, &mut found);
        assert!(
            found.iter().any(|p| p.contains("src/wire.rs") && p.contains('4')),
            "a wrong count in the allow-listed Rust file must be reported: {found:?}"
        );
    }

    #[test]
    fn a_page_a_rule_exempts_holds_a_wording_that_rule_forbids() {
        // `except` is for the record that quotes the wrong sentence in order to CORRECT it - and it
        // is therefore also the one copy of that sentence in the tree no scan reads. Those two facts
        // together are how an exemption outlives its reason: the correction gets reworded, the quote
        // goes, and what is left is a rule registered against a sentence nobody wrote plus a page
        // permanently exempt from it. Neither half reports itself, so the exemption is made to keep
        // earning itself here.
        //
        // Over the glob rather than the literal, because `except` is matched as one - a directory
        // pattern that no longer covers a page that quotes the sentence is the same stale exemption
        // in a shape a path read would have missed.
        //
        // `is_live` for the same reason the gate reads it: a retired rule forbids nothing, so
        // demanding the page keep quoting it would turn tidying that quote red for a rule that is
        // no longer there. Both predicates, or this test and the check disagree about what a rule is.
        let root = crate::repo::root().expect("the repo root");
        let (_root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("could not list the repo");
        for rule in CONTRADICTED
            .iter()
            .filter(|rule| !rule.except.is_empty() && rule.is_live(&root))
        {
            let quoted = files
                .iter()
                .filter(|rel| crate::repo::matches_any(rule.except, rel))
                .filter_map(|rel| std::fs::read_to_string(root.join(rel)).ok())
                .any(|text| {
                    rule.wordings
                        .iter()
                        .any(|wording| !super::wording_lines(&text, wording).is_empty())
                });
            assert!(
                quoted,
                "`{}` exempts {:?}, and nothing there states any wording it forbids - so the \
                 exemption protects nothing. Delete it, or delete the rule",
                rule.name, rule.except
            );
        }
    }

    #[test]
    fn the_number_read_is_the_one_immediately_before_the_marker() {
        use super::counts::stated_numbers;
        // The real sentence, and the real trap in it: the line carries `10001` as well, so a
        // check that read every number on it would report the row cap as a golden count.
        let line = "Both legs pinned: 63 SQL goldens read `LIMIT 10001`, and the engine leg asserts the fetch.";
        assert_eq!(stated_numbers(line, "SQL goldens read"), vec![(1, 63)]);
        // No number there is not a claim about the count.
        assert_eq!(
            stated_numbers("The SQL goldens read the row cap.", "SQL goldens read"),
            vec![]
        );
        // Nor is a line that does not carry the marker at all.
        assert_eq!(stated_numbers("39 of something else entirely", "SQL goldens read"), vec![]);
        // The page that EXPLAINS the marker states no number and must stay unread - this is the
        // sentence a "every page naming the marker carries a number" rule would have failed.
        assert_eq!(
            stated_numbers(
                "the number written before the marker `SQL goldens read` and compares",
                "SQL goldens read"
            ),
            vec![]
        );
    }

    #[test]
    fn a_statement_that_wraps_away_from_its_marker_is_still_read() {
        use super::counts::stated_numbers;
        // THE hole this replaced. Both of these were invisible to the per-line reader, and both
        // are indistinguishable from agreement in its output: the first because the number ended
        // one line above the marker, the second because only the first marker on a line was read.
        // A reflow of a paragraph is enough to produce the first, which is why a sentence saying
        // "keep them on one line" was not the fix.
        let wrapped = "the whole set is 10
SQL goldens read the cap";
        assert_eq!(stated_numbers(wrapped, "SQL goldens read"), vec![(2, 10)]);
        let twice = "| 93 SQL goldens read here | 94 SQL goldens read there |";
        assert_eq!(stated_numbers(twice, "SQL goldens read"), vec![(1, 93), (1, 94)]);
    }

    #[test]
    fn a_count_entry_measures_something() {
        // Same argument as `every_live_rule_still_has_its_evidence`, for the other table: a glob
        // matching nothing would make the check pass vacuously. Caught here, not on a branch.
        let root = crate::repo::root().expect("the repo root");
        let (_root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("could not list the repo");
        // The `unreadable` vec is asserted on, not discarded: on a clean checkout a subject this
        // walk cannot open is a defect in the venue, and a test that threw the vec away would be
        // the visible discard `repo::read_subject` exists to make somebody write out.
        let mut unreadable = Vec::new();
        for counted in COUNTS {
            assert!(
                super::counts::tally(&root, &files, counted, &mut unreadable) > 0,
                "the {} count matches nothing - `{}` under {:?}",
                counted.name,
                counted.holds,
                counted.over
            );
        }
        assert!(
            unreadable.is_empty(),
            "a subject this tally walks could not be read: {unreadable:?}"
        );

        // Pinned rather than merely non-zero: `grep -c StatusCode::UNPROCESSABLE_ENTITY` on the
        // whole file answers fourteen - #676's own "counting these by grep is a trap" - because
        // `every_reason()` inside `#[cfg(test)]` repeats the literal once per case it asserts.
        // `stop_before` is what tells the two apart.
        let entry = COUNTS
            .iter()
            .find(|counted| counted.name == "refusal reasons that land on `422`")
            .expect("the entry stays registered");
        assert_eq!(super::counts::tally(&root, &files, entry, &mut unreadable), 7);
    }

    #[test]
    fn a_count_entry_is_compared_against_a_page_that_states_it() {
        // The mirror of the test above, and it is here because the failure it describes HAPPENED:
        // the row-cap sentence lived in `AGENTS.md`, the router rewrite carried the invariants
        // table into `.agents/skills/`, `mentioned_in` stayed as it was, and the gate went on
        // counting 93 goldens against a number no page stated any more. A count nobody writes
        // down is not a gate - it is a walk of the tree whose verdict is always agreement.
        let root = crate::repo::root().expect("the repo root");
        let (_root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("could not list the repo");
        let mut unreadable = Vec::new();
        for counted in COUNTS {
            assert!(
                !super::counts::statements(&root, &files, counted.mentioned_in, counted, &mut unreadable).is_empty(),
                "no page under {:?} states the {} count before `{}`",
                counted.mentioned_in,
                counted.name,
                counted.marker
            );
        }
        assert!(unreadable.is_empty(), "a subject this walk could not be read: {unreadable:?}");
    }

    #[test]
    fn a_second_occurrence_in_one_file_counts_twice_only_where_the_entry_says_so() {
        use super::counts::Granularity;
        // THE capability this table lacked, and the reason it could not hold the `pub trait`
        // count: two declarations in one file are two ports and one file. Both entries are right
        // about their own literal, and neither answer is a safe default for the other - a golden
        // carrying the row cap twice is still one golden.
        let twice = "pub trait One {}\npub trait Two {}\n";
        assert_eq!(Granularity::Files.count_in(twice, "pub trait "), 1);
        assert_eq!(Granularity::Occurrences.count_in(twice, "pub trait "), 2);
        // Absent counts zero either way, which is what the tally check above reads.
        assert_eq!(Granularity::Files.count_in("mod tests {}\n", "pub trait "), 0);
        assert_eq!(Granularity::Occurrences.count_in("mod tests {}\n", "pub trait "), 0);
        // Non-overlapping, like `grep -o`: `aa` in `aaaaa` is two, not the four an overlapping
        // scan would report.
        assert_eq!(Granularity::Occurrences.count_in("aaaaa", "aa"), 2);
    }
}
