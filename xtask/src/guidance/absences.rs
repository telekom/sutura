//! An absence a document claims, held against the production code that would refute it.
//!
//! **Why a check and not a wording rule.** `github.com/telekom/sutura#370`: the doc comment on
//! `ExecutedAs::and` said *"Nothing constructs a second leg today - there is no combiner"* while the
//! federated answer path called both the method and the combiner, and `just api` republished the
//! sentence on a page in the nav. Nothing read it, so it went false in silence.
//! [`CONTRADICTED`](super::claims::CONTRADICTED) could not have held it: a forbidden wording is a
//! ratchet on a sentence somebody has already got wrong, and this one had not been. **The sentence
//! was TRUE when it was written**, which is the whole class - and the same class
//! [`constants`](super::constants) exists for one subject over.
//!
//! That one resolves a CONSTANT out of the tree. This resolves an ABSENCE: the sentence says the
//! tree holds no such thing, and the entry says what holding one would look like.
//!
//! # Both sides are read, and only one failure is the sighting
//!
//! A count is a witness only when its two numbers come from different places, so an entry is red
//! four ways:
//!
//! * **nothing states the absence** - the tree is scanned and compared to nothing, which is
//!   `count_mismatches`' and `version_mismatches`' failure and was live in both;
//! * **a sighting's globs reach no file** - a scan whose subject is unscanned reports the absence
//!   held without having read anything, which is the defect this repository has shipped most often;
//! * **a file in scope could not be read** - the same defect one file at a time, which
//!   `check-docs` and `check-shipped-binaries` each shipped as a silent `continue` above their own
//!   fail-closed arm;
//! * **a sighting stands** - the absence is refuted, and the message names both ends.
//!
//! # What it holds, and where it stops
//!
//! * **Rust prose is IN scope**, which `guidance::in_scope` excludes for everything else that judges
//!   a sentence, and for a stated reason: a rule table written in Rust holds the phrases it forbids.
//!   [`prose`] is what makes the wider scope safe rather than an exception list - for a `.rs` file
//!   only DOC COMMENTS are read, markers stripped, so this gate's own table (string literals, under
//!   `xtask/`) is unreadable to it twice over.
//! * **A sighting is a literal authored per entry, not a caller set derived from a name.** Deriving
//!   one was measured and rejected: on 2026-09-06 `.and(` occurred 23 times under `crates/`, of which
//!   exactly one was the call the combiner sentence was about and the rest were `Option::and` and a
//!   `SourceSet` builder. A gate reporting those gets switched off, and a switched-off gate holds
//!   nothing.
//! * **The entry's globs are the reach.** A consumer outside them is invisible, which is why each
//!   entry below says what its glob covers. It under-claims in a direction a reader can see.
//! * **Test code does not refute an absence.** [`regions::scope`] classifies it, including an
//!   out-of-line `#[cfg(test)] mod x;` whose marker is in the parent file - the shape the entries
//!   below actually have. What it does not reach is a `#[cfg(test)]` on something other than
//!   an item with a body or a declaration.
//! * **A `///` inside a multi-line string literal reads as prose here**, the same limit `constants`
//!   states about itself. Nothing in this tree is in that shape.
//! * **A literal is matched, so a paraphrase escapes** - the limit `AGENTS.md` records for the leak
//!   guard and `claims` records for itself. This is a reader for the sentence somebody wrote, not
//!   for the one they meant.

use std::path::Path;

use super::claims::flatten;
use crate::causality::regions;
use crate::repo::matches_any;
use crate::serde_parse::scan::code_lines;

/// What the tree would have to hold for an absence to be false.
struct Sighting {
    /// Files to read. Globs against repo-relative paths, and **at least one has to match** - see
    /// [`absences_hold`], where a scan over an empty set is a failure rather than an agreement.
    over: &'static [&'static str],
    /// The literal whose presence in production code refutes the absence.
    holds: &'static str,
    /// What its presence would mean. Printed beside the line, because a verdict whose reason is
    /// unstated gets reverted.
    means: &'static str,
}

/// An absence stated in prose, and what would refute it.
///
/// [`Pin`](crate::guidance::Pin) holds a version against its source and
/// [`Counted`](super::claims::COUNTS) holds a number against a walk of the tree. This holds a
/// NEGATIVE EXISTENTIAL against the code, which neither can express: the sentence's whole content is
/// that a thing is not there, so the only honest reading is to go and look.
struct Absence {
    /// Human name, for the message.
    name: &'static str,
    /// Every wording of the one absence, matched in the flattened prose view. One entry, N sibling
    /// documents - `claims`' doctrine, for `claims`' reason: a correction lands in one file and is
    /// not carried to the others.
    claimed: &'static [&'static str],
    /// Where the absence may be stated. **At least one file here has to state it**, or the entry
    /// scans the tree and compares it to nothing.
    stated_in: &'static [&'static str],
    /// Every scan that would refute it. ALL are performed; ANY one standing is a failure.
    refuted_by: &'static [Sighting],
}

/// Short for the reason `COUNTS` is short: an absence is worth a gate when a reader would plan
/// against it. How many there are is `cargo xtask check-guidance`'s own success line, which prints
/// the statements read and the files opened - written there rather than here, because a number in
/// prose that a command answers is the defect this module exists for.
///
/// Each of these is a limit somebody is told to work around - configure a client out of band, do not
/// alert on a gauge, do not read an accessor as covered - so the day it stops being true is the day
/// an instruction has to be withdrawn. That is the property the entry holds, not the sentence.
///
/// **Not here, and deliberately.** The combiner sentence `#370` opens with is corrected rather than
/// registered: it is already false, so an entry for it would be a gate that fails on landing, and
/// what a sentence somebody has got wrong once needs is `CONTRADICTED`'s ratchet. The replay-window
/// absence is not here either - `crates/sutura-http/src/inbound/tests/review.rs` asserts the replay
/// SUCCEEDS, twice, so a nonce store turns that test red and puts all four of its sentences in front
/// of somebody. An entry duplicating a test that already fails is a second thing to keep true.
const ABSENCES: &[Absence] = &[
    Absence {
        // `#370` row D. Authored places say it and generated pages republish it, and until this
        // entry nothing read any of them: adding the parameter at the challenge would have left
        // every gate and every test green while `docs/serving.md` went on telling an operator to
        // configure a client's issuer out of band.
        name: "the `401` challenge names no protected-resource metadata",
        // The clause they all share. The sentences around it differ - one says the client is
        // configured out of band, another that it learns the authorization server out of band - and
        // registering the shared clause is what makes a sibling that was missed a failure rather
        // than a survivor.
        claimed: &["no `resource_metadata` parameter"],
        stated_in: &["crates/sutura-http/src/**/*.rs", "docs/**/*.md"],
        refuted_by: &[Sighting {
            // The transport crate's own source, which is where the challenge is built. A parameter
            // added anywhere else is not a challenge parameter.
            over: &["crates/sutura-http/src/**/*.rs"],
            holds: "resource_metadata",
            means: "the challenge, or something on its path, now names protected-resource metadata",
        }],
    },
    Absence {
        // `#370` row E, first item. The accessor reads as covered and is called from a `tests/`
        // target alone; `docs/adr/0016` is what the doc comment sends a reader to.
        name: "`MetricAspect::expression()` has no consumer",
        claimed: &["No consumer today"],
        stated_in: &["crates/sutura-catalog-datahub/src/**/*.rs"],
        refuted_by: &[Sighting {
            // EVERY crate's library source, not the adapter's own: a reporter that consumed this
            // would live in `sutura-app`, and an entry that looked only where the accessor is
            // declared would be blind to the consumer the sentence is about. Integration tests
            // under `crates/*/tests/**` are outside the glob and are the callers it has today.
            over: &["crates/*/src/**/*.rs"],
            holds: ".expression()",
            means: "something in the workspace now reads a metric aspect's raw expression",
        }],
    },
    Absence {
        // `#370` row E, third item. `docs/adr/0015` specifies the gauge's absence, and an operator
        // is told not to alert on the reading - so a consumer landing without the record moving is
        // the instruction going stale.
        name: "no gauge reads the `DataFusion` pool",
        claimed: &["a gauge whose absence it currently specifies"],
        stated_in: &["crates/sutura-exec-datafusion/src/**/*.rs"],
        refuted_by: &[Sighting {
            over: &["crates/*/src/**/*.rs"],
            // The CALL, not the declaration: `pub fn memory_pool(` is the accessor itself and would
            // make every entry refute itself. `with_memory_pool(` in the session builder is a
            // different method and does not contain this.
            holds: ".memory_pool()",
            means: "something in the workspace now reads the pool the adapter reserves against",
        }],
    },
];

/// A file's PROSE: for Rust, its doc comments alone, markers stripped; anything else as written.
///
/// **Blanked rather than dropped**, so the line a verdict names is the line a reader opens - the
/// same contract [`flatten`] keeps. And doc comments ONLY, which is what lets this check read `.rs`
/// where `guidance::in_scope` will not: a rule table in Rust is string literals, and a string
/// literal is not a doc comment.
///
/// A claim wrapping across two `///` lines is found here and is NOT found by `claims`, whose own
/// header records that limit - the marker sits mid-sentence in the flattened view. Stripping it is
/// safe here and is not safe there for the reason stated over `flatten`: `#` also opens a Markdown
/// heading, and `///` opens nothing else.
fn prose(rel: &str, text: &str) -> String {
    // Through `Path::extension` rather than `ends_with`, which
    // `clippy::case_sensitive_file_extension_comparisons` refuses: `A.RS` is a Rust file to every
    // tool that reads this tree, and a check deciding otherwise would read such a file as Markdown.
    if !std::path::Path::new(rel)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
    {
        return String::from(text);
    }
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        let body = trimmed
            .strip_prefix("//!")
            .or_else(|| trimmed.strip_prefix("///"))
            .unwrap_or("");
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// Every place one of `claimed`'s wordings is written: the file, and the one-based line.
///
/// `unread` collects a file that is IN scope and could not be read. Dropping one in silence is the
/// shape `check-docs` and `check-shipped-binaries` each shipped - a walk that skips a file it was
/// meant to read and reports the answer over the rest - and the guard is safe to make a failure here
/// because every glob in the table ends `*.rs` or `*.md`, so a PNG is out of SCOPE rather than
/// unreadable.
fn statements(root: &Path, files: &[String], absence: &Absence, unread: &mut Vec<String>) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    for rel in files {
        if !matches_any(absence.stated_in, rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            unread.push(rel.clone());
            continue;
        };
        let (flat, lines) = flatten(&prose(rel, &text));
        for wording in absence.claimed {
            let mut from = 0_usize;
            while let Some(at) = flat.get(from..).and_then(|rest| rest.find(wording)) {
                let offset = from.saturating_add(at);
                found.push((rel.clone(), lines.get(offset).copied().unwrap_or(1)));
                from = offset.saturating_add(wording.len().max(1));
            }
        }
    }
    found
}

/// What one sighting's scan read, and what it found.
///
/// `scanned` is the half that matters in a GREEN run: a hit list is empty both when the absence
/// holds and when the walk read nothing, and those are opposite verdicts.
struct Sighted {
    /// Files actually opened.
    scanned: usize,
    /// Production lines holding the literal: the file, and the one-based line.
    hits: Vec<(String, usize)>,
    /// Files in scope that could not be read.
    unread: Vec<String>,
}

/// Look for what would refute the absence, in production code only.
///
/// A file in scope that cannot be read goes in [`Sighted::unread`] rather than being skipped: the
/// hit list being empty is the verdict, and a walk that quietly dropped a file would report exactly
/// that. See [`statements`] for why the guard cannot fire on a binary.
fn sighted(root: &Path, files: &[String], sighting: &Sighting) -> Sighted {
    // The blanked image is what both the region walk and the search read, so a `#[cfg(test)]` in a
    // comment opens no region and a needle in a comment is not a call. `regions::scope` follows a
    // `#[cfg(test)] mod x;` into the PARENT file, which is why this reads by path rather than
    // taking the text it already has.
    let read = |path: &str| {
        std::fs::read_to_string(root.join(path))
            .ok()
            .map(|text| code_lines(&text).join("\n"))
    };
    let mut found = Sighted {
        scanned: 0,
        hits: Vec::new(),
        unread: Vec::new(),
    };
    for rel in files {
        if !matches_any(sighting.over, rel) {
            continue;
        }
        let Some(code) = read(rel) else {
            found.unread.push(rel.clone());
            continue;
        };
        found.scanned = found.scanned.saturating_add(1);
        let tests = regions::scope(rel, &read);
        for (index, line) in code.lines().enumerate() {
            let number = index.saturating_add(1);
            if tests.covers(number) || !line.contains(sighting.holds) {
                continue;
            }
            found.hits.push((rel.clone(), number));
        }
    }
    found
}

/// What the run actually read, so a green line can say so.
///
/// Two numbers from two different places - statements out of prose, files out of the tree - because
/// one of them alone is what a gate over silence prints.
pub(super) struct Reading {
    /// Statements of an absence found.
    pub(super) stated: usize,
    /// Files a sighting opened.
    pub(super) scanned: usize,
}

/// The absences, held against the tree.
///
/// Takes the table rather than reading the const, so the fixtures in `tests` exercise the code the
/// gate runs and not a re-implementation of it - `remedies_hold`'s reason, and the one that makes
/// the empty-set and truncated-walk cases provable at all.
fn absences_hold(root: &Path, files: &[String], table: &[Absence]) -> (Vec<String>, Reading) {
    let mut problems = Vec::new();
    let mut reading = Reading { stated: 0, scanned: 0 };
    for absence in table {
        let mut unread = Vec::new();
        let stated = statements(root, files, absence, &mut unread);
        reading.stated = reading.stated.saturating_add(stated.len());
        for rel in &unread {
            problems.push(format!(
                "{rel}: in the {} absence's stated_in scope and could not be read - the walk is \
                 short by a file, so what it did not find proves nothing",
                absence.name
            ));
        }
        if stated.is_empty() {
            problems.push(format!(
                "nothing under {:?} states the {} absence - the tree is scanned and compared to \
                 nothing, so this entry in ABSENCES is a gate over silence. State it, or delete \
                 the entry",
                absence.stated_in, absence.name
            ));
        }
        for sighting in absence.refuted_by {
            let found = sighted(root, files, sighting);
            reading.scanned = reading.scanned.saturating_add(found.scanned);
            for rel in &found.unread {
                problems.push(format!(
                    "{rel}: in the {} sighting's scope and could not be read - the scan is short by \
                     a file, so an empty hit list is not an absence",
                    absence.name
                ));
            }
            if found.scanned == 0 {
                problems.push(format!(
                    "the {} sighting (`{}` under {:?}) opened no file - the scan is over an empty \
                     set, so it reports the absence held without having read anything",
                    absence.name, sighting.holds, sighting.over
                ));
                continue;
            }
            let says = stated
                .first()
                .map_or_else(String::new, |(file, line)| format!(", and {file}:{line} says it does not"));
            for (file, line) in &found.hits {
                problems.push(format!(
                    "{file}:{line}: `{}` in production code refutes the {} absence - {}{says}",
                    sighting.holds, absence.name, sighting.means
                ));
            }
        }
    }
    (problems, reading)
}

/// The entry point `guidance::run` wires in.
pub(super) fn absence_problems(root: &Path, files: &[String]) -> (Vec<String>, Reading) {
    absences_hold(root, files, ABSENCES)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Absence, Sighting, absences_hold, prose};

    /// A fixture tree of `(relative path, content)`, and the file list a walk would hand the check.
    fn tree(tag: &str, files: &[(&str, &str)]) -> (PathBuf, Vec<String>) {
        let dir = std::env::temp_dir().join(format!("sutura-absences-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).unwrap_or_default();
        for (rel, body) in files {
            let full = dir.join(rel);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("a fixture directory");
            }
            std::fs::write(&full, body).expect("a fixture file");
        }
        (dir, files.iter().map(|(rel, _)| String::from(*rel)).collect())
    }

    const ENTRY: Absence = Absence {
        name: "the widget has no consumer",
        claimed: &["no consumer today"],
        stated_in: &["crates/*/src/**/*.rs", "docs/**/*.md"],
        refuted_by: &[Sighting {
            over: &["crates/*/src/**/*.rs"],
            holds: ".widget()",
            means: "something reads the widget",
        }],
    };

    /// `ENTRY` with a sighting glob naming a directory the fixture tree has not got.
    const NOWHERE: Absence = Absence {
        name: "the widget has no consumer",
        claimed: &["no consumer today"],
        stated_in: &["docs/**/*.md"],
        refuted_by: &[Sighting {
            over: &["crates/*/nowhere/**/*.rs"],
            holds: ".widget()",
            means: "something reads the widget",
        }],
    };

    /// `ENTRY` with a sighting that looks at the crate a TRUNCATED file list leaves out.
    const OUTSIDE: Absence = Absence {
        name: "the widget has no consumer",
        claimed: &["no consumer today"],
        stated_in: &["crates/*/src/**/*.rs"],
        refuted_by: &[Sighting {
            over: &["crates/b/src/**/*.rs"],
            holds: ".widget()",
            means: "something reads the widget",
        }],
    };

    #[test]
    fn a_stated_absence_the_tree_does_not_refute_holds() {
        let (dir, files) = tree(
            "green",
            &[(
                "crates/a/src/lib.rs",
                "/// The widget, which has\n/// no consumer today.\npub fn widget() -> u8 { 1 }\n",
            )],
        );
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        assert!(problems.is_empty(), "{problems:?}");
        // BOTH numbers, because either alone is what a gate over silence prints: one statement
        // read out of prose, one file opened out of the tree.
        assert_eq!(reading.stated, 1);
        assert_eq!(reading.scanned, 1);
    }

    #[test]
    fn a_claim_wrapped_across_two_doc_comment_lines_is_still_read() {
        // The capability `claims` does not have, and the reason this check reads `.rs` at all: its
        // own header records that a claim wrapping inside a comment block flattens with the marker
        // mid-sentence and is not found.
        let (dir, files) = tree(
            "wrapped",
            &[(
                "crates/a/src/lib.rs",
                "/// There is\n/// no consumer today.\npub fn widget() {}\n",
            )],
        );
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(reading.stated, 1);
    }

    #[test]
    fn a_production_call_refutes_the_absence_and_the_message_names_both_ends() {
        let (dir, files) = tree(
            "refuted",
            &[
                ("crates/a/src/lib.rs", "/// no consumer today\npub fn widget() {}\n"),
                ("crates/b/src/lib.rs", "fn read(a: &A) -> u8 {\n    a.widget()\n}\n"),
            ],
        );
        let (problems, _) = absences_hold(&dir, &files, &[ENTRY]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let only = &problems[0];
        assert!(only.starts_with("crates/b/src/lib.rs:2:"), "{only}");
        assert!(only.contains("crates/a/src/lib.rs:1 says it does not"), "{only}");
    }

    #[test]
    fn a_call_from_test_code_does_not_refute_it() {
        // Which is the point: every live entry's subject is called from tests and reads as covered.
        let (dir, files) = tree(
            "tested",
            &[(
                "crates/a/src/lib.rs",
                "/// no consumer today\npub fn widget() {}\n#[cfg(test)]\nmod tests {\n    fn t(a: &A) { a.widget(); }\n}\n",
            )],
        );
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(reading.scanned, 1);
    }

    #[test]
    fn an_absence_nothing_states_is_a_gate_over_silence() {
        // The half that made `PINS` and `COUNTS` each pass over nothing for weeks: the scan runs,
        // finds no counter-example, and agrees with a sentence that is not there.
        let (dir, files) = tree("silent", &[("crates/a/src/lib.rs", "pub fn widget() {}\n")]);
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("gate over silence"), "{}", problems[0]);
        assert_eq!(reading.stated, 0);
        // The walk still happened - which is exactly why the hit list being empty proves nothing
        // on its own.
        assert_eq!(reading.scanned, 1);
    }

    #[test]
    fn a_sighting_whose_globs_reach_nothing_is_red() {
        // POINTED AT AN EMPTY SET. The entry's own glob matches no file in this tree, so the
        // sighting agrees with everything.
        let (dir, files) = tree(
            "empty",
            &[
                ("docs/a.md", "There is no consumer today.\n"),
                ("crates/a/src/lib.rs", "fn read(a: &A) { a.widget(); }\n"),
            ],
        );
        let (problems, reading) = absences_hold(&dir, &files, &[NOWHERE]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("opened no file"), "{}", problems[0]);
        assert_eq!(reading.scanned, 0);
    }

    #[test]
    fn a_truncated_walk_is_red_rather_than_green() {
        // The other way the same hole opens, and the one five pull requests shipped this week: the
        // globs are right and the FILE LIST handed in does not carry the file. Reading the tree
        // through `files` is what makes that visible instead of silent.
        let (dir, _) = tree(
            "truncated",
            &[
                ("crates/a/src/lib.rs", "/// no consumer today\npub fn widget() {}\n"),
                ("crates/b/src/lib.rs", "fn read(a: &A) { a.widget(); }\n"),
            ],
        );
        let truncated = vec![String::from("crates/a/src/lib.rs")];
        let (whole, _) = absences_hold(
            &dir,
            &[String::from("crates/a/src/lib.rs"), String::from("crates/b/src/lib.rs")],
            &[ENTRY],
        );
        assert_eq!(whole.len(), 1, "the refutation is there to be found: {whole:?}");

        let (problems, reading) = absences_hold(&dir, &truncated, &[OUTSIDE]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("opened no file"), "{}", problems[0]);
        assert_eq!(reading.scanned, 0);
    }

    #[test]
    fn a_file_in_scope_that_cannot_be_read_is_red_on_both_sides() {
        // The silent `continue` two other gates here shipped: the walk is short by a file and the
        // hit list is empty for that reason rather than because the absence holds. Invalid UTF-8 in
        // a path the globs match is a file in SCOPE that cannot be read, which is the case the
        // guard is for - a PNG never matches `*.rs` or `*.md` and so is never reached.
        let (dir, files) = tree(
            "unreadable",
            &[("crates/a/src/lib.rs", "/// no consumer today\npub fn widget() {}\n")],
        );
        std::fs::write(dir.join("crates/a/src/lib.rs"), [0x2f, 0x2f, 0x2f, 0xff, 0xfe]).expect("bad bytes");
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        // Both halves report it, and the statement side ALSO reports the silence it now has.
        assert!(
            problems.iter().any(|p| p.contains("stated_in scope and could not be read")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("sighting's scope and could not be read")),
            "{problems:?}"
        );
        assert_eq!(reading.scanned, 0, "an unreadable file is not a file scanned");
    }

    #[test]
    fn only_doc_comments_are_prose_in_rust() {
        // What makes reading `.rs` safe: a table of forbidden wordings is string literals, and a
        // string literal is not prose here. Line numbers survive, so a verdict names the line.
        let read = prose(
            "crates/a/src/lib.rs",
            "//! Header.\nconst RULE: &str = \"no consumer today\";\n/// Doc.\n",
        );
        assert_eq!(read, " Header.\n\n Doc.\n");
        assert!(!read.contains("no consumer today"));
        // Anything else is its own text, untouched.
        assert_eq!(prose("docs/a.md", "no consumer today\n"), "no consumer today\n");
    }
}
