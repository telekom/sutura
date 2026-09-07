//! Every `RefusalReason` variant is provoked by a test, or listed with a date and a reason.
//!
//! `AGENTS.md` states the rule as *"a variant no test can provoke is what that enum refuses to
//! carry"*, and nothing kept it true. The two transports hold exhaustive matches over the enum, so
//! a variant added to the domain fails to compile until somebody assigns it a status and a
//! sentence - and **that is a compile-time mechanism for RENDERING a variant, not for provoking
//! one.** A variant can be added, rendered, shipped and triggered by nothing.
//!
//! # What counts as provoked, and why a census does not
//!
//! Two kinds of evidence, both syntactic:
//!
//! * a **test-code** occurrence of `RefusalReason::<Variant>` under `crates/`, found by the same
//!   region machinery the causality gate uses - so a `#[cfg(test)]` module in a `src` file counts
//!   and the production match arm beside it does not;
//! * a **committed snapshot** naming the variant. That is the refusal corpus: a question was asked,
//!   the refusal came back, and `insta` recorded it - which is stronger evidence than a Rust
//!   identifier, because the value was produced rather than written.
//!
//! **A file that names EVERY variant is a census, and a census is evidence about none of them.** It
//! is the completeness of the enum being asserted, which the exhaustive matches already force. FIVE
//! files in this tree are censuses - the gate names them in its own verdict rather than counting them -
//! and the rule is not hypothetical for any: `sutura_app::prompt`'s test builds one instance of every
//! variant to check that every refusal has guidance; the two transports' refusal tests hand every
//! variant to their mappers; and - the two that make the rule necessary rather than tidy - **the
//! rendered prompt, snapshotted twice under `crates/sutura-cli/tests/snapshots/`, lists all seventeen
//! refusal guides by name.** Without the census rule those two snapshots alone would make every
//! variant look provoked, and this gate would be decoration.
//!
//! # Scope, and the limits
//!
//! `crates/**` only. Nothing outside can construct a `RefusalReason` - `xtask` does not depend on
//! the domain - so a name elsewhere is text. This module's own fixtures use fabricated variant
//! names for the same reason stated the other way round.
//!
//! * **A NAME is not a provocation.** A test that mentions a variant in an `assert_ne!` counts here.
//!   Deciding otherwise means knowing what a test asserts, which no text scan can; what this gate
//!   buys is that a variant nothing anywhere mentions cannot arrive silently.
//! * **The region walk is the causality gate's**, so its limits are inherited: only the exact
//!   attribute `#[cfg(test)]` counts, and a `#[path = ".."] mod` is not followed. Both directions are
//!   conservative here - an unrecognised test region reads as production code, so evidence is
//!   missed rather than invented.
//! * **The allow file is the escape hatch and it is a ratchet, not a silencer.** A stale entry - one
//!   naming a variant that IS provoked now, or a name the enum no longer has - fails the gate, which
//!   is `deny.toml`'s `unused-allowed-license = "deny"` pointed at this list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::causality::regions;
use crate::{Verdict, repo};

/// Where the enum is declared. A constant so a move fails this gate loudly rather than making it
/// check nothing.
const DECLARED_IN: &str = "crates/sutura-domain/src/query.rs";

/// The declaration line the walk starts at.
const DECLARATION: &str = "pub enum RefusalReason {";

/// Where a variant nothing provokes is argued for.
const ALLOW_FILE: &str = "devco/refusals-unprovoked-allow";

/// One deliberate exception, as the allow file spells it.
struct Excused {
    /// The variant's name.
    variant: String,
    /// The ISO date the exception was taken.
    dated: String,
    /// Why, and what would end it.
    why: String,
    /// 1-based line in the allow file, for the message.
    line: usize,
}

/// What the scan found: which files name which variants, and which of those files are censuses.
struct Evidence {
    /// Variant to the files naming it, censuses already removed.
    by_variant: BTreeMap<String, BTreeSet<String>>,
    /// Files naming every variant. Reported on success, so the rule is visible in the verdict.
    censuses: BTreeSet<String>,
    /// Files read. Printed so a gate that scanned nothing cannot say `ok`.
    scanned: usize,
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::Refusals)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask check-refusal-coverage: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    let declared = match declared_variants(&root) {
        Ok(names) => names,
        Err(message) => {
            eprintln!("xtask check-refusal-coverage: {message}");
            return Verdict::Fail;
        }
    };
    let excused = match excuses(&root) {
        Ok(entries) => entries,
        Err(message) => {
            eprintln!("xtask check-refusal-coverage: {message}");
            return Verdict::Fail;
        }
    };
    let evidence = provocations(&root, &files, &declared);
    report(&declared, &excused, &evidence)
}

/// Decide, and say why.
fn report(declared: &[String], excused: &[Excused], evidence: &Evidence) -> Verdict {
    let mut problems: Vec<String> = Vec::new();
    let excused_names: BTreeSet<&str> = excused.iter().map(|e| e.variant.as_str()).collect();

    for variant in declared {
        let provoked = evidence.by_variant.contains_key(variant);
        if !provoked && !excused_names.contains(variant.as_str()) {
            problems.push(format!(
                "{variant}: no test names it and no snapshot records it, and {ALLOW_FILE} does not excuse it"
            ));
        }
    }
    // The other direction, which is what makes the list a ratchet: an excuse for a variant that is
    // provoked now reads as coverage nobody has, and one naming a variant the enum has lost reads
    // as a rule still being applied.
    for entry in excused {
        if !declared.contains(&entry.variant) {
            problems.push(format!(
                "{ALLOW_FILE}:{}: `{}` is not a RefusalReason variant - it was renamed or removed",
                entry.line, entry.variant
            ));
            continue;
        }
        if let Some(files) = evidence.by_variant.get(&entry.variant) {
            problems.push(format!(
                "{ALLOW_FILE}:{}: `{}` IS provoked now ({}) - delete the entry",
                entry.line,
                entry.variant,
                files.iter().take(2).cloned().collect::<Vec<String>>().join(", ")
            ));
        }
        if entry.dated.is_empty() || entry.why.is_empty() {
            problems.push(format!(
                "{ALLOW_FILE}:{}: `{}` needs a date and a reason on the same line",
                entry.line, entry.variant
            ));
        }
    }

    if problems.is_empty() {
        let provoked = declared.len().saturating_sub(excused.len());
        println!(
            "xtask check-refusal-coverage: ok - {provoked}/{} variant(s) provoked, {} excused, \
             {} file(s) read",
            declared.len(),
            excused.len(),
            evidence.scanned
        );
        // Named rather than counted, because the census rule is the surprising half of this gate:
        // a reader who sees the rendered prompt in this list understands why it counts for nothing.
        for census in &evidence.censuses {
            println!("  census (names every variant, so evidence for none): {census}");
        }
        return Verdict::Pass;
    }

    eprintln!("xtask check-refusal-coverage: FAILED");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    explain();
    Verdict::Fail
}

/// Printed on failure, because a rule whose reason is unstated gets reverted.
fn explain() {
    eprintln!("A refusal is the tool surface's answer to a question this deployment declines, so a");
    eprintln!("variant nothing provokes is a sentence a caller may never have been shown. The two");
    eprintln!("transports' exhaustive matches force a variant to be RENDERED, not triggered.");
    eprintln!();
    eprintln!("What counts: a `RefusalReason::<Variant>` in test code under crates/, or a committed");
    eprintln!("snapshot naming it. A file naming EVERY variant is a census and counts for none of");
    eprintln!("them - the rendered prompt lists all of them, and so does each transport's mapper");
    eprintln!("test.");
    eprintln!();
    eprintln!("If a variant genuinely cannot be provoked yet, say so in");
    eprintln!("  devco/refusals-unprovoked-allow");
    eprintln!("with the date and what would end the exception. An entry is a decision somebody");
    eprintln!("wrote down; a variant nobody can explain belongs out of the enum.");
}

/// The variant names `RefusalReason` declares.
fn declared_variants(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join(DECLARED_IN);
    let text = std::fs::read_to_string(&path).map_err(|cause| format!("{DECLARED_IN} could not be read: {cause}"))?;
    let at = text
        .find(DECLARATION)
        .ok_or_else(|| format!("{DECLARED_IN} no longer declares `{DECLARATION}` - this gate would check nothing"))?;
    let body = enum_body(&text, at.saturating_add(DECLARATION.len()))
        .ok_or_else(|| format!("{DECLARED_IN}: the RefusalReason body has unbalanced braces"))?;
    let names = variant_names(body);
    if names.is_empty() {
        return Err(format!(
            "{DECLARED_IN}: RefusalReason declares no variants - this gate would check nothing"
        ));
    }
    Ok(names)
}

/// The text of the enum body that starts just after its opening brace at `from`.
fn enum_body(text: &str, from: usize) -> Option<&str> {
    let rest = text.get(from..)?;
    let mut depth = 1_usize;
    for (at, character) in rest.char_indices() {
        match character {
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return rest.get(..at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The top-level variant names in an enum body.
///
/// Depth-counted, so a field named `Something` inside a struct variant is not a variant, and
/// indentation is not what decides. A doc comment or an attribute line has no identifier followed
/// by `{`, `(` or `,` at depth zero, so neither reads as one.
fn variant_names(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut depth = 0_usize;
    for line in body.lines() {
        let trimmed = line.trim();
        if depth == 0
            && let Some(name) = variant_at(trimmed)
        {
            names.push(String::from(name));
        }
        depth = depth
            .saturating_add(trimmed.matches('{').count())
            .saturating_sub(trimmed.matches('}').count());
    }
    names
}

/// The variant name this line declares, or `None`.
fn variant_at(trimmed: &str) -> Option<&str> {
    let mut characters = trimmed.char_indices();
    let (_, first) = characters.next()?;
    if !first.is_ascii_uppercase() {
        return None;
    }
    let end = trimmed
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(trimmed.len());
    let name = trimmed.get(..end)?;
    let rest = trimmed.get(end..)?.trim_start();
    // `Name {`, `Name(`, `Name,` and a trailing `Name` are the four shapes a variant is written in.
    (rest.is_empty() || rest.starts_with('{') || rest.starts_with('(') || rest.starts_with(',')).then_some(name)
}

/// Which files name which variants, censuses removed.
fn provocations(root: &Path, files: &[String], declared: &[String]) -> Evidence {
    let read = |rel: &str| -> Option<String> { std::fs::read_to_string(root.join(rel)).ok() };
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut censuses: BTreeSet<String> = BTreeSet::new();
    let mut scanned = 0_usize;

    for rel in files {
        let Some(named) = named_in(root, rel, declared, &read) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        if named.len() == declared.len() {
            censuses.insert(rel.clone());
            continue;
        }
        for variant in named {
            found.entry(variant).or_default().insert(rel.clone());
        }
    }
    Evidence {
        by_variant: found,
        censuses,
        scanned,
    }
}

/// The variants this file names, or `None` when the file is out of scope.
fn named_in(root: &Path, rel: &str, declared: &[String], read: &regions::PostImage<'_>) -> Option<BTreeSet<String>> {
    if !rel.starts_with("crates/") {
        return None;
    }
    if has_extension(rel, "snap") {
        let text = std::fs::read_to_string(root.join(rel)).ok()?;
        return Some(mentioned(&text, declared));
    }
    if !has_extension(rel, "rs") {
        return None;
    }
    let text = std::fs::read_to_string(root.join(rel)).ok()?;
    let scope = regions::scope(rel, read);
    let mut named = BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        if !scope.covers(index.saturating_add(1)) {
            continue;
        }
        for variant in declared {
            if line.contains(&format!("RefusalReason::{variant}")) && names_exactly(line, variant) {
                named.insert(variant.clone());
            }
        }
    }
    Some(named)
}

/// Is this path's extension `wanted`? Case-insensitive, because half of this repo is developed on
/// a case-insensitive filesystem and a case-sensitive test there is a silent hole.
fn has_extension(rel: &str, wanted: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case(wanted))
}

/// Does `line` name `variant` as a whole word after the `RefusalReason::` path?
///
/// The suffix check matters because the enum has no two variants where one is a prefix of the
/// other today and nothing keeps that true - `ResultTooLarge` and a future `ResultTooLargeForX`
/// would otherwise be one name.
fn names_exactly(line: &str, variant: &str) -> bool {
    let needle = format!("RefusalReason::{variant}");
    let mut rest = line;
    while let Some(at) = rest.find(&needle) {
        let after = rest.get(at.saturating_add(needle.len())..).unwrap_or_default();
        if !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            return true;
        }
        rest = after;
    }
    false
}

/// Variants named anywhere in `text`, as whole words.
fn mentioned(text: &str, declared: &[String]) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    for variant in declared {
        if whole_word(text, variant) {
            named.insert(variant.clone());
        }
    }
    named
}

/// Is `word` in `text` with no identifier character either side?
fn whole_word(text: &str, word: &str) -> bool {
    let mut rest = text;
    while let Some(at) = rest.find(word) {
        let before_is_word = rest
            .get(..at)
            .and_then(|head| head.chars().next_back())
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let after = rest.get(at.saturating_add(word.len())..).unwrap_or_default();
        let after_is_word = after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
        if !before_is_word && !after_is_word {
            return true;
        }
        rest = after;
    }
    false
}

/// The allow file's entries.
fn excuses(root: &Path) -> Result<Vec<Excused>, String> {
    let path = root.join(ALLOW_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // An absent file means no exceptions, which is the state this gate hopes to reach.
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(cause) => return Err(format!("{ALLOW_FILE} could not be read: {cause}")),
    };
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        entries.push(parse_excuse(trimmed, index.saturating_add(1)));
    }
    Ok(entries)
}

/// One allow-file line: `<Variant> <ISO date> <why>`.
fn parse_excuse(trimmed: &str, line: usize) -> Excused {
    let mut fields = trimmed.splitn(3, char::is_whitespace);
    let variant = fields.next().unwrap_or_default();
    let dated = fields.next().unwrap_or_default();
    let why = fields.next().unwrap_or_default().trim();
    Excused {
        variant: String::from(variant),
        dated: String::from(if looks_like_a_date(dated) { dated } else { "" }),
        why: String::from(why),
        line,
    }
}

/// Is this an ISO date? Shape only - a gate that validated calendars would be a calendar.
fn looks_like_a_date(field: &str) -> bool {
    field.len() == 10
        && field
            .char_indices()
            .all(|(at, c)| if at == 4 || at == 7 { c == '-' } else { c.is_ascii_digit() })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{
        Evidence, Excused, enum_body, looks_like_a_date, mentioned, names_exactly, parse_excuse, report, variant_names,
        whole_word,
    };
    use crate::Verdict;

    /// Fabricated variant names throughout, so this module's own fixtures cannot be read as
    /// evidence by the gate that scans them.
    fn declared() -> Vec<String> {
        vec![String::from("Alpha"), String::from("Beta"), String::from("Gamma")]
    }

    /// A variant, and the files naming it - the shape every fixture below spells.
    type Named<'a> = (&'a str, &'a [&'a str]);

    fn evidence(pairs: &[Named<'_>], censuses: &[&str]) -> Evidence {
        let mut by_variant: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (variant, files) in pairs {
            let set: BTreeSet<String> = files.iter().map(|f| String::from(*f)).collect();
            drop(by_variant.insert(String::from(*variant), set));
        }
        Evidence {
            by_variant,
            censuses: censuses.iter().map(|c| String::from(*c)).collect(),
            scanned: 9,
        }
    }

    fn excused(variant: &str, dated: &str, why: &str) -> Excused {
        Excused {
            variant: String::from(variant),
            dated: String::from(dated),
            why: String::from(why),
            line: 7,
        }
    }

    #[test]
    fn every_refusal_variant_is_provoked_or_listed_with_a_reason() {
        let all = evidence(
            &[
                ("Alpha", &["crates/a/src/tests.rs"]),
                ("Beta", &["crates/a/tests/golden.rs"]),
                ("Gamma", &["crates/a/tests/snapshots/x.snap"]),
            ],
            &[],
        );
        assert_eq!(report(&declared(), &[], &all), Verdict::Pass);
    }

    #[test]
    fn a_variant_nothing_provokes_fails() {
        let partial = evidence(&[("Alpha", &["crates/a/src/tests.rs"])], &[]);
        assert_eq!(report(&declared(), &[], &partial), Verdict::Fail);
    }

    #[test]
    fn the_allow_file_is_what_makes_it_pass_instead() {
        let partial = evidence(&[("Alpha", &["crates/a/src/tests.rs"])], &[]);
        let excuses = vec![
            excused("Beta", "2026-09-02", "needs a third source"),
            excused("Gamma", "2026-09-02", "needs two remote dimensions"),
        ];
        assert_eq!(report(&declared(), &excuses, &partial), Verdict::Pass);
    }

    #[test]
    fn an_excuse_for_a_variant_that_is_provoked_now_is_itself_a_failure() {
        // The ratchet direction, and `deny.toml`'s `unused-allowed-license = "deny"` is the
        // precedent: a stale exception reads as a rule still being applied.
        let all = evidence(
            &[
                ("Alpha", &["crates/a/src/tests.rs"]),
                ("Beta", &["crates/a/src/tests.rs"]),
                ("Gamma", &["crates/a/src/tests.rs"]),
            ],
            &[],
        );
        let excuses = vec![excused("Beta", "2026-09-02", "stale now")];
        assert_eq!(report(&declared(), &excuses, &all), Verdict::Fail);
    }

    #[test]
    fn an_excuse_naming_something_that_is_not_a_variant_fails() {
        let all = evidence(
            &[
                ("Alpha", &["crates/a/src/tests.rs"]),
                ("Beta", &["crates/a/src/tests.rs"]),
                ("Gamma", &["crates/a/src/tests.rs"]),
            ],
            &[],
        );
        let excuses = vec![excused("Delta", "2026-09-02", "renamed away")];
        assert_eq!(report(&declared(), &excuses, &all), Verdict::Fail);
    }

    #[test]
    fn an_excuse_with_no_date_or_no_reason_fails() {
        let partial = evidence(&[("Alpha", &["crates/a/src/tests.rs"])], &[]);
        let undated = vec![
            excused("Beta", "", "a reason but no date"),
            excused("Gamma", "2026-09-02", ""),
        ];
        assert_eq!(report(&declared(), &undated, &partial), Verdict::Fail);
    }

    #[test]
    fn a_census_counts_for_no_variant() {
        // The rendered prompt lists every refusal guide by name, so without this rule one
        // snapshot would make the whole enum look provoked.
        let only_a_census = evidence(&[], &["crates/cli/tests/snapshots/example_prompt.snap"]);
        assert_eq!(report(&declared(), &[], &only_a_census), Verdict::Fail);
    }

    #[test]
    fn the_variant_walk_reads_the_top_level_only() {
        let body = "\n    /// A doc comment.\n    MetricUnknown { metric: MetricName },\n    Bare,\n    Tuple(u8),\n    Nested {\n        Inner: u8,\n    },\n";
        assert_eq!(
            variant_names(body),
            vec![
                String::from("MetricUnknown"),
                String::from("Bare"),
                String::from("Tuple"),
                String::from("Nested")
            ]
        );
    }

    #[test]
    fn an_attribute_or_a_comment_is_not_a_variant() {
        let body = "\n    #[error(\"x\")]\n    // A note.\n    /// Doc.\n    Real,\n";
        assert_eq!(variant_names(body), vec![String::from("Real")]);
    }

    #[test]
    fn the_enum_body_ends_at_its_own_brace() {
        let text = "pub enum E {\n    A { b: u8 },\n}\nfn after() {}\n";
        let body = enum_body(text, "pub enum E {".len()).expect("balanced");
        assert!(body.contains('A'), "{body}");
        assert!(!body.contains("after"), "{body}");
    }

    #[test]
    fn a_longer_variant_name_is_not_a_shorter_one() {
        assert!(names_exactly("RefusalReason::Alpha {", "Alpha"));
        assert!(!names_exactly("RefusalReason::AlphaBeta {", "Alpha"));
        assert!(names_exactly("m(RefusalReason::AlphaBeta)", "AlphaBeta"));
    }

    #[test]
    fn a_snapshot_names_a_variant_as_a_whole_word() {
        assert!(whole_word("Alpha:\n  metric: revenue\n", "Alpha"));
        assert!(!whole_word("AlphaBeta:\n", "Alpha"));
        assert_eq!(mentioned("Beta:\n", &declared()), BTreeSet::from([String::from("Beta")]));
    }

    #[test]
    fn an_allow_line_is_a_variant_a_date_and_a_reason() {
        let entry = parse_excuse("Beta 2026-09-02 needs a third source, and here is why", 3);
        assert_eq!(entry.variant, "Beta");
        assert_eq!(entry.dated, "2026-09-02");
        assert!(entry.why.starts_with("needs a third source"));
    }

    #[test]
    fn a_line_with_no_date_keeps_the_date_field_empty() {
        // So `report` can say which half is missing rather than reading the reason as a date.
        let entry = parse_excuse("Beta yesterday because I said so", 3);
        assert!(entry.dated.is_empty(), "{}", entry.dated);
    }

    #[test]
    fn a_date_is_checked_for_shape_and_nothing_more() {
        assert!(looks_like_a_date("2026-09-02"));
        assert!(!looks_like_a_date("2026-9-2"));
        assert!(!looks_like_a_date("yesterday"));
        // A calendar check is deliberately absent: this field exists so a stale exception is
        // visible to a reader, not so the gate can audit February.
        assert!(looks_like_a_date("2026-99-99"));
    }
}
