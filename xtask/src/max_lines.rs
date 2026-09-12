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

    // **`NotFound` is not a read failure**, the split this whole module now makes once: a tree
    // with no ignore file legitimately has no exemptions, while a file that exists and will not be
    // read would have silently produced the same empty list - and an empty list is STRICTER, so it
    // would not have shown up as a red. It is the `inert_entries` half that fails open there: every
    // committed exemption reads as inert, so nothing is reported.
    let ignores = match ignores_at(&root.join(IGNORE_FILE)) {
        Ok(ignores) => ignores,
        Err(why) => {
            eprintln!("xtask max-lines: FAILED - {why}");
            return Verdict::Fail;
        }
    };

    let smuggled: Vec<&String> = ignores.all().filter(|p| is_unexemptable(p)).collect();
    if !smuggled.is_empty() {
        eprintln!("xtask max-lines: FAILED - first-party source cannot be exempted");
        for pattern in smuggled {
            eprintln!("  {IGNORE_FILE}: `{pattern}` targets hand-written source; split the file instead");
        }
        return Verdict::Fail;
    }

    let census = match repo::all_files() {
        Ok(census) => census,
        Err(why) => {
            eprintln!("xtask max-lines: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let measured = match measure(census, &[ANCHOR], max, &ignores) {
        Ok(measured) => measured,
        Err(why) => {
            eprintln!("xtask max-lines: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let inert = inert_entries(&ignores, &measured.files, &measured.over_cap);
    report(&measured, &inert, max)
}

/// The exemptions, with `NotFound` split from every other read failure.
///
/// **A tree with no ignore file legitimately has no exemptions**, and that is the only absence this
/// accepts. The `unwrap_or_default()` this replaced treated an ignore file that exists and will not
/// be read as the same thing, which reads as fail-closed - no exemptions is stricter - and is not:
/// [`inert_entries`] then judges every committed exemption against an empty parse, reports none,
/// and the gate says `ok` about a claims list it never read.
fn ignores_at(path: &Path) -> Result<Ignores, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Ignores::parse(&text)),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(Ignores::parse("")),
        Err(why) => Err(format!("could not read {}: {why}", path.display())),
    }
}

/// The file this gate cannot have a verdict without.
///
/// `flake.nix` for the same reason `text-hygiene` and `line-endings` anchor on it: it is present in
/// every checkout and in the Nix build sandbox, it is the marker `repo::root` itself looks for, and
/// it is a `[warn]` entry in the ignore file - so a scan that stopped reaching it would also stop
/// reporting the one live exemption.
const ANCHOR: &str = "flake.nix";

/// What one pass over the tree measured.
struct Measured {
    /// Every subject the census read, so [`inert_entries`] judges an exemption against files whose
    /// presence was OBSERVED rather than against a list a failed read could shorten.
    files: Vec<String>,
    violations: Vec<(String, usize)>,
    warnings: Vec<(String, usize)>,
    over_cap: Vec<String>,
    /// Files the length rule actually ran over - text ones. **The gate's own number**, beside
    /// `witness`, which is the census's.
    checked: usize,
    /// The census's own sentence: discovered, judged, out of scope, absent, bytes read.
    witness: String,
}

/// Measure every subject in the census, over a census this function does not mint.
///
/// **Two fail-opens closed here, and they were separate defects.** `repo::collect_text_files`
/// decided scope with `repo::is_text_file`, which OPENS the file and answers `false` for one it
/// cannot - so an unreadable file left the walk as *not text*. Then `count_lines` was
/// `read_to_string(path).map_or(0, ..)`, and its doc argued the fail-open outright: *"a read error
/// is a different problem"*. It was nobody's problem, and the consequence was that an unreadable
/// file could never trip the **unexemptable** 1000-line cap. Re-measured on `bf59f9dc` with
/// `crates/sutura-domain/src/lib.rs` at mode `000`: `1326 files checked` became `1325`, both exit 0.
///
/// Textness is decided from the bytes the census already read, so *not text* and *could not look*
/// cannot be conflated, and a binary file is out of the LENGTH rule without being refused - which
/// is #412's trap, where `check-shipped-binaries` reddened a correct tree over a PNG.
fn measure(census: repo::Census, must_judge: &[&str], max: usize, ignores: &Ignores) -> Result<Measured, repo::Refusal> {
    let mut measured = Measured {
        files: Vec::new(),
        violations: Vec::new(),
        warnings: Vec::new(),
        over_cap: Vec::new(),
        checked: 0,
        witness: String::new(),
    };
    let scope: repo::Scope = every_subject;
    let inspected = census.inspect(must_judge, scope, |rel, bytes| {
        measured.files.push(String::from(rel));
        if !repo::looks_like_text(bytes) {
            return;
        }
        measured.checked = measured.checked.saturating_add(1);
        // Lossy rather than a UTF-8 read: a file the census opened is one this gate measures, and
        // `read_to_string` would have counted a file it could not decode as zero lines.
        let lines = String::from_utf8_lossy(bytes).lines().count();
        if lines <= max {
            return;
        }
        measured.over_cap.push(String::from(rel));
        if ignores.warn.iter().any(|p| repo::matches(p, rel)) {
            measured.warnings.push((String::from(rel), lines));
        } else if !ignores.silent.iter().any(|p| repo::matches(p, rel)) {
            measured.violations.push((String::from(rel), lines));
        }
    })?;
    measured.files.sort();
    measured.witness = inspected.verdict();
    Ok(measured)
}

/// Every subject, because textness is a question about bytes and a [`repo::Scope`] is handed a
/// path. A bare `fn` with nothing captured, so it cannot count what it passes.
const fn every_subject(_rel: &str) -> bool {
    true
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
/// gate that knows two numbers and prints one is this commit's own subject.
fn report(measured: &Measured, inert: &[String], max: usize) -> Verdict {
    for (path, lines) in &measured.warnings {
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
    if !measured.violations.is_empty() {
        eprintln!(
            "xtask max-lines: FAILED - {} file(s) over {max} lines",
            measured.violations.len()
        );
        for (path, lines) in &measured.violations {
            eprintln!("  {path}: {lines} lines");
        }
        eprintln!("  split the file. Generated or vendored output belongs in {IGNORE_FILE}, nothing else does.");
    }
    if inert.is_empty() && measured.violations.is_empty() {
        // The census's witness beside this gate's count, taken by a different predicate on the
        // other side of the walk: `checked` is the closure's tally of text files, `witness` is the
        // census's tally of subjects it opened. A narrowing that moved one cannot move both.
        println!(
            "xtask max-lines: ok - {} files checked, none over {max} lines ({} warned); {}",
            measured.checked,
            measured.warnings.len(),
            measured.witness
        );
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

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MAX_LINES, Ignores, Measured, inert_entries, is_literal, is_unexemptable, parse_max_lines};
    use crate::Verdict;

    /// A [`Measured`] carrying the named violations and nothing else, for [`super::report`]'s arms.
    fn measured(violations: &[(&str, usize)]) -> Measured {
        Measured {
            files: vec![String::from("BIGFILE.md"), String::from("README.md")],
            violations: violations.iter().map(|(path, lines)| (String::from(*path), *lines)).collect(),
            warnings: Vec::new(),
            over_cap: Vec::new(),
            checked: 2,
            witness: String::from("a scratch witness"),
        }
    }

    /// This gate's own measurement, over a scratch tree rather than over the repo.
    ///
    /// `repo::collect_files` is an existing census door and it takes a ROOT. Its extension arm does
    /// not open a file, so a sealed fixture reaches [`super::measure`]'s read and fails there,
    /// which is the path under test - the `is_text_file` door this replaced would have dropped the
    /// same file from the walk before any of this ran.
    fn measure_over(
        tree: &crate::scratch_tree::Tree,
        anchors: &[&str],
        ignores: &Ignores,
    ) -> Result<Measured, crate::repo::Refusal> {
        super::measure(
            crate::repo::collect_files(tree.root(), tree.root(), &["md", "nix", "png", "rs"]),
            anchors,
            DEFAULT_MAX_LINES,
            ignores,
        )
    }

    /// `n` lines of text, for the length rule.
    fn lines(n: usize) -> Vec<u8> {
        "x\n".repeat(n).into_bytes()
    }

    /// The defect, and the half that made it expensive: an unreadable file could never trip the
    /// **unexemptable** cap, because `count_lines` was `read_to_string(..).map_or(0, ..)` and its
    /// own doc argued that a read error *"is a different problem"*. Nobody owned that problem. This
    /// fixture is 1200 lines under `crates/`, which no exemption may reach.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_over_the_cap_refuses_instead_of_counting_zero_lines() {
        let ignores = Ignores::parse("");
        let over = lines(1200);
        let mut tree = crate::scratch_tree::Tree::of(
            "max-lines-sealed",
            &[("flake.nix", b"{ }\n"), ("crates/thing/src/lib.rs", over.as_slice())],
        );
        let control = measure_over(&tree, &[super::ANCHOR], &ignores).expect("a readable tree measures");
        assert_eq!(control.violations.len(), 1, "{}", control.witness);

        if !tree.seal("crates/thing/src/lib.rs") {
            // Mode bits ignored for this uid; asserting a refusal here would assert nothing.
            return;
        }
        let Err(why) = measure_over(&tree, &[super::ANCHOR], &ignores) else {
            panic!("an unreadable file over the cap produced a verdict");
        };
        assert!(
            why.describe().contains("crates/thing/src/lib.rs"),
            "the refusal has to name the file it could not read: {}",
            why.describe()
        );
    }

    /// #412's trap: a PNG is out of the LENGTH rule without being unreadable, and a remedy that
    /// refuses everything it did not decode reddens a correct tree - which is what
    /// `check-shipped-binaries` did. Textness is decided from the bytes the census read.
    #[test]
    fn binary_data_is_read_and_left_out_of_the_length_rule_rather_than_refused() {
        let ignores = Ignores::parse("");
        let mut png = b"\x89PNG\r\n\x1a\n\x00".to_vec();
        png.extend(lines(1200));
        let tree = crate::scratch_tree::Tree::of(
            "max-lines-binary",
            &[("flake.nix", b"{ }\n"), ("docs/diagram.png", png.as_slice())],
        );
        let measured = measure_over(&tree, &[super::ANCHOR], &ignores).expect("a PNG is not unreadable");
        assert!(measured.violations.is_empty(), "{:?}", measured.violations);
        assert_eq!(measured.checked, 1, "only the text file is measured: {}", measured.witness);
        assert_eq!(measured.files.len(), 2, "both subjects were read: {}", measured.witness);
    }

    /// An empty discovery is a refusal, so this gate has no count of its own to satisfy by
    /// measuring nothing.
    #[test]
    fn an_empty_scope_refuses_rather_than_reporting_zero_files() {
        let ignores = Ignores::parse("");
        let tree = crate::scratch_tree::Tree::of("max-lines-empty", &[("unlisted.bin", b"\x00")]);
        let Err(why) = measure_over(&tree, &[], &ignores) else {
            panic!("an empty discovery produced a verdict");
        };
        assert!(why.describe().contains("no subject at all"), "{}", why.describe());
    }

    /// A narrowed enumeration that no longer reaches the anchor refuses by NAME, whatever the count
    /// says - the arm a `files == 0` floor cannot hold.
    #[test]
    fn an_enumeration_that_no_longer_reaches_the_anchor_refuses() {
        let ignores = Ignores::parse("");
        let tree = crate::scratch_tree::Tree::of("max-lines-anchor", &[("docs/page.md", b"short\n")]);
        let Err(why) = measure_over(&tree, &[super::ANCHOR], &ignores) else {
            panic!("a tree without the anchor produced a verdict");
        };
        assert!(why.describe().contains(super::ANCHOR), "{}", why.describe());
    }

    /// An ignore file that is ABSENT is no exemptions; one that exists and cannot be read is a
    /// failure. `unwrap_or_default()` made both the first, and the second then reported every
    /// committed exemption as inert - or rather, reported nothing, because the parse was empty.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_ignore_file_refuses_while_an_absent_one_is_simply_no_exemptions() {
        let mut tree = crate::scratch_tree::Tree::of("max-lines-ignores", &[("present", b"[silent]\nCargo.lock\n")]);
        let absent = super::ignores_at(&tree.root().join("not-here")).expect("an absent ignore file is no exemptions");
        assert!(
            absent.silent.is_empty() && absent.warn.is_empty(),
            "an absent file parsed exemptions"
        );
        let read = super::ignores_at(&tree.root().join("present")).expect("a readable ignore file parses");
        assert_eq!(read.silent, vec!["Cargo.lock"]);

        if !tree.seal("present") {
            return;
        }
        let Err(why) = super::ignores_at(&tree.root().join("present")) else {
            panic!("an unreadable ignore file parsed as no exemptions");
        };
        assert!(why.contains("present"), "{why}");
    }

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

    #[test]
    fn the_committed_ignore_file_has_no_inert_entry() {
        // Over the REAL file and the REAL tree, because the fixtures above prove the rule and not
        // the configuration. This is the assertion that reddens the day an entry's promise is kept.
        let root = crate::repo::root().expect("the repo root");
        let ignores = Ignores::parse(&std::fs::read_to_string(root.join(super::IGNORE_FILE)).expect("the ignore file"));
        let census = crate::repo::all_files().expect("the tests run inside the repo");
        let measured = super::measure(census, &[super::ANCHOR], DEFAULT_MAX_LINES, &ignores).expect("the tree is readable");
        let inert = inert_entries(&ignores, &measured.files, &measured.over_cap);
        assert!(inert.is_empty(), "{inert:?}");
    }

    #[test]
    fn an_inert_exemption_does_not_hide_a_file_over_the_cap() {
        // Reported in review: the inert block returned before the violations report, so with a
        // stale `[warn]` entry present a 1200-line file went unnamed. Both are reported now, and
        // the verdict is a failure whichever of the two is non-empty.
        let inert = [String::from("`[warn]` `README.md` is under the cap, so it prints nothing")];
        let over = measured(&[("BIGFILE.md", 1200)]);
        let clean = measured(&[]);
        assert_eq!(super::report(&over, &inert, DEFAULT_MAX_LINES), Verdict::Fail);
        // Each half on its own is still a failure, and neither is a pass.
        assert_eq!(super::report(&over, &[], DEFAULT_MAX_LINES), Verdict::Fail);
        assert_eq!(super::report(&clean, &inert, DEFAULT_MAX_LINES), Verdict::Fail);
        // AND THE ARM THAT STILL FIRES: neither half, and the success line is printed.
        assert_eq!(super::report(&clean, &[], DEFAULT_MAX_LINES), Verdict::Pass);
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
