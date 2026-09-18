//! `VENDOR.md`'s mimalloc row states a cardinal count of local modifications and enumerates
//! them; `Cargo.toml` and `REUSE.toml` each repeat the same cardinal. Nothing compared any of
//! the four until this gate, which is why `#857` took the count four to five, its own sweep
//! missed two of the three repeaters (case-sensitive, and one occurrence is capitalised at
//! sentence start - *"Four local changes."*), and a reviewer had to find them by hand.
//! `github.com/telekom/sutura#865`.
//!
//! # WHAT IT CHECKS
//!
//! For the `vendor/mimalloc_rust/**` row specifically: `VENDOR.md`'s "Local changes" cell states
//! a leading cardinal (`Six local changes.`) and enumerates `(1)` through `(N)`; this asserts the
//! two agree, and that [`OTHER_SITES`] - the two files `#865` found repeating the same cardinal -
//! agree with both. Matched CASE-INSENSITIVELY, which is the specific bug `#857` shipped: the
//! search pattern was lowercase-only and missed the sentence-initial capital.
//!
//! # WHY THREE NAMED FILES, NOT A WALK OF THE TREE
//!
//! `check-arrow` reads two named files rather than every manifest in the workspace, and the
//! argument carries over: the phrase `"<cardinal> local change(s)"` appears nowhere else in this
//! tree today (`tests::the_real_tree_passes_this_gate` is the anchor that would catch it moving),
//! and a repo-wide scan would pay to open every vendored byte in `vendor/mimalloc_rust/**` for a
//! collision that has not happened. Widening [`OTHER_SITES`] is one line the day a third file
//! starts repeating this count - narrower than a generic "cardinal near a count noun" rule, which
//! `#865` itself warns off: two ADRs, an `xtask` table-column comment and a credential doc
//! comment (mirrored into a generated page) all say "N local changes/fixes" for something else
//! entirely, and a wider pattern would refuse every one of them.
//!
//! # WHAT IT DOES NOT CATCH
//!
//! Whether the count is TRUE. A modification added to `build.rs` without touching `VENDOR.md` at
//! all leaves the cardinal and the enumeration agreeing with each other and with both other
//! files - this closes *"the stated counts disagree"*, not *"the stated count is right"*.
//! Catching that needs a diff against the vendored tree itself, which is a different mechanism
//! and is not this gate's job.
//!
//! # ABSENCE IS A FAULT HERE, NOT A LEGITIMATE TREE
//!
//! Unlike `check-arrow`'s allowlist - where no row is a real, permitted state - a missing
//! mimalloc row, an unreadable cell, or a named [`OTHER_SITES`] file that has stopped repeating
//! the count are each refused rather than passed. `vendor/mimalloc_rust/**` is vendored,
//! recorded, load-bearing infrastructure; a `VENDOR.md` that stops documenting it is losing the
//! record, not retiring an exception, and a gate that read that as "nothing to check" would be
//! exactly the vacuous pass this issue exists to close. See `tests::an_absent_row_is_a_fault`
//! and its siblings.

use std::path::Path;

use crate::Verdict;
use crate::repo;

/// Where the row lives.
const VENDOR_MD: &str = "VENDOR.md";

/// The literal that identifies the row among every other row in the table.
const ROW_ANCHOR: &str = "vendor/mimalloc_rust/**";

/// The two other files `#865` found repeating the same cardinal, each read whole rather than at
/// a pinned line number - a line number is a second thing to keep true, and `#871` already moved
/// both once.
const OTHER_SITES: &[&str] = &["Cargo.toml", "REUSE.toml"];

/// The phrase a cardinal must sit directly before. Matches `"local change"` and `"local
/// changes"` - see [`phrase_end`] for the plural and the word-boundary check.
const PHRASE: &str = "local change";

/// Cardinal words this gate recognises, lowercase. Twelve is headroom past the six mimalloc
/// carries today - cheap to state, and cheaper than a fifth PR widening it by one word.
const CARDINALS: &[(&str, u32)] = &[
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("eleven", 11),
    ("twelve", 12),
];

/// The value a cardinal word names, matched case-insensitively. `word` is already lowercase.
fn cardinal_value(word: &str) -> Option<u32> {
    CARDINALS.iter().find(|(w, _)| *w == word).map(|(_, v)| *v)
}

/// One `"<cardinal> local change(s)"` statement found in a file: the line it starts on, the
/// word as WRITTEN (so a message can show `"Six"`, not `"six"`), and the value it names.
struct Occurrence<'a> {
    line: usize,
    word: &'a str,
    value: u32,
}

/// Where the phrase match ends, or `None` if the character after it says this is not the whole
/// word - `"changed"` and `"changer"` are not `"change"`, and neither is a plural with a third
/// letter glued on.
fn phrase_end(lowered: &str, start: usize) -> Option<usize> {
    let base = start + PHRASE.len();
    let bytes = lowered.as_bytes();
    if bytes.get(base) == Some(&b's') {
        if bytes.get(base + 1).is_some_and(u8::is_ascii_alphabetic) {
            return None;
        }
        Some(base + 1)
    } else if bytes.get(base).is_some_and(u8::is_ascii_alphabetic) {
        None
    } else {
        Some(base)
    }
}

/// The cardinal word directly before `start`, if there is exactly one whitespace-separated word
/// there and it is one [`CARDINALS`] recognises.
///
/// Requires whitespace immediately before `start`, so `"nonlocal change"` - which contains
/// `"local change"` as a substring - is refused by [`occurrences_in`]'s own left-boundary check
/// before this is even called; this function refuses a word glued to `"local"` with nothing
/// between them for the same reason.
///
/// No `[]` indexing anywhere: `head` is `lowered`'s prefix up to `start`, so `trimmed` - `head`
/// with its trailing whitespace stripped - is ITSELF a prefix starting at byte 0, which is what
/// lets `trimmed.len()` and the length after stripping the trailing alphabetic run double as
/// absolute byte offsets into both `lowered` and `original` without recomputing anything.
fn cardinal_before<'a>(original: &'a str, lowered: &str, start: usize) -> Option<(&'a str, u32)> {
    let head = lowered.get(..start)?;
    if !head.ends_with(|c: char| c.is_ascii_whitespace()) {
        return None;
    }
    let trimmed = head.trim_end_matches(|c: char| c.is_ascii_whitespace());
    let word_end = trimmed.len();
    let word_start = trimmed.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len();
    if word_start == word_end {
        return None;
    }
    let value = cardinal_value(lowered.get(word_start..word_end)?)?;
    Some((original.get(word_start..word_end)?, value))
}

/// Every well-formed `"<cardinal> local change(s)"` statement in `text`, case-insensitively.
///
/// `to_ascii_lowercase` rather than `to_lowercase`: it changes only ASCII letters, one byte for
/// one byte, so a byte offset found in the lowered copy still slices the original correctly -
/// which is what lets [`cardinal_before`] hand back the WRITTEN case for a message.
fn occurrences_in(text: &str) -> Vec<Occurrence<'_>> {
    let lowered = text.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(rel) = lowered.get(from..).and_then(|s| s.find(PHRASE)) {
        let start = from.saturating_add(rel);
        from = start.saturating_add(1);
        let head = lowered.get(..start).unwrap_or_default();
        // Left boundary: a letter directly before `start` means this is the tail of some other
        // word (`"nonlocal change"`), not a standalone `"local"`.
        if head.ends_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        let Some(_end) = phrase_end(&lowered, start) else {
            continue;
        };
        let Some((word, value)) = cardinal_before(text, &lowered, start) else {
            continue;
        };
        let line = head.bytes().filter(|&b| b == b'\n').count().saturating_add(1);
        found.push(Occurrence { line, word, value });
    }
    found
}

/// The `(N)` markers in a cell, in the order they appear - `enumerated(N) items` per `#865`'s
/// own shape. Any parenthesised run of digits counts; [`well_formed`] is what asks whether the
/// result is a clean `1..=N` sequence rather than noise.
fn enumerated(cell: &str) -> Vec<u32> {
    let mut items = Vec::new();
    let mut rest = cell;
    while let Some(open) = rest.find('(') {
        rest = rest.get(open.saturating_add(1)..).unwrap_or_default();
        let Some(close) = rest.find(')') else { break };
        let inside = rest.get(..close).unwrap_or_default();
        if !inside.is_empty()
            && inside.bytes().all(|b| b.is_ascii_digit())
            && let Ok(n) = inside.parse::<u32>()
        {
            items.push(n);
        }
        rest = rest.get(close.saturating_add(1)..).unwrap_or_default();
    }
    items
}

/// Is `items` exactly `(1), (2), ..., (N)` with nothing skipped, repeated or out of order?
///
/// Empty is refused rather than trivially agreeing: `1..=0` is itself an empty range, so an
/// empty `items` would otherwise equal it vacuously and a cell that lost its whole enumeration
/// would read as "well formed".
fn well_formed(items: &[u32]) -> bool {
    if items.is_empty() {
        return false;
    }
    let Ok(len) = u32::try_from(items.len()) else { return false };
    items.iter().copied().eq(1..=len)
}

/// The mimalloc row's line number (one-based) and its "Local changes" cell, if the row is there.
///
/// The row is one physical line - a markdown table row - so no cell in this file spans more
/// than one `.lines()` entry. The cell is the last non-empty pipe-delimited segment, which holds
/// for every row in this table's six columns and does not depend on the header naming them in a
/// particular order.
fn mimalloc_row(text: &str) -> Option<(usize, String)> {
    for (i, line) in text.lines().enumerate() {
        if line.contains(ROW_ANCHOR) {
            let cell = line.split('|').map(str::trim).rfind(|c| !c.is_empty())?;
            return Some((i + 1, String::from(cell)));
        }
    }
    None
}

/// One occurrence elsewhere that disagrees with the row, or a site that does not carry the
/// statement at all - both are findings, not silence.
enum SiteFinding {
    /// `file:line` states `word` (`value`), and it is not `expected`.
    Mismatch {
        file: &'static str,
        line: usize,
        word: String,
        value: u32,
    },
    /// `file` states the phrase zero times, or more than once.
    Absent {
        file: &'static str,
    },
    Ambiguous {
        file: &'static str,
        count: usize,
    },
}

/// Read one of [`OTHER_SITES`] and check it against `expected`.
fn check_site(root: &Path, file: &'static str, expected: u32) -> Result<(), SiteFinding> {
    let Ok(text) = std::fs::read_to_string(root.join(file)) else {
        return Err(SiteFinding::Absent { file });
    };
    match occurrences_in(&text).as_slice() {
        [] => Err(SiteFinding::Absent { file }),
        [one] if one.value == expected => Ok(()),
        [one] => Err(SiteFinding::Mismatch {
            file,
            line: one.line,
            word: String::from(one.word),
            value: one.value,
        }),
        many => Err(SiteFinding::Ambiguous { file, count: many.len() }),
    }
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-vendor-count: could not determine the repo root");
        return Verdict::Fail;
    };
    run_in(&root)
}

/// The gate, over any root. Split from [`run`] so a test can hand it a crafted tree and assert on
/// the VERDICT, the way `check-arrow`'s own `run_in` does.
fn run_in(root: &Path) -> Verdict {
    const NAME: &str = "xtask check-vendor-count";

    let Ok(vendor_md) = std::fs::read_to_string(root.join(VENDOR_MD)) else {
        eprintln!("{NAME}: FAILED - could not read {VENDOR_MD}");
        return Verdict::Fail;
    };
    let Some((row_line, cell)) = mimalloc_row(&vendor_md) else {
        eprintln!(
            "{NAME}: FAILED - {VENDOR_MD} carries no row for `{ROW_ANCHOR}` - the vendored \
             mimalloc row is gone, or no longer names its own path literally. A missing row is a \
             fault here, not a legitimate tree: `{ROW_ANCHOR}` is load-bearing vendored \
             infrastructure, and losing the record is not the same thing as retiring it."
        );
        return Verdict::Fail;
    };

    let (word, cardinal_value) = match occurrences_in(&cell).as_slice() {
        [] => {
            eprintln!(
                "{NAME}: FAILED - {VENDOR_MD}:{row_line}'s Local changes cell states no cardinal \
                 near \"local change(s)\""
            );
            return Verdict::Fail;
        }
        [one] => (String::from(one.word), one.value),
        many => {
            let words: Vec<&str> = many.iter().map(|o| o.word).collect();
            eprintln!(
                "{NAME}: FAILED - {VENDOR_MD}:{row_line}'s cell states {} cardinals near \"local \
                 change(s)\", not one: {words:?}",
                many.len()
            );
            return Verdict::Fail;
        }
    };

    let items = enumerated(&cell);
    if !well_formed(&items) {
        eprintln!(
            "{NAME}: FAILED - {VENDOR_MD}:{row_line}'s cell enumerates {items:?}, not a clean \
             (1)..(N) sequence"
        );
        return Verdict::Fail;
    }
    let Ok(enumerated_count) = u32::try_from(items.len()) else {
        eprintln!("{NAME}: FAILED - {VENDOR_MD}:{row_line}'s cell enumerates an unreadable count");
        return Verdict::Fail;
    };

    let mut problems: Vec<String> = Vec::new();
    if cardinal_value != enumerated_count {
        problems.push(format!(
            "{VENDOR_MD}:{row_line}: says \"{word} local change(s)\" ({cardinal_value}) but \
             enumerates {enumerated_count} item(s)"
        ));
    }
    for &site in OTHER_SITES {
        if let Err(finding) = check_site(root, site, cardinal_value) {
            problems.push(match finding {
                SiteFinding::Mismatch { file, line, word, value } => {
                    format!("{file}:{line}: says \"{word} local change(s)\" ({value}) - {VENDOR_MD} says {cardinal_value}")
                }
                SiteFinding::Absent { file } => {
                    format!(
                        "{file}: states no cardinal near \"local change(s)\" - {VENDOR_MD}:{row_line} \
                         says {cardinal_value}, and #865 recorded {file} as repeating it"
                    )
                }
                SiteFinding::Ambiguous { file, count } => {
                    format!("{file}: states {count} cardinals near \"local change(s)\", not one")
                }
            });
        }
    }

    if !problems.is_empty() {
        eprintln!(
            "{NAME}: FAILED - {} mismatch(es) in the vendored mimalloc count",
            problems.len()
        );
        for problem in &problems {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("  {VENDOR_MD}'s row states a cardinal and enumerates it; {OTHER_SITES:?} each repeat");
        eprintln!("  the same cardinal. Fix the wrong number, or the enumeration, so exactly one");
        eprintln!("  count is true everywhere it is written.");
        return Verdict::Fail;
    }

    println!(
        "{NAME}: ok - {VENDOR_MD}:{row_line} states {cardinal_value} local change(s), matching \
         its own {enumerated_count} enumerated item(s) and {} site(s) in {OTHER_SITES:?}",
        OTHER_SITES.len()
    );
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capitalised_cardinal_at_sentence_start_is_still_read() {
        // THE #857 BUG, ISOLATED: the sweep that missed this was case-sensitive lowercase, and
        // this is the exact shape that escaped it - a cardinal capitalised because it opens a
        // sentence mid-cell, not because it opens the cell.
        let found = occurrences_in("untouched. Six local changes. (1) one thing.");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].word, "Six");
        assert_eq!(found[0].value, 6);
    }

    #[test]
    fn plural_and_singular_both_match() {
        assert_eq!(occurrences_in("One local change.")[0].value, 1);
        assert_eq!(occurrences_in("Two local changes.")[0].value, 2);
    }

    #[test]
    fn a_word_glued_to_local_is_not_a_match() {
        // "nonlocal changes" contains "local change" as a raw substring - the left-boundary
        // check exists so this is not read as a cardinal-less statement of nothing, and is not
        // misread as some OTHER word's cardinal either.
        assert!(occurrences_in("a nonlocal changes to the schema").is_empty());
    }

    #[test]
    fn changed_and_changer_are_not_change() {
        assert!(occurrences_in("Six local changed the outcome").is_empty());
        assert!(occurrences_in("Six local changer arrived").is_empty());
    }

    #[test]
    fn a_non_cardinal_word_before_the_phrase_is_not_a_match() {
        assert!(occurrences_in("Several local changes landed").is_empty());
    }

    #[test]
    fn enumeration_reads_the_parenthesised_markers_in_order() {
        assert_eq!(enumerated("(1) a. (2) b. (3) c."), vec![1, 2, 3]);
        assert!(well_formed(&enumerated("(1) a. (2) b. (3) c.")));
    }

    #[test]
    fn a_skipped_or_repeated_item_is_not_well_formed() {
        assert!(!well_formed(&enumerated("(1) a. (3) b.")), "skipped (2)");
        assert!(!well_formed(&enumerated("(1) a. (1) b.")), "repeated (1)");
        assert!(!well_formed(&[]), "an empty enumeration explains nothing");
    }

    #[test]
    fn the_row_is_found_by_its_own_path_literal() {
        let vendor_md = "| a | b |\n| --- | --- |\n| `vendor/mimalloc_rust/**` | Six local changes. (1) a. |\n";
        let (line, cell) = mimalloc_row(vendor_md).expect("the row");
        assert_eq!(line, 3);
        assert_eq!(cell, "Six local changes. (1) a.");
    }

    #[test]
    fn an_unrelated_row_is_not_the_mimalloc_row() {
        assert!(mimalloc_row("| `vendor/other/**` | Two local changes. (1) x. (2) y. |\n").is_none());
    }

    // ---------------------------------------------------------------- the verdict, over a tree ---

    /// A scratch tree, unique per test and per process. `VENDOR.md` and the two other sites are
    /// each optional, so a test can omit one to probe the absent-site arm.
    fn tree(tag: &str, vendor_md: Option<&str>, cargo_toml: Option<&str>, reuse_toml: Option<&str>) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-vendor-count-{}-{tag}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("clearing the scratch tree");
        }
        std::fs::create_dir_all(&root).expect("the scratch tree");
        if let Some(text) = vendor_md {
            std::fs::write(root.join(VENDOR_MD), text).expect("VENDOR.md");
        }
        if let Some(text) = cargo_toml {
            std::fs::write(root.join("Cargo.toml"), text).expect("Cargo.toml");
        }
        if let Some(text) = reuse_toml {
            std::fs::write(root.join("REUSE.toml"), text).expect("REUSE.toml");
        }
        root
    }

    /// A clean tree: all three sites agree on six.
    fn row(cardinal: &str, items: &str) -> String {
        format!("| `vendor/mimalloc_rust/**` | up | MIT | abc | 2026-01-01 | Vendored. {cardinal} local changes. {items} |\n")
    }

    const SIX_ITEMS: &str = "(1) a. (2) b. (3) c. (4) d. (5) e. (6) f.";

    #[test]
    fn a_clean_tree_passes_at_the_verdict() {
        let root = tree(
            "clean",
            Some(&row("Six", SIX_ITEMS)),
            Some("# six local changes to the build script and both manifests.\n"),
            Some("# the six local changes; this records the licence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Pass);
    }

    #[test]
    fn desync_vendor_md_cardinal_alone_fails() {
        // VENDOR.md changed to Seven while its own enumeration and both other sites stay at six.
        let root = tree(
            "desync-vendor",
            Some(&row("Seven", SIX_ITEMS)),
            Some("# six local changes to the build script and both manifests.\n"),
            Some("# the six local changes; this records the licence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn desync_cargo_toml_alone_fails() {
        let root = tree(
            "desync-cargo",
            Some(&row("Six", SIX_ITEMS)),
            Some("# seven local changes to the build script and both manifests.\n"),
            Some("# the six local changes; this records the licence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn desync_reuse_toml_alone_fails() {
        let root = tree(
            "desync-reuse",
            Some(&row("Six", SIX_ITEMS)),
            Some("# six local changes to the build script and both manifests.\n"),
            Some("# the seven local changes; this records the licence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn desync_the_enumeration_alone_fails() {
        // The cardinal still says Six; one enumerated item was deleted.
        let five_items = "(1) a. (2) b. (3) c. (4) d. (5) e.";
        let root = tree(
            "desync-enum",
            Some(&row("Six", five_items)),
            Some("# six local changes to the build script and both manifests.\n"),
            Some("# the six local changes; this records the licence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn a_negative_control_rewrite_stays_green() {
        // Semantically identical to the clean tree: reworded prose around the same six items,
        // same cardinal, same enumeration. A gate keyed on wording rather than the count would
        // fail this; this one must not.
        let root = tree(
            "negative-control",
            Some(&row(
                "Six",
                "(1) alpha, reworded. (2) beta, reworded. (3) gamma. (4) delta. (5) epsilon. (6) zeta.",
            )),
            Some("# exactly six local changes, restated differently here.\n"),
            Some("# recording the six local changes under a new sentence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Pass);
    }

    #[test]
    fn an_absent_row_is_a_fault() {
        let root = tree(
            "absent-row",
            Some("no mimalloc row at all\n"),
            Some("nothing here\n"),
            Some("nothing here\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn an_absent_vendor_md_is_a_fault() {
        let root = tree(
            "absent-vendor-md",
            None,
            Some("six local changes\n"),
            Some("six local changes\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn a_named_site_that_stopped_repeating_the_count_is_a_fault() {
        // Not a vacuous pass: deleting the comment instead of fixing a wrong number must not
        // read as "nothing to compare".
        let root = tree(
            "site-dropped",
            Some(&row("Six", SIX_ITEMS)),
            Some("no mention of any local changes here\n"),
            Some("# the six local changes; this records the licence.\n"),
        );
        assert_eq!(run_in(&root), Verdict::Fail);
    }

    #[test]
    fn the_real_tree_passes_this_gate() {
        // The fixtures above are shapes; this is the repository, the same anchor `check-arrow`
        // and `check-workflows` each keep.
        let Some(root) = repo::root() else { return };
        assert_eq!(run_in(&root), Verdict::Pass);
    }
}
