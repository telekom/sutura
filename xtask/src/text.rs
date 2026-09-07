//! The text hygiene gates, implemented here rather than pulled from a hook registry.
//!
//! `.pre-commit-config.yaml` used to get these from `github.com/pre-commit/pre-commit-hooks`.
//! That host is not reachable from every network this repo is developed on, and a hook that
//! cannot be fetched is a hook that does not run - so the checks live in the repo, in the
//! same binary as the other gates, with tests.
//!
//! Covered: merge-conflict markers, trailing whitespace, end-of-file newline, oversized
//! files. Each is a `--fix`-able formatting concern except the first two, which are
//! reported; `--fix` rewrites what can be rewritten mechanically.
//!
//! And - `github.com/telekom/sutura#412` - a file this gate is meant to read and cannot, which
//! used to leave the walk with no finding and no bucket. See [`Tally`] for what the verdict's two
//! numbers are and what comparing them is worth.
//!
//! NOT covered, deliberately: `check-yaml` and `check-toml`. Every YAML and TOML file here
//! is already parsed by a tool in the gates - cargo reads the manifests, `cargo-deny` reads
//! `deny.toml`, clippy reads `clippy.toml`, the toolchain file is read by rustup and by Nix,
//! `zizmor` and the workflow parser read `.github/workflows`, and `prek` reads its own
//! config. A syntax error in any of them already fails something. Adding a YAML and a TOML
//! parser to this crate to re-check what is already checked would buy nothing and cost two
//! dependencies.

use crate::Verdict;
use crate::repo;

/// Anything larger is almost certainly not source. The pre-commit default is 500 kB; this
/// matches the 512 kB the removed hook was configured with.
const MAX_BYTES: u64 = 512 * 1024;

/// Conflict markers, as byte patterns at the start of a line.
///
/// `=======` is deliberately NOT one of them: it is also valid Markdown heading underlining
/// and a common ASCII rule, so it produces false positives on documentation. The `<<<<<<<`
/// and `>>>>>>>` markers are unambiguous, and no real conflict has only the middle marker.
const CONFLICT_MARKERS: &[&str] = &["<<<<<<< ", ">>>>>>> ", "<<<<<<<\t", ">>>>>>>\t"];

/// The em dash. Every doc and comment in this repo uses a plain hyphen instead.
///
/// A convention with no mechanism is a wish: AGENTS.md has said "plain hyphens" from the
/// start, five files were reviewed for it by hand, and `.gitattributes` still shipped one -
/// found by an agent reading that file for an unrelated reason. That is the failure mode
/// this gate exists for.
///
/// Fixable, because unlike a conflict marker there is exactly one right answer.
const EM_DASH: char = '\u{2014}';

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finding {
    ConflictMarker { line: usize, marker: String },
    TrailingWhitespace { line: usize },
    EmDash { line: usize },
    MissingFinalNewline,
    MultipleFinalNewlines(usize),
    TooLarge(u64),
    Unreadable(String),
}

impl Finding {
    fn describe(&self) -> String {
        match *self {
            Self::ConflictMarker { line, ref marker } => {
                format!("line {line}: merge conflict marker `{}`", marker.trim())
            }
            Self::TrailingWhitespace { line } => format!("line {line}: trailing whitespace"),
            Self::EmDash { line } => {
                format!("line {line}: em dash - this repo uses a plain hyphen")
            }
            Self::MissingFinalNewline => String::from("no newline at end of file"),
            Self::MultipleFinalNewlines(n) => format!("{n} blank lines at end of file"),
            Self::TooLarge(bytes) => {
                format!("{bytes} bytes exceeds the {MAX_BYTES}-byte limit for a tracked file")
            }
            // The OS error travels with it: "could not read" without the reason sends the reader
            // back to the shell to work out which of the four it was.
            Self::Unreadable(ref cause) => {
                format!("this gate is meant to read this file and could not: {cause}")
            }
        }
    }

    /// Can `--fix` repair this mechanically? A conflict marker cannot: choosing a side is a
    /// judgement, and a gate that guessed would destroy work.
    const fn fixable(&self) -> bool {
        matches!(
            *self,
            Self::TrailingWhitespace { .. } | Self::EmDash { .. } | Self::MissingFinalNewline | Self::MultipleFinalNewlines(_)
        )
    }
}

/// Does the file declare itself generated?
///
/// Generated and vendored content is exempt from the whitespace and final-newline rules,
/// because it is not hand-edited and "fixing" it would make it differ from its source - which
/// for content tracked by a recorded hash turns the hash into a lie.
///
/// The marker has to be IN the file rather than in an ignore list: an ignore list is a second
/// place to forget, and a file that stops being generated should stop being exempt in the
/// same edit that removes the marker.
///
/// Conflict markers and file size are still checked. A conflict marker in a generated file
/// means a bad merge, which is worth hearing about wherever it happens.
fn is_generated(text: &str) -> bool {
    text.lines().take(5).any(|l| l.to_ascii_lowercase().contains("do not edit"))
}

/// Inspect one file's contents.
/// Paths whose punctuation is not ours to change.
///
/// `.agents/skill-library/**` is a MIRROR - VENDOR.md records the upstream commit and
/// `cargo xtask check-skills` verifies a sha256 per file. `ms-rust/` is generated from an
/// upstream snapshot and records a content hash for the same reason. Normalising an em dash
/// in either would make the recorded hash a lie, which is worse than the punctuation.
///
/// Deliberately narrow: this exempts only the em-dash rule, and only these two trees. The
/// whitespace and conflict-marker rules still apply, because those are about the file being
/// well-formed rather than about its prose.
const VENDORED_PROSE: &[&str] = &[".agents/skill-library/", ".agents/skills/engineering/ms-rust/"];

/// Is this path vendored prose, exempt from the em-dash rule?
fn is_vendored_prose(path: &str) -> bool {
    VENDORED_PROSE.iter().any(|prefix| path.starts_with(prefix))
}

/// Paths whose BYTES are the point, not merely their punctuation.
///
/// `vendor/**` is third-party source recorded in `VENDOR.md` against an upstream artifact
/// hash. Upstream's trailing whitespace, blank lines and missing final newlines are part of
/// those bytes: "fixing" them would make the vendored copy differ from the release it claims
/// to be, turn the recorded hash into a lie, and make the next update a diff of our edits
/// interleaved with upstream's changes. Vendoring exists to hold someone else's code exactly
/// as they published it, so a gate that rewrites it defeats the purpose.
///
/// WIDER than `VENDORED_PROSE`, which exempts only the em dash: this also exempts the
/// whitespace and final-newline rules. Conflict markers and the size limit still apply,
/// because those are about a file being well-formed rather than about its formatting - a bad
/// merge in a vendored tree is still a bad merge, and an enormous blob is still a problem.
const VENDORED_SOURCE: &[&str] = &["vendor/"];

/// Is this path vendored third-party source, exempt from the formatting rules?
fn is_vendored_source(path: &str) -> bool {
    VENDORED_SOURCE.iter().any(|prefix| path.starts_with(prefix))
}

pub(crate) fn inspect(path: &str, text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    // Both mean "do not reformat these bytes": a generated file is rewritten by its own
    // generator, and a vendored file has to keep matching the upstream artifact it records.
    let byte_exact = is_generated(text) || is_vendored_source(path);
    let prose_exempt = byte_exact || is_vendored_prose(path);

    for (i, line) in text.lines().enumerate() {
        for marker in CONFLICT_MARKERS {
            if line.starts_with(marker) {
                findings.push(Finding::ConflictMarker {
                    line: i + 1,
                    marker: String::from(*marker),
                });
            }
        }
        // `\r` is the line-endings gate's business, not this one's; strip it so a CRLF file
        // does not also report trailing whitespace on every line.
        let without_cr = line.strip_suffix('\r').unwrap_or(line);
        if !byte_exact && without_cr != without_cr.trim_end() {
            findings.push(Finding::TrailingWhitespace { line: i + 1 });
        }
        // Generated and vendored text is exempt for the reason above: it must match its
        // source byte for byte, and `.agents/skill-library/**` is upstream prose whose
        // punctuation is not ours to change.
        if !prose_exempt && without_cr.contains(EM_DASH) {
            findings.push(Finding::EmDash { line: i + 1 });
        }
    }

    // An empty file is fine and needs no terminator.
    if !byte_exact && !text.is_empty() {
        if text.ends_with('\n') {
            let trailing = text.len() - text.trim_end_matches('\n').len();
            if trailing > 1 {
                findings.push(Finding::MultipleFinalNewlines(trailing - 1));
            }
        } else {
            findings.push(Finding::MissingFinalNewline);
        }
    }

    findings
}

/// Apply the mechanical repairs: strip trailing whitespace, end with exactly one newline.
pub(crate) fn fixed(path: &str, text: &str) -> String {
    // Returned byte for byte: `--fix` must never be the thing that makes a vendored tree
    // differ from the upstream release it records.
    if is_generated(text) || is_vendored_source(path) {
        return String::from(text);
    }
    let mut out = String::with_capacity(text.len());
    let prose_exempt = is_vendored_prose(path);
    for line in text.lines() {
        let without_cr = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = without_cr.trim_end();
        if prose_exempt {
            out.push_str(trimmed);
        } else {
            // A plain hyphen, not `--`: the convention is one character, and doubling it
            // would only move the wrongness.
            for ch in trimmed.chars() {
                out.push(if ch == EM_DASH { '-' } else { ch });
            }
        }
        out.push('\n');
    }
    // `lines()` drops the information that the file was empty; keep it empty.
    if text.is_empty() {
        return String::new();
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// One file and everything wrong with it.
type Offender = (String, Vec<Finding>);

/// What the walk did with every path the listing offered.
///
/// **The denominator, and the reason it exists.** `checked` on its own is a number nothing
/// compares: a file this gate could not read left the loop with no finding and no bucket, so the
/// verdict said `ok - N text file(s) checked` with N one lower than the tree and nothing to notice
/// it by (`github.com/telekom/sutura#412`; #402 measured `chmod 000 devenv.nix` at exit 0). Every
/// listed path now lands in exactly one bucket, so the sum is comparable against the LENGTH of the
/// listing.
///
/// **The one place a path could still leave the walk in silence is closed**, and it was the reason
/// this gate was strong in the inner loop and blind where it counted: `repo::collect_all` used to
/// `return` on a directory it could not open, dropping the whole subtree, and `checks.hygiene`
/// runs exactly that fallback. It now emits the directory as a path, so it arrives here as an
/// unreadable in-scope entry like any other.
///
/// **What the comparison is worth, and it is narrower than it looks.** Every iteration of the loop
/// buckets its path unconditionally, so NO TREE CAN MAKE THE TWO NUMBERS DISAGREE - only an edit to
/// this file can. It is a guard against a future change that stops reading, not evidence about the
/// repository, and the mutations on the pull request are what show it catches that: `.take(1)` on
/// the walk turns it red. Read it as a tripwire, not as a measurement.
///
/// **What IS evidence about the tree is the floor**, which is why [`report`] refuses an empty
/// listing: `0 of 0` satisfies every equality above, and [`repo::root`]'s own comment records
/// `ok - 0 text file(s) checked` shipping as a false pass when a store binary walked a directory
/// that no longer existed.
#[derive(Debug, Default, PartialEq, Eq)]
struct Tally {
    /// Read as text and inspected.
    checked: usize,
    /// Read, and not text. Out of scope: a PNG is not a text file this gate failed to read.
    binary: usize,
    /// Listed and not on disk. Git's business - a deleted file that is still in the index - and
    /// bucketed rather than reported so the gate does not redden while somebody is mid-edit.
    absent: usize,
    /// An index symlink checked out as a pointer file, skipped for the reason
    /// [`repo::INDEX_SYMLINKS`] gives.
    pointer: usize,
    /// In scope and unreadable. Each of these is ALSO a finding, which is the half that makes the
    /// count more than bookkeeping.
    unreadable: usize,
    /// Rewritten by `--fix`. Not part of [`Self::accounted`] - a repaired file was checked.
    fixed: usize,
}

impl Tally {
    /// Every listed path this walk reached a conclusion about.
    const fn accounted(&self) -> usize {
        self.checked + self.binary + self.absent + self.pointer + self.unreadable
    }
}

/// Judges one listing, and says what it did with every entry.
///
/// Split out of [`run`] so both halves are reachable from a test: this reads a tree a test can
/// build, and [`report`] turns what it found into the exit code.
fn walk(root: &std::path::Path, files: &[String], fix: bool) -> (Vec<Offender>, Tally) {
    let mut offenders: Vec<Offender> = Vec::new();
    let mut tally = Tally::default();

    for rel in files {
        // A symlink stored as a pointer file has no trailing newline, because a symlink
        // target does not. Where git is available these never reach here; where it is not,
        // this is what stops the walk from failing them. See repo::INDEX_SYMLINKS.
        if repo::is_index_symlink(rel) {
            tally.pointer += 1;
            continue;
        }
        let path = root.join(rel);

        // The size check applies to every file, text or not: a 40 MB binary in git is the
        // problem this catches.
        let mut findings = Vec::new();
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() > MAX_BYTES
        {
            findings.push(Finding::TooLarge(meta.len()));
        }

        match repo::probe_text(&path) {
            repo::TextProbe::Binary => tally.binary += 1,
            // Absent is not this gate's finding to make: `git status` already says so, and a gate
            // that reddens between `rm` and `git add` is one somebody switches off.
            repo::TextProbe::Unreadable(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
                tally.absent += 1;
            }
            repo::TextProbe::Unreadable(cause) => {
                tally.unreadable += 1;
                findings.push(Finding::Unreadable(cause.to_string()));
            }
            // Sniffed as text and still not readable in full: the prefix decoded and a byte past
            // it did not. That file is in scope by this gate's own boundary, so it is a finding
            // for the same reason the arm above is.
            repo::TextProbe::Text => match std::fs::read_to_string(&path) {
                Err(cause) => {
                    tally.unreadable += 1;
                    findings.push(Finding::Unreadable(cause.to_string()));
                }
                Ok(text) => {
                    tally.checked += 1;
                    findings.extend(inspect(rel, &text));

                    if fix && findings.iter().any(Finding::fixable) {
                        let repaired = fixed(rel, &text);
                        if repaired != text {
                            match std::fs::write(&path, repaired.as_bytes()) {
                                Ok(()) => {
                                    tally.fixed += 1;
                                    findings.retain(|f| !f.fixable());
                                }
                                Err(e) => eprintln!("xtask text-hygiene: could not write {rel}: {e}"),
                            }
                        }
                    }
                }
            },
        }

        if !findings.is_empty() {
            offenders.push((rel.clone(), findings));
        }
    }

    (offenders, tally)
}

/// Turns what the walk found into the exit code, and prints both numbers behind it.
fn report(fix: bool, offered: usize, tally: &Tally, offenders: &[Offender]) -> Verdict {
    if fix && tally.fixed > 0 {
        println!("xtask text-hygiene: repaired {} file(s)", tally.fixed);
    }

    let accounted = tally.accounted();
    let mut failed = false;

    if !offenders.is_empty() {
        failed = true;
        eprintln!("xtask text-hygiene: FAILED");
        for (path, findings) in offenders {
            for f in findings {
                eprintln!("  {path}: {}", f.describe());
            }
        }
        eprintln!();
        // EACH REMEDY IS PRINTED ONLY WHERE IT APPLIES, which is this pull request's other half
        // turned on itself: `github.com/telekom/sutura#386` is about a refusal naming two variables
        // that were not set, and an unconditional "run --fix" under a permission denial is the same
        // defect - a remedy the reader cannot act on, inside the change that added the finding.
        let any = |wanted: fn(&Finding) -> bool| offenders.iter().any(|(_, f)| f.iter().any(wanted));
        if any(Finding::fixable) {
            eprintln!("Run `cargo xtask text-hygiene --fix` for the mechanical ones (whitespace and");
            eprintln!("final newlines).");
        }
        if any(|f| matches!(*f, Finding::ConflictMarker { .. } | Finding::TooLarge(_))) {
            eprintln!("Conflict markers and oversized files need a decision.");
        }
        if any(|f| matches!(*f, Finding::Unreadable(_))) {
            eprintln!("A file this gate cannot read needs its permissions or its bytes fixed - no");
            eprintln!("rewrite can be attempted on content nothing has seen.");
        }
    }

    // THE FLOOR. Nothing above can fail on an EMPTY listing: `0 == 0` accounted, no offenders, and
    // the verdict would read `ok`. That is not hypothetical - `repo::root`'s comment records
    // `nix run .#xtask -- text-hygiene` printing `ok - 0 text file(s) checked` and exiting 0
    // because a store binary's compile-time path pointed at a build sandbox that was gone.
    if offered == 0 {
        failed = true;
        eprintln!(
            "xtask text-hygiene: FAILED - the listing was empty, so this gate read nothing. A \
             repository with no files is not a clean one; something upstream of here could not \
             enumerate the tree."
        );
    }

    if accounted != offered {
        failed = true;
        eprintln!(
            "xtask text-hygiene: FAILED - {accounted} of {offered} listed path(s) were accounted \
             for. Every listed path gets one of read, binary, absent, symlink pointer or \
             unreadable here, so a shortfall is this gate having stopped reading rather than a \
             smaller tree."
        );
    }

    if failed {
        return Verdict::Fail;
    }

    println!(
        "xtask text-hygiene: ok - {} text file(s) checked; {accounted} of {offered} listed path(s) \
         accounted for ({} binary, {} absent, {} symlink pointer(s))",
        tally.checked, tally.binary, tally.absent, tally.pointer
    );
    Verdict::Pass
}

/// `xtask text-hygiene [--fix]` - the hook and formatter entry point.
///
/// Everything but the listing lives in [`walk`] and [`report`], which is what lets a test drive
/// the verdict over a tree it built. What is left here - and therefore covered by no unit test -
/// is the four lines that find the repo and hand the two halves to each other.
pub(crate) fn run(args: &[String]) -> Verdict {
    let fix = args.iter().any(|a| a == "--fix");

    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask text-hygiene: could not determine the repo root");
        return Verdict::Fail;
    };

    let (offenders, tally) = walk(&root, &files, fix);
    report(fix, files.len(), &tally, &offenders)
}

#[cfg(test)]
mod tests {
    /// A path that is not vendored, for the tests that are about the text rather than the path.
    const ANY: &str = "src/example.rs";

    use super::{Finding, fixed, inspect};

    #[test]
    fn clean_text_has_no_findings() {
        assert_eq!(inspect(ANY, "fn main() {}\n"), vec![]);
        assert_eq!(inspect(ANY, ""), vec![], "an empty file needs no terminator");
    }

    #[test]
    fn finds_trailing_whitespace() {
        assert_eq!(inspect(ANY, "a  \nb\n"), vec![Finding::TrailingWhitespace { line: 1 }]);
        assert_eq!(inspect(ANY, "a\t\nb\n"), vec![Finding::TrailingWhitespace { line: 1 }]);
    }

    #[test]
    fn finds_final_newline_problems() {
        assert_eq!(inspect(ANY, "a"), vec![Finding::MissingFinalNewline]);
        assert_eq!(inspect(ANY, "a\n\n\n"), vec![Finding::MultipleFinalNewlines(2)]);
    }

    #[test]
    fn finds_conflict_markers() {
        let text = "<<<<<<< HEAD\nmine\n>>>>>>> theirs\n";
        let found = inspect(ANY, text);
        assert!(found.iter().any(|f| matches!(*f, Finding::ConflictMarker { line: 1, .. })));
        assert!(found.iter().any(|f| matches!(*f, Finding::ConflictMarker { line: 3, .. })));
    }

    #[test]
    fn a_markdown_rule_is_not_a_conflict_marker() {
        // The `=======` marker is excluded precisely so this passes: setext headings and
        // ASCII rules are common in the docs this repo is full of.
        assert_eq!(inspect(ANY, "Heading\n=======\n"), vec![]);
        assert_eq!(inspect(ANY, "-------\n"), vec![]);
    }

    #[test]
    fn a_crlf_file_reports_line_endings_not_whitespace() {
        // The line-endings gate owns CRLF. Reporting it twice, as whitespace on every line,
        // would bury the real message.
        assert_eq!(inspect(ANY, "a\r\nb\r\n"), vec![]);
    }

    #[test]
    fn fix_repairs_what_it_claims_to() {
        assert_eq!(fixed(ANY, "a  \nb\t\n"), "a\nb\n");
        assert_eq!(fixed(ANY, "a"), "a\n");
        assert_eq!(fixed(ANY, "a\n\n\n"), "a\n");
        assert_eq!(fixed(ANY, ""), "");
        // Idempotent, or a --fix run would keep producing a diff.
        let once = fixed(ANY, "a  \n\n\n");
        assert_eq!(fixed(ANY, &once), once);
    }

    // Fixtures are ASSEMBLED rather than written as literals, so no line of this file
    // contains trailing whitespace or begins with a conflict marker. A detector whose own
    // test data trips it needs a self-exclusion, and a self-exclusion is a hole; building
    // the input instead leaves the gate with nothing to exclude.
    fn with_trailing_space() -> String {
        let mut s = String::from("trailing");
        s.push_str("   \nno newline");
        s
    }

    fn conflict_line() -> String {
        let mut s = String::from("<");
        s.push_str("<<<<<< HEAD");
        s
    }

    #[test]
    fn finds_an_em_dash() {
        // Assembled rather than written literally: a rule table containing its own needle
        // makes the gate fire on its own source, which is how three earlier detectors in
        // this repo failed.
        let line = format!("a clause{}and its continuation\n", super::EM_DASH);
        assert_eq!(inspect(ANY, &line), vec![Finding::EmDash { line: 1 }]);
    }

    #[test]
    fn fix_replaces_an_em_dash_with_one_hyphen() {
        let line = format!("a clause {} and more\n", super::EM_DASH);
        assert_eq!(fixed(ANY, &line), "a clause - and more\n");
    }

    #[test]
    fn vendored_source_keeps_its_formatting_but_not_a_bad_merge() {
        // `vendor/**` holds upstream bytes recorded in VENDOR.md against a release hash, so
        // the formatting rules must not touch it. Without this the vendored mimalloc tree
        // reported 343 findings that were every one of them upstream's own whitespace.
        let vendored = "vendor/mimalloc_rust/libmimalloc-sys/c_src/mimalloc/v3/src/alloc.c";
        let messy = "int a;  \nint b;\n\n\n";
        assert_eq!(inspect(vendored, messy), vec![], "formatting is upstream's business");
        assert_eq!(fixed(vendored, messy), messy, "`--fix` returns vendored bytes unchanged");
        assert!(!inspect(ANY, messy).is_empty(), "our own files are still checked");

        // The other half, and the reason the exemption is narrow: a bad merge is a bad merge
        // wherever it lands. This is about formatting, not about being well-formed.
        let conflicted = "<<<<<<< HEAD\nint a;\n>>>>>>> theirs\n";
        assert!(
            inspect(vendored, conflicted)
                .iter()
                .any(|f| matches!(*f, Finding::ConflictMarker { .. })),
            "conflict markers are still reported in vendored source"
        );
    }

    #[test]
    fn vendored_prose_keeps_its_em_dashes() {
        // `.agents/skill-library/**` is a mirror whose sha256 `check-skills` verifies. It has
        // no "do not edit" marker because upstream did not write one, so the exemption has to
        // be by path - and without it this rule reported 349 findings in imported prose.
        let line = format!(
            "upstream wrote{}this
",
            super::EM_DASH
        );
        let vendored = ".agents/skill-library/planning/idea-refine/SKILL.md";
        assert!(!inspect(vendored, &line).iter().any(|f| matches!(*f, Finding::EmDash { .. })));
        assert_eq!(fixed(vendored, &line), line, "vendored prose is returned unchanged");
        // The same text in our own file is still a finding.
        assert_eq!(inspect(ANY, &line), vec![Finding::EmDash { line: 1 }]);
    }

    #[test]
    fn vendored_prose_is_still_checked_for_conflict_markers() {
        // The exemption is for PROSE, not for well-formedness. A bad merge in a mirror is
        // still a bad merge.
        let text = format!(
            "{}HEAD
text
",
            "<<<<<<< "
        );
        let vendored = ".agents/skill-library/x/SKILL.md";
        assert!(
            inspect(vendored, &text)
                .iter()
                .any(|f| matches!(*f, Finding::ConflictMarker { .. }))
        );
    }

    #[test]
    fn a_generated_file_keeps_its_em_dashes() {
        // Vendored prose must match its source byte for byte; rewriting it would break the
        // hash that records what was imported.
        let text = format!(
            "<!-- generated, DO NOT EDIT -->\nupstream prose {} verbatim\n",
            super::EM_DASH
        );
        assert!(!inspect(ANY, &text).iter().any(|f| matches!(*f, Finding::EmDash { .. })));
        assert_eq!(fixed(ANY, &text), text, "a generated file is returned unchanged");
    }

    #[test]
    fn a_generated_file_is_exempt_from_whitespace_rules() {
        let mut generated = String::from("<!-- Generated by generate.py; do not edit. -->\n\n");
        generated.push_str(&with_trailing_space());
        assert_eq!(inspect(ANY, &generated), vec![], "generated content is not hand-edited");
        // The same content without the marker is judged normally.
        assert!(!inspect(ANY, &with_trailing_space()).is_empty());
    }

    #[test]
    fn a_generated_file_is_still_checked_for_conflict_markers() {
        // A bad merge is worth hearing about wherever it lands.
        let mut text = String::from("<!-- do not edit -->\n");
        text.push_str(&conflict_line());
        text.push_str("\nx\n");
        assert!(
            inspect(ANY, &text)
                .iter()
                .any(|f| matches!(*f, Finding::ConflictMarker { .. }))
        );
    }

    #[test]
    fn a_conflict_marker_is_not_fixable() {
        let f = Finding::ConflictMarker {
            line: 1,
            marker: String::from("<<<<<<< "),
        };
        assert!(!f.fixable(), "choosing a side is a judgement, not a fix");
        assert!(Finding::TrailingWhitespace { line: 1 }.fixable());
        assert!(!Finding::TooLarge(1).fixable());
        // Nor is a file that could not be opened: there is nothing to rewrite.
        assert!(!Finding::Unreadable(String::from("Permission denied")).fixable());
    }

    /// A scratch tree of this test's own, emptied first so a rerun starts clean.
    ///
    /// Keyed on the process id AND removed first, for the reason `crate::falsifier` gives at its
    /// own constructor: a pid is reusable, so a tree an earlier run left behind would seed files
    /// into a walk whose whole argument is that its contents are known.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-text-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch tree");
        dir
    }

    /// The listing entries `walk` is handed, as it would get them from `repo::all_files`.
    fn listing(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| String::from(*n)).collect()
    }

    #[test]
    fn an_unreadable_in_scope_file_is_a_finding_and_not_a_smaller_count() {
        use std::io::Write as _;

        // **`github.com/telekom/sutura#412`.** A file this gate is meant to read and cannot was
        // neither counted nor reported: `is_text_file` answered `false` for it exactly as for a
        // PNG, the `read_to_string` was never reached, `offenders` stayed empty, and the verdict
        // printed a count one lower than the tree with nothing to compare it against. #402
        // measured that as `chmod 000 devenv.nix` leaving `ok - 1150 text file(s)` at exit 0.
        let dir = scratch("unreadable");
        std::fs::write(dir.join("clean.md"), "a line\n").expect("a clean file");
        // A DIRECTORY, not a mode-000 file: `open` succeeds on unix and the read fails with
        // `EISDIR` whatever the process's privileges are, where mode bits deny a root process
        // nothing - so a permission fixture would assert nothing on a container that runs as root.
        std::fs::create_dir_all(dir.join("opaque")).expect("an unreadable path");
        // The second way in, and the one no permission bit is involved in: the sniff window
        // decodes and a byte past it does not, so this file passes `probe_text` and still cannot
        // be read to a string. 0xFF sits well past `repo`'s 8 KiB prefix.
        let mut late = std::fs::File::create(dir.join("late-binary.txt")).expect("create");
        late.write_all(&vec![b'a'; 16 * 1024]).expect("a decodable prefix");
        late.write_all(&[0xFF]).expect("and a byte that is not UTF-8");

        let files = listing(&["clean.md", "late-binary.txt", "opaque"]);
        let (offenders, tally) = super::walk(&dir, &files, false);

        let named: Vec<&str> = offenders.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            named,
            vec!["late-binary.txt", "opaque"],
            "an in-scope file that cannot be read is reported by name: {offenders:?}"
        );
        assert!(
            offenders
                .iter()
                .all(|(_, found)| found.iter().any(|one| matches!(*one, Finding::Unreadable(_)))),
            "and the OS error travels with it: {offenders:?}"
        );
        assert_eq!(tally.unreadable, 2);
        assert_eq!(tally.checked, 1, "only the readable file was checked");
        assert_eq!(
            tally.accounted(),
            files.len(),
            "the drop is visible in the accounting as well as in the findings"
        );

        // **And it reaches the exit code**, which is the half a helper test would not prove: three
        // gates reviewed on this tree had their verdict reach the exit code through a line nothing
        // covered.
        assert_eq!(super::report(false, files.len(), &tally, &offenders), crate::Verdict::Fail);

        // The control. Same walk over the readable file alone must PASS, or the assertion above is
        // about the harness rather than about the unreadable files.
        let clean = listing(&["clean.md"]);
        let (none, ok) = super::walk(&dir, &clean, false);
        assert!(none.is_empty(), "{none:?}");
        assert_eq!(super::report(false, clean.len(), &ok, &none), crate::Verdict::Pass);

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn every_listed_path_lands_in_exactly_one_bucket() {
        use std::io::Write as _;

        // The denominator half of #412. `checked` alone is a number nothing compares, so the walk
        // now has to say what it did with EVERY entry - and this is what a loop that stops early
        // trips over: `.take(n)` here leaves `accounted` short of the listing it was handed.
        let dir = scratch("buckets");
        std::fs::write(dir.join("prose.md"), "a line\n").expect("a text file");
        let mut binary = std::fs::File::create(dir.join("favicon.png")).expect("create");
        binary.write_all(b"\x89PNG\r\n\x1a\n\x00\x00").expect("write");
        std::fs::create_dir_all(dir.join("opaque")).expect("an unreadable path");
        // `.claude/skills` is an index symlink; on a checkout without symlink support it is a
        // pointer file with no final newline, which is what the skip exists for. Not created on
        // disk on purpose - the skip is by path and happens before anything is opened.

        let files = listing(&["prose.md", "favicon.png", "gone.md", ".claude/skills", "opaque"]);
        let (_, tally) = super::walk(&dir, &files, false);

        assert_eq!(
            tally,
            super::Tally {
                checked: 1,
                binary: 1,
                absent: 1,
                pointer: 1,
                unreadable: 1,
                fixed: 0,
            },
            "each entry has exactly one answer"
        );
        assert_eq!(tally.accounted(), files.len());

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn fix_rewrites_the_file_on_disk_and_an_unreadable_one_survives_it() {
        // `--fix` IS THE `just fmt` WRITE PATH (`justfile:109`, and `devenv.nix` calls the same
        // task), so "untested" understated it: nothing covered the arm that REWRITES a tracked
        // file. This drives it over a scratch tree and reads the bytes back.
        let dir = scratch("fix");
        let messy = dir.join("messy.md");
        std::fs::write(&messy, "trailing   \nno final newline").expect("a fixable file");
        std::fs::create_dir_all(dir.join("opaque")).expect("an unreadable path beside it");

        let files = listing(&["messy.md", "opaque"]);
        let (offenders, tally) = super::walk(&dir, &files, true);

        assert_eq!(tally.fixed, 1, "the writable file was rewritten: {tally:?}");
        assert_eq!(
            std::fs::read_to_string(&messy).expect("read back"),
            "trailing\nno final newline\n",
            "the bytes on disk are what changed, not just the finding list"
        );
        // The fixable findings are gone from the report because they were repaired; the unreadable
        // path is still a finding, because `--fix` cannot rewrite what it cannot read.
        let named: Vec<&str> = offenders.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(named, vec!["opaque"], "{offenders:?}");
        assert_eq!(tally.accounted(), files.len());
        assert_eq!(super::report(true, files.len(), &tally, &offenders), crate::Verdict::Fail);

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn an_empty_listing_is_refused_rather_than_called_clean() {
        // THE FLOOR. Every equality in `report` is satisfied by nothing at all, so without this a
        // gate that enumerated no files would print `ok`. `repo::root`'s comment records exactly
        // that shipping: `ok - 0 text file(s) checked`, exit 0, from a store binary whose
        // compile-time path pointed into a build sandbox that no longer existed.
        let nothing = super::Tally::default();
        assert_eq!(
            super::report(false, 0, &nothing, &[]),
            crate::Verdict::Fail,
            "a gate that read nothing may not report a clean tree"
        );
        // And one real file is still a pass, so the floor is not simply refusing everything.
        let one = super::Tally {
            checked: 1,
            ..super::Tally::default()
        };
        assert_eq!(super::report(false, 1, &one, &[]), crate::Verdict::Pass);
    }

    #[test]
    fn a_shortfall_between_the_listing_and_the_walk_is_refused() {
        // **The wiring, driven directly.** A gate reviewed on this tree today printed `19 of 19`
        // where both numbers came from one walk, and `.take(2)` made it `2 of 2` and green. Here
        // the offered count is the LENGTH of the listing `repo::all_files` produced and the
        // accounted count is summed inside the loop, so they cannot both shrink together.
        let complete = super::Tally {
            checked: 3,
            ..super::Tally::default()
        };
        assert_eq!(super::report(false, 3, &complete, &[]), crate::Verdict::Pass);
        assert_eq!(
            super::report(false, 4, &complete, &[]),
            crate::Verdict::Fail,
            "a path the walk reached no conclusion about is a gate that stopped reading"
        );

        // A finding fails too, with the accounting complete - so the two arms are independent
        // rather than one of them carrying the other.
        let offender = vec![(String::from("x.md"), vec![Finding::MissingFinalNewline])];
        assert_eq!(super::report(false, 3, &complete, &offender), crate::Verdict::Fail);
    }
}
