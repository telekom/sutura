//! One Arrow major per lock file, because two are two incompatible sets of types.
//!
//! WHY THIS GATE EXISTS AND `deny.toml` IS NOT IT. `deny.toml` sets `multiple-versions = "warn"`,
//! deliberately - denying it would need a skip entry per duplicated crate, and on this workspace
//! that is twenty-seven of them, churning on every `just update`. A gate that fails on correct
//! code gets disabled, which is the same reasoning `allow-wildcard-paths` already carries there.
//! So the general duplicate check stays a warning and this gate is narrow.
//!
//! WHAT MAKES ARROW DIFFERENT from the other twenty-six duplicates. Arrow is a TYPE vocabulary
//! rather than a library: `RecordBatch` and `Schema` from one major are not the ones from the
//! next, so the moment first-party code holds a value from one and hands it to something
//! expecting the other, it does not compile - or, forced across the C data interface, it is
//! undefined behaviour. A duplicated hashing crate costs build time. A duplicated Arrow costs
//! correctness the day anything crosses between them.
//!
//! AND WHY IT IS NOT ALREADY BROKEN. As of writing, this workspace holds two Arrow majors and it
//! is a duplicate rather than a type boundary: the engine pulls one, the `DuckDB` adapter's crate
//! pulls the other, and `sutura-exec-duckdb` declares no `arrow` dependency and names no Arrow
//! type - it converts to a neutral row type instead. `differential.rs` compares rows rather than
//! batches for the same reason. **The test for which one you have is not the lock file, it is
//! whether any first-party crate names the type.** This gate reads the lock file, so it cannot
//! make that distinction - which is exactly why an accepted split needs a written reason rather
//! than being silently tolerated.
//!
//! The allowlist is `devco/arrow-majors-allow`, one row per line as `<major> <ISO date> <reason>`.
//! A row is a decision somebody wrote down, and the date is what makes a stale one visible.
//!
//! # BOTH DIRECTIONS, because only one of them used to be asked
//!
//! This gate returned on `majors.len() <= 1` before it read the allowlist at all, so on the day a
//! split closes it printed `ok - one Arrow major` and the row that permitted the split stayed in
//! the file permitting nothing, its dated paragraph intact. What rots is not the row, it is the
//! reasoning beside it - and that paragraph is the only thing a reviewer reads to decide whether
//! the exception is still earned. `max-lines` and `check-refusal-coverage` each hold their own
//! annotated allowlist to that rule already; this is the third list of the same shape.
//!
//! So the allowlist is read on every run, and compared both ways: a major in the lock with no row
//! fails, and a row naming a major the lock does not hold fails. See [`assess`].
//!
//! **THE CHOICE, and there were two.** `59` is the engine's own major and is not an exception to
//! anything - it is in the file so the failure message can name a reason for every major present.
//! Exempting it needs the file to mark which row is the engine's, which is a second thing to keep
//! true and a marker nothing checks. So: **every row is a row like any other and must name a major
//! the lock file holds.** The consequence, stated rather than left to be discovered: were Arrow to
//! leave the dependency graph entirely, every row goes inert at once and the gate fails until the
//! file is deleted along with them. That is the intended reading - a list of tolerated Arrow majors
//! on a tree with no Arrow is a rule that lost its subject.
//!
//! # THE VERDICT IS DERIVED FROM THE REPORT, not written beside it
//!
//! Every finding lives in one [`Findings`], the report prints from it and [`Findings::verdict`]
//! reads the same value, so the gate cannot print `FAILED` and exit 0. That was not a hypothetical:
//! the first cut of the second direction wired its report and its return statement separately, and
//! deleting one disjunct from the return produced two contradictory sentences in one run at exit 0
//! with every unit test still green - because every test was a pure-function test and nothing
//! called [`run`]. The tests below drive [`run_in`] over crafted trees for that reason, and
//! `tests::the_real_tree_passes_this_gate` drives it over this repo, the way `check-workflows`
//! and `check-venues` each anchor on the real file their fixtures only imitate.
//!
//! # WHAT IT READS, AND WHAT IT REFUSES TO GUESS
//!
//! * **An UNREADABLE allowlist is a fault, an ABSENT one is configuration.** They were one case,
//!   `unwrap_or_default()`, which was survivable only while the read happened in the split path
//!   where the answer went red anyway. Feeding a PASS that positively asserts the file is clean, it
//!   is fail-open: on a single-major lock, every way of not reading the file - missing, empty,
//!   permission-denied, not UTF-8 - reached the same `ok`, and the last two can hold real stale
//!   rows. This is the rule the docker gate already states the other way round: no container
//!   runtime is a legitimate developer machine, an unreadable input is not. So a missing file means
//!   *nothing is tolerated* and the census line says so, and every other error is [`Verdict::Fail`].
//!   Held by `tests::an_absent_allowlist_is_configuration_and_an_unreadable_one_is_a_fault`, which
//!   drives the non-UTF-8 case because it is the one that behaves the same for every user; a
//!   permission bit does not, under a build sandbox that may own the file.
//! * **It prints what it read, as a PAIR from two different places.** Every package in the lock and
//!   the Arrow family among them, plus the allowlist row count. One number cannot witness itself: a
//!   broken parser reads zero family members over a lock that legitimately has none. So the outer
//!   count has a floor - a lock parsing to zero packages is the scan being broken rather than the
//!   tree being clean - and so does a present-but-empty allowlist.
//!
//! **WHAT THIS STILL DOES NOT ANSWER.** Not the DATE. A row must carry something in the date
//! position, and the gate never parses it: a row dated last year reads exactly like one written
//! this morning, which the allowlist's own header already discloses. Not whether the split is a
//! duplicate or a TYPE BOUNDARY either - that is answered by whether a first-party crate names the
//! type, which no lock file can say.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Verdict;
use crate::repo;

/// The allowlist, relative to the repo root.
const ALLOWLIST: &str = "devco/arrow-majors-allow";

/// The lock file, relative to the repo root.
const LOCK: &str = "Cargo.lock";

/// Crates whose major must agree. The whole family moves together upstream, so a split in any one
/// of them is the same defect as a split in `arrow` itself - and checking only `arrow` would miss
/// a transitive dependant pinning `arrow-schema` alone.
const FAMILY_PREFIX: &str = "arrow";

/// One `[[package]]` stanza's name and version, as read from `Cargo.lock`.
///
/// `pub(crate)` rather than private because `attribution` reads the same pairs from the same
/// file for the same reason: the crate list, no TOML parser.
pub(crate) struct Package<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
}

/// Parse `name` / `version` pairs out of a lock file without a TOML dependency.
///
/// `xtask` has one dependency and adding a TOML parser to read two fields would be a poor trade -
/// the same argument `changes.rs` makes for keeping its area table in Rust. The shape this relies
/// on is `cargo`'s own output: within a `[[package]]` stanza, `name` precedes `version`, and both
/// are `key = "value"` on their own line.
pub(crate) fn packages(lock: &str) -> Vec<Package<'_>> {
    let mut found = Vec::new();
    let mut name: Option<&str> = None;
    for line in lock.lines() {
        if let Some(value) = line.strip_prefix("name = ") {
            name = unquote(value);
        } else if let Some(value) = line.strip_prefix("version = ")
            && let Some(n) = name.take()
            && let Some(v) = unquote(value)
        {
            found.push(Package { name: n, version: v });
        }
    }
    found
}

/// Strip the surrounding quotes from a lock-file value, or `None` if it is not quoted.
fn unquote(value: &str) -> Option<&str> {
    value.strip_prefix('"')?.strip_suffix('"')
}

/// The major component of a semver string, as text so a four-digit major cannot overflow.
fn major(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

/// Is this package one the Arrow family?
fn is_family(name: &str) -> bool {
    name == FAMILY_PREFIX || name.starts_with("arrow-")
}

/// The leading whitespace-delimited token, and the rest of the line with its leading space gone.
///
/// Not `splitn(3, char::is_whitespace)`, which splits on EACH whitespace character and so turns a
/// row aligned with two spaces or a tab into an empty field. That did not matter while an empty
/// field only produced an empty reason; it matters now that an empty reason is a finding.
fn split_token(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(head, tail)| (head, tail.trim_start()))
}

/// The allowlist file as the gate found it. A missing file is not an unreadable one.
enum Allowlist {
    /// Not there. Legitimate configuration - nothing is tolerated - and the report says so.
    Absent,
    /// Read.
    Present(String),
}

impl Allowlist {
    /// What the file holds, which for an absent one is nothing.
    fn contents(&self) -> &str {
        match self {
            Self::Absent => "",
            Self::Present(contents) => contents,
        }
    }

    /// A file that is there is one whose rows must add up. An absent one has none to add up.
    const fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }
}

/// Read the allowlist, keeping *absent* and *unreadable* apart.
///
/// `NotFound` is the only error that is configuration. Permission denied, a directory in its place
/// and content that is not UTF-8 are all faults, and a fault must reach the verdict rather than be
/// laundered into an empty file that reads as *nothing is stale*.
fn read_allowlist(path: &Path) -> Result<Allowlist, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Allowlist::Present(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Allowlist::Absent),
        Err(error) => Err(error),
    }
}

/// The allowlist's rows, parsed - and the ones that are not rows.
struct Rows {
    /// Majors a well-formed row accepts, mapped to the dated reason beside them.
    permitted: BTreeMap<String, String>,
    /// Majors named by a row with no date or no reason. The file's whole value is the paragraph,
    /// so a row without one permits a split while explaining nothing - and the pass line used to
    /// call that *explained*.
    malformed: Vec<String>,
    /// Majors named more than once. A `BTreeMap` keeps the last, so the earlier decision's
    /// paragraph vanished with no notice.
    duplicated: Vec<String>,
}

/// Parse the allowlist: `<major> <ISO date> <reason>`, one per line, `#` comments, blanks ignored.
///
/// A row must carry all three. The date is never parsed - see the module header - but its POSITION
/// must be filled, because `58 2026-08-28` and `58` are equally unexplained and only one of them
/// looks it.
fn allowed(contents: &str) -> Rows {
    let mut permitted: BTreeMap<String, String> = BTreeMap::new();
    let mut malformed = Vec::new();
    let mut duplicated = Vec::new();
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (major, rest) = split_token(line);
        let (date, reason) = split_token(rest);
        if date.is_empty() || reason.is_empty() {
            malformed.push(major.to_owned());
        } else if permitted.insert(major.to_owned(), format!("{date} {reason}")).is_some() {
            duplicated.push(major.to_owned());
        }
    }
    Rows {
        permitted,
        malformed,
        duplicated,
    }
}

/// Which majors the Arrow family resolves to, and which members carry each one.
///
/// The members are what a failure message needs: naming one crate would send somebody to the
/// wrong place.
fn family_majors(lock: &str) -> BTreeMap<String, Vec<String>> {
    let mut majors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for package in packages(lock).iter().filter(|p| is_family(p.name)) {
        majors
            .entry(major(package.version).to_owned())
            .or_default()
            .push(format!("{}@{}", package.name, package.version));
    }
    majors
}

/// What the run actually read, so the verdict names its own subject rather than asserting one.
struct Census {
    /// Every `[[package]]` stanza in the lock file, the Arrow family included. **The OUTER number**,
    /// and the one with a floor: a broken parser and a lock with no Arrow both read zero family
    /// members, and only this one tells them apart.
    packages: usize,
    /// Arrow-family members among them. **The INNER number.**
    family: usize,
    /// Well-formed allowlist rows.
    rows: usize,
    /// Whether the allowlist file is there at all.
    allowlist_present: bool,
}

impl Census {
    /// One line naming both numbers and where each came from.
    fn line(&self) -> String {
        let file = if self.allowlist_present {
            ALLOWLIST
        } else {
            "no allowlist file"
        };
        format!(
            "  read: {} package(s) in {LOCK}, {} arrow-family; {} row(s) in {file}",
            self.packages, self.family, self.rows
        )
    }
}

/// Everything wrong, in one value. **The report prints from this and [`Findings::verdict`] reads
/// the same value**, which is what makes the exit code and the sentences above it one statement.
struct Findings {
    /// Majors present in the lock file that no row explains.
    unexplained: Vec<String>,
    /// Rows naming a major the lock file does not hold - the split closed, the paragraph stayed.
    inert: Vec<String>,
    /// Rows with no date or no reason.
    malformed: Vec<String>,
    /// Majors a row names twice.
    duplicated: Vec<String>,
    /// A floor tripped, or an input the gate refused to guess at. Each is a whole sentence.
    faults: Vec<String>,
}

impl Findings {
    const fn verdict(&self) -> Verdict {
        if self.unexplained.is_empty()
            && self.inert.is_empty()
            && self.malformed.is_empty()
            && self.duplicated.is_empty()
            && self.faults.is_empty()
        {
            Verdict::Pass
        } else {
            Verdict::Fail
        }
    }
}

/// The two directions the lock file and the allowlist get compared in.
struct Directions {
    unexplained: Vec<String>,
    inert: Vec<String>,
}

/// Compare the two lists both ways.
///
/// **`unexplained` is asked only when there is a split**, and that is a rule rather than an
/// oversight: one Arrow major is the state this workspace wants, so demanding a row for it would
/// fail a tree that is exactly right - and a gate that fails on correct code gets disabled, which is
/// the argument `deny.toml` already carries. **`inert` is asked on every run**, that one included,
/// because the day a split closes is precisely the day its row stops being earned, and until this
/// gate read the file unconditionally nothing looked.
fn assess(majors: &BTreeMap<String, Vec<String>>, permitted: &BTreeMap<String, String>) -> Directions {
    let unexplained = if majors.len() > 1 {
        majors.keys().filter(|m| !permitted.contains_key(*m)).cloned().collect()
    } else {
        Vec::new()
    };
    Directions {
        unexplained,
        inert: permitted.keys().filter(|m| !majors.contains_key(*m)).cloned().collect(),
    }
}

/// The floors. Each answers *was there a subject at all*, which a count alone cannot.
fn floors(census: &Census, malformed: &[String]) -> Vec<String> {
    let mut faults = Vec::new();
    if census.packages == 0 {
        faults.push(format!(
            "{LOCK} parsed to zero packages - the lock reader is broken, not the tree. Every \
             verdict below it would be about nothing."
        ));
    }
    if census.allowlist_present && census.rows == 0 && malformed.is_empty() {
        faults.push(format!(
            "{ALLOWLIST} is present and parsed to no rows - either it lost its content or the row \
             reader is broken. An empty list of exceptions is a file that should not exist."
        ));
    }
    faults
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-arrow: could not determine the repo root");
        return Verdict::Fail;
    };
    run_in(&root)
}

/// The gate, over any root. Split from [`run`] so a test can hand it a crafted tree and assert on
/// the VERDICT - without which nothing ties a printed finding to the exit code.
fn run_in(root: &Path) -> Verdict {
    let Ok(lock) = std::fs::read_to_string(root.join(LOCK)) else {
        eprintln!("xtask check-arrow: could not read {LOCK}");
        return Verdict::Fail;
    };
    let allowlist = match read_allowlist(&root.join(ALLOWLIST)) {
        Ok(allowlist) => allowlist,
        Err(error) => {
            eprintln!("xtask check-arrow: FAILED - {ALLOWLIST} exists and could not be read: {error}");
            eprintln!("  A MISSING allowlist means nothing is tolerated and is a legitimate tree.");
            eprintln!("  One that cannot be read is a fault: every row in it could be stale and");
            eprintln!("  this gate would report a clean list either way. Fix the file, not the gate.");
            return Verdict::Fail;
        }
    };

    let all = packages(&lock);
    let majors = family_majors(&lock);
    let Rows {
        permitted,
        malformed,
        duplicated,
    } = allowed(allowlist.contents());
    let census = Census {
        packages: all.len(),
        family: all.iter().filter(|p| is_family(p.name)).count(),
        rows: permitted.len(),
        allowlist_present: allowlist.is_present(),
    };

    let faults = floors(&census, &malformed);
    let Directions { unexplained, inert } = assess(&majors, &permitted);
    let found = Findings {
        unexplained,
        inert,
        malformed,
        duplicated,
        faults,
    };

    let verdict = found.verdict();
    if verdict == Verdict::Pass {
        report_pass(&majors, &permitted, &census);
    } else {
        report_findings(&found, &majors, &permitted, &census);
    }
    verdict
}

/// The pass line, and the census under it.
fn report_pass(majors: &BTreeMap<String, Vec<String>>, permitted: &BTreeMap<String, String>, census: &Census) {
    // Every major here has a row - `unexplained` being empty is what says so - and the headline's
    // count is the same predicate the rows under it are printed by, so the sentence and its own
    // body cannot be two numbers.
    let explained = majors.keys().filter(|m| permitted.contains_key(*m)).count();
    match majors.len() {
        0 => println!("xtask check-arrow: ok - no Arrow crate in {LOCK}, and no row names one"),
        1 => {
            let which = majors.keys().next().map_or_else(String::new, |m| format!("major {m}"));
            println!(
                "xtask check-arrow: ok - one Arrow major in {LOCK} ({which}), and no row in {ALLOWLIST} names a major it does not hold"
            );
        }
        _ => {
            println!("xtask check-arrow: ok - {explained} Arrow majors, every one explained in {ALLOWLIST}");
            for (major, members) in majors {
                if let Some(reason) = permitted.get(major) {
                    println!("  major {major}: {} crate(s) - {reason}", members.len());
                }
            }
        }
    }
    println!("{}", census.line());
}

/// Every finding, then the census. **All of them**: a gate that knows several and prints the first
/// sends its reader back for a second run to find the half it withheld.
fn report_findings(
    found: &Findings,
    majors: &BTreeMap<String, Vec<String>>,
    permitted: &BTreeMap<String, String>,
    census: &Census,
) {
    for fault in &found.faults {
        eprintln!("xtask check-arrow: FAILED - {fault}");
    }
    if !found.malformed.is_empty() {
        report_malformed(&found.malformed);
    }
    if !found.duplicated.is_empty() {
        report_duplicated(&found.duplicated);
    }
    if !found.inert.is_empty() {
        report_inert(&found.inert, majors);
    }
    if !found.unexplained.is_empty() {
        report_split(majors, permitted);
    }
    eprintln!("{}", census.line());
}

/// A row that names a major and explains nothing.
fn report_malformed(malformed: &[String]) {
    eprintln!(
        "xtask check-arrow: FAILED - {ALLOWLIST} holds {} row(s) with no date or no reason",
        malformed.len()
    );
    for major in malformed {
        eprintln!("  major {major}: the row is `<major>` alone, or `<major> <date>` with nothing after it");
    }
    eprintln!();
    eprintln!("  The paragraph IS the entry. A bare major permits the split and explains it to");
    eprintln!("  nobody, and the pass line would have called that major explained. Write it as");
    eprintln!("  `<major> <ISO date> <why it is acceptable, and what would end it>`, or delete it.");
    eprintln!();
}

/// The same major twice - the later row's reason silently replaced the earlier one's.
fn report_duplicated(duplicated: &[String]) {
    eprintln!(
        "xtask check-arrow: FAILED - {ALLOWLIST} names {} major(s) more than once",
        duplicated.len()
    );
    for major in duplicated {
        eprintln!("  major {major}: a later row replaced an earlier row's reason, and only one was read");
    }
    eprintln!();
    eprintln!("  Two decisions about one major is one decision and one lost paragraph. Keep the");
    eprintln!("  row that is still true, with the date of the decision that is still being made.");
    eprintln!();
}

/// A row naming a major the lock file no longer holds.
fn report_inert(inert: &[String], majors: &BTreeMap<String, Vec<String>>) {
    eprintln!(
        "xtask check-arrow: FAILED - {ALLOWLIST} names {} major(s) {LOCK} does not hold",
        inert.len()
    );
    for major in inert {
        eprintln!("  major {major}: no arrow crate in the lock file resolves to it");
    }
    if majors.is_empty() {
        eprintln!("  the lock file holds no arrow crate at all");
    } else {
        let present: Vec<&str> = majors.keys().map(String::as_str).collect();
        eprintln!("  present: {}", present.join(", "));
    }
    eprintln!();
    eprintln!("  The split that row permitted has closed. An exception nothing needs is a claim");
    eprintln!("  nobody checks, and this file is a list of claims: the paragraph beside a row is");
    eprintln!("  the only thing a reviewer reads to decide whether it is still earned. Delete the");
    eprintln!("  row and its argument together, in the change that closed the split.");
    eprintln!();
}

/// A major in the lock file that no row explains.
fn report_split(majors: &BTreeMap<String, Vec<String>>, permitted: &BTreeMap<String, String>) {
    eprintln!("xtask check-arrow: FAILED - {LOCK} holds {} Arrow majors", majors.len());
    for (major, members) in majors {
        let note = permitted.get(major).map_or("NOT EXPLAINED", String::as_str);
        eprintln!("  major {major}: {} crate(s) [{note}]", members.len());
        for member in members.iter().take(3) {
            eprintln!("    {member}");
        }
        if members.len() > 3 {
            eprintln!("    ... and {} more", members.len() - 3);
        }
    }
    eprintln!();
    eprintln!("  The engine's Arrow major is the workspace's Arrow major: a data source is");
    eprintln!("  replaceable and the engine is not, so every other Arrow-consuming dependency");
    eprintln!("  conforms to it. Downgrading the engine to match an adapter is not the fix.");
    eprintln!();
    eprintln!("  In preference order, each reached only because the one above failed:");
    eprintln!("    1. bump the conforming crate to a release that already agrees");
    eprintln!("    2. disable the feature that pulls the conflicting version, if unused");
    eprintln!("    3. `[patch.crates-io]` on the DEPENDENT crate - NOT on arrow itself, which");
    eprintln!("       cannot widen a requirement and is silently ignored across disjoint majors");
    eprintln!("    4. vendor, last, and never silently: VENDOR.md, and it makes us the security");
    eprintln!("       response for that copy permanently");
    eprintln!();
    eprintln!("  Do not vendor, inline, fork or pin back on your own judgement - raise it, saying");
    eprintln!("  which crates conflict, whether it is a duplicate or a TYPE BOUNDARY, what each");
    eprintln!("  option costs, and what breaks if nothing is done.");
    eprintln!();
    eprintln!("  To accept a split deliberately, add to {ALLOWLIST}:");
    for major in majors.keys().filter(|m| !permitted.contains_key(*m)) {
        eprintln!("    {major} <ISO date> <why this split is acceptable, and what would end it>");
    }
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_major_is_read_out_of_a_lock_file() {
        let lock =
            "[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n\n[[package]]\nname = \"arrow-schema\"\nversion = \"59.2.0\"\n";
        let found = packages(lock);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "arrow");
        assert_eq!(major(found[1].version), "59");
    }

    #[test]
    fn a_version_line_without_a_preceding_name_is_not_a_package() {
        // A lock file's `[metadata]` and `[[patch]]` tables carry bare values, and reading one as
        // a package would invent a dependency - the defect class this gate exists to prevent.
        let found = packages("version = \"3\"\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version, "59.2.0");
    }

    #[test]
    fn the_whole_family_counts_not_only_the_arrow_crate() {
        // A transitive dependant may pin `arrow-schema` alone, which is the same defect and is
        // what checking only `arrow` would miss.
        let majors = family_majors("[[package]]\nname = \"arrow-schema\"\nversion = \"58.4.0\"\n");
        assert_eq!(majors.keys().collect::<Vec<_>>(), vec!["58"]);
    }

    #[test]
    fn a_crate_merely_starting_with_arrow_is_not_family() {
        // `arrowhead` would be a different project. The family is `arrow` or `arrow-*`.
        assert!(!is_family("arrowhead"));
        assert!(is_family("arrow"));
        assert!(is_family("arrow-schema"));
    }

    #[test]
    fn an_allowlist_row_carries_its_reason() {
        let rows = allowed("# a comment\n58 2026-08-28 the duckdb crate lags; dev-dependency only\n\n");
        assert_eq!(rows.permitted.len(), 1);
        assert!(rows.permitted["58"].contains("2026-08-28"), "the date is part of the reason");
        assert!(rows.permitted["58"].contains("duckdb"));
        assert!(rows.malformed.is_empty());
    }

    #[test]
    fn a_blank_or_commented_allowlist_permits_nothing() {
        assert!(allowed("\n# nothing here\n\n").permitted.is_empty());
    }

    #[test]
    fn a_row_aligned_with_tabs_or_extra_spaces_is_still_well_formed() {
        // `splitn(3, char::is_whitespace)` splits on EACH whitespace character, so this row parsed
        // to an empty date - harmless while that only meant an empty reason, a false finding now.
        let rows = allowed("58\t2026-08-28  the duckdb crate lags\n");
        assert!(rows.malformed.is_empty(), "got {:?}", rows.malformed);
        assert_eq!(rows.permitted["58"], "2026-08-28 the duckdb crate lags");
    }

    #[test]
    fn a_row_with_no_reason_is_malformed_rather_than_a_permission() {
        // `58` and `58 2026-08-28` are equally unexplained; only one of them looks it. Accepting
        // either put an EMPTY reason in the map, which the pass line then called `explained`.
        let rows = allowed("58\n59 2026-08-28\n");
        assert!(rows.permitted.is_empty(), "neither row explains anything");
        assert_eq!(rows.malformed, vec!["58".to_owned(), "59".to_owned()]);
    }

    #[test]
    fn a_major_named_twice_is_reported_rather_than_collapsed() {
        // A `BTreeMap` keeps the last, so the first decision's paragraph vanished with no notice.
        let rows = allowed("58 2026-01-01 the first decision\n58 2026-08-28 the second\n");
        assert_eq!(rows.duplicated, vec!["58".to_owned()]);
    }

    /// The lock file this workspace has today: an engine major and an adapter's.
    fn split_lock() -> &'static str {
        "[[package]]\nname = \"arrow\"\nversion = \"58.4.0\"\n\n[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n"
    }

    #[test]
    fn a_row_for_a_major_the_lock_no_longer_holds_is_inert() {
        // The day the split closes. Before this gate looked, the run printed `ok - one Arrow
        // major` and the 58 row stayed in the file with its dated paragraph, permitting nothing.
        let majors = family_majors("[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        let rows = allowed("58 2026-08-28 the duckdb crate lags\n59 2026-08-28 the engine\n");
        let found = assess(&majors, &rows.permitted);
        assert_eq!(found.inert, vec!["58".to_owned()], "the closed split's row is named");
        assert!(found.unexplained.is_empty(), "one major needs no row");
    }

    #[test]
    fn a_single_major_with_an_allowlist_that_matches_it_is_clean() {
        // The other half of the same rule: reading the allowlist on a single-major tree must not
        // turn a correct tree red, or the gate is one somebody disables.
        let majors = family_majors("[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        let found = assess(&majors, &allowed("59 2026-08-28 the engine's major\n").permitted);
        assert!(found.inert.is_empty());
        assert!(found.unexplained.is_empty());
    }

    #[test]
    fn a_single_major_needs_no_row_at_all() {
        // One major is the state this workspace wants, not an exception to anything.
        let majors = family_majors("[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n");
        let found = assess(&majors, &allowed("").permitted);
        assert!(found.unexplained.is_empty());
        assert!(found.inert.is_empty());
    }

    #[test]
    fn every_row_goes_inert_when_arrow_leaves_the_graph_entirely() {
        // The stated consequence of treating the engine's own row as a row like any other: a list
        // of tolerated Arrow majors on a tree with no Arrow is a rule that lost its subject.
        let majors = family_majors("[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n");
        let rows = allowed("58 2026-08-28 the adapter\n59 2026-08-28 the engine\n");
        assert_eq!(assess(&majors, &rows.permitted).inert, vec!["58".to_owned(), "59".to_owned()]);
    }

    #[test]
    fn a_split_every_row_explains_is_clean_in_both_directions() {
        let rows = allowed("58 2026-08-28 the adapter\n59 2026-08-28 the engine\n");
        let found = assess(&family_majors(split_lock()), &rows.permitted);
        assert!(found.unexplained.is_empty());
        assert!(found.inert.is_empty());
    }

    #[test]
    fn a_split_still_fails_on_the_major_no_row_names() {
        // Today's failure, unchanged - and the row for a major that IS absent is reported beside
        // it rather than instead of it.
        let rows = allowed("57 2026-08-28 long gone\n59 2026-08-28 the engine\n");
        let found = assess(&family_majors(split_lock()), &rows.permitted);
        assert_eq!(found.unexplained, vec!["58".to_owned()]);
        assert_eq!(found.inert, vec!["57".to_owned()]);
    }

    // ------------------------------------------------------------------ the verdict ---
    //
    // Everything above is a pure function. Nothing above would notice if the report and the return
    // statement disagreed, which they did: deleting one disjunct from the old return guard printed
    // `FAILED` and exited 0 with all of them green. These drive `run_in` over a crafted tree, so
    // each finding is wired to the exit code by a test rather than by reading.

    /// A tree with a lock file and, optionally, an allowlist. Unique per test and per process.
    fn tree(tag: &str, lock: &str, allow: Option<&str>) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-arrow-{}-{tag}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("clearing the scratch tree");
        }
        std::fs::create_dir_all(root.join("devco")).expect("the scratch tree");
        std::fs::write(root.join(LOCK), lock).expect("the lock file");
        if let Some(allow) = allow {
            std::fs::write(root.join(ALLOWLIST), allow).expect("the allowlist");
        }
        root
    }

    /// A lock file with `count` non-Arrow packages, so the outer floor has something to read.
    fn filler(count: usize) -> String {
        "[[package]]\nname = \"filler\"\nversion = \"1.0.0\"\n\n".repeat(count)
    }

    #[test]
    fn a_stale_row_on_a_single_major_tree_reaches_the_exit_code() {
        // THE issue's case, at the verdict rather than at `assess`: one major, a row for a major
        // that closed. On `main` this printed `ok - one Arrow major` and exited 0.
        let lock = format!("{}[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n", filler(3));
        let root = tree("stale", &lock, Some("58 2026-08-28 the adapter\n59 2026-08-28 the engine\n"));
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn a_clean_single_major_tree_passes_at_the_verdict() {
        let lock = format!("{}[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n", filler(3));
        let root = tree("clean", &lock, Some("59 2026-08-28 the engine's major\n"));
        assert_eq!(run_in(&root), Verdict::Pass);
    }

    #[test]
    fn an_unexplained_split_reaches_the_exit_code() {
        let lock = format!("{}{}", filler(3), split_lock());
        let root = tree("split", &lock, Some("59 2026-08-28 the engine\n"));
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn a_bare_row_is_not_an_explanation_at_the_verdict() {
        // `58\n59\n` against a split lock used to print `ok - 2 Arrow majors, every one explained`
        // with an empty reason beside each - the headline asserting a control that was not there.
        let lock = format!("{}{}", filler(3), split_lock());
        let root = tree("bare", &lock, Some("58\n59\n"));
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn a_duplicated_row_reaches_the_exit_code() {
        let lock = format!("{}{}", filler(3), split_lock());
        let root = tree(
            "dup",
            &lock,
            Some("58 2026-01-01 the first\n58 2026-08-28 the second\n59 2026-08-28 the engine\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn an_absent_allowlist_is_configuration_and_an_unreadable_one_is_a_fault() {
        // The pair that `unwrap_or_default()` made one case. Absent means nothing is tolerated;
        // unreadable means every row in it could be stale and the gate cannot tell.
        let lock = format!("{}[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n", filler(3));
        let absent = tree("absent", &lock, None);
        assert_eq!(run_in(&absent), Verdict::Pass, "a missing allowlist is a legitimate tree");

        // Not UTF-8: `read_to_string` gives `InvalidData`, which is not `NotFound`.
        let unreadable = tree("unreadable", &lock, Some(""));
        std::fs::write(unreadable.join(ALLOWLIST), [0x66, 0xff, 0xfe, 0x0a]).expect("the bytes");
        assert_eq!(run_in(&unreadable), Verdict::Fail, "an unreadable allowlist is a fault");
    }

    #[test]
    fn a_lock_that_parses_to_nothing_is_the_scan_being_broken() {
        // The outer floor. Zero family members reads the same on a broken parser and on a tree
        // with no Arrow, so the count that witnesses the scan is the one over ALL packages.
        let root = tree("empty-lock", "# a lock file this reader cannot parse\n", None);
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn a_present_allowlist_that_parses_to_nothing_is_a_fault() {
        // The inner floor, and it is a different file from the one above: a list of exceptions
        // with no exceptions in it is a file that should have been deleted, or a broken reader.
        let lock = format!("{}[[package]]\nname = \"arrow\"\nversion = \"59.2.0\"\n", filler(3));
        let root = tree("empty-allow", &lock, Some("# every row was deleted\n"));
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn the_real_tree_passes_this_gate() {
        // The fixtures above are shapes; this is the repository. The assertion that goes red when
        // somebody edits `devco/arrow-majors-allow` or `just update` closes the split, rather than
        // minutes into a nix step - the same anchor `check-workflows` and `check-venues` each keep.
        let Some(root) = repo::root() else { return };
        let lock = std::fs::read_to_string(root.join(LOCK)).expect(LOCK);
        let all = packages(&lock);
        assert!(all.len() > 100, "the lock reader found {} packages - it is broken", all.len());
        assert!(
            all.iter().any(|p| is_family(p.name)),
            "no arrow-family package in the real lock file - the family filter is broken"
        );
        assert_eq!(run_in(&root), Verdict::Pass);
    }
}
