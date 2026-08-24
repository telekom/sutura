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

use std::path::Path;
use std::process::ExitCode;

use crate::repo;

/// Extensions treated as text. Anything else is skipped: a trailing-whitespace rule applied
/// to a binary is nonsense.
const TEXT_EXT: &[&str] = &[
    "rs",
    "toml",
    "nix",
    "md",
    "yml",
    "yaml",
    "sh",
    "json",
    "lock",
    "example",
    "envrc",
    "txt",
    "gitignore",
    "gitattributes",
];

/// Anything larger is almost certainly not source. The pre-commit default is 500 kB; this
/// matches the 512 kB the removed hook was configured with.
const MAX_BYTES: u64 = 512 * 1024;

/// Conflict markers, as byte patterns at the start of a line.
///
/// `=======` is deliberately NOT one of them: it is also valid Markdown heading underlining
/// and a common ASCII rule, so it produces false positives on documentation. The `<<<<<<<`
/// and `>>>>>>>` markers are unambiguous, and no real conflict has only the middle marker.
const CONFLICT_MARKERS: &[&str] = &["<<<<<<< ", ">>>>>>> ", "<<<<<<<\t", ">>>>>>>\t"];

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finding {
    ConflictMarker { line: usize, marker: String },
    TrailingWhitespace { line: usize },
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
            Self::TrailingWhitespace { .. } | Self::MissingFinalNewline | Self::MultipleFinalNewlines(_)
        )
    }
}

/// Inspect one file's contents.
pub(crate) fn inspect(text: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

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
        if without_cr != without_cr.trim_end() {
            findings.push(Finding::TrailingWhitespace { line: i + 1 });
        }
    }

    // An empty file is fine and needs no terminator.
    if !text.is_empty() {
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
pub(crate) fn fixed(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let without_cr = line.strip_suffix('\r').unwrap_or(line);
        out.push_str(without_cr.trim_end());
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

fn text_extension(path: &Path) -> bool {
    path.extension().and_then(std::ffi::OsStr::to_str).map_or_else(
        || {
            // Dotfiles with no extension: `.envrc`, `.gitignore`.
            path.file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|f| f.starts_with('.'))
        },
        |e| TEXT_EXT.contains(&e),
    )
}

/// `xtask text-hygiene [--fix]` - the hook and formatter entry point.
pub(crate) fn run(args: &[String]) -> ExitCode {
    let fix = args.iter().any(|a| a == "--fix");

    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask text-hygiene: could not determine the repo root");
        return ExitCode::FAILURE;
    };

    let mut offenders: Vec<Offender> = Vec::new();
    let mut fixed_count = 0_usize;
    let mut checked = 0_usize;

    for rel in files {
        let path = root.join(&rel);
        let p = Path::new(&rel);

        // The size check applies to every file, text or not: a 40 MB binary in git is the
        // problem this catches.
        let mut findings = Vec::new();
        if let Ok(meta) = std::fs::metadata(&path)
            && meta.len() > MAX_BYTES
        {
            findings.push(Finding::TooLarge(meta.len()));
        }

        if text_extension(p)
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            {
                checked += 1;
                findings.extend(inspect(&text));

                if fix && findings.iter().any(Finding::fixable) {
                    let repaired = fixed(&text);
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
        return ExitCode::SUCCESS;
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
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{Finding, fixed, inspect};

    #[test]
    fn clean_text_has_no_findings() {
        assert_eq!(inspect("fn main() {}\n"), vec![]);
        assert_eq!(inspect(""), vec![], "an empty file needs no terminator");
    }

    #[test]
    fn finds_trailing_whitespace() {
        assert_eq!(inspect("a  \nb\n"), vec![Finding::TrailingWhitespace { line: 1 }]);
        assert_eq!(inspect("a\t\nb\n"), vec![Finding::TrailingWhitespace { line: 1 }]);
    }

    #[test]
    fn finds_final_newline_problems() {
        assert_eq!(inspect("a"), vec![Finding::MissingFinalNewline]);
        assert_eq!(inspect("a\n\n\n"), vec![Finding::MultipleFinalNewlines(2)]);
    }

    #[test]
    fn finds_conflict_markers() {
        let text = "<<<<<<< HEAD\nmine\n>>>>>>> theirs\n";
        let found = inspect(text);
        assert!(found.iter().any(|f| matches!(*f, Finding::ConflictMarker { line: 1, .. })));
        assert!(found.iter().any(|f| matches!(*f, Finding::ConflictMarker { line: 3, .. })));
    }

    #[test]
    fn a_markdown_rule_is_not_a_conflict_marker() {
        // The `=======` marker is excluded precisely so this passes: setext headings and
        // ASCII rules are common in the docs this repo is full of.
        assert_eq!(inspect("Heading\n=======\n"), vec![]);
        assert_eq!(inspect("-------\n"), vec![]);
    }

    #[test]
    fn a_crlf_file_reports_line_endings_not_whitespace() {
        // The line-endings gate owns CRLF. Reporting it twice, as whitespace on every line,
        // would bury the real message.
        assert_eq!(inspect("a\r\nb\r\n"), vec![]);
    }

    #[test]
    fn fix_repairs_what_it_claims_to() {
        assert_eq!(fixed("a  \nb\t\n"), "a\nb\n");
        assert_eq!(fixed("a"), "a\n");
        assert_eq!(fixed("a\n\n\n"), "a\n");
        assert_eq!(fixed(""), "");
        // Idempotent, or a --fix run would keep producing a diff.
        let once = fixed("a  \n\n\n");
        assert_eq!(fixed(&once), once);
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
