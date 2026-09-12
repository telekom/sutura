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
//! # Every way this reports `ok` over nothing, and the arm that refuses it
//!
//! A number is a witness only when it comes from somewhere the mutation cannot reach, so an entry
//! is red six ways and only one of them is the sighting. **Five of the six were measured as live
//! holes in review** rather than imagined:
//!
//! * **the table is empty** - both printed numbers derive from iterating it, so they collapse
//!   together and `0 statement(s) over 0 file(s)` reads exactly like a clean tree. Measured:
//!   `ABSENCES = &[]` left 1023 tests green and the gate at exit 0;
//! * **nothing states the absence** - the tree is scanned and compared to nothing, which is
//!   `count_mismatches`' and `version_mismatches`' failure and was live in both;
//! * **a sighting's globs reach no file** - a scan whose subject is unscanned reports the absence
//!   held without having read anything, which is the defect this repository has shipped most often;
//! * **a sighting's walk is TRUNCATED**, over files or inside one - and a printed number was never
//!   the thing that caught it. `github.com/telekom/sutura#414`, measured on `036bce03`:
//!   `.take(100)` on the per-file line walk left `just hygiene` at **exit 0** with 214500 of 294744
//!   production lines unread and `just test` green at 2831 passed, because the depth floor
//!   (`lines > files * 4`, i.e. 656) was derived from the very collection the loop narrows; one
//!   `continue` under the glob filter left 136 of 164 files unscanned, also exit 0, also green.
//!   Both walks are [`crate::repo::Offered::each`] now - the loop lives inside the witness, whose
//!   payload is one outcome per subject, and each is PAIRED against a count this module takes
//!   itself. A short walk is a refusal that names both numbers;
//! * **a file in scope could not be read** - the same defect one file at a time, which
//!   `check-docs` and `check-shipped-binaries` each shipped as a silent `continue` above their own
//!   fail-closed arm;
//! * **a sighting stands** - the absence is refuted, and the message names both ends.
//!
//! The composition is held one layer up: [`absence_problems`] is called from `guidance::tree_problems`
//! rather than from `run`, so `tests::a_check_dropped_from_the_run_is_caught` covers the CALL.
//! Measured before that move: replacing `problems.extend(refuted)` with a discard left 1023 tests
//! green and printed a verdict byte-identical to a clean run over a planted refutation.
//!
//! # What it holds, and where it stops
//!
//! * **Rust prose is IN scope**, which `guidance::in_scope` excludes for everything else that judges
//!   a sentence, and for a stated reason: a rule table written in Rust holds the phrases it forbids.
//!   [`prose`] is what makes the wider scope safe rather than an exception list - for a `.rs` file
//!   only DOC COMMENTS are read, markers stripped, so this gate's own table (string literals, under
//!   `xtask/`) is unreadable to it twice over.
//! * **A sighting reads the code image AND the string literals.** `code_lines` blanks the interior of
//!   any string that SPANS LINES, which is its documented contract - so a `format!` whose literal is
//!   `\`-continued in this tree's house style was invisible to the blanked image alone. Measured: the
//!   very line `#370` names, `inbound/gate.rs:173`, rewritten as a continued literal carrying the
//!   parameter, passed at exit 0 while the single-line form failed; **the difference was one
//!   backslash**, and 291 continued literals sit under `crates/*/src`. [`string_literals`] is
//!   `code_lines`' documented inverse over the same walk, so reading both asks one question twice
//!   rather than answering it twice.
//! * **A sighting is a literal authored per entry, not a caller set derived from a name.** Deriving
//!   one was measured and rejected: on 2026-09-06 `.and(` occurred 23 times under `crates/`, of which
//!   exactly one was the call the combiner sentence was about and the rest were `Option::and` and a
//!   `SourceSet` builder. A gate reporting those gets switched off, and a switched-off gate holds
//!   nothing.
//! * **The entry's globs are the reach.** A consumer outside them is invisible, which is why each
//!   entry below says what its glob covers. It under-claims in a direction a reader can see.
//! * **Test code does not refute an absence.** [`regions::scope`] classifies it, including an
//!   out-of-line `#[cfg(test)] mod x;` whose marker is in the parent file - the shape the entries
//!   below actually have. What it does not reach is a `#[cfg(test)]` on something other than an item
//!   with a body or a declaration. **And it exempts a file whose PATH holds `/tests/` whole**, on the
//!   path alone - so `crates/*/src/**/tests/*.rs` is invisible to a sighting. That is why the floor
//!   counts PRODUCTION LINES: such a file contributes none, where a per-file count would have risen
//!   from 446 to 448 while coverage went down.
//! * **A `///` inside a multi-line string literal reads as prose here**, the same limit `constants`
//!   states about itself. Nothing in this tree is in that shape.
//! * **A literal is matched, so a paraphrase escapes** - the limit `AGENTS.md` records for the leak
//!   guard and `claims` records for itself. This is a reader for the sentence somebody wrote, not
//!   for the one they meant.

use std::collections::BTreeSet;
use std::path::Path;

use super::claims::flatten;
use crate::causality::regions;
use crate::repo::{Offered, matches_any};
use crate::serde_parse::scan::{code_lines, string_literals};

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
/// The `versions` check refuses a version a comment states and
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
/// the statements read, the files opened and the production lines searched - written there rather
/// than here, because a number in prose that a command answers is the defect this module exists for.
///
/// **Emptying this table is a FAILURE, not a clean tree** - see [`absences_hold`]. Both printed
/// numbers derive from iterating it, so they are two counters from one source and collapse together;
/// the arm that refuses that is what makes them a floor rather than a decoration.
///
/// Each entry is a limit somebody is told to work around - configure a client out of band, do not
/// alert on a gauge, do not read an accessor as covered - so the day it stops being true is the day
/// an instruction has to be withdrawn. That is the property held here, not the sentence.
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
        // `#432`, from the review of `#338`. `CompileFailure` gained a second arm and three
        // sentences downstream went on naming the first: a caller was told the BUNDLE would not
        // compile its question for a failure that is this workspace's own splitter and assembler
        // disagreeing. The three were reworded, and a mutation putting them back left the whole
        // suite green - 2658 tests, exit 0 - because no test and no snapshot names any of them. So
        // the reword was held by review and by nothing, which is what this entry changes.
        name: "no compile-failure sentence blames the pinned bundle",
        claimed: &["No sentence on this variant's path blames the pinned bundle"],
        stated_in: &["crates/sutura-app/src/**/*.rs"],
        // One sighting per wording, because the three live in three crates and a reword that
        // reached two of them is the failure this is for. ALL are scanned; ANY standing fails.
        refuted_by: &[
            Sighting {
                // Every crate's library source rather than `sutura-app`'s: the sentence a caller
                // reads is written wherever the transport writes it, and an entry that looked only
                // where the variant is declared would be blind to two of the three.
                over: &["crates/*/src/**/*.rs"],
                holds: "compiled against the pinned bundle",
                means: "the failure's own `Display` blames the bundle again for a cause that may be an assembly defect",
            },
            Sighting {
                over: &["crates/*/src/**/*.rs"],
                holds: "pinned bundle did not compile",
                means: "the HTTP sink's log line blames the bundle again",
            },
            Sighting {
                over: &["crates/*/src/**/*.rs"],
                holds: "against its own bundle",
                means: "the MCP tool result tells a model the bundle is at fault again",
            },
        ],
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
fn statements(
    root: &Path,
    files: &[String],
    absence: &Absence,
    unread: &mut Vec<String>,
    short: &mut Vec<String>,
) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    // The glob filter is inside [`Offered::matching`] and the loop is inside `each`, so this
    // function holds no sequence and no filter a narrowing could be written beside - see
    // `crate::repo::accounting`. A truncation inside `each` cannot mint the witness either.
    let walked = Offered::matching("stating file", files, absence.stated_in).each(|_, rel| {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            unread.push(String::from(rel));
            return;
        };
        let (flat, lines) = flatten(&prose(rel, &text));
        for wording in absence.claimed {
            let mut from = 0_usize;
            while let Some(at) = flat.get(from..).and_then(|rest| rest.find(wording)) {
                let offset = from.saturating_add(at);
                found.push((String::from(rel), lines.get(offset).copied().unwrap_or(1)));
                from = offset.saturating_add(wording.len().max(1));
            }
        }
    });
    // The PAIR, and both halves matter: `Err` is a walk shorter than the subjects it selected,
    // and the recount is `matches_any` called from HERE against the same call inside `matching` -
    // so a narrowing written at either site moves one side alone. Same shape as `pages`' `read`
    // against `offered`, which is this repository's own reference for it.
    let owed = files.iter().filter(|rel| matches_any(absence.stated_in, rel)).count();
    match walked {
        Ok(ref reached) if reached.subjects() == owed => {}
        Ok(ref reached) => short.push(format!(
            "the {} absence's stated_in scan was handed {} of the {owed} file(s) its scope names - \
             a scan over a subset cannot say the absence is unstated",
            absence.name,
            reached.subjects()
        )),
        Err(ref why) => short.push(format!("the {} absence's stated_in scan {}", absence.name, why.describe())),
    }
    found
}

/// What one sighting's scan read, and what it found.
///
/// `files` and `lines` are the halves that matter in a GREEN run: a hit list is empty when the
/// absence holds, when the walk opened nothing, **and when it opened everything and read the first
/// two lines of each**. Neither of them is the FLOOR any more, and that is `#414`: both were
/// derived by iterating the same collection the walk narrows, so a truncation moved the witness
/// with itself. What holds the depth now is [`Sighted::short`].
struct Sighted {
    /// Distinct files that contributed at least one production line.
    files: BTreeSet<String>,
    /// Production lines actually searched. A file `regions::scope` exempts whole contributes none,
    /// which is why this cannot rise while coverage falls.
    lines: usize,
    /// Production lines holding the literal: the file, and the one-based line. A set, because a
    /// single-line literal is found by the code image AND by the literal walk.
    hits: BTreeSet<(String, usize)>,
    /// Files in scope that could not be read.
    unread: Vec<String>,
    /// A walk that did not reach every subject it was offered, worded by
    /// `crate::repo::accounting::Short` or by this module where the second count is its own.
    ///
    /// **This is the arm that makes the numbers above worth printing.** Each entry is a pair of
    /// counts taken on opposite sides of one walk: the witness's own length against the caller's
    /// record of what it decided.
    short: Vec<String>,
}

/// Look for what would refute the absence, in production code only.
///
/// A file in scope that cannot be read goes in [`Sighted::unread`] rather than being skipped: the
/// hit list being empty is the verdict, and a walk that quietly dropped a file would report exactly
/// that. See [`statements`] for why the guard cannot fire on a binary.
///
/// **TWO images of every file, and the second is not an optimisation.** The blanked one gives a line
/// number for a call; [`string_literals`] gives the body of a literal whose interior the blanked one
/// removes when it spans lines. The module header records the measurement that forced it.
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
        files: BTreeSet::new(),
        lines: 0,
        hits: BTreeSet::new(),
        unread: Vec::new(),
        short: Vec::new(),
    };
    let walked = Offered::matching("file", files, sighting.over).each(|_, rel| {
        let (Some(code), Ok(raw)) = (read(rel), std::fs::read_to_string(root.join(rel))) else {
            found.unread.push(String::from(rel));
            return None;
        };
        let tests = regions::scope(rel, &read);
        // The walk INSIDE the file, accounted the same way the walk over files is: one outcome per
        // LINE, so `#414`'s first instance - `.take(100)` here, 214500 of 294744 lines gone at
        // exit 0 - cannot mint a witness. `true` means the line is production and was searched.
        let stepped = Offered::lines("line", &code).each(|number, line| {
            if tests.covers(number) {
                return false;
            }
            if line.contains(sighting.holds) {
                found.hits.insert((String::from(rel), number));
            }
            true
        });
        // The half the blanked image cannot show. A literal is attributed to the line its opening
        // quote sits on, which is the line a reader opens.
        for literal in string_literals(&raw) {
            if !tests.covers(literal.line) && literal.body.contains(sighting.holds) {
                found.hits.insert((String::from(rel), literal.line));
            }
        }
        // SECOND DERIVATION, and it is the reason this is not a tautology: `production` is counted
        // off the witness's payload, `owed` off `code.lines()` at a different call site. A
        // truncation on either side moves one of them, and the closure declining a line it was
        // handed - which no type here can forbid - moves the first alone.
        let production = match stepped {
            Ok(ref lines) => lines.outcomes().iter().filter(|counted| **counted).count(),
            Err(ref why) => {
                found.short.push(format!("{rel}: the line walk {}", why.describe()));
                return None;
            }
        };
        let owed = code
            .lines()
            .enumerate()
            .filter(|(index, _)| !tests.covers(index.saturating_add(1)))
            .count();
        if production != owed {
            found.short.push(format!(
                "{rel}: searched {production} of the {owed} production line(s) this file offers - \
                 a scan that read less than the file cannot report what the rest of it holds"
            ));
            return None;
        }
        if production > 0 {
            found.files.insert(String::from(rel));
        }
        Some(production)
    });
    // The file half of the same pair: `matches_any` from here against the same call inside
    // `matching`. `#414`'s second instance was one `continue` under that filter - 136 of 164 files
    // unscanned, gate exit 0, suite green at 2831 - and the filter is not beside the loop any more,
    // so the narrowing has to be written at one of these two sites and moves one side of this.
    let owed = files.iter().filter(|rel| matches_any(sighting.over, rel)).count();
    match walked {
        Ok(reached) if reached.subjects() != owed => found.short.push(format!(
            "the `{}` sighting was handed {} of the {owed} file(s) its scope names - a scan over a \
             subset reports the absence held without having read the rest",
            sighting.holds,
            reached.subjects()
        )),
        Ok(reached) => {
            // Summed off the WITNESS's payload rather than off a counter the loop incremented, so
            // there is no accumulator a narrowed walk simply stops touching.
            found.lines = reached
                .outcomes()
                .iter()
                .filter_map(|one| *one)
                .fold(0_usize, usize::saturating_add);
            // The caller's own record against the witness's length: a subject the closure declined
            // is neither read nor reported, and this is what makes that a refusal rather than a
            // smaller number. Reported here because only this function knows both.
            let dropped = reached.outcomes().iter().filter(|one| one.is_none()).count();
            if dropped != found.unread.len().saturating_add(found.short.len()) {
                found.short.push(format!(
                    "the `{}` sighting was offered {} file(s), accounted for {} and named {} it \
                     could not read - a file it neither searched nor reported is a subject this \
                     scan dropped in silence",
                    sighting.holds,
                    reached.subjects(),
                    reached.subjects().saturating_sub(dropped),
                    found.unread.len()
                ));
            }
        }
        Err(why) => found
            .short
            .push(format!("the `{}` sighting's file walk {}", sighting.holds, why.describe())),
    }
    found
}

/// What the run actually read, so a green line can say so.
///
/// Three numbers from three different places - statements out of prose, distinct files out of the
/// tree walk, production lines out of the walk INSIDE each file.
///
/// **None of the three is a floor, and that correction is `github.com/telekom/sutura#414`.** All
/// three are derived by iterating what the walks return, so a narrowed walk moves them along with
/// itself: the depth threshold that used to guard them was satisfied by a walk that read a hundred
/// lines per file. They are here to be READ by somebody comparing runs. What refuses a short walk
/// is [`Sighted::short`], a pair of counts taken on opposite sides of it.
pub(super) struct Reading {
    /// Statements of an absence found.
    pub(super) stated: usize,
    /// Distinct files a sighting opened and read at least one production line of. Distinct rather
    /// than summed: the old number counted one file once per entry, so it grew with the table.
    pub(super) files: usize,
    /// Production lines searched, across every sighting.
    pub(super) lines: usize,
}

/// The absences, held against the tree.
///
/// Takes the table rather than reading the const, so the fixtures in `tests` exercise the code the
/// gate runs and not a re-implementation of it - `remedies_hold`'s reason, and the one that makes
/// the empty-table, empty-set and truncated-walk cases provable at all.
fn absences_hold(root: &Path, files: &[String], table: &[Absence]) -> (Vec<String>, Reading) {
    let mut problems = Vec::new();
    let mut stated_total = 0_usize;
    let mut scanned: BTreeSet<String> = BTreeSet::new();
    let mut lines = 0_usize;
    if table.is_empty() {
        // FAIL CLOSED on the table itself. Every number below is derived by iterating it, so an
        // empty table makes them agree at zero and the verdict reads like a clean tree - measured,
        // with 1023 tests green. A check with no subject is not a check.
        problems.push(String::from(
            "ABSENCES is empty, so this check reads nothing and every number it prints is zero - \
             a gate with no subject, not a tree with no defect",
        ));
    }
    for absence in table {
        let mut unread = Vec::new();
        let stated = statements(root, files, absence, &mut unread, &mut problems);
        stated_total = stated_total.saturating_add(stated.len());
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
            scanned.extend(found.files.iter().cloned());
            lines = lines.saturating_add(found.lines);
            // FIRST, because every number below it is about a subset while one of these stands.
            // This is the arm `#414` asked for: a walk shorter than the set it was offered, worded
            // from two counts taken on opposite sides of it rather than from a floor the
            // truncation moves too.
            problems.extend(found.short.iter().cloned());
            for rel in &found.unread {
                problems.push(format!(
                    "{rel}: in the {} sighting's scope and could not be read - the scan is short by \
                     a file, so an empty hit list is not an absence",
                    absence.name
                ));
            }
            if found.files.is_empty() {
                problems.push(format!(
                    "the {} sighting (`{}` under {:?}) read no production line - the scan is over an \
                     empty set, so it reports the absence held without having read anything",
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
    (
        problems,
        Reading {
            stated: stated_total,
            files: scanned.len(),
            lines,
        },
    )
}

/// The entry point `guidance::tree_problems` wires in.
pub(super) fn absence_problems(root: &Path, files: &[String]) -> (Vec<String>, Reading) {
    absences_hold(root, files, ABSENCES)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Absence, Sighting, absence_problems, absences_hold, prose};

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
        // THREE numbers, because each alone is what some green-over-nothing prints: one statement
        // read out of prose, one file opened out of the tree, and the lines actually searched.
        assert_eq!(reading.stated, 1);
        assert_eq!(reading.files, 1);
        assert_eq!(reading.lines, 3, "every line of the fixture is production");
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
    fn a_refutation_far_down_a_file_is_found_and_the_line_floor_moves_with_it() {
        // THE TRUNCATED-WALK CASE, per file rather than per tree. `.enumerate().take(2)` on the line
        // walk left every other fixture green - each plants its call on line 1 or 2 - and neither a
        // file count nor a statement count moved. Both halves are asserted here: the hit is found,
        // and `lines` is the number that a truncation would drop.
        let mut body = String::from("/// no consumer today\nfn head() {}\n");
        for _ in 0..40 {
            body.push_str("// filler\n");
        }
        body.push_str("fn read(a: &A) -> u8 {\n    a.widget()\n}\n");
        let (dir, files) = tree("deep", &[("crates/a/src/lib.rs", body.as_str())]);
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("crates/a/src/lib.rs:44:"), "{}", problems[0]);
        assert_eq!(reading.files, 1);
        assert!(
            reading.lines > 40,
            "the line floor has to see the whole file, not its first lines: {}",
            reading.lines
        );
    }

    #[test]
    fn a_needle_inside_a_line_continued_string_literal_is_found() {
        // `#370`'s own headline scenario, in the shape this tree actually writes. `code_lines`
        // blanks the interior of a string that SPANS LINES, so the blanked image alone passed this
        // at exit 0 while the single-line form failed - the difference being one backslash.
        let (dir, files) = tree(
            "continued",
            &[
                ("crates/a/src/lib.rs", "/// no consumer today\npub fn widget() {}\n"),
                (
                    "crates/b/src/lib.rs",
                    "fn challenge() -> String {\n    format!(\n        \"Bearer realm, \\\n         a.widget()={}\",\n        realm\n    )\n}\n",
                ),
            ],
        );
        let (problems, _) = absences_hold(&dir, &files, &[ENTRY]);
        assert_eq!(problems.len(), 1, "a continued literal is still code: {problems:?}");
        assert!(problems[0].starts_with("crates/b/src/lib.rs:3:"), "{}", problems[0]);
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
        assert_eq!(reading.files, 1);
    }

    #[test]
    fn an_empty_table_is_red_rather_than_a_clean_tree() {
        // MEASURED: `const ABSENCES: &[Absence] = &[];` left 1023 tests green and the gate at exit 0
        // printing `0 statement(s) over 0 file(s)`. Both numbers are derived by iterating the table,
        // so they are two counters from one source and cannot witness each other.
        let (dir, files) = tree("emptytable", &[("crates/a/src/lib.rs", "pub fn widget() {}\n")]);
        let (problems, reading) = absences_hold(&dir, &files, &[]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("ABSENCES is empty"), "{}", problems[0]);
        assert_eq!(reading.stated, 0);
        assert_eq!(reading.files, 0);
        assert_eq!(reading.lines, 0);
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
        assert!(problems[0].contains("read no production line"), "{}", problems[0]);
        assert_eq!(reading.files, 0);
        assert_eq!(reading.lines, 0);
    }

    #[test]
    fn a_file_regions_exempts_whole_raises_no_floor() {
        // The corollary review measured: `regions::is_dedicated_test_target` exempts a path holding
        // `/tests/` on the SUBSTRING alone, so `crates/*/src/**/tests/*.rs` is invisible to a
        // sighting. A per-FILE floor rose 446 -> 448 over such a file while coverage went down; a
        // production-LINE floor cannot, because the file contributes none.
        let (dir, files) = tree(
            "exempt",
            &[
                ("crates/a/src/lib.rs", "/// no consumer today\npub fn widget() {}\n"),
                ("crates/a/src/pool/tests/planted.rs", "fn t(a: &A) { a.widget(); }\n"),
            ],
        );
        let (problems, reading) = absences_hold(&dir, &files, &[ENTRY]);
        assert!(problems.is_empty(), "test code refutes nothing: {problems:?}");
        assert_eq!(reading.files, 1, "the exempted file raises no file floor either");
        assert_eq!(reading.lines, 2, "and contributes no production line");
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
        assert_eq!(reading.files, 1);
        assert!(reading.lines < 6, "the test region is not searched: {}", reading.lines);
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
        assert!(problems[0].contains("read no production line"), "{}", problems[0]);
        assert_eq!(reading.files, 0);
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
        assert_eq!(reading.files, 0, "an unreadable file is not a file scanned");
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

    #[test]
    fn the_real_workspace_is_what_the_floor_is_about() {
        // THE LIVE TABLE, run against the real tree. Before this, `grep -rn ABSENCES` found three
        // hits all inside this file: every fixture above passes a table built in the test, so the
        // three entries the gate actually ships were exercised by nothing. Every sibling in this
        // gate has this test - `constants.rs`, `advice.rs`, `claims.rs`, `guidance.rs` - and each
        // states its floor rather than only its emptiness.
        let (root, files) = crate::repo::all_files()
            .and_then(|census| census.into_listing(crate::repo::Unmigrated::Guidance))
            .expect("could not locate the repo");
        assert!(!super::ABSENCES.is_empty(), "the shipped table is what this is about");
        let (problems, reading) = absence_problems(&root, &files);
        assert!(problems.is_empty(), "{problems:#?}");
        // The numbers are still asserted non-zero - a scan that opened nothing is a scan whose
        // empty hit list means nothing - but the DEPTH floor that used to sit here is gone, and
        // deleting it is the point of `github.com/telekom/sutura#414`. It read
        // `lines > files * 4`, which is 656 against a real 294744: measured on `036bce03`,
        // `.take(100)` on the per-file line walk satisfied it at `just hygiene` exit 0 with 214500
        // of 294744 lines unread and `just test` green at 2831 passed. Both sides came off the
        // collection the loop narrows, so the floor moved with the mutation. What holds the depth
        // now is `problems`, one line above: every walk here is `crate::repo::Offered::each`, whose
        // payload is one outcome per subject, paired against a count this module takes itself - so
        // a truncated walk is a REFUSAL rather than a smaller number that clears a threshold.
        assert!(reading.stated > 0, "no page states any registered absence");
        assert!(reading.files > 0, "no sighting opened a file");
        assert!(reading.lines > 0, "no sighting searched a production line");
    }
}
