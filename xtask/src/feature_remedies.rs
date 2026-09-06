//! A refusal that tells an operator to rebuild names a feature the crate actually declares.
//!
//! `github.com/telekom/sutura#366`: `sutura-serve` refused `catalog.kind: datahub` by telling the
//! reader to "build the binary with the feature that provides it". There is no such feature, there
//! is no `--features` value that satisfies the sentence, and the adapter is not a dependency of
//! that crate in any form - so the refusal was correct about what the build does and its remedy was
//! unactionable. Nothing could see that, because the sentence is a string literal and a string
//! literal is prose to every gate here.
//!
//! # Why this shape rather than a wording fix
//!
//! The same rule already exists one class over. `github.com/telekom/sutura#246` made the remedy a
//! *gate* prints resolve - a cited `just` task must exist, a cited path must be in the tree - and
//! #245 made a missing-service remedy cite its own venue's task. A remedy naming a cargo feature is
//! the same kind of claim, about the manifest instead of about the justfile, and it had no reader.
//! Fixing only the sentence leaves the next one unheld.
//!
//! # What it reads, and what triggers it
//!
//! Every string literal under `crates/*/src/`, through
//! [`string_literals`](crate::serde_parse::scan::string_literals) - the inverse of the reader the
//! other Rust-reading gates use, over the same walk, so a COMMENT describing a refusal is invisible
//! here exactly as it is code there. Test regions come out through
//! [`regions::scope`](crate::causality::regions::scope), so a fixture asserting on a message is not
//! itself a message.
//!
//! A literal is a subject when it instructs a rebuild: **a build verb, and the word `feature` after
//! it.** The order is the trigger and not merely both words, because "this build does not link the
//! adapter" is a statement about the build and not an instruction - it earns no remedy and is not
//! asked for one. What a subject must then do is name a feature its own crate's `[features]` table
//! declares, read out of `crates/<name>/Cargo.toml`.
//!
//! # Fails closed
//!
//! A scan that finds NO subject has stopped reading rather than found a
//! clean tree - the failure mode `guidance/claims/remedies.rs` names for its own path citations - so
//! zero subjects is red, and so is a manifest that cannot be read. How many there are is PRINTED
//! with the files, not written down here: a number in a comment is a second thing to keep true, and
//! a count with no list behind it is not a witness.
//!
//! # Three limits, next to the claim
//!
//! **The crate is the file's own, not the crate the sentence names.** `sutura-serve`'s message says
//! "Build `sutura-serve`", and resolving that name instead would be a second lookup for no gain
//! today - both messages sit in the crate they name. A message directing a reader to ANOTHER
//! crate's feature resolves against the wrong manifest, and would be a false verdict in either
//! direction. Nothing in the tree does that; if something does, this comment is what a reviewer is
//! owed rather than a silently wrong answer.
//!
//! **A feature that exists but delivers nothing is green.** This resolves a NAME against a table. A
//! `datahub = []` entry added to satisfy the gate would pass it while linking no adapter - the
//! composition half of `github.com/telekom/sutura#134`, which is a different problem and not one a
//! text reader can hold.
//!
//! **It reads the literal, not the format.** A message assembled from pieces, or one whose feature
//! name arrives through a `{}`, is not a subject: the name is not in the text. Both messages in the
//! tree spell theirs out.

use std::collections::BTreeSet;

use crate::Verdict;
use crate::causality::regions::{self, PostImage};
use crate::repo;
use crate::serde_parse::scan::string_literals;

/// The word a remedy has to resolve, and the one this gate keys on.
const FEATURE: &str = "feature";

/// The verb that makes a sentence an instruction rather than a description.
///
/// One spelling covers `Build` and `Rebuild` too, because the comparison is over lowercased text -
/// and both forms are live in this tree.
const BUILD: &str = "build";

/// One literal that instructs a rebuild.
#[derive(Debug, PartialEq, Eq)]
struct Subject {
    /// Repo-relative file.
    file: String,
    /// 1-based line the literal opens on.
    line: usize,
    /// Feature names the sentence spells out, in the order it spells them.
    names: Vec<String>,
}

/// Is this file one whose literals are a binary's messages?
fn in_scope(rel: &str) -> bool {
    rel.starts_with("crates/")
        && rel.contains("/src/")
        && std::path::Path::new(rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
}

/// The crate directory a source file belongs to: `crates/<name>`.
fn crate_dir(rel: &str) -> Option<String> {
    let (dir, _) = rel.split_once("/src/")?;
    Some(String::from(dir))
}

/// Does this text instruct a rebuild - a build verb, and the word `feature` after it?
///
/// The ORDER is the trigger. "this build does not link the adapter" is a statement about what was
/// linked; "build it with the `x` feature" is a direction to go somewhere. Only the second owes a
/// reader a name, and demanding one of the first would fail correct prose.
fn instructs_a_rebuild(body: &str) -> bool {
    let lowered = body.to_lowercase();
    lowered
        .find(BUILD)
        .and_then(|at| at.checked_add(BUILD.len()))
        .and_then(|after| lowered.get(after..))
        .is_some_and(|tail| tail.contains(FEATURE))
}

/// Every feature name the sentence spells out.
///
/// Two shapes, which are the two ways this workspace writes one: the value after `--features`, and
/// a backticked span sitting immediately before the word `feature`. Anything else is prose, and a
/// gate that guessed at a bare word would resolve half the sentence.
fn named_features(body: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find("--features") {
        let Some(tail) = at.checked_add("--features".len()).and_then(|after| rest.get(after..)) else {
            break;
        };
        if let Some(name) = tail.split_whitespace().next().map(trim_name)
            && !name.is_empty()
        {
            names.push(String::from(name));
        }
        rest = tail;
    }
    let mut rest = body;
    while let Some(at) = rest.find(FEATURE) {
        if let Some(before) = rest.get(..at)
            && let Some(name) = backticked_tail(before)
        {
            names.push(String::from(name));
        }
        let Some(tail) = at.checked_add(FEATURE.len()).and_then(|after| rest.get(after..)) else {
            break;
        };
        rest = tail;
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// The contents of a backtick span that ENDS this text, ignoring trailing spaces.
///
/// `` `bigquery` feature `` is how both live messages spell it. A span that does not sit right
/// before the word is some other backticked thing in the sentence - a settings key, a kind - and
/// reading it as a feature name would invent a citation the author never made.
fn backticked_tail(before: &str) -> Option<&str> {
    let trimmed = before.trim_end();
    let inner = trimmed.strip_suffix('`')?;
    let at = inner.rfind('`')?;
    inner.get(at.checked_add(1)?..).filter(|name| !name.is_empty())
}

/// One feature name with the punctuation a sentence puts around it removed.
fn trim_name(raw: &str) -> &str {
    raw.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
}

/// Every feature `manifest` declares, read as text.
///
/// Text rather than a manifest parser for `pins.rs`'s reason: a gate runs on a host with no
/// resolver and no network. The `[features]` table's keys are what a `--features` value may name,
/// and a continuation line inside an array value never looks like one because it holds no `=`.
fn declared_features(manifest: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[features]";
            continue;
        }
        if !inside || trimmed.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = trimmed.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                found.insert(String::from(name));
            }
        }
    }
    found
}

/// Every rebuild instruction in the tree, outside test code.
fn subjects(files: &[String], read: &PostImage<'_>) -> Result<Vec<Subject>, String> {
    let mut found = Vec::new();
    for rel in files.iter().filter(|rel| in_scope(rel)) {
        let text = read(rel).ok_or_else(|| {
            format!("could not read {rel}, so the scan that decides which messages this gate reads is incomplete")
        })?;
        if !text.contains(FEATURE) {
            continue;
        }
        let tests = regions::scope(rel, read);
        for literal in string_literals(&text) {
            if tests.covers(literal.line) || !instructs_a_rebuild(&literal.body) {
                continue;
            }
            found.push(Subject {
                file: rel.clone(),
                line: literal.line,
                names: named_features(&literal.body),
            });
        }
    }
    Ok(found)
}

/// Each subject against its own crate's table.
fn unresolved(root: &std::path::Path, found: &[Subject]) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    for subject in found {
        let dir = crate_dir(&subject.file)
            .ok_or_else(|| format!("{}: could not tell which crate this file belongs to", subject.file))?;
        let path = format!("{dir}/Cargo.toml");
        let manifest = std::fs::read_to_string(root.join(&path)).map_err(|error| {
            format!(
                "could not read {path}, so the features {} cites resolve against nothing: {error}",
                subject.file
            )
        })?;
        let declared = declared_features(&manifest);
        let at = format!("{}:{}", subject.file, subject.line);
        if subject.names.is_empty() {
            problems.push(format!(
                "{at}: tells an operator to rebuild with a feature and names none - there is no \
                 `--features` value that satisfies it, so the remedy is unactionable"
            ));
            continue;
        }
        if !subject.names.iter().any(|name| declared.contains(name)) {
            problems.push(format!(
                "{at}: cites the feature(s) {} - {path} declares {}",
                subject.names.join(", "),
                if declared.is_empty() {
                    String::from("none")
                } else {
                    declared.iter().cloned().collect::<Vec<_>>().join(", ")
                }
            ));
        }
    }
    Ok(problems)
}

fn check() -> Result<Vec<String>, String> {
    let repo::RepoFiles { root, files } = repo::all_files().ok_or_else(|| String::from("could not locate the repo root"))?;
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    let found = subjects(&files, &read)?;
    if found.is_empty() {
        return Err(String::from(
            "read no rebuild instruction out of any crate source - the literal scan is broken, not \
             the tree. Several messages in this workspace instruct one, and a reader that finds \
             none of them passes everything",
        ));
    }
    let problems = unresolved(&root, &found)?;
    if problems.is_empty() {
        return Ok(found
            .iter()
            .map(|subject| format!("{}:{} -> {}", subject.file, subject.line, subject.names.join(", ")))
            .collect());
    }
    Err(problems.join("\n  "))
}

/// `cargo xtask check-feature-remedies` - a refusal that says to rebuild names a real feature.
pub(crate) fn run(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("usage: check-feature-remedies - it takes no arguments");
        return Verdict::Usage;
    }
    match check() {
        Ok(held) => {
            // The subjects are PRINTED rather than counted, because a number is not a witness: a
            // reader who cannot see which messages were read cannot tell a clean tree from a
            // reader that stopped reading, and this gate's whole failure mode is the second one.
            eprintln!(
                "xtask check-feature-remedies: ok - {} rebuild instruction(s) name a declared feature",
                held.len()
            );
            for one in &held {
                eprintln!("  {one}");
            }
            Verdict::Pass
        }
        Err(problem) => {
            eprintln!("xtask check-feature-remedies: FAILED\n  {problem}");
            eprintln!(
                "  A remedy naming a cargo feature is a claim about the manifest. Name a feature the\n  \
                 crate declares, or say what the build does and drop the instruction to rebuild."
            );
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{backticked_tail, declared_features, instructs_a_rebuild, named_features, subjects, unresolved};

    /// The reader every fixture test uses: a fixed map, so classification needs no checkout.
    fn reading<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + use<'a> {
        move |path: &str| {
            files
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, text)| String::from(*text))
        }
    }

    #[test]
    fn a_description_of_the_build_is_not_an_instruction_to_rebuild() {
        // THE distinction the gate rests on. Both sentences hold the word `build`; only the second
        // sends a reader somewhere, and only the second owes them a name. Demanding one of the
        // first would fail the very message #366 replaces the broken one with.
        assert!(!instructs_a_rebuild(
            "names a metadata adapter no build of this binary links - write `markdown` instead"
        ));
        assert!(instructs_a_rebuild("build the binary with the feature that provides it"));
        // The word has to come AFTER the verb: a sentence that names a feature and then describes
        // what was built is not directing anyone to rebuild.
        assert!(!instructs_a_rebuild(
            "the `bigquery` feature is off, so this build links nothing"
        ));
        // Case is not the trigger.
        assert!(instructs_a_rebuild("Build `sutura-serve` with `--features bigquery`"));
    }

    #[test]
    fn a_feature_name_is_read_from_a_flag_or_from_the_span_before_the_word() {
        assert_eq!(
            named_features("Build `sutura-serve` with `--features bigquery`, or else"),
            vec!["bigquery"]
        );
        assert_eq!(
            named_features("built without the `bigquery` feature - so it links nothing"),
            vec!["bigquery"]
        );
        // #366's literal: it instructs a rebuild and spells no name at all.
        assert!(named_features("build the binary with the feature that provides it, or write `markdown`").is_empty());
        // A backticked span that is NOT adjacent to the word is some other thing in the sentence -
        // a settings key here - and reading it as a feature would invent a citation.
        assert!(named_features("`catalog.kind: datahub` needs a feature").is_empty());
        assert_eq!(backticked_tail("built without the `bigquery` "), Some("bigquery"));
        assert_eq!(backticked_tail("nothing backticked here "), None);
    }

    #[test]
    fn the_features_table_is_read_and_the_next_section_ends_it() {
        let manifest = "[package]\nname = \"x\"\n\n[features]\n# a comment\ntls = [\"dep:a\"]\nbigquery = [\n  \"dep:b\",\n]\n\n[dependencies]\nserde = \"1\"\n";
        let declared = declared_features(manifest);
        assert!(declared.contains("tls"), "{declared:?}");
        assert!(declared.contains("bigquery"), "{declared:?}");
        // `[dependencies]` entries are not features, and neither is a continuation line inside a
        // feature's own array - it carries no `=`.
        assert!(!declared.contains("serde"), "{declared:?}");
        assert!(!declared.contains("name"), "{declared:?}");
        assert_eq!(declared.len(), 2, "{declared:?}");
    }

    #[test]
    fn an_instruction_naming_no_feature_is_the_defect_and_a_declared_one_is_not() {
        // `github.com/telekom/sutura#366` as a fixture, beside the shape that is correct.
        let files = [
            (
                "crates/thing/src/lib.rs",
                "fn refuse() -> String {\n    String::from(\"names an adapter this build does not link - build the binary with the feature that provides it\")\n}\n",
            ),
            ("crates/thing/Cargo.toml", "[features]\ntls = []\n"),
        ];
        let read = reading(&files);
        let found = subjects(&[String::from("crates/thing/src/lib.rs")], &read).expect("the fixture reads");
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.first().expect("one subject").names.is_empty(), "{found:?}");

        let ok = [(
            "crates/thing/src/lib.rs",
            "fn refuse() -> String {\n    String::from(\"Build `thing` with `--features tls`, or declare a plain source\")\n}\n",
        )];
        let read = reading(&ok);
        let found = subjects(&[String::from("crates/thing/src/lib.rs")], &read).expect("the fixture reads");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found.first().expect("one subject").names, vec!["tls"], "{found:?}");
    }

    #[test]
    fn a_message_quoted_in_a_comment_or_asserted_in_a_test_is_not_a_subject() {
        // The two ways a gate of this shape over-claims. The comment case is `string_literals`'s
        // half - the same walk that makes a comment invisible to `code_lines` - and the test case
        // is `regions::scope`'s. Without either, this gate would redden the fixtures above.
        let files = [(
            "crates/thing/src/lib.rs",
            "// build the binary with the feature that provides it\n\
             fn fine() {}\n\
             #[cfg(test)]\n\
             mod tests {\n\
                 #[test]\n\
                 fn t() {\n\
                     assert!(msg().contains(\"build the binary with the feature that provides it\"));\n\
                 }\n\
             }\n",
        )];
        let read = reading(&files);
        let found = subjects(&[String::from("crates/thing/src/lib.rs")], &read).expect("the fixture reads");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn the_tree_this_gate_ships_on_has_subjects_and_they_all_resolve() {
        // Not a restatement of the gate: it is the FAIL-CLOSED half, and the count is the witness.
        // A scan that finds nothing is a reader that stopped reading, and it would pass everything.
        let Some(crate::repo::RepoFiles { root, files }) = crate::repo::all_files() else {
            return;
        };
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        let found = subjects(&files, &read).expect("every crate source reads");
        assert!(!found.is_empty(), "no rebuild instruction found - the literal scan is broken");
        assert_eq!(unresolved(&root, &found).expect("every manifest reads"), Vec::<String>::new());
    }
}
