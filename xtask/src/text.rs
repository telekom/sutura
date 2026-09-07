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

/// `xtask text-hygiene [--fix]` - the hook and formatter entry point.
pub(crate) fn run(args: &[String]) -> Verdict {
    let fix = args.iter().any(|a| a == "--fix");

    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::TextHygiene)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask text-hygiene: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut offenders: Vec<Offender> = Vec::new();
    let mut fixed_count = 0_usize;
    let mut checked = 0_usize;

    for rel in files {
        // A symlink stored as a pointer file has no trailing newline, because a symlink
        // target does not. Where git is available these never reach here; where it is not,
        // this is what stops the walk from failing them. See repo::INDEX_SYMLINKS.
        if repo::is_index_symlink(&rel) {
            continue;
        }
        let path = root.join(&rel);

        // The size check applies to every file, text or not: a 40 MB binary in git is the
        // problem this catches.
        let mut findings = Vec::new();
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() > MAX_BYTES
        {
            findings.push(Finding::TooLarge(meta.len()));
        }

        if repo::is_text_file(&path)
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            {
                checked += 1;
                findings.extend(inspect(&rel, &text));

                if fix && findings.iter().any(Finding::fixable) {
                    let repaired = fixed(&rel, &text);
                    if repaired != text {
                        match std::fs::write(&path, repaired.as_bytes()) {
                            Ok(()) => {
                                fixed_count += 1;
                                findings.retain(|f| !f.fixable());
                            }
                            Err(e) => eprintln!("xtask text-hygiene: could not write {rel}: {e}"),
                        }
                    }
                }
            }
        }

        if !findings.is_empty() {
            offenders.push((rel, findings));
        }
    }

    if fix && fixed_count > 0 {
        println!("xtask text-hygiene: repaired {fixed_count} file(s)");
    }

    if offenders.is_empty() {
        println!("xtask text-hygiene: ok - {checked} text file(s) checked");
        return Verdict::Pass;
    }

    eprintln!("xtask text-hygiene: FAILED");
    for (path, findings) in &offenders {
        for f in findings {
            eprintln!("  {path}: {}", f.describe());
        }
    }
    eprintln!();
    eprintln!("Run `cargo xtask text-hygiene --fix` for the mechanical ones (whitespace and");
    eprintln!("final newlines). Conflict markers and oversized files need a decision.");
    Verdict::Fail
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
    }
}
