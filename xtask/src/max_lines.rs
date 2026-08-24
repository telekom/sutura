//! The file-length gate: no file over `DEFAULT_MAX_LINES` lines.
//!
//! A 2000-line file is not a style problem — it is a file nobody reviews, because the
//! diff never fits in a reviewer's head and the module boundary that should exist inside
//! it was never drawn. The limit is deliberately blunt: a number a tool can check beats a
//! judgement call nobody makes.
//!
//! Generated and vendored output is exempt via `.max-lines-ignore`. Hand-written source
//! is not exemptable at all — see [`UNEXEMPTABLE_PREFIXES`].

use crate::repo;
use std::path::Path;
use std::process::ExitCode;

/// The limit. Blunt on purpose.
const DEFAULT_MAX_LINES: usize = 1000;

/// Ignore patterns live in a file, not in this source, so adding an exemption is a
/// reviewable one-line diff next to the reason for it.
const IGNORE_FILE: &str = ".max-lines-ignore";

/// Text we own or review. Binary blobs and images are not line-counted.
const CHECKED_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "nix", "yaml", "yml", "json", "sql", "py", "sh", "lock", "rb", "ts", "js",
];

/// Hand-written source. An ignore pattern pointing here is rejected outright and the gate
/// fails: the fix for a 1200-line module is to split it, and an exemption list that can
/// swallow first-party code is a gate that quietly stops gating.
const UNEXEMPTABLE_PREFIXES: &[&str] = &["crates/", "xtask/"];

/// Patterns from the ignore file, split by what they promise.
struct Ignores {
    /// Generated, vendored or lock-like. Never reported.
    silent: Vec<String>,
    /// Hand-written and over the limit, with a split in progress. Reported as WARN, does
    /// not fail — visible debt rather than a silent exemption.
    warn: Vec<String>,
}

impl Ignores {
    fn parse(text: &str) -> Self {
        let mut silent = Vec::new();
        let mut warn = Vec::new();
        let mut in_warn_section = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            match trimmed {
                "[silent]" => in_warn_section = false,
                "[warn]" => in_warn_section = true,
                pattern if in_warn_section => warn.push(pattern.to_owned()),
                pattern => silent.push(pattern.to_owned()),
            }
        }
        Self { silent, warn }
    }

    fn all(&self) -> impl Iterator<Item = &String> {
        self.silent.iter().chain(self.warn.iter())
    }
}

/// Run the gate. `args` are the arguments after the task name.
pub(crate) fn run(args: &[String]) -> ExitCode {
    let max = match parse_max_lines(args) {
        Ok(max) => max,
        Err(message) => {
            eprintln!("xtask max-lines: {message}");
            return ExitCode::from(2);
        }
    };
    let Some(root) = repo::root() else {
        eprintln!("xtask max-lines: could not locate the repo root");
        return ExitCode::FAILURE;
    };

    let ignore_path = root.join(IGNORE_FILE);
    let ignores = Ignores::parse(&std::fs::read_to_string(&ignore_path).unwrap_or_default());

    let smuggled: Vec<&String> = ignores.all().filter(|p| is_unexemptable(p)).collect();
    if !smuggled.is_empty() {
        eprintln!("xtask max-lines: FAILED — first-party source cannot be exempted");
        for pattern in smuggled {
            eprintln!("  {IGNORE_FILE}: `{pattern}` targets hand-written source; split the file instead");
        }
        return ExitCode::FAILURE;
    }

    let mut files = Vec::new();
    repo::collect_files(&root, &root, CHECKED_EXTENSIONS, &mut files);
    files.sort();

    let mut violations: Vec<(String, usize)> = Vec::new();
    let mut warnings: Vec<(String, usize)> = Vec::new();
    for rel in &files {
        let lines = count_lines(&root.join(rel));
        if lines <= max {
            continue;
        }
        if ignores.warn.iter().any(|p| repo::matches(p, rel)) {
            warnings.push((rel.clone(), lines));
        } else if !ignores.silent.iter().any(|p| repo::matches(p, rel)) {
            violations.push((rel.clone(), lines));
        }
    }

    report(&files, &violations, &warnings, max)
}

fn report(files: &[String], violations: &[(String, usize)], warnings: &[(String, usize)], max: usize) -> ExitCode {
    for (path, lines) in warnings {
        println!("xtask max-lines: WARN {path} has {lines} lines (max {max}) — split pending");
    }
    if violations.is_empty() {
        println!(
            "xtask max-lines: ok — {} files checked, none over {max} lines ({} warned)",
            files.len(),
            warnings.len()
        );
        return ExitCode::SUCCESS;
    }
    eprintln!("xtask max-lines: FAILED — {} file(s) over {max} lines", violations.len());
    for (path, lines) in violations {
        eprintln!("  {path}: {lines} lines");
    }
    eprintln!("  split the file. Generated or vendored output belongs in {IGNORE_FILE}, nothing else does.");
    ExitCode::FAILURE
}

/// The only argument is `--max-lines N`, and it exists so the gate can be *demonstrated*
/// failing on a repo that is currently clean. The committed limit stays
/// [`DEFAULT_MAX_LINES`].
fn parse_max_lines(args: &[String]) -> Result<usize, String> {
    match args {
        [] => Ok(DEFAULT_MAX_LINES),
        [flag, raw] if flag == "--max-lines" => raw.parse::<usize>().map_err(|e| format!("`{raw}` is not a line count: {e}")),
        [flag] if flag == "--max-lines" => Err("--max-lines needs a number".to_owned()),
        [other, ..] => Err(format!("unknown argument `{other}` (accepts --max-lines N)")),
    }
}

/// A pattern is unexemptable if it reaches into first-party source. Checked on the raw
/// pattern text, so a wildcard cannot sneak past by matching nothing today.
fn is_unexemptable(pattern: &str) -> bool {
    let normalized = pattern.trim_start_matches("./");
    UNEXEMPTABLE_PREFIXES.iter().any(|prefix| normalized.starts_with(prefix))
}

/// Lines in a file. Unreadable or non-UTF-8 files count as zero rather than failing the
/// gate: this check is about length, and a read error is a different problem.
fn count_lines(path: &Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MAX_LINES, Ignores, is_unexemptable, parse_max_lines};

    #[test]
    fn sections_split_silent_from_warn() {
        let ignores = Ignores::parse("# comment\n[silent]\nCargo.lock\ndocs/generated/*\n\n[warn]\ndocs/long.md\n");
        assert_eq!(ignores.silent, vec!["Cargo.lock", "docs/generated/*"]);
        assert_eq!(ignores.warn, vec!["docs/long.md"]);
    }

    #[test]
    fn patterns_before_any_header_are_silent() {
        let ignores = Ignores::parse("Cargo.lock\n");
        assert_eq!(ignores.silent, vec!["Cargo.lock"]);
        assert!(ignores.warn.is_empty());
    }

    #[test]
    fn first_party_source_cannot_be_exempted() {
        assert!(is_unexemptable("crates/sutura-domain/src/lib.rs"));
        assert!(is_unexemptable("./xtask/src/main.rs"));
        assert!(is_unexemptable("crates/**"));
        assert!(!is_unexemptable("Cargo.lock"));
        assert!(!is_unexemptable("docs/generated/openapi.json"));
    }

    #[test]
    fn the_limit_is_one_thousand_unless_overridden() {
        assert_eq!(parse_max_lines(&[]), Ok(DEFAULT_MAX_LINES));
        assert_eq!(DEFAULT_MAX_LINES, 1000);
        assert_eq!(parse_max_lines(&["--max-lines".to_owned(), "5".to_owned()]), Ok(5));
        assert!(parse_max_lines(&["--nope".to_owned()]).is_err_and(|e| e.contains("unknown argument")));
        assert!(parse_max_lines(&["--max-lines".to_owned()]).is_err_and(|e| e.contains("needs a number")));
        assert!(parse_max_lines(&["--max-lines".to_owned(), "x".to_owned()]).is_err_and(|e| e.contains("not a line count")));
    }
}
