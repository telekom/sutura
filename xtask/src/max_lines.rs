//! The file-length gate: no file over `DEFAULT_MAX_LINES` lines.
//!
//! A 2000-line file is not a style problem - it is a file nobody reviews, because the
//! diff never fits in a reviewer's head and the module boundary that should exist inside
//! it was never drawn. The limit is deliberately blunt: a number a tool can check beats a
//! judgement call nobody makes.
//!
//! Generated and vendored output is exempt via `devco/max-lines-ignore`. Hand-written source
//! is not exemptable at all - see [`UNEXEMPTABLE_PREFIXES`].
//!
//! **The `ok` verdict names the files closest to the cap**, because a binary gate is silent until it
//! refuses and the refusal arrives mid-change, after the work is written. See [`headroom`] for what
//! that readout is derived from and what it does not prove.
//!
//! # An exemption that exempts nothing
//!
//! The ignore file's own header calls a `[warn]` entry *a promise to split, not a way to silence
//! the gate - and it keeps printing until somebody keeps that promise*. It kept printing only
//! while the file was still over the cap, because the warning was collected inside the
//! `lines > max` branch: an entry for a file that had come back under the limit warned about
//! nothing, failed nothing and said nothing. **The live instance:** a `[warn]` entry for a
//! workflow, with a paragraph arguing the cap and citing the file at 1042 lines; the workflow was
//! then split to 911 and the verdict read `none over 1000 lines (0 warned)` with the entry present
//! and the argument intact. What rots is not the entry - it is the reasoning beside it, which is
//! the only thing a reviewer reads to decide whether the exemption is still earned. See
//! [`inert_entries`].
//!
//! **Why the rule is not *every literal entry under the cap*, which is what the report asked for.**
//! Measured before it was written: `devenv.lock` is 86 lines and `flake.lock` is 98, both literal
//! `[silent]` entries, and both correct - a lockfile's length is a function of the dependency graph
//! and may cross the cap on any given day. That rule would have failed a clean tree, and a gate
//! that fails correct configuration is one somebody disables. What the two sections PROMISE is the
//! seam: `[silent]` is a claim about a CLASS of file, so its length today decides nothing, while
//! `[warn]` is a claim about one file's length and is exactly falsifiable.

use crate::Verdict;
use crate::repo;
use std::path::Path;

/// The limit. Blunt on purpose.
const DEFAULT_MAX_LINES: usize = 1000;

/// Ignore patterns live in a file, not in this source, so adding an exemption is a
/// reviewable one-line diff next to the reason for it.
const IGNORE_FILE: &str = "devco/max-lines-ignore";

/// How many near-cap files the `ok` verdict names.
///
/// Five, and the number is MEASURED rather than chosen. One name hides the file behind it, and
/// `github.com/telekom/sutura#626` found a whole column within ten lines of the cap with one at
/// zero - so the window has to be wider than the tightest file. It was set to three until the
/// defect recurred while this was being written: a six-line test fixture took
/// `crates/sutura-exec-datafusion/src/lib.rs` from 995 lines to 1001, refusing a commit mid-slice
/// and truncating the rest of the sweep. **Counted at `a96440c8`, THREE files were tighter than it
/// was**, so a three-name readout would not have reached it - a count at a named commit and not a
/// rank, because #626's own correction is that a rank moves with every other file's length.
/// Printing every file under the cap instead is a page on every green run, and a success verdict
/// nobody reads is the silence this replaces.
const NEAR_CAP_REPORTED: usize = 5;

/// Hand-written source. An ignore pattern pointing here is rejected outright and the gate
/// fails: the fix for a 1200-line module is to split it, and an exemption list that can
/// swallow first-party code is a gate that quietly stops gating.
const UNEXEMPTABLE_PREFIXES: &[&str] = &["crates/", "xtask/"];

/// Patterns from the ignore file, split by what they promise.
struct Ignores {
    /// Generated, vendored or lock-like. Never reported.
    silent: Vec<String>,
    /// Hand-written and over the limit, with a split in progress. Reported as WARN, does
    /// not fail - visible debt rather than a silent exemption.
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
pub(crate) fn run(args: &[String]) -> Verdict {
    let max = match parse_max_lines(args) {
        Ok(max) => max,
        Err(message) => {
            eprintln!("xtask max-lines: {message}");
            return Verdict::Usage;
        }
    };
    let Some(root) = repo::root() else {
        eprintln!("xtask max-lines: could not locate the repo root");
        return Verdict::Fail;
    };

    let ignore_path = root.join(IGNORE_FILE);
    let ignores = Ignores::parse(&std::fs::read_to_string(&ignore_path).unwrap_or_default());

    let smuggled: Vec<&String> = ignores.all().filter(|p| is_unexemptable(p)).collect();
    if !smuggled.is_empty() {
        eprintln!("xtask max-lines: FAILED - first-party source cannot be exempted");
        for pattern in smuggled {
            eprintln!("  {IGNORE_FILE}: `{pattern}` targets hand-written source; split the file instead");
        }
        return Verdict::Fail;
    }

    // Every text file, decided by content: an extension list is a list to forget, and a
    // 5000-line generated file with an unlisted extension is exactly what this should catch.
    let (_root, mut files) = match repo::collect_text_files(&root, &root).into_listing(repo::Unmigrated::MaxLines) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask max-lines: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    files.sort();

    let mut violations: Vec<(String, usize)> = Vec::new();
    let mut warnings: Vec<(String, usize)> = Vec::new();
    let mut over_cap: Vec<String> = Vec::new();
    let mut under_cap: Vec<(&str, usize)> = Vec::new();
    for rel in &files {
        let lines = count_lines(&root.join(rel));
        if lines <= max {
            under_cap.push((rel, lines));
            continue;
        }
        over_cap.push(rel.clone());
        if ignores.warn.iter().any(|p| repo::matches(p, rel)) {
            warnings.push((rel.clone(), lines));
        } else if !ignores.silent.iter().any(|p| repo::matches(p, rel)) {
            violations.push((rel.clone(), lines));
        }
    }

    let inert = inert_entries(&ignores, &files, &over_cap);
    let near_cap = headroom(&under_cap, &ignores, max, NEAR_CAP_REPORTED);
    report(&files, &violations, &warnings, &inert, &near_cap, max)
}

/// The files closest to the cap, tightest first, as the lines the `ok` verdict prints.
///
/// **Computed every run, because a transcribed number rots.** `github.com/telekom/sutura#626`: the
/// cap said nothing until it refused, so the headroom was invisible until a commit was rejected
/// mid-change and the only figures anyone could cite had been typed into prose - where they were
/// found stale, one of them asserting a table entry *"is not available"* that a later refactor had
/// made false. The issue arguing that then carried a RANK of its own which was wrong the day it was
/// written, a rank being a function of two lengths that both move. The loop above already knows
/// every file's length, so this is a formatting question and not a measurement one.
///
/// **The figure is [`count_lines`]' definition of a line, not `wc -l`'s.** A file whose last line
/// has no terminator counts as a line here and not there, so the two disagree by one on it - the
/// committed fuzz corpus seeds are the measured instance. It is the same counter the cap itself is
/// applied with, which is the property that matters: the headroom printed is the headroom the
/// refusal will use.
///
/// **An exempted file is not on the wall, in either section.** The cap does not refuse it, so
/// printing its headroom would claim a limit that is not there - and the `[silent]` globs cover the
/// published prose, where the longest files in the tree live.
///
/// **What it cannot see is the walk that fed it.** These lines are derived from the same listing the
/// verdict's file count is, so a narrowed walk moves both together and this readout would report the
/// tightest file of a smaller tree without saying so - `github.com/telekom/sutura#414`'s shape,
/// which the `Unmigrated::MaxLines` door above is the record of. It makes the cap's remaining
/// headroom visible; it is not evidence about which files were read.
fn headroom(under_cap: &[(&str, usize)], ignores: &Ignores, max: usize, keep: usize) -> Vec<String> {
    let mut near: Vec<(&str, usize)> = under_cap
        .iter()
        .copied()
        .filter(|(rel, _)| !ignores.all().any(|pattern| repo::matches(pattern, rel)))
        .collect();
    // Stable, over a path-sorted listing, so equal headroom prints in path order rather than in
    // whatever order the walk happened to reach two files in.
    near.sort_by_key(|&(_, lines)| std::cmp::Reverse(lines));
    near.iter()
        .take(keep)
        .map(|&(path, lines)| format!("  {} lines left: {path}", max.saturating_sub(lines)))
        .collect()
}

/// Exemptions that exempt nothing, each with the sentence saying why.
///
/// **Literal paths only.** A glob is how generated and vendored trees are covered - `vendor/**`,
/// `docs/*.md`, `site/**` - and those legitimately match files that may or may not be over the cap
/// on any given day, so a per-file rule would fire on correct configuration. A pattern with no
/// glob metacharacter is a claim about one path, and it is the shape every hand-written exemption
/// here has.
///
/// Two rules, and the second is the one the ignore file's header already promised:
///
/// * **a literal naming no file in the tree** - in either section. The path was renamed or deleted
///   and the exemption outlived it.
/// * **a `[warn]` literal whose file is under the cap** - the promise was kept and the paragraph
///   arguing for it stayed. Only `[warn]`, for the reason in the module header: `[silent]` claims
///   something about a CLASS of file rather than about today's line count, and two of its literal
///   entries are correct while sitting well under the cap.
fn inert_entries(ignores: &Ignores, files: &[String], over_cap: &[String]) -> Vec<String> {
    let mut inert = Vec::new();
    for pattern in ignores.all().filter(|pattern| is_literal(pattern)) {
        let present = files.iter().any(|rel| repo::matches(pattern, rel));
        if !present {
            inert.push(format!(
                "`{pattern}` names no file in the tree - it was renamed or deleted and the exemption outlived it"
            ));
        } else if ignores.warn.iter().any(|warned| warned == pattern) && !over_cap.iter().any(|rel| repo::matches(pattern, rel)) {
            inert.push(format!(
                "`[warn]` `{pattern}` is under the cap, so it prints nothing - the promise to split was kept and the argument for the exemption stayed"
            ));
        }
    }
    inert
}

/// Is this pattern a plain path rather than a glob?
///
/// `*` and `?` are the two metacharacters `repo::matches` reads, so they are the two that decide.
fn is_literal(pattern: &str) -> bool {
    !pattern.contains('*') && !pattern.contains('?')
}

/// Both findings, then the verdict.
///
/// **BOTH, and the early return is why that needed saying.** The inert-exemption block returned
/// before the violations report, so with one stale `[warn]` entry a 1200-line file was not named -
/// measured in review: the exit code was right and the report was half of what the gate knew. A
/// gate that knows two numbers and prints one is this commit's own subject. The `ok` verdict carries
/// a THIRD number for the same reason - see [`headroom`], which is `#626` arriving at this function.
fn report(
    files: &[String],
    violations: &[(String, usize)],
    warnings: &[(String, usize)],
    inert: &[String],
    near_cap: &[String],
    max: usize,
) -> Verdict {
    for (path, lines) in warnings {
        println!("xtask max-lines: WARN {path} has {lines} lines (max {max}) - split pending");
    }
    if !inert.is_empty() {
        eprintln!(
            "xtask max-lines: FAILED - {} exemption(s) in {IGNORE_FILE} exempt nothing",
            inert.len()
        );
        for entry in inert {
            eprintln!("  {entry}");
        }
        eprintln!();
        eprintln!("  An exemption nothing needs is a claim nothing checks, and this file is a list of");
        eprintln!("  claims: the paragraph beside an entry is the only thing a reviewer reads to decide");
        eprintln!("  whether it is still earned. Delete the entry and its argument together.");
    }
    if !violations.is_empty() {
        eprintln!("xtask max-lines: FAILED - {} file(s) over {max} lines", violations.len());
        for (path, lines) in violations {
            eprintln!("  {path}: {lines} lines");
        }
        eprintln!("  split the file. Generated or vendored output belongs in {IGNORE_FILE}, nothing else does.");
    }
    if inert.is_empty() && violations.is_empty() {
        println!(
            "xtask max-lines: ok - {} files checked, none over {max} lines ({} warned)",
            files.len(),
            warnings.len()
        );
        for line in near_cap {
            println!("{line}");
        }
        return Verdict::Pass;
    }
    Verdict::Fail
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
    use super::{
        DEFAULT_MAX_LINES, Ignores, NEAR_CAP_REPORTED, headroom, inert_entries, is_literal, is_unexemptable, parse_max_lines,
    };
    use crate::Verdict;

    #[test]
    fn a_warn_entry_for_a_file_back_under_the_cap_is_reported() {
        // The measured instance: an entry added while the file was 1042 lines, still present after
        // the split took it to 911 with the argument for it intact, and the verdict read
        // `none over 1000 lines (0 warned)`. Nobody would have been told.
        let ignores = Ignores::parse("[silent]\nCargo.lock\n\n[warn]\n.github/workflows/ci.yml\n");
        let files = vec![String::from("Cargo.lock"), String::from(".github/workflows/ci.yml")];
        let discharged = inert_entries(&ignores, &files, &[String::from("Cargo.lock")]);
        assert_eq!(discharged.len(), 1, "{discharged:?}");
        assert!(discharged.first().is_some_and(|row| row.contains("[warn]")), "{discharged:?}");
        // AND THE ARM THAT STILL FIRES: while the file IS over the cap the entry is live, prints
        // its WARN and fails nothing. That is the state the exemption exists for.
        let live = inert_entries(&ignores, &files, &files);
        assert!(live.is_empty(), "{live:?}");
    }

    #[test]
    fn a_literal_entry_naming_a_path_that_is_gone_is_reported_in_either_section() {
        let ignores = Ignores::parse("[silent]\nremoved.lock\n\n[warn]\nalso-gone.md\n");
        let inert = inert_entries(&ignores, &[String::from("Cargo.lock")], &[]);
        assert_eq!(inert.len(), 2, "{inert:?}");
        assert!(inert.iter().all(|row| row.contains("names no file")), "{inert:?}");
    }

    #[test]
    fn a_silent_literal_under_the_cap_and_a_glob_matching_nothing_are_both_left_alone() {
        // MEASURED, and the reason this rule is not the one the report asked for: `devenv.lock` is
        // 86 lines and `flake.lock` 98, both correct - a lockfile's length is a function of the
        // dependency graph and may cross the cap any day. A rule failing every literal under the
        // cap would fail a clean tree.
        let ignores = Ignores::parse("[silent]\ndevenv.lock\ndocs/generated/*\nvendor/**\n");
        let files = vec![String::from("devenv.lock")];
        assert!(
            inert_entries(&ignores, &files, &[]).is_empty(),
            "no entry is inert against an empty file list"
        );
        assert!(is_literal("devenv.lock"));
        assert!(!is_literal("docs/generated/*"));
        assert!(!is_literal("vendor/**"));
        assert!(!is_literal("docs/adr/000?.md"));
    }

    /// The committed ignore file and the listing the gate itself walks, for the tests that assert
    /// over the real tree rather than a fixture.
    ///
    /// **One function because [`crate::repo::Census::into_listing`] is the transitional door
    /// `UNMIGRATED_DOORS` counts.** A second copy of this read raises that ratchet - measured, `51
    /// call site(s) ... 50 declared` - and the number is meant to fall as gates migrate, not rise
    /// for a duplicated read. So the two live-tree tests below share one door.
    fn committed_tree() -> (std::path::PathBuf, Ignores, Vec<String>) {
        let root = crate::repo::root().expect("the repo root");
        let ignores = Ignores::parse(&std::fs::read_to_string(root.join(super::IGNORE_FILE)).expect("the ignore file"));
        let (_root, files) = crate::repo::collect_text_files(&root, &root)
            .into_listing(crate::repo::Unmigrated::MaxLines)
            .expect("the tests run inside the repo");
        (root, ignores, files)
    }

    #[test]
    fn the_committed_ignore_file_has_no_inert_entry() {
        // Over the REAL file and the REAL tree, because the fixtures above prove the rule and not
        // the configuration. This is the assertion that reddens the day an entry's promise is kept.
        let (root, ignores, files) = committed_tree();
        let over_cap: Vec<String> = files
            .iter()
            .filter(|rel| super::count_lines(&root.join(rel)) > DEFAULT_MAX_LINES)
            .cloned()
            .collect();
        let inert = inert_entries(&ignores, &files, &over_cap);
        assert!(inert.is_empty(), "{inert:?}");
    }

    #[test]
    fn an_inert_exemption_does_not_hide_a_file_over_the_cap() {
        // Reported in review: the inert block returned before the violations report, so with a
        // stale `[warn]` entry present a 1200-line file went unnamed. Both are reported now, and
        // the verdict is a failure whichever of the two is non-empty.
        let over = [(String::from("BIGFILE.md"), 1200_usize)];
        let inert = [String::from("`[warn]` `README.md` is under the cap, so it prints nothing")];
        let files = [String::from("BIGFILE.md"), String::from("README.md")];
        assert_eq!(
            super::report(&files, &over, &[], &inert, &[], DEFAULT_MAX_LINES),
            Verdict::Fail
        );
        // Each half on its own is still a failure, and neither is a pass.
        assert_eq!(super::report(&files, &over, &[], &[], &[], DEFAULT_MAX_LINES), Verdict::Fail);
        assert_eq!(super::report(&files, &[], &[], &inert, &[], DEFAULT_MAX_LINES), Verdict::Fail);
        // AND THE ARM THAT STILL FIRES: neither half, and the success line is printed.
        assert_eq!(super::report(&files, &[], &[], &[], &[], DEFAULT_MAX_LINES), Verdict::Pass);
    }

    #[test]
    fn the_ok_verdict_names_the_files_closest_to_the_cap() {
        // `github.com/telekom/sutura#626`: the gate was binary, so a file creeping from 700 to 999
        // was invisible and the wall was met by whoever was unlucky. Five claims, and the fixture
        // is what makes each of them falsifiable on its own - tightest FIRST, the arithmetic, the
        // truncation, that an exempted file is not on the wall in EITHER section, and that the
        // counter holds at both ends: a file AT the cap reports zero left, an EMPTY one reports the
        // whole cap and sorts last rather than dividing by a length nobody has.
        let ignores = Ignores::parse("[silent]\ndocs/*.md\n\n[warn]\nflake.nix\n");
        let under_cap = [
            ("crates/sutura-domain/src/a.rs", 1000),
            ("crates/sutura-domain/src/b.rs", 900),
            ("docs/prose.md", 999),
            ("flake.nix", 998),
            ("xtask/src/c.rs", 997),
            ("xtask/src/d.rs", 10),
            ("xtask/src/empty.rs", 0),
        ];
        assert_eq!(
            headroom(&under_cap, &ignores, DEFAULT_MAX_LINES, 3),
            [
                "  0 lines left: crates/sutura-domain/src/a.rs",
                "  3 lines left: xtask/src/c.rs",
                "  100 lines left: crates/sutura-domain/src/b.rs",
            ]
        );
        // The same fixture with room for every non-exempt file: the truncation above is the only
        // thing that hid the last two, and the order runs to the end without the two exempt files
        // appearing anywhere in it.
        assert_eq!(
            headroom(&under_cap, &ignores, DEFAULT_MAX_LINES, 6),
            [
                "  0 lines left: crates/sutura-domain/src/a.rs",
                "  3 lines left: xtask/src/c.rs",
                "  100 lines left: crates/sutura-domain/src/b.rs",
                "  990 lines left: xtask/src/d.rs",
                "  1000 lines left: xtask/src/empty.rs",
            ]
        );
        // A tree with nothing to report prints nothing, rather than a line about no file.
        assert_eq!(headroom(&[], &ignores, DEFAULT_MAX_LINES, 3), [] as [String; 0]);

        // AND OVER THE REAL TREE, because the fixture proves the arithmetic and not that the
        // readout survives the walk the gate actually does. This is the assertion that reddens if
        // the exemption filter ever matches everything: the verdict would still say `ok`, the
        // readout would be empty, and the wall would be invisible again with nobody told.
        let (root, committed, files) = committed_tree();
        let lengths: Vec<(&str, usize)> = files
            .iter()
            .map(|rel| (rel.as_str(), super::count_lines(&root.join(rel))))
            .filter(|&(_, lines)| lines <= DEFAULT_MAX_LINES)
            .collect();
        let live = headroom(&lengths, &committed, DEFAULT_MAX_LINES, NEAR_CAP_REPORTED);
        assert_eq!(live.len(), NEAR_CAP_REPORTED, "{live:?}");
        assert!(live.iter().all(|line| line.contains(" lines left: ")), "{live:?}");
    }

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
        assert!(ignores.warn.is_empty(), "the parsed ignore file has no warning-only entries");
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
