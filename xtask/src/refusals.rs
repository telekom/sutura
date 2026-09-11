//! Name coverage for query and startup refusals, with separate dated exception lists.
//!
//! A test-code occurrence of `<Enum>::<Variant>` under `crates/`, or a whole variant word in a
//! committed snapshot, supplies evidence. A file naming EVERY variant of an enum is a census
//! and supplies no evidence for that enum; it may still supply evidence for the other enum.
//! This prevents exhaustive mapper tests and snapshots of all refusal guides from counting.
//!
//! **A name is not a provocation or proof of execution in any venue.** A test mentioning a
//! variant without triggering it counts. Snapshot words are unqualified, so the same word can
//! supply evidence for both enums. Word boundaries recognize ASCII alphanumerics and underscores,
//! not the full Rust identifier grammar; a Unicode prefix can still count as a boundary.
//! The test-region walk inherits the causality gate's limits:
//! only the exact `#[cfg(test)]` attribute counts, and `#[path = ".."] mod` is not followed.
//!
//! Each enum has its own allow file. Missing files mean no exceptions; unknown names, entries
//! with evidence now, or entries lacking a date and reason fail. Dates are checked for shape,
//! not calendar validity or expiration. This module's fixtures live outside the `crates/` scope.
//!
//! **THE SUBJECT IS AN ENROLLED LIST, NOT "REFUSALS".** [`ENROLLED`] names every enum this gate
//! reads, and it is chosen rather than discovered: the tree carries around two hundred `pub enum`s
//! and the refusal-shaped ones are told apart by what they refuse, which is a judgement no
//! predicate here makes. So the enrolment goes stale in ONE direction only - an enrolled enum that
//! moved, was renamed, or gained or lost a variant is a refusal, because
//! [`declared::Declared::read`] compares the walk against the enrolled count - and in the other
//! direction it does not: **a new refusal-shaped enum that nobody enrols is subject to nothing
//! here**, and no gate says so. `github.com/telekom/sutura#428` records which candidates were
//! measured and left out, and the measurement is the argument: enrolling an enum whose variants
//! are mostly unnamed buys a column of dated excuses rather than coverage.
//!
//! What the enrolment being a list no longer costs is REACH. [`declared::Enrolled`] owns the loop
//! over it and mints one verdict per subject the check RETURNED for, so neither a narrowed
//! argument at [`run`]'s call site nor a `continue` inside the loop can leave a subject
//! unadjudicated at exit 0 - both were measured green before that type existed.

mod declared;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::causality::regions;
use crate::refusals::declared::{Declared, Enrolled, Subject, variants};
use crate::{Verdict, repo};

/// Where the enum is declared. A constant so a move fails this gate loudly rather than making it
/// check nothing.
const DECLARED_IN: &str = "crates/sutura-domain/src/query.rs";

/// Where a query variant with no name evidence is argued for.
const ALLOW_FILE: &str = "devco/refusals-unprovoked-allow";

/// One refused question: a caller asked something this deployment does not answer.
const QUERY: Subject = Subject {
    name: "RefusalReason",
    declared_in: DECLARED_IN,
    allow_file: ALLOW_FILE,
    variants: variants(19),
};

/// One refused deployment: the settings are not fit to serve and the process does not start.
const STARTUP: Subject = Subject {
    name: "NotFitToServe",
    declared_in: "crates/sutura-config/src/settings/posture.rs",
    allow_file: "devco/startup-refusals-unprovoked-allow",
    variants: variants(12),
};

/// One refused deployment again, and from the other side of the boot: the settings were fit and the
/// pinned bundle is not validated, so `verify_and_validate` refuses and the process does not start.
///
/// **The asymmetry `github.com/telekom/sutura#428` is about.** A `RefusalReason` refuses one
/// question; this refuses the whole deployment, and until it was enrolled the only thing holding
/// its variants was whether an author happened to look. All six were named by tests already, which
/// is what makes the enrolment free: what changes is that the seventh cannot arrive unnamed.
const VALIDATION: Subject = Subject {
    name: "NotValidated",
    declared_in: "crates/sutura-domain/src/pinned.rs",
    allow_file: "devco/validation-refusals-unprovoked-allow",
    variants: variants(6),
};

/// Every enum this gate reads. Widening it is a diff here and nowhere else; see the module doc for
/// the direction this list does NOT hold.
const ENROLLED: [&Subject; 3] = [&QUERY, &STARTUP, &VALIDATION];

/// Two subjects sharing one allow file would share their exceptions, and
/// `startup_exceptions_are_validated_and_do_not_cross_enum_boundaries` is the rule that forbids
/// it - a rule nothing compared until now, and invisible today only because all three lists are
/// empty. A `const` block, so a duplicated path fails the build rather than a run.
#[expect(
    clippy::indexing_slicing,
    reason = "const-evaluated and guarded by the loop bound: an out-of-range index here is a compile error, not a panic in a run"
)]
const _: () = {
    let mut outer = 0_usize;
    while outer < ENROLLED.len() {
        let mut inner = outer + 1;
        while inner < ENROLLED.len() {
            assert!(
                !same_path(ENROLLED[outer].allow_file, ENROLLED[inner].allow_file),
                "two enrolled subjects share an allow file, so their exceptions would cross"
            );
            inner += 1;
        }
        outer += 1;
    }
};

/// Byte equality on two paths, in a `const` context.
#[expect(
    clippy::indexing_slicing,
    reason = "const-evaluated and guarded by the loop bound: an out-of-range index here is a compile error, not a panic in a run"
)]
const fn same_path(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut at = 0_usize;
    while at < left.len() {
        if left[at] != right[at] {
            return false;
        }
        at += 1;
    }
    true
}

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
    over(Enrolled::declared())
}

/// The gate over an enrolled set of subjects.
///
/// [`run`] hands it [`Enrolled::declared`], which names [`ENROLLED`] itself; the tests hand it
/// fixture subjects over a fixture tree, so the whole path - the file listing, the declaration
/// walk, the evidence scan and the verdict - is driven without the fixture having to reproduce the
/// real enums' variant counts.
///
/// **The loop is not here.** [`Enrolled::each`] owns it and pushes one verdict per RETURN, so a
/// subject skipped between the counter and the work is a refusal instead of a smaller `ok` - the
/// shape this gate shipped with, measured at `just hygiene` exit 0 with `NotValidated` missing
/// from the output altogether. And because `Enrolled`'s field is private to
/// `refusals::declared`, this function has no set to narrow and `run` has no argument to shorten.
fn over(enrolled: Enrolled<'_>) -> Verdict {
    let (root, files) = match repo::all_files().and_then(|census| census.into_listing(repo::Unmigrated::Refusals)) {
        Ok(listing) => listing,
        Err(why) => {
            eprintln!("xtask check-refusal-coverage: FAILED - {}", why.describe());
            return Verdict::Fail;
        }
    };
    match enrolled.each(|subject| check(subject, &root, &files)) {
        Ok(verdict) => verdict,
        Err(why) => {
            eprintln!("xtask check-refusal-coverage: FAILED - {why}");
            Verdict::Fail
        }
    }
}

/// Keep every evidence set and every exception list independent, using the same file listing.
fn check(subject: &Subject, root: &Path, files: &[String]) -> Verdict {
    let declared = match Declared::read(subject, root) {
        Ok(names) => names,
        Err(message) => {
            eprintln!("xtask check-refusal-coverage: {message}");
            return Verdict::Fail;
        }
    };
    let excused = match excuses(subject, root) {
        Ok(entries) => entries,
        Err(message) => {
            eprintln!("xtask check-refusal-coverage: {message}");
            return Verdict::Fail;
        }
    };
    let evidence = name_evidence(subject, root, files, &declared);
    report(subject, &declared, &excused, &evidence)
}

/// Everything wrong with one enum's coverage, in the order a reader wants it.
///
/// Separated from the printing so each refusal can be asserted on its own SENTENCE rather than on
/// the verdict the whole set collapses to - which is what `sutura/gates` means by predicate tested,
/// refusal untested: with one boolean per gate, neutralising any single refusal leaves some other
/// one red and the suite green-looking either way.
fn problems(subject: &Subject, declared: &Declared, excused: &[Excused], evidence: &Evidence) -> Vec<String> {
    let Subject { name, allow_file, .. } = subject;
    let mut problems: Vec<String> = Vec::new();
    let excused_names: BTreeSet<&str> = excused.iter().map(|e| e.variant.as_str()).collect();

    for variant in declared.names() {
        let named = evidence.by_variant.contains_key(variant);
        if !named && !excused_names.contains(variant.as_str()) {
            problems.push(format!(
                "{variant}: no test names it and no snapshot records it, and {allow_file} does not excuse it"
            ));
        }
    }
    // The other direction, which is what makes the list a ratchet: an excuse for a variant that is
    // named now reads as coverage nobody has, and one naming a variant the enum has lost reads
    // as a rule still being applied.
    for entry in excused {
        if !declared.declares(&entry.variant) {
            problems.push(format!(
                "{allow_file}:{}: `{}` is not a {name} variant - it was renamed or removed",
                entry.line, entry.variant
            ));
            continue;
        }
        if let Some(files) = evidence.by_variant.get(&entry.variant) {
            problems.push(format!(
                "{allow_file}:{}: `{}` IS named now ({}) - delete the entry",
                entry.line,
                entry.variant,
                files.iter().take(2).cloned().collect::<Vec<String>>().join(", ")
            ));
        }
        if entry.dated.is_empty() || entry.why.is_empty() {
            problems.push(format!(
                "{allow_file}:{}: `{}` needs a date and a reason on the same line",
                entry.line, entry.variant
            ));
        }
    }

    // THE FLOOR, and its three numbers come from three derivations: the variants the FILE SCAN
    // found evidence for, the entries the ALLOW FILE carries, and the length of the walk over the
    // DECLARATION. On a clean tree they reconcile exactly, because every variant is named or
    // excused and the ratchet above forbids a variant being both. So a `.take(n)` on the loop
    // above - which would drop a variant nothing names and print `ok` - is a failure here instead.
    let named = evidence.by_variant.len();
    if named.saturating_add(excused.len()) != declared.count() {
        problems.push(format!(
            "{named} variant(s) with evidence plus {} excused does not account for the {} \
             {name} declares - this gate did not adjudicate every variant",
            excused.len(),
            declared.count()
        ));
    }
    problems
}

/// Decide, and say why.
fn report(subject: &Subject, declared: &Declared, excused: &[Excused], evidence: &Evidence) -> Verdict {
    let Subject { name, .. } = subject;
    let problems = problems(subject, declared, excused, evidence);

    if problems.is_empty() {
        println!(
            "xtask check-refusal-coverage: {name}: ok - {}/{} variant(s) named, {} excused, \
             enrolment declares {}, {} file(s) read",
            evidence.by_variant.len(),
            declared.count(),
            excused.len(),
            subject.variants,
            evidence.scanned
        );
        // Named rather than counted, because the census rule is the surprising half of this gate:
        // a reader who sees the rendered prompt in this list understands why it counts for nothing.
        for census in &evidence.censuses {
            println!("  census (names every variant, so evidence for none): {census}");
        }
        return Verdict::Pass;
    }

    eprintln!("xtask check-refusal-coverage: {name}: FAILED");
    for problem in &problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    explain(subject);
    Verdict::Fail
}

/// Printed on failure, because a rule whose reason is unstated gets reverted.
fn explain(subject: &Subject) {
    eprintln!(
        "What counts: `{}::<Variant>` in test code under crates/, or a snapshot word.",
        subject.name
    );
    eprintln!("A file naming EVERY variant is a census and counts for none of that enum's variants.");
    eprintln!("A name is not proof of provocation or execution in any venue; snapshot words are unqualified.");
    eprintln!(
        "A deliberate missing-name exception needs a date and reason in {}.",
        subject.allow_file
    );
    eprintln!(
        "This gate reads {} ENROLLED enum(s) and no others; a refusal-shaped enum nobody enrolled is unheld.",
        ENROLLED.len()
    );
}

/// Which files name which variants, censuses removed.
fn name_evidence(subject: &Subject, root: &Path, files: &[String], declared: &Declared) -> Evidence {
    let read = |rel: &str| -> Option<String> { std::fs::read_to_string(root.join(rel)).ok() };
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut censuses: BTreeSet<String> = BTreeSet::new();
    let mut scanned = 0_usize;

    for rel in files {
        let Some(named) = named_in(subject, root, rel, declared, &read) else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        if named.len() == declared.count() {
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
fn named_in(
    subject: &Subject,
    root: &Path,
    rel: &str,
    declared: &Declared,
    read: &regions::PostImage<'_>,
) -> Option<BTreeSet<String>> {
    if !rel.starts_with("crates/") {
        return None;
    }
    if has_extension(rel, "snap") {
        let text = std::fs::read_to_string(root.join(rel)).ok()?;
        return Some(mentioned(&text, declared.names()));
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
        for variant in declared.names() {
            if names_exactly(subject, line, variant) {
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

/// ASCII word boundaries exclude longer ASCII names but allow `crate::Enum::Variant`.
fn names_exactly(subject: &Subject, line: &str, variant: &str) -> bool {
    whole_word(line, &format!("{}::{variant}", subject.name))
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

/// Is `word` in `text` with no ASCII alphanumeric or underscore on either side?
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
fn excuses(subject: &Subject, root: &Path) -> Result<Vec<Excused>, String> {
    let allow_file = subject.allow_file;
    let path = root.join(allow_file);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // An absent file means no exceptions, which is the state this gate hopes to reach.
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(cause) => return Err(format!("{allow_file} could not be read: {cause}")),
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

    use super::declared::{Declared, Enrolled, Subject, variants};
    use super::{Evidence, Excused, looks_like_a_date, mentioned, names_exactly, parse_excuse, problems, report, whole_word};
    use crate::Verdict;

    const STARTUP: &str = "crates/sutura-config/src/settings/posture.rs";
    const QUERY_A: &str = "crates/example/tests/query_a.rs";
    const QUERY_B: &str = "crates/example/tests/query_b.rs";
    const STARTUP_A: &str = "crates/example/tests/startup_a.rs";
    const STARTUP_B: &str = "crates/example/tests/startup_b.rs";
    const STARTUP_ALLOW: &str = "devco/startup-refusals-unprovoked-allow";

    /// Replace a fixture file's contents, or remove it.
    type Change<'a> = (&'a str, Option<&'a str>);
    /// A named fixture edit and its expected gate verdict.
    type Case<'a> = (&'a str, &'a [Change<'a>], Verdict);

    /// The query subject as the fixture declares it: two variants, not the real enum's eighteen.
    const F_QUERY: Subject = Subject {
        name: "RefusalReason",
        declared_in: super::DECLARED_IN,
        allow_file: super::ALLOW_FILE,
        variants: variants(2),
    };

    /// The startup subject, likewise.
    const F_STARTUP: Subject = Subject {
        name: "NotFitToServe",
        declared_in: STARTUP,
        allow_file: STARTUP_ALLOW,
        variants: variants(2),
    };

    /// A subject for the decision function's own fixtures, whose lists are three names long.
    const F_THREE: Subject = Subject {
        name: "RefusalReason",
        declared_in: super::DECLARED_IN,
        allow_file: super::ALLOW_FILE,
        variants: variants(3),
    };

    /// Drive the whole gate over two independently covered enums, not report's helpers.
    fn gate_with(changes: &[Change<'_>]) -> Verdict {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "this fixture changes cwd: run under just test for one process per test"
        );
        let tree = crate::falsifier::falsifier_tree();
        let seed = [
            (super::DECLARED_IN, "pub enum RefusalReason {\n Alpha,\n Beta,\n}\n"),
            (STARTUP, "pub enum NotFitToServe {\n Alpha,\n Beta,\n}\n"),
            (QUERY_A, "fn query_a() { RefusalReason::Alpha; }\n"),
            (QUERY_B, "fn query_b() { RefusalReason::Beta; }\n"),
            (STARTUP_A, "fn startup_a() { NotFitToServe::Alpha; }\n"),
            (STARTUP_B, "fn startup_b() { NotFitToServe::Beta; }\n"),
        ];
        for (path, text) in seed
            .iter()
            .map(|(path, text)| (*path, Some(*text)))
            .chain(changes.iter().copied())
        {
            let path = tree.join(path);
            if let Some(text) = text {
                std::fs::create_dir_all(path.parent().expect("a fixture parent")).expect("fixture directories");
                std::fs::write(path, text).expect("fixture content");
            } else {
                std::fs::remove_file(path).expect("remove a seeded file");
            }
        }
        let original = std::env::current_dir().expect("a current directory");
        std::env::set_current_dir(&tree).expect("enter the fixture");
        let verdict = super::over(Enrolled::of(&[&F_QUERY, &F_STARTUP]));
        std::env::set_current_dir(original).expect("restore before asserting the verdict");
        std::fs::remove_dir_all(tree).expect("remove the owned fixture");
        verdict
    }

    #[test]
    fn startup_coverage_requires_an_exact_test_name_or_snapshot_word() {
        let cases: &[Case<'_>] = &[
            ("covered", &[], Verdict::Pass),
            ("missing", &[(STARTUP_A, None)], Verdict::Fail),
            (
                "other enum",
                &[(STARTUP_A, Some("fn f() { RefusalReason::Alpha; }"))],
                Verdict::Fail,
            ),
            (
                "enum prefix",
                &[(STARTUP_A, Some("fn f() { OtherNotFitToServe::Alpha; }"))],
                Verdict::Fail,
            ),
            (
                "variant suffix",
                &[(STARTUP_A, Some("fn f() { NotFitToServe::AlphaLong; }"))],
                Verdict::Fail,
            ),
            (
                "qualified",
                &[(STARTUP_A, Some("fn f() { crate::NotFitToServe::Alpha; }"))],
                Verdict::Pass,
            ),
            (
                "production",
                &[
                    (STARTUP_A, None),
                    ("crates/example/src/lib.rs", Some("fn f() { NotFitToServe::Alpha; }")),
                ],
                Verdict::Fail,
            ),
            (
                "snapshot",
                &[(STARTUP_A, None), ("crates/example/tests/a.snap", Some("Alpha:\n"))],
                Verdict::Pass,
            ),
            (
                "snapshot suffix",
                &[(STARTUP_A, None), ("crates/example/tests/a.snap", Some("AlphaLong:\n"))],
                Verdict::Fail,
            ),
        ];
        let wrong: Vec<_> = cases
            .iter()
            .filter_map(|(name, changes, expected)| {
                let actual = gate_with(changes);
                (actual != *expected).then_some((*name, actual, *expected))
            })
            .collect();
        assert!(wrong.is_empty(), "incorrect name-coverage verdicts: {wrong:?}");
    }

    #[test]
    fn refusal_censuses_are_decided_independently_for_each_enum() {
        let mixed = "fn f() { RefusalReason::Alpha; RefusalReason::Beta; NotFitToServe::Alpha; }";
        let converse = "fn f() { NotFitToServe::Alpha; NotFitToServe::Beta; RefusalReason::Alpha; }";
        let cases: &[Case<'_>] = &[
            (
                "query census still supplies startup evidence",
                &[(STARTUP_A, Some(mixed))],
                Verdict::Pass,
            ),
            (
                "startup census still supplies query evidence",
                &[(QUERY_A, Some(converse))],
                Verdict::Pass,
            ),
            (
                "startup census alone",
                &[(STARTUP_A, Some(converse)), (STARTUP_B, None)],
                Verdict::Fail,
            ),
            (
                "query census alone",
                &[(QUERY_A, Some(mixed)), (QUERY_B, None)],
                Verdict::Fail,
            ),
        ];
        let wrong: Vec<_> = cases
            .iter()
            .filter_map(|(name, changes, expected)| {
                let actual = gate_with(changes);
                (actual != *expected).then_some((*name, actual, *expected))
            })
            .collect();
        assert!(wrong.is_empty(), "incorrect census verdicts: {wrong:?}");
    }

    #[test]
    fn startup_exceptions_are_validated_and_do_not_cross_enum_boundaries() {
        let valid = "Alpha 2026-09-08 needs a fixture\n";
        let cases: &[Case<'_>] = &[
            ("valid", &[(STARTUP_A, None), (STARTUP_ALLOW, Some(valid))], Verdict::Pass),
            (
                "date missing",
                &[(STARTUP_A, None), (STARTUP_ALLOW, Some("Alpha yesterday needs a fixture\n"))],
                Verdict::Fail,
            ),
            (
                "reason missing",
                &[(STARTUP_A, None), (STARTUP_ALLOW, Some("Alpha 2026-09-08\n"))],
                Verdict::Fail,
            ),
            ("stale", &[(STARTUP_ALLOW, Some(valid))], Verdict::Fail),
            (
                "unknown",
                &[(STARTUP_ALLOW, Some("Gamma 2026-09-08 renamed\n"))],
                Verdict::Fail,
            ),
            (
                "same name separately excused",
                &[
                    (QUERY_A, None),
                    (STARTUP_A, None),
                    (super::ALLOW_FILE, Some(valid)),
                    (STARTUP_ALLOW, Some(valid)),
                ],
                Verdict::Pass,
            ),
            (
                "query excuse is not a startup excuse",
                &[(QUERY_A, None), (STARTUP_A, None), (super::ALLOW_FILE, Some(valid))],
                Verdict::Fail,
            ),
            (
                "startup excuse is not a query excuse",
                &[(QUERY_A, None), (STARTUP_A, None), (STARTUP_ALLOW, Some(valid))],
                Verdict::Fail,
            ),
        ];
        let wrong: Vec<_> = cases
            .iter()
            .filter_map(|(name, changes, expected)| {
                let actual = gate_with(changes);
                (actual != *expected).then_some((*name, actual, *expected))
            })
            .collect();
        assert!(wrong.is_empty(), "incorrect exception verdicts: {wrong:?}");
    }

    #[test]
    fn the_startup_declaration_cannot_silently_disappear_empty_or_outgrow_its_enrolment() {
        assert_eq!(gate_with(&[]), Verdict::Pass);
        let wrong: Vec<_> = [
            None,
            Some("pub enum Renamed {\n Alpha,\n}\n"),
            Some("pub enum NotFitToServe {}\n"),
            Some("pub enum NotFitToServe {\n Alpha,\n"),
            // The enrolment-staleness direction: a variant added without moving the enrolled count
            // leaves the gate checking a shorter list than the enum has.
            Some("pub enum NotFitToServe {\n Alpha,\n Beta,\n Gamma,\n}\n"),
            // And the same disagreement from the other side.
            Some("pub enum NotFitToServe {\n Alpha,\n}\n"),
        ]
        .into_iter()
        .filter_map(|text| {
            let actual = gate_with(&[(STARTUP, text)]);
            (actual != Verdict::Fail).then_some((text, actual))
        })
        .collect();
        assert!(wrong.is_empty(), "unreadable startup declarations passed: {wrong:?}");
    }

    /// Nothing to check is not a pass. `run` cannot reach this - `Enrolled::declared` names the
    /// whole table - so the refusal is driven from here, and it is the one disjunct of that
    /// condition which has a provocation at all.
    #[test]
    fn an_empty_enrolment_is_a_refusal() {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "sibling fixtures in this module change cwd: run under just test for one process per test"
        );
        assert_eq!(super::over(Enrolled::of(&[])), Verdict::Fail);
    }

    /// **Every enrolled subject reaches the check**, asserted from the visitor's own record.
    ///
    /// This is the half no floor counted off the loop could ever hold: a `continue` written in
    /// `Enrolled::each` leaves this list short as well as tripping the length refusal, so the
    /// skip has a PROVOCATION now rather than only a diff. Measured before the loop moved: the
    /// same skip was `just hygiene` exit 0 with `NotValidated` absent and the suite green.
    #[test]
    fn every_enrolled_subject_reaches_the_check() {
        let mut reached: Vec<&str> = Vec::new();
        let verdict = Enrolled::declared()
            .each(|subject| {
                reached.push(subject.name);
                Verdict::Pass
            })
            .expect("every enrolled subject reached");
        assert_eq!(verdict, Verdict::Pass);
        assert_eq!(reached, ["RefusalReason", "NotFitToServe", "NotValidated"]);
    }

    /// A visitor's verdict is the gate's verdict, so `each` cannot fold a refusal away.
    #[test]
    fn one_refused_subject_refuses_the_set() {
        let verdict = Enrolled::declared()
            .each(|subject| {
                if subject.name == "NotValidated" {
                    Verdict::Fail
                } else {
                    Verdict::Pass
                }
            })
            .expect("every enrolled subject reached");
        assert_eq!(verdict, Verdict::Fail);
    }

    /// The enrolled counts are held against the REAL enums, so a variant added to any of the three
    /// is a red test as well as a red gate - and dropping a subject from `ENROLLED` is red here too.
    ///
    /// Driven THROUGH `Enrolled::each` rather than over the constant, so this test sees what the
    /// gate sees rather than what the table says.
    #[test]
    fn the_enrolment_resolves_against_the_real_tree() {
        assert!(
            std::env::var_os("NEXTEST").is_some(),
            "sibling fixtures in this module change cwd: run under just test for one process per test"
        );
        let root = crate::repo::root().expect("this repository's own root");
        let mut unresolved: Vec<String> = Vec::new();
        let mut names: Vec<&str> = Vec::new();
        let verdict = Enrolled::declared()
            .each(|subject| {
                names.push(subject.name);
                if let Err(why) = Declared::read(subject, &root) {
                    unresolved.push(why);
                }
                Verdict::Pass
            })
            .expect("every enrolled subject reached");
        assert_eq!(verdict, Verdict::Pass);
        assert!(unresolved.is_empty(), "enrolled subjects that do not resolve: {unresolved:?}");
        assert_eq!(names, ["RefusalReason", "NotFitToServe", "NotValidated"]);
    }

    /// Fabricated variant names throughout, so this module's own fixtures cannot be read as
    /// evidence by the gate that scans them.
    fn declared() -> Declared {
        Declared::for_tests(&["Alpha", "Beta", "Gamma"])
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
        assert_eq!(report(&F_THREE, &declared(), &[], &all), Verdict::Pass);
    }

    #[test]
    fn a_variant_nothing_provokes_fails() {
        let partial = evidence(&[("Alpha", &["crates/a/src/tests.rs"])], &[]);
        assert_eq!(report(&F_THREE, &declared(), &[], &partial), Verdict::Fail);
    }

    #[test]
    fn the_allow_file_is_what_makes_it_pass_instead() {
        let partial = evidence(&[("Alpha", &["crates/a/src/tests.rs"])], &[]);
        let excuses = vec![
            excused("Beta", "2026-09-02", "needs a third source"),
            excused("Gamma", "2026-09-02", "needs two remote dimensions"),
        ];
        assert_eq!(report(&F_THREE, &declared(), &excuses, &partial), Verdict::Pass);
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
        assert_eq!(report(&F_THREE, &declared(), &excuses, &all), Verdict::Fail);
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
        assert_eq!(report(&F_THREE, &declared(), &excuses, &all), Verdict::Fail);
    }

    #[test]
    fn an_excuse_with_no_date_or_no_reason_fails() {
        let partial = evidence(&[("Alpha", &["crates/a/src/tests.rs"])], &[]);
        let undated = vec![
            excused("Beta", "", "a reason but no date"),
            excused("Gamma", "2026-09-02", ""),
        ];
        assert_eq!(report(&F_THREE, &declared(), &undated, &partial), Verdict::Fail);
    }

    /// Each refusal asserted on its own SENTENCE.
    ///
    /// One verdict per case would leave every one of these provable by any other refusal firing,
    /// which is how a neutralised refusal reads as covered.
    #[test]
    fn each_refusal_is_reported_by_its_own_sentence() {
        let two_of_three = evidence(
            &[("Alpha", &["crates/a/src/tests.rs"]), ("Beta", &["crates/a/src/tests.rs"])],
            &[],
        );
        let all_three = evidence(
            &[
                ("Alpha", &["crates/a/src/tests.rs"]),
                ("Beta", &["crates/a/src/tests.rs"]),
                ("Gamma", &["crates/a/src/tests.rs"]),
            ],
            &[],
        );
        let said = |excuses: &[Excused], evidence: &Evidence| problems(&F_THREE, &declared(), excuses, evidence).join("\n");

        let unnamed = said(&[], &two_of_three);
        assert!(unnamed.contains("Gamma: no test names it"), "{unnamed}");

        let stale = said(&[excused("Beta", "2026-09-02", "stale now")], &all_three);
        assert!(stale.contains("`Beta` IS named now"), "{stale}");

        let unknown = said(&[excused("Delta", "2026-09-02", "renamed away")], &all_three);
        assert!(unknown.contains("`Delta` is not a RefusalReason variant"), "{unknown}");

        let undated = said(&[excused("Gamma", "", "no date")], &two_of_three);
        assert!(undated.contains("`Gamma` needs a date and a reason"), "{undated}");

        // THE FLOOR on its own: every variant the loop above can see is named, and the counts
        // still do not reconcile - which is what a walk that read one variant twice, or an
        // adjudication loop that stopped early, leaves behind.
        let duplicated = Declared::for_tests(&["Alpha", "Alpha", "Beta"]);
        let both_named = evidence(
            &[("Alpha", &["crates/a/src/tests.rs"]), ("Beta", &["crates/a/src/tests.rs"])],
            &[],
        );
        let floor = problems(&F_THREE, &duplicated, &[], &both_named).join("\n");
        assert!(
            floor.contains("does not account for the 3") && floor.contains("2 variant(s) with evidence"),
            "{floor}"
        );
        assert_eq!(report(&F_THREE, &duplicated, &[], &both_named), Verdict::Fail);
    }

    #[test]
    fn a_census_counts_for_no_variant() {
        // The rendered prompt lists every refusal guide by name, so without this rule one
        // snapshot would make the whole enum look provoked.
        let only_a_census = evidence(&[], &["crates/cli/tests/snapshots/example_prompt.snap"]);
        assert_eq!(report(&F_THREE, &declared(), &[], &only_a_census), Verdict::Fail);
    }

    #[test]
    fn a_longer_variant_name_is_not_a_shorter_one() {
        assert!(names_exactly(&F_THREE, "RefusalReason::Alpha {", "Alpha"));
        assert!(!names_exactly(&F_THREE, "RefusalReason::AlphaBeta {", "Alpha"));
        assert!(names_exactly(&F_THREE, "m(RefusalReason::AlphaBeta)", "AlphaBeta"));
    }

    #[test]
    fn a_snapshot_names_a_variant_as_a_whole_word() {
        assert!(whole_word("Alpha:\n  metric: revenue\n", "Alpha"));
        assert!(!whole_word("AlphaBeta:\n", "Alpha"));
        assert_eq!(
            mentioned("Beta:\n", declared().names()),
            BTreeSet::from([String::from("Beta")])
        );
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
