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

/// Anything larger is almost certainly not source, and catching that is what this cap is
/// for: a huge blob checked in by accident. The number itself is not a considered limit for
/// any particular kind of file - it is inherited from a pre-commit hook this repo no longer
/// runs - so treat 512 kB as "big enough to ask a question", not as a size anything was
/// designed to fit.
///
/// [`is_generated_api_doc`] is the one exemption: a generated `docs/api/*.md` page's size is
/// a function of how much public surface a crate has, which this cap cannot tell apart from
/// an accident, so a documented crate would otherwise be unable to grow.
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

/// Where the generated API reference pages live.
///
/// `docs/api/index.md` is the one file in this directory that is hand-written, which is why
/// the path alone is not the check - see [`is_generated_api_doc`].
const API_DOCS_DIR: &str = "docs/api/";

/// Is this an oversized generated API page, rather than an oversized anything else?
///
/// Scoped to `docs/api/` rather than to `devco/max-lines-ignore`'s whole `[silent]` section,
/// which is the LINE cap's exemption list and reaches `docs/*.md`, `docs/adr/*.md` and
/// `vendor/**` for reasons that do not carry over here - this gate's own [`BYTE_EXACT`] doc
/// keeps a vendored file's size checked on purpose, so reading that list whole would silently
/// exempt vendored blobs from the one check that catches an accidental one. A dedicated,
/// narrower predicate keeps the two caps' exemptions independent, the same way [`BYTE_EXACT`]
/// and [`VENDORED_PROSE`] already are.
///
/// BOTH conditions matter: the path scopes this to `docs/api/`, and [`is_generated`]
/// distinguishes the pages `nix run .#api-docs` writes from `docs/api/index.md` - a
/// hand-written landing page in the same directory that a person still edits, and that this
/// cap still has to cover.
fn is_generated_api_doc(path: &str, text: &str) -> bool {
    path.starts_with(API_DOCS_DIR) && is_generated(text)
}

/// Should [`Finding::TooLarge`] fire for these bytes at this path?
///
/// Pulled out of the census closure in [`run`] so the decision itself - not the whole walk -
/// is what a test exercises directly.
fn oversized(rel: &str, bytes: &[u8]) -> Option<Finding> {
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size <= MAX_BYTES {
        return None;
    }
    let exempt = repo::looks_like_text(bytes) && is_generated_api_doc(rel, &String::from_utf8_lossy(bytes));
    (!exempt).then_some(Finding::TooLarge(size))
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
/// `fuzz/seeds/**` is the same property arrived at from the other direction. A fuzz seed IS
/// its bytes: the file is an input handed to a parser, and the seeds that matter most are
/// crash reproducers - the exact input that once made a process abort. Appending a final
/// newline to one of those changes the input and can un-reproduce the crash it was committed
/// to hold, so the gate that reformats it silently destroys the regression.
///
/// `devco/claim-mutations/**` is the same property again, arrived at from `git apply`'s side.
/// A claim-cell mutation is a unified diff, and a unified diff represents an unchanged BLANK
/// line as one context line holding a single space and nothing else - which is exactly
/// `Finding::TrailingWhitespace`, so an unfixed patch is refused by this gate outright. That
/// alone is reason enough to exempt the prefix; `--fix` stripping the space is not merely
/// cosmetic on top of it. When the bare-space line is the file's LAST line, [`fixed`]'s own
/// trailing-blank-line collapse (`while out.ends_with("\n\n")`) deletes the line outright,
/// so the hunk's body has one fewer line than its `@@` header declares and `git apply
/// --check` refuses it as corrupt - verified, not assumed. A bare-space line stripped
/// mid-hunk is milder: `git apply` tolerates the resulting markerless blank line as an
/// implicit context line, so that shape alone would not have forced this exemption; the
/// gate's own refusal is what does. A hand-authored patch is the whole point of the file, the
/// same way a fuzz seed's bytes are the whole point of it.
///
/// WIDER than `VENDORED_PROSE`, which exempts only the em dash: this also exempts the
/// whitespace and final-newline rules. Conflict markers and the size limit still apply,
/// because those are about a file being well-formed rather than about its formatting - a bad
/// merge in a vendored tree is still a bad merge, and an enormous blob is still a problem.
const BYTE_EXACT: &[&str] = &["vendor/", "fuzz/seeds/", "devco/claim-mutations/"];

/// Is this path byte-exact, exempt from the formatting rules?
fn is_byte_exact(path: &str) -> bool {
    BYTE_EXACT.iter().any(|prefix| path.starts_with(prefix))
}

pub(crate) fn inspect(path: &str, text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    // Both mean "do not reformat these bytes": a generated file is rewritten by its own
    // generator, and a vendored file has to keep matching the upstream artifact it records.
    let byte_exact = is_generated(text) || is_byte_exact(path);
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
    if is_generated(text) || is_byte_exact(path) {
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

/// A symlink stored as a pointer file has no trailing newline, because a symlink target does not.
///
/// Where git is available these never reach the walk; where it is not, this is what stops it
/// failing them. See [`repo::INDEX_SYMLINKS`]. A [`repo::Scope`], so it is a bare `fn` with
/// nothing captured - it cannot count its subjects and it is not handed the content.
fn not_an_index_symlink(rel: &str) -> bool {
    !repo::is_index_symlink(rel)
}

/// `xtask text-hygiene [--fix]` - the hook and formatter entry point.
pub(crate) fn run(args: &[String]) -> Verdict {
    let fix = args.iter().any(|a| a == "--fix");

    // `Census::inspect` RATHER THAN `into_listing`, WHICH IS `github.com/telekom/sutura#412`
    // ITSELF. What stood here was:
    //
    //     if repo::is_text_file(&path) && let Ok(text) = std::fs::read_to_string(&path)
    //
    // and `repo::is_text_file` OPENS THE FILE, so a file this gate is meant to read and cannot
    // fails the FIRST test: it recorded no finding, `checked` never incremented, and the verdict
    // printed `ok - N text file(s) checked` with N one lower than the tree. The only tell was a
    // number nothing compared - and it could not be that tell, because `checked` was incremented
    // by the very loop the drop happened in. `chmod 000 devenv.nix` left `ok - 1150 text file(s)`
    // at exit 0, measured in #402.
    //
    // **The read is the census's now, and it is the same migration `crate::line_endings` already
    // took** - deliberately the same rather than a second pattern, since these two gates read one
    // listing and a second answer to *is this file readable* is how the two could disagree. A
    // subject in scope is one `inspect` opened, so an unreadable file is a `Refusal::Unreachable`
    // this gate has no arm to downgrade, and textness is decided from the bytes already in hand
    // rather than by re-opening the file to ask.
    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask text-hygiene: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let Some(root) = repo::root() else {
        eprintln!("xtask text-hygiene: FAILED - no repository root");
        return Verdict::Fail;
    };

    let mut offenders: Vec<Offender> = Vec::new();
    let mut fixed_count = 0_usize;
    let mut checked = 0_usize;

    let scope: repo::Scope = not_an_index_symlink;
    let anchored = census.inspect(&["flake.nix"], scope, |rel, bytes| {
        // The size check applies to every file, text or not: a 40 MB binary in git is the problem
        // it catches. Taken from the bytes the census already read rather than from a second
        // `metadata` call - one read, one answer, and no window in which the two disagree.
        let mut findings = Vec::new();
        if let Some(finding) = oversized(rel, bytes) {
            findings.push(finding);
        }

        // Textness from the bytes in hand. `looks_like_text` cannot conflate *not text* with
        // *could not look*, which `is_text_file` did by construction - and a binary file is still
        // out of this half's scope, which is the trap #412 names: `check-shipped-binaries`
        // reddened a correct tree because a PNG is out of scope rather than unreadable.
        if repo::looks_like_text(bytes) {
            // Lossy rather than a UTF-8 read: a file the census opened is one this gate judges,
            // and turning a decode failure back into an unread file rebuilds the drop.
            let text = String::from_utf8_lossy(bytes);
            checked = checked.saturating_add(1);
            findings.extend(inspect(rel, &text));

            if fix && findings.iter().any(Finding::fixable) {
                let repaired = fixed(rel, &text);
                if repaired != text {
                    match std::fs::write(root.join(rel), repaired.as_bytes()) {
                        Ok(()) => {
                            fixed_count = fixed_count.saturating_add(1);
                            findings.retain(|f| !f.fixable());
                        }
                        Err(e) => eprintln!("xtask text-hygiene: could not write {rel}: {e}"),
                    }
                }
            }
        }

        if !findings.is_empty() {
            offenders.push((String::from(rel), findings));
        }
    });
    let counted = match anchored {
        Ok(counted) => counted,
        Err(why) => {
            eprintln!("xtask text-hygiene: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };

    if fix && fixed_count > 0 {
        println!("xtask text-hygiene: repaired {fixed_count} file(s)");
    }

    if offenders.is_empty() {
        // The census's own witness beside this gate's count, which is `#414`'s half: `checked` is
        // this closure's tally of files it judged as text, and `counted.verdict()` is the census's
        // tally of subjects it OFFERED, taken by a different predicate on the other side of the
        // walk. A narrowing that moved one cannot move both.
        println!(
            "xtask text-hygiene: ok - {checked} text file(s) checked; {}",
            counted.verdict()
        );
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

    use super::{Finding, fixed, inspect, oversized};

    /// One byte past the cap: cheap to build in a test, and never committed as a fixture.
    fn oversized_bytes() -> Vec<u8> {
        let max = usize::try_from(super::MAX_BYTES).expect("the cap fits a usize on this target");
        vec![b'x'; max + 1]
    }

    /// A generated API page's header, padded past the cap with content a real page would
    /// carry - rustdoc prose, not `x` repeated, so `looks_like_text` has something to judge.
    fn oversized_generated_api_doc() -> Vec<u8> {
        let max = usize::try_from(super::MAX_BYTES).expect("the cap fits a usize on this target");
        let mut text = String::from("<!-- GENERATED FILE - do not edit. -->\n");
        while text.len() <= max {
            text.push_str("pub fn documented_item() {}\n");
        }
        text.into_bytes()
    }

    #[test]
    fn the_cap_fires_on_an_oversized_hand_written_file() {
        // Rung 1 of the proof: a plain file over the limit is still refused.
        let big = oversized_bytes();
        assert_eq!(oversized(ANY, &big), Some(Finding::TooLarge(big.len() as u64)));
    }

    #[test]
    fn the_cap_does_not_fire_on_an_oversized_generated_api_page() {
        // Rung 2: the exemption this branch adds. Same size, but this is what
        // `nix run .#api-docs` actually writes at a path under `docs/api/`.
        let big = oversized_generated_api_doc();
        assert_eq!(oversized("docs/api/sutura-domain.md", &big), None);
    }

    #[test]
    fn an_oversized_hand_written_page_in_the_same_directory_still_trips_the_cap() {
        // Rung 3, the boundary: `docs/api/index.md` sits in the exempt DIRECTORY but carries
        // no generated marker, because a person still edits it. The path alone cannot be the
        // check, or this file would be exempt too.
        let big = oversized_bytes();
        assert_eq!(
            oversized("docs/api/index.md", &big),
            Some(Finding::TooLarge(big.len() as u64)),
            "a hand-written page must stay covered even inside docs/api/"
        );
    }

    #[test]
    fn a_generated_marker_outside_docs_api_does_not_exempt_the_size_cap() {
        // The other half of the boundary: the marker alone is not the check either. Widening
        // this to "any generated file" is the over-broad alternative the brief warns against.
        let big = oversized_generated_api_doc();
        assert_eq!(
            oversized("docs/generated/openapi.json", &big),
            Some(Finding::TooLarge(big.len() as u64))
        );
    }

    #[test]
    fn a_small_generated_api_page_never_reaches_the_size_check() {
        assert_eq!(oversized("docs/api/sutura-domain.md", b"<!-- do not edit -->\nsmall\n"), None);
    }

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
    fn a_claim_mutation_patch_keeps_its_bare_space_context_line() {
        // `devco/claim-mutations/**` holds a unified diff `git apply` depends on byte for
        // byte. A context line quoting an unchanged blank line is one leading space with
        // nothing after it - exactly `Finding::TrailingWhitespace` - so an unfixed patch is
        // refused by this gate outright, whatever line the space sits on.
        let claim_mutation = "devco/claim-mutations/some_claim_cell.patch";
        let patch = "@@ -1,3 +1,3 @@\n context\n \n-old\n+new\n";
        assert_eq!(inspect(claim_mutation, patch), vec![], "the diff's own bytes are the point");
        assert_eq!(fixed(claim_mutation, patch), patch, "`--fix` returns the patch unchanged");
        assert!(!inspect(ANY, patch).is_empty(), "our own files are still checked");

        // The other half, and the reason the exemption is narrow: a bad merge is a bad merge
        // wherever it lands. This is about formatting, not about being well-formed.
        let conflicted = "<<<<<<< HEAD\nold\n>>>>>>> theirs\n";
        assert!(
            inspect(claim_mutation, conflicted)
                .iter()
                .any(|f| matches!(*f, Finding::ConflictMarker { .. })),
            "conflict markers are still reported under devco/claim-mutations/"
        );

        // The trailing slash anchors the prefix to the directory, not to a string that
        // merely starts with it: a sibling directory and a same-named file both stay covered.
        assert!(
            !inspect("devco/claim-mutations-scratch/x.patch", patch).is_empty(),
            "a sibling directory sharing the prefix is not exempt"
        );
        assert!(
            !inspect("devco/claim-mutations.md", patch).is_empty(),
            "a same-named file is not exempt"
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
        assert!(
            !inspect(ANY, &with_trailing_space()).is_empty(),
            "content without the marker still inspects as non-empty"
        );
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
