//! The DELTA half of the CRAP gate: two reports in, a verdict about the CHANGE out.
//!
//! `crap.rs` answers "is anything over the line". This answers "did this branch make anything
//! worse", which is the question an absolute threshold structurally cannot ask. Both run. The
//! delta is IN ADDITION to the number in `.cargo-crap.toml`, never instead of it: a threshold
//! is the only rule that still holds when there is no baseline to compare against, and there
//! is no baseline on a first run, on a fork, or on any commit whose base never got a CI run.
//!
//! WHY THIS LIVES IN CI AND NOT IN THE FLAKE CHECK, because `docs/crap.md` used to argue there
//! could be no baseline at all and that argument was right about the sandbox and wrong about
//! the conclusion. A Nix build sandbox has no network and no `.git`, so it cannot FETCH a
//! baseline - true, and unchanged. What follows is only that the FETCH cannot be sandboxed, not
//! that the comparison cannot happen: the sandboxed check emits its report as a file, CI
//! downloads the base commit's report from an artifact, and this module compares two files. The
//! sandbox still computes the scores; nothing about the absolute gate moves or weakens.
//!
//! NO SECOND COVERAGE RUN, and that is the constraint the whole shape follows from. Coverage is
//! a separate compiler profile - `-C instrument-coverage` shares nothing with the artifacts the
//! test and clippy checks cache - so a delta that re-measured anything would double the one
//! genuinely expensive part of the gate. It re-measures nothing. The head numbers are the ones
//! the `crap` check already computed, carried out of the sandbox in `$out`. The base numbers
//! were computed when the base commit was built and have been sitting in an artifact since.
//! Two file reads and a join.
//!
//! WHY THE JOIN IS HERE AND NOT `cargo crap --baseline`. The tool has that flag, and using it
//! would have been the smaller diff. It re-ANALYSES to apply it: `--baseline` is a modifier on
//! a fresh run, so it needs the source tree and the LCOV in the same place at the same time.
//! Neither is true here - the LCOV stayed in the sandbox, and its `SF:` records name sandbox
//! paths that no longer exist. Handing it a rewritten LCOV to work around that would mean
//! normalising two formats instead of one and re-running the analysis to learn what the report
//! already says. So the tool keeps every judgement it is the authority on - complexity,
//! coverage, the CRAP score - and this module does the one thing left: pair up two lists of
//! numbers the tool produced. A join is not a second implementation of the metric.
//!
//! PATHS ARE THE PART THAT BITES, and it was measured rather than feared. `cargo crap` reports
//! ABSOLUTE paths, and the absolute path of this repo inside a Nix sandbox is derived from the
//! source store path: with a dirty tree the derivation's `src` is
//! `/nix/store/<hash>-<tree-hash>-source`, and the build directory follows it. Two trees, two
//! roots. Handed a baseline whose roots do not match, `cargo crap` did NOT fail - it reported
//! 181 unchanged, 4 new and 4 removed for a tree with no changes at all, because its fallback
//! matching gets most of the way and then silently loses the functions that share a name inside
//! one file (`Real::fmt` appears twice in `sutura-domain`, as `Display` and `LowerExp`). A gate that
//! invents four new functions is worse than one that fails. So the baseline this repo writes is
//! PORTABLE: every `file` is repo-relative, and the file says so in a `paths` key that
//! [`read_portable`] refuses to compare without. A stale absolute baseline is then a message
//! rather than four phantom rows.

// The ONE lint escape in this repository, and it is file-scoped rather than a workspace
// override for exactly that reason.
//
// `clippy::float_arithmetic` comes from the `restriction` category, which this workspace enables
// wholesale - see the reasoning beside `arithmetic_side_effects` in Cargo.toml, which is allowed
// there on a strictly stronger argument. No other first-party file does float arithmetic at all,
// which is why the lint has never been hit before now: `sutura-domain` models calendars and
// identifiers, not measurements.
//
// This module cannot avoid it. A CRAP score is a real number - complexity weighted by a coverage
// FRACTION - and the whole question here is "is this one larger than that one, and by how much".
// A difference and a sum are the operations. The alternatives were considered and are worse: a
// fixed-point representation would need a second copy of every score beside the `f64` the tool
// reported, and two representations of one number is the drift this file argues against
// elsewhere; parsing the decimal text of each JSON number into integer milli-units would put a
// hand-written decimal parser between the tool and the verdict.
//
// `expect` and not `allow`, so a future clippy that stops flagging this fails the build rather
// than leaving a stale escape behind. Float EQUALITY is a separate lint and is not suppressed:
// nothing here compares scores with `==`, and `epsilon` is what stands in for it.
#![expect(clippy::float_arithmetic, reason = "a CRAP delta is a difference of two measured reals")]

use std::collections::BTreeMap;
use std::path::Path;

use super::report::{Entry, Report, read_report};

/// One `(file, function)` pairing key.
type Key<'a> = (&'a str, &'a str);
/// Entries grouped under [`Key`], each group in line order.
type Groups<'a> = BTreeMap<Key<'a>, Vec<&'a Entry>>;
/// One head entry and the baseline entry claimed for it, if any.
type Paired<'a> = (&'a Entry, Option<&'a Entry>);
/// One group paired up, plus the baseline entries nothing claimed.
type GroupPairing<'a> = (Vec<Paired<'a>>, Vec<&'a Entry>);

/// Score changes at or below this count as unchanged.
///
/// `cargo-crap`'s own default for the same purpose, and deliberately the same number: the tool
/// still computes the scores, so a different tolerance here would mean the dev shell running
/// `cargo crap --baseline` by hand and this gate disagreeing about which functions moved.
/// `.cargo-crap.toml` may state `epsilon` and this is the fallback when it does not.
pub(crate) const DEFAULT_EPSILON: f64 = 0.01;

/// The key a portable baseline carries, and the value it must carry.
///
/// Not decoration. Without it, a baseline written before this module existed - or by a
/// `cargo crap --format json` run somebody did by hand - would compare cleanly against nothing
/// and report every function as new. See the module header for the measured version of that.
const PATHS_KEY: &str = "paths";
/// What [`PATHS_KEY`] must say.
const PATHS_VALUE: &str = "repo-relative";

/// The first line of the rendered comment, so a second run finds and replaces its own comment
/// rather than adding one per push.
pub(crate) const COMMENT_MARKER: &str = "<!-- sutura-crap-delta -->";

/// How many rows any one table in the rendered comment may carry.
///
/// A cap and not a paginator: a delta with more rows than this is not a review comment, it is a
/// refactor, and the count in the header is the number that matters then. The full report is in
/// the job log either way.
const ROW_CAP: usize = 25;

// ------------------------------------------------------------- the portable baseline ---

/// Rewrite a report's absolute `file` paths to repo-relative and stamp it portable.
///
/// Called on EVERY `cargo xtask crap` run, not only in CI, so the file a developer looks at is
/// the same shape as the one an artifact holds. The stamp goes on last: a half-rewritten
/// baseline is refused below rather than written, because the failure it would cause - functions
/// read as new because their path did not match - looks like a finding rather than like a broken
/// file.
pub(crate) fn portable(report_json: &str, root: &Path) -> Result<String, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(report_json).map_err(|error| format!("the report is not valid JSON: {error}"))?;
    let mut prefix = root.display().to_string();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }

    let object = value
        .as_object_mut()
        .ok_or_else(|| String::from("the report is not a JSON object"))?;
    let entries = object
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| String::from("the report has no `entries` array"))?;
    if entries.is_empty() {
        return Err(String::from(
            "the report analysed no functions, so there is nothing to make a baseline of",
        ));
    }

    let total = entries.len();
    let mut stubborn: Vec<String> = Vec::new();
    for entry in entries.iter_mut() {
        let Some(file) = entry.get_mut("file") else {
            continue;
        };
        let Some(text) = file.as_str().map(String::from) else {
            continue;
        };
        if let Some(relative) = text.strip_prefix(prefix.as_str()) {
            *file = serde_json::Value::String(String::from(relative));
        } else if text.starts_with('/') {
            stubborn.push(text);
        }
    }
    if let Some(first) = stubborn.first() {
        return Err(format!(
            "{} of {total} entries have a path outside the repo root {}, the first being {first}.\n  \
             Refused rather than written: a baseline that is absolute in part compares as `new` \
             for exactly those functions.",
            stubborn.len(),
            root.display()
        ));
    }

    object.insert(String::from(PATHS_KEY), serde_json::Value::String(String::from(PATHS_VALUE)));
    serde_json::to_string_pretty(&value).map_err(|error| format!("could not serialise the baseline: {error}"))
}

/// Read a portable baseline, refusing one that is not portable.
///
/// The `expected` check is the same one [`read_report`] makes for the head report, and it matters
/// more here: a baseline written when the scope was narrower would otherwise compare a widened
/// scope's whole new crate as `new` - true, but saying nothing - and would hide that the baseline
/// is the wrong shape.
pub(crate) fn read_portable(json: &str, expected: &[&str]) -> Result<Vec<Entry>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("the baseline is not valid JSON: {error}"))?;
    if value.get(PATHS_KEY).and_then(serde_json::Value::as_str) != Some(PATHS_VALUE) {
        return Err(format!(
            "the baseline does not declare `\"{PATHS_KEY}\": \"{PATHS_VALUE}\"`.\n  Refused rather \
             than compared: its `file` values are then whatever absolute paths the machine that \
             wrote it happened to have, and a root that does not match reports functions as new \
             instead of failing."
        ));
    }
    match read_report(json, expected) {
        Report::Scored(entries) => Ok(entries),
        Report::Empty(reason) => Err(format!("the baseline is not a usable report: {reason}")),
    }
}

// ------------------------------------------------------------- the join ---

/// What happened to one function between the two reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    /// Same score, same line.
    Unchanged,
    /// Same score, different line. Named rather than folded into `Unchanged` so a reviewer can
    /// see that the pairing was a judgement and not an identity.
    Moved,
    /// Score up.
    Regressed,
    /// Score down.
    Improved,
    /// No counterpart in the baseline.
    New,
}

impl Status {
    /// The word a report line uses. ASCII, because the workspace denies `non_ascii_literal` and
    /// an arrow glyph in a gate's output is invisible in a diff besides.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Unchanged => "same",
            Self::Moved => "moved",
            Self::Regressed => "worse",
            Self::Improved => "better",
            Self::New => "new",
        }
    }
}

/// One head function, and what the baseline said about it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Change {
    /// Repo-relative path.
    pub(crate) file: String,
    /// Function path, for example `Grain::as_str`.
    pub(crate) function: String,
    /// Line in the head tree.
    pub(crate) line: u64,
    /// Cyclomatic complexity now.
    pub(crate) cyclomatic: f64,
    /// Line coverage now, as a percentage.
    pub(crate) coverage: f64,
    /// CRAP score now.
    pub(crate) crap: f64,
    /// CRAP score in the baseline, absent for a new function.
    pub(crate) baseline_crap: Option<f64>,
    /// What the pairing concluded.
    pub(crate) status: Status,
}

impl Change {
    /// How much the score rose. Zero for a new function, because a function with no counterpart
    /// has not got worse - it is new code, which the absolute threshold judges on its own terms.
    pub(crate) fn delta(&self) -> f64 {
        self.baseline_crap.map_or(0.0, |before| self.crap - before)
    }
}

/// The two sides of the comparison, shaped the way `cargo crap --format json --baseline` shapes
/// its own output: one record per head function, plus the baseline records nothing claimed.
#[derive(Debug, PartialEq)]
pub(crate) struct Comparison {
    /// One per head entry.
    pub(crate) changes: Vec<Change>,
    /// Baseline entries with no head counterpart. Reported, never a failure: deleting a crappy
    /// function is the outcome this gate exists to encourage.
    pub(crate) removed: Vec<Entry>,
}

/// Pair the two reports up.
///
/// Keyed on `(file, function)` and NOT on the line, because a line number changes when anything
/// above it does and pairing on it would report an untouched function as one `new` plus one
/// `removed` on every edit. The line is used to disambiguate INSIDE a key, which is needed:
/// `Real::fmt` appears twice in one file in `sutura-domain` today - `Display` and `LowerExp` on the
/// same type. `Secret::fmt` was the second example until `docs/adr/0020` deleted both of that type's
/// hand-written formatters; the case is down to one instance, not gone, so the disambiguation stays.
pub(crate) fn compare(baseline: &[Entry], head: &[Entry], epsilon: f64) -> Comparison {
    let mut base_groups = grouped(baseline);
    let mut changes: Vec<Change> = Vec::with_capacity(head.len());
    let mut removed: Vec<Entry> = Vec::new();

    for (key, heads) in grouped(head) {
        let bases = base_groups.remove(&key).unwrap_or_default();
        let (paired, leftover) = pair_group(&heads, &bases);
        for (entry, before) in paired {
            changes.push(change_of(entry, before, epsilon));
        }
        removed.extend(leftover.into_iter().cloned());
    }
    // Whole keys the head does not have at all: a deleted file, a renamed function.
    for orphans in base_groups.into_values() {
        removed.extend(orphans.into_iter().cloned());
    }

    changes.sort_by(|a, b| b.delta().total_cmp(&a.delta()).then_with(|| b.crap.total_cmp(&a.crap)));
    removed.sort_by(|a, b| b.crap.total_cmp(&a.crap));
    Comparison { changes, removed }
}

/// Entries by `(file, function)`, each group in line order.
fn grouped<'a>(entries: &'a [Entry]) -> Groups<'a> {
    let mut groups: Groups<'a> = BTreeMap::new();
    for entry in entries {
        groups
            .entry((entry.file.as_str(), entry.function.as_str()))
            .or_default()
            .push(entry);
    }
    for group in groups.values_mut() {
        group.sort_by_key(|entry| entry.line);
    }
    groups
}

/// Pair one `(file, function)` group, and say which baseline entries nothing claimed.
///
/// TWO PASSES, and the order is the whole correctness argument. An unchanged function keeps its
/// line, so an equal-line match is certainly right; taking every one of those FIRST is what
/// stops two same-named functions in one file from being paired with each other's counterpart
/// and reported as two regressions that never happened. Only then does the remainder pair up in
/// line order, which is a guess - and it is labelled `moved` when it is.
fn pair_group<'a>(heads: &[&'a Entry], bases: &[&'a Entry]) -> GroupPairing<'a> {
    let mut pool: Vec<Option<&'a Entry>> = bases.iter().copied().map(Some).collect();
    let mut paired: Vec<Paired<'a>> = Vec::with_capacity(heads.len());

    for head in heads {
        let claimed = pool
            .iter_mut()
            .find(|slot| slot.is_some_and(|base| base.line == head.line))
            .and_then(Option::take);
        paired.push((*head, claimed));
    }
    for slot in &mut paired {
        if slot.1.is_none() {
            slot.1 = pool.iter_mut().find(|candidate| candidate.is_some()).and_then(Option::take);
        }
    }
    (paired, pool.into_iter().flatten().collect())
}

/// One paired function's record.
fn change_of(head: &Entry, before: Option<&Entry>, epsilon: f64) -> Change {
    Change {
        file: head.file.clone(),
        function: head.function.clone(),
        line: head.line,
        cyclomatic: head.cyclomatic,
        coverage: head.coverage,
        crap: head.crap,
        baseline_crap: before.map(|entry| entry.crap),
        status: status_of(head, before, epsilon),
    }
}

/// What to call the pairing.
fn status_of(head: &Entry, before: Option<&Entry>, epsilon: f64) -> Status {
    let Some(before) = before else {
        return Status::New;
    };
    let delta = head.crap - before.crap;
    if delta.abs() <= epsilon {
        if head.line == before.line {
            Status::Unchanged
        } else {
            Status::Moved
        }
    } else if delta > 0.0 {
        Status::Regressed
    } else {
        Status::Improved
    }
}

// ------------------------------------------------------------- the ratchet ---

/// The verdict material: which rules a comparison broke, and the counts a reader wants.
#[derive(Debug)]
pub(crate) struct Ratchet<'a> {
    /// Functions this branch pushed from at-or-under the line to over it.
    pub(crate) crossed: Vec<&'a Change>,
    /// Functions already over the line that got worse.
    pub(crate) worse_over_line: Vec<&'a Change>,
    /// Every function whose score rose, worst first. Includes the two lists above.
    pub(crate) regressions: Vec<&'a Change>,
    /// CRAP points added to functions that existed before. What the third rule spends.
    pub(crate) budget_used: f64,
    /// How many scores fell.
    pub(crate) improvements: usize,
    /// How many functions the head has that the baseline did not.
    pub(crate) added: usize,
}

/// Apply the three delta rules.
///
/// THE THREE, and what each one is for. `threshold` is the same number the absolute gate uses;
/// this introduces no second knob, because a second number is a second thing to defend.
///
///   1. CROSSED THE LINE - `baseline <= threshold < head`. A function this branch pushed over.
///   2. WORSE WHILE OVER THE LINE - `head > threshold` and the score rose.
///   3. THE BUDGET - the CRAP points added to PRE-EXISTING functions, summed, may not exceed
///      `threshold`. One branch may not add as much rot to code that already existed as a whole
///      new over-the-line function is worth.
///
/// Rules 1 and 2 are SUBSUMED by the absolute gate at today's policy: nothing may be over 30 at
/// all, so nothing can cross the line or worsen while over it without the absolute rule failing
/// first. They are written anyway, and not as decoration. They are the two rules that survive
/// the only change that would let this gate cover more than one crate: a scope widened over code
/// whose inherited debt is already above the line - which is the state `sutura-semantic` is
/// measurably in, seven functions at CRAP 210 because its tests live in another crate. In that
/// world the absolute rule has to be relaxed for the inherited set, and these two are what
/// replaces it. Rule 3 is the one that blocks something today.
///
/// WHAT DELIBERATELY DOES NOT FAIL: a single sub-threshold regression. Not leniency - the metric
/// makes it unfixable. At full coverage CRAP equals CC, so adding one covered branch to a
/// fully-tested function moves it from 5 to 6 permanently, and no test can bring it back. A rule
/// that failed it could only be escaped by an allowlist entry, which `.cargo-crap.toml` forbids
/// for exactly this case, so it would be bypassed or deleted rather than obeyed. It is REPORTED
/// instead, which is what "a regression under the line should still be visible" asks for, and
/// rule 3 is what stops a hundred of them from adding up unnoticed.
pub(crate) fn ratchet(comparison: &Comparison, threshold: f64) -> Ratchet<'_> {
    let mut crossed = Vec::new();
    let mut worse_over_line = Vec::new();
    let mut regressions = Vec::new();
    let mut budget_used = 0.0_f64;
    let mut improvements = 0_usize;
    let mut added = 0_usize;

    for change in &comparison.changes {
        match change.status {
            Status::New => added = added.saturating_add(1),
            Status::Improved => improvements = improvements.saturating_add(1),
            Status::Regressed => {
                regressions.push(change);
                budget_used += change.delta();
                if change.crap > threshold {
                    if change.baseline_crap.is_some_and(|before| before > threshold) {
                        worse_over_line.push(change);
                    } else {
                        crossed.push(change);
                    }
                }
            }
            Status::Unchanged | Status::Moved => {}
        }
    }
    Ratchet {
        crossed,
        worse_over_line,
        regressions,
        budget_used,
        improvements,
        added,
    }
}

impl Ratchet<'_> {
    /// Did any of the three rules fail?
    pub(crate) fn broken(&self, threshold: f64) -> bool {
        !self.crossed.is_empty() || !self.worse_over_line.is_empty() || self.budget_used > threshold
    }

    /// The rules that failed, as the sentences a report prints.
    pub(crate) fn reasons(&self, threshold: f64) -> Vec<String> {
        let mut reasons = Vec::new();
        if !self.crossed.is_empty() {
            reasons.push(format!(
                "{} function(s) crossed CRAP {threshold} on this branch",
                self.crossed.len()
            ));
        }
        if !self.worse_over_line.is_empty() {
            reasons.push(format!(
                "{} function(s) already over CRAP {threshold} got worse",
                self.worse_over_line.len()
            ));
        }
        if self.budget_used > threshold {
            reasons.push(format!(
                "{:.1} CRAP points added to pre-existing functions, over the {threshold}-point budget",
                self.budget_used
            ));
        }
        reasons
    }
}

// ------------------------------------------------------------- rendering ---

/// The table header both renderers use, so the log and the comment cannot describe different
/// columns.
const COLUMNS: &str = "| CRAP | was | delta | CC | cov | status | function | location |";
/// GitHub-flavoured alignment row for [`COLUMNS`].
const ALIGN: &str = "| ---: | ---: | ---: | ---: | ---: | --- | --- | --- |";

/// One markdown table row.
fn row(change: &Change) -> String {
    let was = change
        .baseline_crap
        .map_or_else(|| String::from("-"), |before| format!("{before:.1}"));
    let delta = change.baseline_crap.map_or_else(
        || String::from("-"),
        |_| {
            let value = change.delta();
            format!("{}{value:.1}", if value > 0.0 { "+" } else { "" })
        },
    );
    format!(
        "| {:.1} | {was} | {delta} | {:.0} | {:.1}% | {} | `{}` | `{}:{}` |\n",
        change.crap,
        change.cyclomatic,
        change.coverage,
        change.status.label(),
        change.function,
        change.file,
        change.line
    )
}

/// The whole comment, markdown, ready to POST.
///
/// One table and a headline. NOT the tool's own `pr-comment` format, which is prettier: it uses
/// glyphs this workspace bans in literals, and producing it would need a second analysis run -
/// see the module header for why there is not one.
pub(crate) fn comment(comparison: &Comparison, threshold: f64, scope: &str, base: &str) -> String {
    let verdict = ratchet(comparison, threshold);
    let mut out = String::from(COMMENT_MARKER);
    out.push_str("\n## CRAP delta\n\n");

    let intro = format!("Scope `{scope}`, absolute threshold CRAP {threshold}, baseline from the merge base `{base}`.\n\n");
    out.push_str(&intro);

    if verdict.broken(threshold) {
        out.push_str("**The delta ratchet FAILED.**\n\n");
        for reason in verdict.reasons(threshold) {
            let line = format!("- {reason}\n");
            out.push_str(&line);
        }
        out.push('\n');
    } else if verdict.regressions.is_empty() {
        out.push_str("No function got worse.\n\n");
    } else {
        out.push_str("Nothing failed. The scores below rose without crossing the line or the budget.\n\n");
    }

    let counts = format!(
        "{} worse, {} better, {} new, {} removed, {:.1} CRAP point(s) added to pre-existing \
         functions (budget {threshold}).\n\n",
        verdict.regressions.len(),
        verdict.improvements,
        verdict.added,
        comparison.removed.len(),
        verdict.budget_used
    );
    out.push_str(&counts);

    out.push_str(&table(&verdict.regressions));
    out
}

/// The rows, capped, or nothing at all when no score rose.
fn table(regressions: &[&Change]) -> String {
    if regressions.is_empty() {
        return String::new();
    }
    let mut out = String::from(COLUMNS);
    out.push('\n');
    out.push_str(ALIGN);
    out.push('\n');
    for change in regressions.iter().take(ROW_CAP) {
        out.push_str(&row(change));
    }
    if let Some(hidden) = regressions.len().checked_sub(ROW_CAP).filter(|count| *count > 0) {
        let note = format!("\n{hidden} further regression(s) not listed; the job log has all of them.\n");
        out.push_str(&note);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Comparison, DEFAULT_EPSILON, Status, comment, compare, portable, ratchet, read_portable};
    use crate::crap::report::Entry;
    use std::path::Path;

    fn entry(file: &str, function: &str, line: u64, crap: f64) -> Entry {
        Entry {
            krate: String::from("sutura-domain"),
            file: String::from(file),
            function: String::from(function),
            line,
            cyclomatic: 6.0,
            coverage: 0.0,
            crap,
        }
    }

    fn statuses(comparison: &Comparison) -> Vec<(&str, Status)> {
        comparison
            .changes
            .iter()
            .map(|change| (change.function.as_str(), change.status))
            .collect()
    }

    // ---------------------------------------------------------- portability ---

    #[test]
    fn a_report_is_made_repo_relative_and_stamped() {
        let json = r#"{"entries":[{"crate":"sutura-domain","file":"/build/x-source/crates/sutura-domain/src/model.rs",
            "function":"f","line":1,"cyclomatic":1.0,"coverage":100.0,"crap":1.0}]}"#;
        let made = portable(json, Path::new("/build/x-source")).expect("rewrites");
        assert!(made.contains("\"crates/sutura-domain/src/model.rs\""), "{made}");
        assert!(!made.contains("/build/x-source"), "no absolute path may survive: {made}");
        // The stamp, which is what read_portable insists on.
        assert!(made.contains("\"paths\": \"repo-relative\""), "{made}");
        // And it round-trips.
        let entries = read_portable(&made, &["sutura-domain"]).expect("reads back");
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries.first().map(|e| e.file.as_str()),
            Some("crates/sutura-domain/src/model.rs")
        );
    }

    #[test]
    fn a_path_outside_the_root_is_refused_rather_than_half_rewritten() {
        // THE MEASURED FAILURE: with mismatched roots `cargo crap` reported 4 new and 4 removed
        // for an unchanged tree instead of failing. A baseline that is absolute in part must
        // therefore never be written at all.
        let json = r#"{"entries":[{"crate":"sutura-domain","file":"/somewhere/else/src/model.rs",
            "function":"f","line":1,"cyclomatic":1.0,"coverage":100.0,"crap":1.0}]}"#;
        let error = portable(json, Path::new("/build/source")).expect_err("must be refused");
        assert!(error.contains("/somewhere/else/src/model.rs"), "{error}");
    }

    #[test]
    fn an_unstamped_baseline_is_refused_rather_than_compared() {
        // Exactly the shape `cargo crap --format json` writes by hand, and the shape a baseline
        // from before this gate existed would have.
        let raw = r#"{"paths":"absolute","entries":[{"crate":"sutura-domain","file":"/a/b.rs",
            "function":"f","line":1,"cyclomatic":1.0,"coverage":100.0,"crap":1.0}]}"#;
        let error = read_portable(raw, &["sutura-domain"]).expect_err("must be refused");
        assert!(error.contains("repo-relative"), "{error}");
        let missing = r#"{"entries":[{"crate":"sutura-domain","file":"a.rs","function":"f","line":1,
            "cyclomatic":1.0,"coverage":100.0,"crap":1.0}]}"#;
        read_portable(missing, &["sutura-domain"]).expect_err("a baseline with no `paths` key must be refused");
    }

    #[test]
    fn an_empty_or_broken_baseline_is_refused() {
        // The same refusals the head report gets, reached through the baseline door.
        for raw in [
            "not json",
            r#"{"paths":"repo-relative"}"#,
            r#"{"paths":"repo-relative","entries":[]}"#,
        ] {
            read_portable(raw, &["sutura-domain"]).expect_err(raw);
        }
        portable(r#"{"entries":[]}"#, Path::new("/x")).expect_err("an empty report is not a baseline");
    }

    // ---------------------------------------------------------- the join ---

    #[test]
    fn an_untouched_function_reads_as_unchanged_even_when_its_line_moved() {
        let before = [entry("a.rs", "f", 10, 5.0)];
        let after = [entry("a.rs", "f", 40, 5.0)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        // Pairing on the line would have said one `new` and one `removed` for a function nobody
        // touched, which is the noise that gets a delta gate switched off.
        assert_eq!(statuses(&comparison), vec![("f", Status::Moved)]);
        assert!(
            comparison.removed.is_empty(),
            "no function reads as removed when only its line moved"
        );
    }

    #[test]
    fn two_functions_of_the_same_name_in_one_file_pair_with_their_own_counterparts() {
        // MEASURED, not hypothetical: `Real::fmt` appears twice in `sutura-domain` today, as
        // `Display` and `LowerExp` on one type. cargo-crap's own fallback matching lost exactly
        // this when the paths did not line up. `Secret::fmt` was the second measured instance
        // until `docs/adr/0020` removed that type's two hand-written formatters, which is why the
        // fixture is named after the one that is still there rather than after the one that was.
        let before = [entry("i.rs", "Real::fmt", 10, 2.0), entry("i.rs", "Real::fmt", 90, 8.0)];
        let after = [entry("i.rs", "Real::fmt", 90, 8.0), entry("i.rs", "Real::fmt", 10, 2.0)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        assert_eq!(
            statuses(&comparison),
            vec![("Real::fmt", Status::Unchanged), ("Real::fmt", Status::Unchanged)]
        );
        assert!(comparison.removed.is_empty(), "no same-named function reads as removed");
    }

    #[test]
    fn a_score_that_rose_fell_or_appeared_is_named_as_such() {
        let before = [
            entry("a.rs", "up", 1, 5.0),
            entry("a.rs", "down", 2, 40.0),
            entry("a.rs", "gone", 3, 9.0),
        ];
        let after = [
            entry("a.rs", "up", 1, 9.0),
            entry("a.rs", "down", 2, 3.0),
            entry("a.rs", "fresh", 4, 1.0),
        ];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let mut found = statuses(&comparison);
        found.sort_by_key(|pair| pair.0);
        assert_eq!(
            found,
            vec![("down", Status::Improved), ("fresh", Status::New), ("up", Status::Regressed)]
        );
        assert_eq!(comparison.removed.len(), 1);
        assert_eq!(comparison.removed.first().map(|e| e.function.as_str()), Some("gone"));
    }

    #[test]
    fn a_change_inside_the_tolerance_is_not_a_regression() {
        // Same tolerance cargo-crap uses, so running the tool by hand agrees with the gate.
        let before = [entry("a.rs", "f", 1, 5.0)];
        let after = [entry("a.rs", "f", 1, 5.005)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        assert_eq!(statuses(&comparison), vec![("f", Status::Unchanged)]);
    }

    #[test]
    fn the_join_reproduces_what_the_tool_itself_reported() {
        // A CAPTURED FRAGMENT of real `cargo crap 0.4.3 --format json --baseline` output for this
        // workspace, with the tool's own `status` values kept as the expectation. This is what
        // stops the join from drifting away from the tool whose numbers it pairs up: if a future
        // cargo-crap changes what it calls these, this test says so.
        let before = [
            entry("crates/sutura-domain/src/calendar.rs", "Date::day", 203, 0.5),
            entry("crates/sutura-domain/src/calendar.rs", "Date::days_in_month", 169, 3.0),
            entry(
                "crates/sutura-domain/src/warehouse.rs",
                "RowSet::scalar",
                298,
                5.072_886_297_376_093,
            ),
        ];
        let after = [
            entry("crates/sutura-domain/src/calendar.rs", "Date::day", 203, 1.0),
            entry("crates/sutura-domain/src/calendar.rs", "Date::days_in_month", 169, 3.0),
            entry(
                "crates/sutura-domain/src/warehouse.rs",
                "RowSet::scalar",
                298,
                5.072_886_297_376_093,
            ),
        ];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let mut found = statuses(&comparison);
        found.sort_by_key(|pair| pair.0);
        assert_eq!(
            found,
            vec![
                ("Date::day", Status::Regressed),
                ("Date::days_in_month", Status::Unchanged),
                ("RowSet::scalar", Status::Unchanged),
            ],
            "the tool reported 1 regressed and the rest unchanged for this input"
        );
    }

    // ---------------------------------------------------------- the ratchet ---

    #[test]
    fn one_small_regression_under_the_line_is_reported_and_does_not_fail() {
        // The case that makes an any-regression rule unshippable: at full coverage CRAP equals
        // CC, so a covered added branch is a permanent +1 no test can undo.
        let before = [entry("a.rs", "f", 1, 5.0)];
        let after = [entry("a.rs", "f", 1, 6.0)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let verdict = ratchet(&comparison, 30.0);
        assert_eq!(verdict.regressions.len(), 1, "it must still be visible");
        assert!(!verdict.broken(30.0), "and it must not fail the build");
    }

    #[test]
    fn crossing_the_line_fails() {
        let before = [entry("a.rs", "f", 1, 28.0)];
        let after = [entry("a.rs", "f", 1, 31.0)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let verdict = ratchet(&comparison, 30.0);
        assert_eq!(verdict.crossed.len(), 1);
        assert!(verdict.worse_over_line.is_empty(), "no entry reads as worse over the line");
        assert!(verdict.broken(30.0));
        let reasons = verdict.reasons(30.0);
        assert!(reasons.iter().any(|reason| reason.contains("crossed")), "{reasons:?}");
    }

    #[test]
    fn debt_that_is_already_over_the_line_may_not_get_worse() {
        // The rule that keeps meaning something if the scope is ever widened over inherited
        // debt, which is the only way this gate covers more than one crate.
        let before = [entry("a.rs", "f", 1, 210.0)];
        let after = [entry("a.rs", "f", 1, 240.0)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let verdict = ratchet(&comparison, 30.0);
        assert_eq!(verdict.worse_over_line.len(), 1);
        assert!(verdict.crossed.is_empty(), "it did not cross, it was already over");
        assert!(verdict.broken(30.0));
        // An improvement to the same debt is not a failure, however far over the line it is.
        let better = compare(&before, &[entry("a.rs", "f", 1, 180.0)], DEFAULT_EPSILON);
        assert!(!ratchet(&better, 30.0).broken(30.0));
    }

    #[test]
    fn many_small_regressions_exhaust_the_budget() {
        // Rule three, and the one that blocks something the absolute threshold does not: slow rot
        // that never crosses the line.
        let before: Vec<Entry> = (0..10).map(|n| entry("a.rs", "f", n, 5.0)).collect();
        let after: Vec<Entry> = (0..10).map(|n| entry("a.rs", "f", n, 9.0)).collect();
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let verdict = ratchet(&comparison, 30.0);
        assert!((verdict.budget_used - 40.0).abs() < 0.001, "{}", verdict.budget_used);
        assert!(verdict.broken(30.0), "40 points added is over the 30-point budget");
        // Seven of the same is inside it, and must pass.
        let fewer: Vec<Entry> = after.iter().take(7).cloned().collect();
        let inside = compare(&before, &fewer, DEFAULT_EPSILON);
        assert!(!ratchet(&inside, 30.0).broken(30.0));
    }

    #[test]
    fn new_code_does_not_spend_the_budget() {
        // New functions are judged by the absolute threshold. Charging them to the budget would
        // make a branch that adds well-tested code fail for adding code.
        let before: Vec<Entry> = Vec::new();
        let after: Vec<Entry> = (0..20).map(|n| entry("a.rs", "f", n, 25.0)).collect();
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let verdict = ratchet(&comparison, 30.0);
        assert_eq!(verdict.added, 20);
        assert!((verdict.budget_used - 0.0).abs() < f64::EPSILON);
        assert!(!verdict.broken(30.0));
    }

    #[test]
    fn deleting_a_crappy_function_is_never_a_failure() {
        let before = [entry("a.rs", "awful", 1, 210.0)];
        let comparison = compare(&before, &[], DEFAULT_EPSILON);
        assert_eq!(comparison.removed.len(), 1);
        assert!(!ratchet(&comparison, 30.0).broken(30.0));
    }

    // ---------------------------------------------------------- rendering ---

    #[test]
    fn the_comment_starts_with_its_marker_and_is_pure_ascii() {
        let before = [entry("a.rs", "f", 1, 28.0)];
        let after = [entry("a.rs", "f", 1, 31.0)];
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let body = comment(&comparison, 30.0, "sutura-domain", "abc1234");
        // The marker is how the next push finds and replaces this comment instead of adding one.
        assert!(body.starts_with(super::COMMENT_MARKER), "{body}");
        // The workspace denies `non_ascii_literal`, and a terminal without UTF-8 is a real host.
        assert!(body.is_ascii(), "the comment must be ASCII: {body}");
        assert!(body.contains("FAILED"), "{body}");
        assert!(body.contains("+3.0"), "the delta has to be in the row: {body}");
        assert!(body.contains("abc1234"), "the reader must be told which base this is: {body}");
    }

    #[test]
    fn a_clean_comparison_still_renders_something_a_person_can_read() {
        let before = [entry("a.rs", "f", 1, 5.0)];
        let comparison = compare(&before, &before, DEFAULT_EPSILON);
        let body = comment(&comparison, 30.0, "sutura-domain", "deadbee");
        assert!(body.contains("No function got worse."), "{body}");
        assert!(!body.contains("FAILED"), "{body}");
    }

    #[test]
    fn a_huge_delta_is_capped_and_says_so() {
        let before: Vec<Entry> = (0..40).map(|n| entry("a.rs", "f", n, 1.0)).collect();
        let after: Vec<Entry> = (0..40).map(|n| entry("a.rs", "f", n, 2.0)).collect();
        let comparison = compare(&before, &after, DEFAULT_EPSILON);
        let body = comment(&comparison, 30.0, "sutura-domain", "cafe123");
        assert!(body.contains("further regression(s) not listed"), "{body}");
        // 25 rows and no more; a comment is a summary, the log is the record.
        assert_eq!(
            body.lines().filter(|line| line.starts_with("| 2.0 |")).count(),
            super::ROW_CAP
        );
    }
}
