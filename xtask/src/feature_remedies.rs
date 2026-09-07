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
//! A literal is a subject when it directs a reader at a cargo feature: a feature FLAG on its own,
//! or the word `feature` together with a verb about acquiring one, **in either order**. The order
//! used to be the trigger and a review broke that with five sentence shapes, the worst being this
//! tree's own house style - *"built without the `x` feature ... Rebuild it"* - where the noun
//! precedes the verb. What still earns no remedy is a message that never says `feature`: "no build
//! of this binary links the adapter" describes the binary and sends nobody anywhere.
//!
//! A subject must name a feature its own crate's `[features]` table declares, read out of
//! `crates/<name>/Cargo.toml`. **Every name it cites, not one of them** - `any` let a bogus feature
//! ride along beside a real one and then printed the bogus name as though it had resolved.
//!
//! # Fails closed
//!
//! A scan that finds NO subject has stopped reading rather than found a
//! clean tree - the failure mode `guidance/claims/remedies.rs` names for its own path citations - so
//! zero subjects is red, and so is a manifest that cannot be read. How many there are is PRINTED
//! with the files, not written down here: a number in a comment is a second thing to keep true, and
//! a count with no list behind it is not a witness.
//!
//! **Zero was not enough, and a review proved it.** Making the walk `break` after its first hit left
//! three of four messages unread, the verdict `ok`, and every test in this module green. So the
//! verdict carries DENOMINATORS - files read against in-scope files counted from the listing, and
//! per file, literals classified against the literals that file holds. Two levels, because fixing
//! only the first was measured insufficient: the same `break` moved one level down left the file
//! count at 204 of 204 and read `ok` again. Both are pairs of numbers taken from different places,
//! which is the only reason comparing them proves anything, and a shortfall in either is red.
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

/// Ways a sentence spells a feature SELECTION, which is an instruction on its own.
///
/// Either of these in a message is a remedy whatever the surrounding prose says, so they need no
/// verb beside them.
const FLAGS: &[&str] = &["--features", "--all-features", "-f "];

/// Verbs that turn a mention of a feature into a direction to go and do something.
///
/// `build` covers `Build` and `Rebuild`, because the comparison is over lowercased text. The set is
/// deliberately about ACQUIRING a feature - a review broke the single-verb, ordered version of this
/// rule with five sentence shapes it could not see, of which the most damning was this repository's
/// OWN house style: *"built without the `x` feature ... Rebuild it"*, where the word `feature`
/// precedes the only verb.
const VERBS: &[&str] = &["build", "enable", "install", "compile", "activate", "turn on"];

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

/// Does this text direct a reader at a cargo feature?
///
/// A feature FLAG is one on its own. Otherwise it takes the word `feature` and a verb about
/// acquiring one, in either order - **the order used to be the trigger and that was wrong**: it let
/// through every sentence naming the feature before the verb, which is how most of this tree writes
/// them. What still earns no remedy is a message that never says `feature` at all: "no build of
/// this binary links the adapter" describes the binary and sends nobody anywhere.
fn instructs_a_rebuild(body: &str) -> bool {
    let lowered = body.to_lowercase();
    FLAGS.iter().any(|flag| lowered.contains(flag))
        || (lowered.contains(FEATURE) && VERBS.iter().any(|verb| lowered.contains(verb)))
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
        // Split on commas: `--features tls,bigquery` is one legal argument naming two features, and
        // reading it as a single name reddened a correct message.
        if let Some(argument) = tail.split_whitespace().next() {
            for piece in argument.split(',') {
                let name = trim_name(piece);
                if !name.is_empty() {
                    names.push(String::from(name));
                }
            }
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
            // The header's own text, without a trailing comment. `[features] # what this links` is
            // legal TOML and an exact-line comparison read it as some other section, which made the
            // whole table invisible and produced "declares none" about a manifest declaring two.
            // A false RED with a false REASON, and this tree's manifests are heavily commented.
            inside = trimmed.split('#').next().unwrap_or(trimmed).trim() == "[features]";
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

/// What one walk of the tree saw.
///
/// `scanned` exists because a review broke this gate by making it `break` out of the file loop
/// after the first hit: three of four messages went unread, the verdict was `ok`, and every one of
/// this module's tests stayed green. **A count of what was FOUND cannot see that.** A count of what
/// was LOOKED AT, compared against a list built independently, can.
struct Scan {
    /// The instructions, in source order.
    found: Vec<Subject>,
    /// In-scope files actually read.
    scanned: usize,
    /// Literals examined - the other number that collapses when a reader stops early.
    literals: usize,
}

/// Every rebuild instruction in the tree, outside test code.
fn subjects(files: &[String], read: &PostImage<'_>) -> Result<Scan, String> {
    let mut scan = Scan {
        found: Vec::new(),
        scanned: 0,
        literals: 0,
    };
    for rel in files.iter().filter(|rel| in_scope(rel)) {
        let text = read(rel).ok_or_else(|| {
            format!("could not read {rel}, so the scan that decides which messages this gate reads is incomplete")
        })?;
        scan.scanned = scan.scanned.saturating_add(1);
        if !text.contains(FEATURE) && !FLAGS.iter().any(|flag| text.contains(flag)) {
            continue;
        }
        let tests = regions::scope(rel, read);
        // Two numbers about the SAME loop, one taken before it and one counted inside it. The file
        // denominator above does not reach here: a review's `break` after the first hit left files
        // at 204 of 204 and still read `ok`, because what collapsed was the literal walk. This is
        // the number that collapses with it.
        let literals = string_literals(&text);
        let total = literals.len();
        let mut classified = 0_usize;
        for literal in literals {
            classified = classified.saturating_add(1);
            if tests.covers(literal.line) || !instructs_a_rebuild(&literal.body) {
                continue;
            }
            scan.found.push(Subject {
                file: rel.clone(),
                line: literal.line,
                names: named_features(&literal.body),
            });
        }
        if classified != total {
            return Err(format!(
                "classified {classified} of {total} literal(s) in {rel} - the literal walk stopped \
                 early, so this verdict is about part of the file"
            ));
        }
        scan.literals = scan.literals.saturating_add(total);
    }
    Ok(scan)
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
        // EVERY name, not any of them. `any` passed a message citing a real feature and a bogus
        // one together - and then PRINTED the bogus name in the ok line as though it had resolved,
        // so the witness asserted the opposite of the truth.
        let missing: Vec<&str> = subject
            .names
            .iter()
            .map(String::as_str)
            .filter(|name| !declared.contains(*name))
            .collect();
        if !missing.is_empty() {
            problems.push(format!(
                "{at}: cites {} - {path} declares {}",
                missing.join(", "),
                if declared.is_empty() {
                    String::from("no features at all")
                } else {
                    declared.iter().cloned().collect::<Vec<_>>().join(", ")
                }
            ));
        }
    }
    Ok(problems)
}

fn check() -> Result<Vec<String>, String> {
    let (root, files) = repo::all_files()
        .and_then(|census| census.into_listing(repo::Unmigrated::FeatureRemedies))
        .map_err(|why| why.describe())?;
    let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
    // The denominator, built from the LISTING rather than from the walk - the two cannot agree by
    // construction, which is the only reason comparing them proves anything.
    let expected = files.iter().filter(|rel| in_scope(rel)).count();
    let scan = subjects(&files, &read)?;
    if scan.scanned != expected {
        return Err(format!(
            "read {} of {expected} in-scope file(s) - the walk stopped early, so this verdict is \
             about part of the tree",
            scan.scanned
        ));
    }
    let found = scan.found;
    if found.is_empty() {
        return Err(String::from(
            "read no rebuild instruction out of any crate source - the literal scan is broken, not \
             the tree. Several messages in this workspace instruct one, and a reader that finds \
             none of them passes everything",
        ));
    }
    let problems = unresolved(&root, &found)?;
    if problems.is_empty() {
        let mut held: Vec<String> = found
            .iter()
            .map(|subject| format!("{}:{} -> {}", subject.file, subject.line, subject.names.join(", ")))
            .collect();
        held.push(format!(
            "(read {} literal(s) in {} of {expected} in-scope file(s))",
            scan.literals, scan.scanned
        ));
        return Ok(held);
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
                held.len().saturating_sub(1)
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
    fn a_message_that_never_says_feature_sends_nobody_anywhere() {
        // THE line the gate rests on, and it moved. This test used to assert the ORDERING rule -
        // that a sentence naming a feature BEFORE its verb was a description and owed no name - and
        // a review disproved it with five shapes, the worst being this tree's own house style. So
        // the surviving distinction is narrower and honest: a message that never says `feature` and
        // carries no feature flag is not directing anyone at one.
        assert!(!instructs_a_rebuild(
            "names a metadata adapter no build of this binary links - write `markdown` instead"
        ));
        assert!(instructs_a_rebuild("build the binary with the feature that provides it"));
        assert!(instructs_a_rebuild("Build `sutura-serve` with `--features bigquery`"));

        // And the widening is DELIBERATE, so it is pinned rather than left to be rediscovered: a
        // sentence that merely describes which feature is off is now a subject too. Nothing a text
        // scan can do separates it from the house-style instruction, and the only thing being asked
        // of it - name a feature that exists - is a correct demand of a description as well.
        assert!(instructs_a_rebuild(
            "the `bigquery` feature is off, so this build links nothing"
        ));
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
        let found = subjects(&[String::from("crates/thing/src/lib.rs")], &read)
            .expect("the fixture reads")
            .found;
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found.first().expect("one subject").names.is_empty(), "{found:?}");

        let ok = [(
            "crates/thing/src/lib.rs",
            "fn refuse() -> String {\n    String::from(\"Build `thing` with `--features tls`, or declare a plain source\")\n}\n",
        )];
        let read = reading(&ok);
        let found = subjects(&[String::from("crates/thing/src/lib.rs")], &read)
            .expect("the fixture reads")
            .found;
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
        let found = subjects(&[String::from("crates/thing/src/lib.rs")], &read)
            .expect("the fixture reads")
            .found;
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_bogus_feature_beside_a_real_one_does_not_ride_along() {
        // Found by review. `any` passed this and then PRINTED `zzz_bogus` in the ok line as if it
        // had resolved - a witness asserting the opposite of the truth. Run against a real manifest
        // on disk, because `unresolved` reads one and that read is half of what is being fixed.
        let root = std::env::temp_dir().join(format!("sutura-feature-remedies-{}", std::process::id()));
        let src = root.join("crates/thing/src");
        std::fs::create_dir_all(&src).expect("a scratch crate");
        std::fs::write(
            root.join("crates/thing/Cargo.toml"),
            "[features] # commented, as this tree's are\ntls = []\n",
        )
        .expect("a manifest");
        let files = [(
            "crates/thing/src/lib.rs",
            "fn refuse() -> String {\n    String::from(\"Rebuild with `--features tls`, or for metadata the `zzz_bogus` feature\")\n}\n",
        )];
        let read = reading(&files);
        let scan = subjects(&[String::from("crates/thing/src/lib.rs")], &read).expect("the fixture reads");
        let names = &scan.found.first().expect("one subject").names;
        assert!(
            names.contains(&String::from("tls")) && names.contains(&String::from("zzz_bogus")),
            "{names:?}"
        );

        let problems = unresolved(&root, &scan.found).expect("the manifest reads");
        let _swept = std::fs::remove_dir_all(&root);
        // One problem, naming ONLY the name that does not resolve - not the real one beside it.
        assert_eq!(problems.len(), 1, "{problems:?}");
        let problem = problems.first().expect("one problem");
        assert!(problem.contains("zzz_bogus"), "{problem}");
        assert!(
            !problem.contains("cites tls"),
            "the resolving name is not the complaint: {problem}"
        );
    }

    #[test]
    fn the_five_shapes_a_review_slipped_past_the_ordered_rule_are_all_subjects() {
        // Each of these left the single-verb, ordered trigger at `ok`, exit 0. The first is this
        // repository's own house style, which is what made it the worst of them.
        for sentence in [
            "this binary was built without the `zzz` feature, so it links no adapter. Rebuild it and try again",
            "`catalog.kind: datahub` needs the `zzz` feature - build the binary again to link it",
            "start it with `cargo run -p sutura-serve --features zzz` to link the adapter",
            "enable the `zzz` feature and try again",
            "install the binary with `--features zzz`",
        ] {
            assert!(instructs_a_rebuild(sentence), "not seen as an instruction: {sentence}");
        }
        // And the message #366 replaces the broken one with is still NOT a subject.
        assert!(!instructs_a_rebuild(
            "names a metadata adapter no build of this binary links - write `markdown`, the one catalog kind it can open"
        ));
    }

    #[test]
    fn the_walk_reports_how_many_files_it_read_not_just_what_it_found() {
        // Found by review: a `break` after the first hit left three of four messages unread and the
        // verdict `ok`, with every test here green. The denominator is what sees that.
        let files = [
            (
                "crates/a/src/lib.rs",
                "fn f() { let _ = \"Rebuild with `--features tls`\"; }\n",
            ),
            ("crates/b/src/lib.rs", "fn g() { let _ = \"nothing to say here\"; }\n"),
        ];
        let read = reading(&files);
        let listing = vec![String::from("crates/a/src/lib.rs"), String::from("crates/b/src/lib.rs")];
        let scan = subjects(&listing, &read).expect("the fixture reads");
        assert_eq!(scan.scanned, 2, "both in-scope files are read, not only the one that hit");
        assert_eq!(scan.found.len(), 1);
    }

    #[test]
    fn a_commented_table_header_and_a_comma_list_are_both_legal_and_neither_is_a_red() {
        // Both were FALSE REDS found by review, and a false red is the shape that gets a gate
        // disabled. The manifests in this tree are heavily commented.
        let declared = declared_features("[features] # what this binary can link\ntls = []\nbigquery = []\n");
        assert!(declared.contains("tls") && declared.contains("bigquery"), "{declared:?}");
        // One legal argument naming two features.
        assert_eq!(
            named_features("Rebuild with `--features tls,bigquery`"),
            vec!["bigquery", "tls"]
        );
    }

    #[test]
    fn the_tree_this_gate_ships_on_has_subjects_and_they_all_resolve() {
        // Not a restatement of the gate: it is the FAIL-CLOSED half, and the count is the witness.
        // A scan that finds nothing is a reader that stopped reading, and it would pass everything.
        let Ok((root, files)) =
            crate::repo::all_files().and_then(|census| census.into_listing(crate::repo::Unmigrated::FeatureRemedies))
        else {
            return;
        };
        let read = |path: &str| std::fs::read_to_string(root.join(path)).ok();
        let scan = subjects(&files, &read).expect("every crate source reads");
        let found = scan.found;
        assert!(!found.is_empty(), "no rebuild instruction found - the literal scan is broken");
        assert_eq!(unresolved(&root, &found).expect("every manifest reads"), Vec::<String>::new());
    }
}
