//! What ships is declared in `nix/shipped.nix`, and every workflow that builds it spells the same
//! set again as a literal. This is the gate that makes the two agree.
//!
//! # Why the arrival check in `release.yml` cannot do this
//!
//! `Check that every build arrived` counts one `facts/` file per binary per target and fails on a
//! shortfall, which reads like the mechanism and is not: both `want` and the files it counts are
//! driven by the SAME `BINARIES` string, and `build-artefacts` loops over that same input. Add a
//! third record to `nix/shipped.nix` and touch nothing else, and that check still sees
//! `want == have`, every job is green, and the new binary is in no release. The cross link
//! matrix's own `BINARIES` literal has the same shape: it proves that the names IT holds link,
//! and says nothing about the names nix ships. Which workflow that is, is
//! [`refusal::hosting`]'s answer and not a sentence here - see it for what naming the file cost.
//!
//! That is the omission class `github.com/telekom/sutura#111` was - the release derivations named
//! the binary that existed before `sutura-serve` did, and nothing compared that name to anything -
//! so a fix for #111 whose own consistency rests on a comment would be the same defect one level
//! up. Found in review of the change that closed it.
//!
//! # Why a gate rather than deriving the names
//!
//! Deriving them is the obvious alternative and it does not work where it is needed. A
//! `strategy.matrix` takes literals, and a job cannot evaluate a flake before it has installed
//! nix - so the earliest a workflow could learn the set is after a step that is itself part of
//! what the set decides. `check-pins`, `check-scope` and `check-hook-tiers` are the same shape for
//! the same reason: two files that cannot be derived from each other, reconciled by something that
//! reads both.
//!
//! # What it does NOT check
//!
//! * **The `justfile`.** `just build`, `just image` and `just build-all` write the nix attribute
//!   names out, and a subset there costs a developer a surprise rather than a release a binary.
//!   Parsing recipe bodies for attribute prefixes is brittle in the direction that matters - a
//!   gate that fails on a correct tree gets disabled - so the release path is the scope and this
//!   sentence is the limit.
//! * **A file that declares no set at all.** A workflow with no `BINARIES` does not loop over
//!   binaries, so there is nothing to disagree with. What this catches is a literal that exists
//!   and is wrong, in either direction. **That sentence was true of a workflow with no loop and
//!   false of one that keeps the loop and empties the value** - a zero-iteration loop is a green
//!   job that built nothing. [`loops`] is the rule that closes it, and the `checked == 0` refusal
//!   below is not it: `checked` counts every SET, and an empty one is a set, so it reaches 0 only
//!   where nothing spells the literal at all. **A declaration that EXISTS is no longer in this
//!   bullet** - see the section below.
//! * **An input declaring `binaries:` with a body and no `default:` in it.** Nothing is asserted
//!   about what a caller must then pass, so it is skipped rather than guessed at. A `with:` block
//!   passing `binaries:` and nothing beneath it is NOT that shape and is the empty set.
//! * **The FEATURES each binary ships with.** That is `checks.shipped-features`, which reads them
//!   out of the built artifact rather than out of any text.
//! * **A `probeFeatures` entry no page documents.** A probe nobody asked for costs a job and
//!   breaks no claim, so it is not a failure. The direction that matters is the other one.
//! * **A documented `cargo run --features ...`.** Deliberately out of scope: it builds for the
//!   host and executes, and the risk a probe exists for is the CROSS link. `docs/serving.md`
//!   documents one, and demanding a probe for it would fail a correct tree.
//!
//! # A VALUE IS NOT AN ABSENCE, and the count was the only thing that said otherwise
//!
//! Everything the *is this a literal* predicate said no to was SKIPPED - not compared, not
//! reported - so the most a reader ever got was a count that moved. Three silent-green routes
//! measured on this tree, with the outputs and the argument in [`declaration`]: two printed
//! `ok - 3 literal(s)` where 4 is right, and the third printed `ok - 4`, never counted at all -
//! which is why the count could not have been the control. Every in-scope value is now a set
//! (compared, and ZERO NAMES IS A SET), a reference naming this same set (named in the verdict),
//! or a refusal, and the verdict prints the ROWS behind the count. A fourth class - an input
//! declaring the key with a body and no `default:` - is a REFUSAL rather than a `None` since
//! `#414`, because the predicate's `false` branch is where #329's symptom survived the first fix:
//! deleting a live `default:` moved the count with nothing saying which comparison had stopped. **What it
//! still does not reach:** whether a reference resolves to the literal it names, and a set spelled
//! under some third key. A set spelled where neither key reaches at the head of its line IS
//! counted now - a sequence item, a quoted key, a space before the colon and an unexpected case
//! are all refusals rather than silences, held by a substring floor the parse cannot narrow.
//!
//! # The second rule: a documented feature build is a probe, or it is unproven
//!
//! `nix/shipped.nix`'s `probeFeatures` decides which feature-on builds the four cross link legs
//! LINK, and `docs/adr/0017` claims on that basis that *the documented feature-on source build is
//! linked on every pull request*. The coupling that claim rests on was prose: a page tells a reader to run
//! `cargo build --release -p sutura-cli --features bigquery`, and nothing said that `bigquery` had
//! to appear in `probeFeatures`.
//!
//! **The step's own refusal cannot cover it, and that is why this rule is here.** The
//! `feature-probes-` step refuses an EMPTY probe manifest, which is equivalent to *the probe is
//! gone* only because one binary declares no probe features. Declare one for `sutura-serve` and
//! delete `"bigquery"` from `sutura-cli`'s, and the manifest is still non-empty: the leg goes
//! green, and the claim reverts to *assumed* with no signal at all. Found in review of this
//! rule's own absence.

use std::collections::BTreeMap;

// All split off under the unexemptable 1000-line cap, and each takes its own NEW tests with it -
// `.agents/skills/sutura/gates/SKILL.md`'s orphan rule is about moving tests OUT of a file, and
// every assertion that was here is still here. `documented` reads pages, `refusal` locates a
// workflow, `declaration` reads and classifies the YAML; none touches the nix parse below.
mod declaration;
mod documented;
mod finder;
mod loops;
mod refusal;
mod rules;

use crate::Verdict;
use crate::repo;

/// The declaration every literal is compared against.
const SOURCE: &str = "nix/shipped.nix";

/// The key a workflow spells the set under, and the key an action's input carries it as.
const KEYS: [&str; 2] = ["BINARIES", "binaries"];

/// Every `bin` inside `nix/shipped.nix`'s `binaries = [ ... ]`, in declaration order.
///
/// **A VIEW OF [`records`] rather than a second scan of the same list**, for the reason
/// `nix/shipped.nix` itself derives `featurePackages` and `probeManifests` from one `probes` list:
/// two parsers of one declaration are two answers nobody can reconcile. The reading rules and
/// what forced them are on `records`; these two are worth keeping beside the name.
///
/// SCOPED TO THAT LIST rather than grepping the file: `bin` is also a parameter name in `ociFor`'s
/// signature and a field read as `b.bin` in four places, and a whole-file scan would answer for
/// lines that declare nothing.
///
/// FOUND ANYWHERE ON THE LINE, not only at its start, and that is a correction its own tests
/// forced. The first version matched a trimmed line beginning `bin = `, which is the shape
/// `nixpkgs-fmt` produces and not the only legal one: `{ bin = "sutura"; package = "sutura-cli"; }`
/// is one record on one line, and it was read as no record at all - so a gate whose whole job is
/// to notice a missing binary would have silently missed one. That is the failure mode
/// `workflows::declared_block` records twice, in the same words: a parser that silently sees half
/// a file is worse than no parser.
fn declared(text: &str) -> Vec<String> {
    records(text)
        .into_iter()
        .map(|r| r.bin)
        .filter(|bin| !bin.is_empty())
        .collect()
}

/// Every `bin = "..."` value on one line, left to right.
///
/// The preceding character must not be part of a name, so a hypothetical `mainBin = "x"` is not
/// read as a `bin`. An unterminated quote yields nothing rather than the rest of the file.
fn bins_in(line: &str) -> impl Iterator<Item = &str> {
    const KEY: &str = "bin = \"";
    let mut rest = line;
    core::iter::from_fn(move || {
        loop {
            let at = rest.find(KEY)?;
            // The character before the key, so a longer name ending in `bin` is not one.
            let is_key = rest
                .get(..at)
                .and_then(|s| s.chars().next_back())
                .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-');
            let tail = rest.get(at.saturating_add(KEY.len())..)?;
            // An unterminated quote ends the scan rather than swallowing the rest of the line.
            let end = tail.find('"')?;
            rest = tail.get(end.saturating_add(1)..).unwrap_or_default();
            if is_key {
                return tail.get(..end);
            }
        }
    })
}

/// One record of `nix/shipped.nix`'s `binaries` list: the executable, its cargo package, and the
/// features a SOURCE build of it is probed with.
///
/// `bin` alone is what the literal rule needs. `package` is what a documented `cargo build -p ...`
/// names, so the second rule cannot be written without it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Record {
    /// The installed executable's name.
    bin: String,
    /// The cargo package it is built from.
    package: String,
    /// `probeFeatures`: which feature-on source builds the cross link legs link.
    probe_features: Vec<String>,
}

/// Every record in `binaries = [ ... ]`, in declaration order.
///
/// Records are split at `{`, which is what makes the one-line shape
/// `{ bin = "sutura"; package = "sutura-cli"; }` one record rather than none - the same shape that
/// forced [`bins_in`] to exist. A field absent from a record yields an empty value and the caller
/// decides whether that is a failure: `probeFeatures` missing is an EVALUATION error in nix, since
/// there is no `or [ ]` default, so a binary cannot silently opt out of being probed and this gate
/// does not duplicate that.
///
/// A `probeFeatures` list is read across lines as well as on one, because `nixpkgs-fmt` breaks a
/// long list and a parser that saw only the one-line shape would report a correct tree as missing
/// a probe. That is the direction this whole gate exists to prevent, one level down.
fn records(text: &str) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    let mut indent: Option<usize> = None;
    let mut collecting = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let depth = line.len().saturating_sub(line.trim_start().len());
        let Some(open) = indent else {
            if trimmed.starts_with("binaries = [") {
                indent = Some(depth);
            }
            continue;
        };
        if trimmed == "];" && depth == open {
            break;
        }
        if trimmed.starts_with('{') {
            out.push(Record {
                bin: String::new(),
                package: String::new(),
                probe_features: Vec::new(),
            });
            collecting = false;
        }
        let Some(record) = out.last_mut() else {
            continue;
        };
        if let Some(bin) = bins_in(line).next() {
            record.bin = String::from(bin);
        }
        if let Some(package) = quoted_after(line, "package = \"") {
            record.package = String::from(package);
        }
        if let Some((_, rest)) = line.split_once("probeFeatures = [") {
            record.probe_features = quoted_items(rest);
            collecting = !rest.contains(']');
        } else if collecting {
            record.probe_features.extend(quoted_items(line));
            collecting = !line.contains(']');
        }
    }
    out
}

/// The quoted value following `key` on this line, or nothing.
///
/// The character before the key must not be part of a name, for [`bins_in`]'s reason: a field
/// called `hostPackage` is not `package`.
fn quoted_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(key)?;
    let boundary = line
        .get(..at)
        .and_then(|s| s.chars().next_back())
        .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-');
    if !boundary {
        return None;
    }
    let tail = line.get(at.saturating_add(key.len())..)?;
    let end = tail.find('"')?;
    tail.get(..end)
}

/// Every `"..."` in a fragment, left to right. An unterminated quote ends the scan rather than
/// swallowing the rest of the line.
fn quoted_items(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = fragment;
    while let Some(at) = rest.find('"') {
        let Some(tail) = rest.get(at.saturating_add(1)..) else {
            break;
        };
        let Some(end) = tail.find('"') else {
            break;
        };
        if let Some(item) = tail.get(..end) {
            out.push(String::from(item));
        }
        rest = tail.get(end.saturating_add(1)..).unwrap_or_default();
    }
    out
}

/// Every workflow and every local composite action, keyed by repo-relative path.
///
/// A named alias rather than the bare map, because `clippy::type_complexity` refuses the
/// `Result<BTreeMap<..>, ..>` the fail-closed read below returns.
type Yaml = BTreeMap<String, String>;

/// What reconciling `probeFeatures` against the documented builds found.
///
/// A named struct rather than a tuple, because `clippy::type_complexity` refuses the tuple - and it
/// is right to, for `workflows::Scan`'s reason one file over: two `Vec<String>` side by side say
/// nothing about which one is the failure and which one is the evidence that anything was checked.
struct Reconciliation {
    /// Documented feature builds no probe covers. Each row names a page, a line and a feature.
    problems: Vec<String>,
    /// One row per documented build that named a shipped package, so the report can SAY what it
    /// reconciled. Empty is a failure and not a pass - see [`run`].
    probed: Vec<String>,
}

/// The second rule: a documented `cargo build --features` of a SHIPPED package is probed.
///
/// Returns the failures, and one row per documented build that named a shipped package - the
/// second is what keeps the first from running over an empty set, the failure mode a
/// text-scanning check is most prone to, and it is rows rather than a count so a reader can see
/// WHICH builds were reconciled instead of trusting a number.
fn unprobed(records: &[Record], documented: &[documented::DocumentedBuild]) -> Reconciliation {
    let mut problems = Vec::new();
    let mut probed = Vec::new();
    for build in documented {
        let Some(record) = records.iter().find(|r| r.package == build.package) else {
            // A page documenting a feature build of something this repository does not ship has
            // no `probeFeatures` field to disagree with. Not this gate's business.
            continue;
        };
        // NAMED rather than counted. A number nobody can attribute is the shape `sutura/gates`
        // warns about: this one read 2 when one page documents one build, and only printing the
        // rows said where the second came from.
        probed.push(format!(
            "{}:{} {} [{}]",
            build.page,
            build.line,
            build.package,
            build.features.join(",")
        ));
        for feature in &build.features {
            if !record.probe_features.iter().any(|p| p == feature) {
                problems.push(format!(
                    "{}:{} documents `cargo build -p {} --features {feature}`, and {SOURCE} does \
                     not list `{feature}` in {}'s probeFeatures - so no link leg builds it",
                    build.page, build.line, build.package, record.bin
                ));
            }
        }
    }
    Reconciliation { problems, probed }
}

/// `cargo xtask check-shipped-binaries` - every release-path literal equals `nix/shipped.nix`.
pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-shipped-binaries: could not determine the repo root");
        return Verdict::Fail;
    };
    let path = root.join(SOURCE);
    let source = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("xtask check-shipped-binaries: could not read {}: {error}", path.display());
            return Verdict::Fail;
        }
    };

    let expected = declared(&source);
    if expected.is_empty() {
        eprintln!("xtask check-shipped-binaries: FAILED - parsed no binaries out of {SOURCE}");
        eprintln!("  The scan is broken, not the workflows: a gate that compares against an empty");
        eprintln!("  list would pass every literal. `binaries = [` and `bin = \"...\";` are the two");
        eprintln!("  shapes it reads.");
        return Verdict::Fail;
    }

    let files = match finder::yaml_files(&root) {
        Ok(found) => found,
        Err(why) => {
            eprintln!("xtask check-shipped-binaries: FAILED - a file or directory under `.github` could not be read");
            eprintln!("  {why}");
            eprintln!("  Dropping it would take it out of the denominator as well as out of the scan, so");
            eprintln!("  every count below would agree with itself over a tree this never looked at.");
            return Verdict::Fail;
        }
    };
    let missing = finder::unanchored(&files);
    if !missing.is_empty() {
        eprintln!(
            "xtask check-shipped-binaries: FAILED - {} file(s) this verdict is about were not read",
            missing.len()
        );
        for name in &missing {
            eprintln!("  {name} is not among the {} file(s) handed over", files.len());
        }
        eprintln!();
        eprintln!("  These are the release path and the composite action #111 was about. A count of");
        eprintln!("  what was judged cannot say THEY were judged - a walk that lost them satisfies it");
        eprintln!("  by reading the workflows - so this asks for them by name. If one was renamed,");
        eprintln!("  rename it here in the same commit.");
        return Verdict::Fail;
    }
    let Some(read) = declaration::read(&files, &expected) else {
        return Verdict::Fail;
    };
    let declaration::Read {
        literals,
        references,
        mismatches,
        offered,
    } = read;
    let checked = literals.len();

    // EVERY RULE REPORTS BEFORE ANY OF THEM RETURNS, which is `rules`' whole reason: the loop rule
    // used to return here, so a tree with an empty shipped set in one file and a drifted literal
    // in another printed the first and hid the second. Each of the four is still fail-closed on
    // its own subject - a zero-iteration loop is a green job that linked, audited and inventoried
    // nothing; a `docs/adr/0017` claim resting on a refusal no workflow holds has no referent; a
    // page this cannot read or lex goes unreconciled while the others keep the count non-empty -
    // and now a reader learns about all of them in one run.
    let reconciled = match documented::pages(&root) {
        Ok(documented) => Ok(unprobed(&records(&source), &documented)),
        Err(why) => Err(why),
    };
    let host = refusal::hosting(&files, &refusal::PROBE_REFUSAL);
    let refused = rules::refusals(
        loops::verdict(&files),
        &expected,
        &mismatches,
        host.as_deref(),
        reconciled.as_ref().map_err(String::as_str),
    );
    if rules::report(&refused) == Verdict::Fail {
        return Verdict::Fail;
    }
    drop(refused);
    let probed = reconciled.map_or_else(|_| Vec::new(), |reconciliation| reconciliation.probed);

    println!(
        "xtask check-shipped-binaries: ok - {} literal(s) across {} file(s) agree with {SOURCE} ({})",
        checked,
        offered,
        expected.join(" ")
    );
    // NAMED rather than counted, for `unprobed`'s reason one function up: a count is not a witness
    // that the thing counted was judged correctly, and a declaration that stops being compared has
    // to lose a ROW rather than move a number.
    for row in &literals {
        println!("  literal: {row}");
    }
    for row in &references {
        println!("  reference: {row}");
    }
    for row in &probed {
        println!("  probed: {row}");
    }
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::declaration::{Carried, spelled};
    use super::declared;

    #[test]
    fn a_record_carries_its_package_and_its_probe_features() {
        // `bin` alone cannot answer the second rule: a page documents `cargo build -p <package>`.
        let nix = concat!(
            "  binaries = [\n",
            "    {\n",
            "      bin = \"sutura\";\n",
            "      package = \"sutura-cli\";\n",
            "      probeFeatures = [ \"bigquery\" ];\n",
            "    }\n",
            "    { bin = \"sutura-serve\"; package = \"sutura-serve\"; probeFeatures = [ ]; }\n",
            "  ];\n",
        );
        let records = super::records(nix);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].package, "sutura-cli");
        assert_eq!(records[0].probe_features, vec![String::from("bigquery")]);
        assert_eq!(records[1].package, "sutura-serve");
        assert!(records[1].probe_features.is_empty(), "the serve record probes no feature set");
    }

    #[test]
    fn a_probe_feature_list_broken_across_lines_is_still_the_list() {
        // `nixpkgs-fmt` breaks a long list, and a parser that read only the one-line shape would
        // report a correct tree as having no probe - the exact direction this gate exists to stop,
        // one level down from where it stops it.
        let nix = concat!(
            "  binaries = [\n",
            "    {\n",
            "      bin = \"sutura\";\n",
            "      package = \"sutura-cli\";\n",
            "      probeFeatures = [\n",
            "        \"bigquery\"\n",
            "        \"postgres\"\n",
            "      ];\n",
            "    }\n",
            "  ];\n",
        );
        assert_eq!(
            super::records(nix)[0].probe_features,
            vec![String::from("bigquery"), String::from("postgres")]
        );
    }

    #[test]
    fn a_documented_feature_build_is_read_in_every_shape_a_page_writes_it() {
        let page = concat!(
            // A MENTION, outside any block, and it must NOT be read - a record quoting a command
            // is not a page instructing a reader to run it. This line is why the fence boundary
            // exists: `docs/adr/0017` gained exactly this sentence, and the gate's report went
            // from one reconciled build to two.
            "the page says `cargo build -p sutura-cli --features bigquery`, so it must be probed\n",
            "```bash\n",
            "cargo build --release -p sutura-cli --features bigquery\n",
            "cargo build --package sutura-cli --features=bigquery,postgres\n",
            // Neither of these is a documented feature BUILD, even inside the block: the first has
            // no feature list, the second is a `cargo run`, which builds for the host and is not
            // what a cross probe answers for.
            "cargo build --release -p sutura-cli\n",
            "cargo run -p sutura-serve --features tls\n",
            "```\n",
            "and outside the block again: cargo build -p sutura-cli --features nonsense\n",
        );
        let found = super::documented::builds("docs/p.md", page).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].package, "sutura-cli");
        assert_eq!(found[0].line, 3);
        assert_eq!(found[0].features, vec![String::from("bigquery")]);
        assert_eq!(found[1].features, vec![String::from("bigquery"), String::from("postgres")]);
    }

    #[test]
    fn a_documented_feature_the_probe_set_omits_is_linked_by_nothing() {
        // The scenario the `feature-probes-` step's own refusal CANNOT catch: a second binary keeps the
        // manifest non-empty, so the job stays green while the documented build is probed by
        // nothing. RED before this rule existed, because nothing compared the two.
        let nix = concat!(
            "  binaries = [\n",
            "    { bin = \"sutura\"; package = \"sutura-cli\"; probeFeatures = [ ]; }\n",
            "    { bin = \"sutura-serve\"; package = \"sutura-serve\"; probeFeatures = [ \"tls\" ]; }\n",
            "  ];\n",
        );
        let documented = super::documented::builds(
            "docs/getting-started.md",
            "```bash\ncargo build --release -p sutura-cli --features bigquery\n```\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let found = super::unprobed(&super::records(nix), &documented);
        assert_eq!(found.probed.len(), 1, "{:?}", found.probed);
        assert_eq!(found.problems.len(), 1);
        // Line 2, not 1: the fence opener is line 1. The number is asserted because a message
        // naming a page and not a line is a message nobody can act on.
        assert!(
            found.problems[0].contains("docs/getting-started.md:2"),
            "{:?}",
            found.problems
        );
        assert!(found.problems[0].contains("bigquery"));
        assert!(found.problems[0].contains("sutura"));
    }

    #[test]
    fn a_documented_build_of_something_we_do_not_ship_is_not_this_gates_business() {
        // No `probeFeatures` field exists to disagree with, and `reconciled` must not count it -
        // or the fail-closed test below would pass on a tree where nothing was reconciled.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; probeFeatures = [ ]; }\n  ];\n";
        let documented = super::documented::builds(
            "docs/p.md",
            "```bash\ncargo build -p some-other-crate --features whatever\n```\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let found = super::unprobed(&super::records(nix), &documented);
        assert!(
            found.problems.is_empty(),
            "no probe problems when every recorded crate is documented"
        );
        assert!(found.probed.is_empty(), "{:?}", found.probed);
    }

    #[test]
    fn a_probe_no_page_documents_is_not_a_failure() {
        // Deliberate: a probe nobody asked for costs a job and breaks no claim. The direction
        // this gate holds is the other one.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; probeFeatures = [ \"bigquery\" \"postgres\" ]; }\n  ];\n";
        let documented = super::documented::builds("docs/p.md", "```bash\ncargo build -p sutura-cli --features bigquery\n```\n")
            .unwrap_or_else(|e| panic!("{e}"));
        let found = super::unprobed(&super::records(nix), &documented);
        assert!(found.problems.is_empty(), "a single real gap raises no other problems");
        assert_eq!(found.probed.len(), 1, "{:?}", found.probed);
    }

    #[test]
    fn the_repositorys_own_documented_feature_build_is_probed() {
        // The rule over the REAL tree, so the reconciliation is not only exercised on fixtures.
        //
        // **POSITIVE assertions and not `problems.is_empty()`, because that shape is green for the
        // wrong reason and it was measured being so.** With the feature comparison disabled -
        // `if false && !record.probe_features…` - `just test` reported this test PASS and only
        // `a_documented_feature_the_probe_set_omits_is_linked_by_nothing` went red. An assertion
        // that nothing is wrong cannot distinguish a clean tree from a check that does nothing, so
        // what is asserted here is which pair was reconciled, by name.
        let root = crate::repo::root().expect("could not locate the repo");
        let source = std::fs::read_to_string(root.join(super::SOURCE)).expect("could not read nix/shipped.nix");
        let documented = super::documented::pages(&root).expect("a page under docs/ could not be read or lexed");
        let records = super::records(&source);

        let build = documented
            .iter()
            .find(|b| b.package == "sutura-cli")
            .expect("no page under docs/ documents a `cargo build -p sutura-cli --features ...`");
        assert!(build.features.contains(&String::from("bigquery")), "{build:?}");
        let record = records
            .iter()
            .find(|r| r.package == "sutura-cli")
            .expect("nix/shipped.nix declares no sutura-cli record");
        assert!(
            record.probe_features.contains(&String::from("bigquery")),
            "{:?}",
            record.probe_features
        );

        // And the gate's own verdict on the tree, which is what a reader of a green run assumes.
        let found = super::unprobed(&records, &documented);
        assert!(
            !found.probed.is_empty(),
            "nothing was reconciled, so the rule ran over an empty set"
        );
        assert!(found.problems.is_empty(), "{:?}", found.problems);
    }

    #[test]
    fn the_binaries_list_is_read_in_declaration_order() {
        let nix = concat!(
            "  binaries = [\n",
            "    {\n",
            "      bin = \"sutura\";\n",
            "      package = \"sutura-cli\";\n",
            "    }\n",
            "    {\n",
            "      bin = \"sutura-serve\";\n",
            "      package = \"sutura-serve\";\n",
            "    }\n",
            "  ];\n",
        );
        assert_eq!(declared(nix), vec![String::from("sutura"), String::from("sutura-serve")]);
    }

    #[test]
    fn a_record_written_on_one_line_is_still_a_record() {
        // The shape that forced `bins_in` to exist. `nixpkgs-fmt` puts every field on its own
        // line, so the committed file never looks like this - and a gate that silently reads
        // fewer binaries than are declared is the one failure this gate must not have.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; }\n    { bin = \"sutura-serve\"; }\n  ];\n";
        assert_eq!(declared(nix), vec![String::from("sutura"), String::from("sutura-serve")]);
    }

    #[test]
    fn a_longer_key_ending_in_bin_is_not_a_bin() {
        let nix = "  binaries = [\n    { mainBin = \"decoy\"; bin = \"sutura\"; }\n  ];\n";
        assert_eq!(declared(nix), vec![String::from("sutura")]);
    }

    #[test]
    fn a_bin_outside_the_list_is_not_a_declaration() {
        // `bin` is a parameter name in `ociFor`'s signature and a field read as `b.bin`, so a
        // whole-file grep would answer for lines that declare nothing. The list ends at the `];`
        // at its own indent.
        let nix = concat!(
            "  binaries = [\n",
            "    { bin = \"sutura\"; }\n",
            "  ];\n",
            "  ociFor = { package, architecture, bin, entrypoint }: {\n",
            "    bin = \"not-a-shipped-binary\";\n",
            "  };\n",
        );
        assert_eq!(declared(nix), vec![String::from("sutura")]);
    }

    /// The names a declaration carries, or a panic naming what it carried instead.
    fn set(found: &super::declaration::Spelled) -> &[String] {
        match &found.carries {
            Carried::Set(names) => names,
            other => panic!("expected a literal set, got {other:?}"),
        }
    }

    #[test]
    fn a_workflow_env_literal_is_a_spelled_set() {
        let yaml = "env:\n  IMAGE: ghcr.io/x\n  BINARIES: sutura sutura-serve\n";
        let found = spelled(yaml).found;
        assert_eq!(found.len(), 1, "got {found:?}");
        assert_eq!(set(&found[0]), vec![String::from("sutura"), String::from("sutura-serve")]);
        assert_eq!(found[0].line, 3);
    }

    #[test]
    fn a_declaration_carrying_nothing_is_a_set_of_zero_names_at_its_own_line() {
        // #329, three routes, and the count is what said none of them was happening: each was
        // SKIPPED, so `spelled` returned one fewer row and the verdict printed one fewer literal
        // with no line naming the comparison that had stopped. Measured on this tree,
        // `ok - 3 literal(s)` at exit 0 where a clean tree prints 4.
        //
        // The third is a `with:` passing the key with nothing under it - a null ARGUMENT, which is
        // not the input block's opening shape however identically it is spelled.
        for (yaml, at) in [
            ("env:\n  BINARIES:\n", 2),
            ("inputs:\n  binaries:\n    required: false\n    default:\n", 4),
            ("        with:\n          binaries:\n          target: x\n", 2),
        ] {
            let found = spelled(yaml).found;
            assert_eq!(found.len(), 1, "{yaml:?} declared the empty set: {found:?}");
            assert_eq!(found[0].line, at, "{found:?}");
            assert!(set(&found[0]).is_empty(), "{found:?}");
        }
    }

    #[test]
    fn a_declaration_this_gate_cannot_compare_is_reported_rather_than_dropped() {
        // The fourth route: an expression naming a DIFFERENT set. Empty nowhere, so no empty-set
        // rule could have caught it, and the literal count moved from 4 to 3 at exit 0.
        let found = spelled("env:\n  BINARIES: ${{ env.SHIPPED }}\n").found;
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(matches!(found[0].carries, Carried::Opaque(_)), "{found:?}");
    }

    #[test]
    fn an_action_input_default_is_a_spelled_set() {
        let yaml = concat!(
            "inputs:\n",
            "  target:\n",
            "    description: the triple\n",
            "    default: nothing-to-do-with-binaries\n",
            "  binaries:\n",
            "    description: >\n",
            "      the shipped set\n",
            "    required: false\n",
            "    default: sutura sutura-serve\n",
        );
        let found = spelled(yaml).found;
        assert_eq!(found.len(), 1, "only the binaries input's default counts: {found:?}");
        assert_eq!(set(&found[0]), vec![String::from("sutura"), String::from("sutura-serve")]);
    }

    #[test]
    fn a_reference_to_the_literal_is_not_itself_a_literal() {
        // A call site passes `binaries: ${{ env.BINARIES }}`, which is a reference to the set
        // declared elsewhere. Comparing an expression to a list would fail on a correct tree,
        // which is how a gate gets disabled. NAMED rather than dropped since #329: a declaration
        // this gate decides not to compare has to survive into the verdict, or the count is the
        // only thing that moves when one stops being compared.
        let yaml = "      - uses: ./.github/actions/build-artefacts\n        with:\n          binaries: ${{ env.BINARIES }}\n";
        let found = spelled(yaml).found;
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].carries,
            Carried::Reference(String::from("env.BINARIES")),
            "{found:?}"
        );
    }

    #[test]
    fn a_drifted_literal_is_what_this_catches() {
        // THE failure the gate exists for: a third binary added to `nix/shipped.nix` while a
        // workflow still spells two. Nothing else in the release path notices - the arrival check
        // counts what its own literal produced.
        let nix =
            "  binaries = [\n    { bin = \"sutura\"; }\n    { bin = \"sutura-serve\"; }\n    { bin = \"sutura-mcp\"; }\n  ];\n";
        let yaml = "env:\n  BINARIES: sutura sutura-serve\n";
        let expected = declared(nix);
        let found = spelled(yaml).found;
        assert_eq!(expected.len(), 3);
        assert_eq!(found.len(), 1);
        assert_ne!(set(&found[0]), expected, "the drift must be visible");
    }

    #[test]
    fn order_is_part_of_the_comparison() {
        let nix = "  binaries = [\n    { bin = \"sutura\"; }\n    { bin = \"sutura-serve\"; }\n  ];\n";
        let yaml = "env:\n  BINARIES: sutura-serve sutura\n";
        assert_ne!(set(&spelled(yaml).found[0]), declared(nix));
    }

    #[test]
    fn the_real_tree_agrees_with_itself() {
        // The gate against the tree it guards, so a refactor of either parse cannot pass its own
        // fixtures and fail the repo. `shared_client`'s suite does the same.
        let Some(root) = crate::repo::root() else {
            return;
        };
        let Ok(nix) = std::fs::read_to_string(root.join(super::SOURCE)) else {
            return;
        };
        let expected = declared(&nix);
        assert!(!expected.is_empty(), "nix/shipped.nix declares no binaries");
        let files = super::finder::yaml_files(&root).expect("a file under `.github` could not be read");
        let walk = super::declaration::declarations(&files);
        // The production pair, over the real tree: the walk's own list against the finder's.
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(walk.inspected, names, "the walk did not read every file `.github` holds");

        // THE EXACT ROWS, not a floor over them. `literals >= 2` was the previous assertion and
        // the tree has four, so LOSING HALF sat inside it - which is *at least one row defends
        // nothing about WHICH row*, the shape `loops.rs`' own header names. Measured in review:
        // skipping both composite actions - the files `github.com/telekom/sutura#111` was about -
        // left this test green. A row moving here is a deliberate edit to the release path, and
        // then this list moves with it.
        // AND NOTHING WENT UNCOUNTED, from the predicate the parse cannot narrow. This is the
        // arm that reaches a `- binaries:` sequence item, which moved no row and no count.
        assert!(
            walk.unaccounted.is_empty(),
            "a line spells this set's key and was not classified: {:?}",
            walk.unaccounted
        );

        let mut literals: Vec<String> = Vec::new();
        let mut references: Vec<String> = Vec::new();
        let mut undefaulted: Vec<String> = Vec::new();
        for (name, found) in walk.rows {
            let at = format!("{name}:{}", found.line);
            // EXHAUSTIVE, with no `_` arm: a fifth class cannot arrive here already exempt - it is
            // `error[E0004]` in this test and in `declaration::read` both. #414.
            match found.carries {
                Carried::Set(names) => {
                    assert_eq!(names, expected, "{at} disagrees");
                    literals.push(at);
                }
                Carried::Reference(expression) => references.push(format!("{at} -> {expression}")),
                Carried::Undefaulted => undefaulted.push(at),
                // #329: this used to be the silent bucket, and it was `spelled` returning
                // nothing rather than a bucket at all.
                Carried::Opaque(value) => panic!("{at} carries `{value}`, which nothing compares"),
            }
        }
        assert_eq!(
            literals,
            [
                ".github/actions/build-artefacts/action.yml:16",
                ".github/actions/embedded-dependency-list/action.yml:15",
                ".github/workflows/cross-link.yml:101",
                ".github/workflows/release.yml:82",
            ],
            "the set of files spelling the shipped set literally has changed"
        );
        // AND THE REFERENCES, because a literal turned into an expression is the same 4-to-3 drop
        // this rule exists to close, and the gate cannot hold it: a reference is legitimate and
        // nothing resolves one. This list is what notices.
        assert_eq!(
            references,
            [
                ".github/actions/build-artefacts/action.yml:61 -> inputs.binaries",
                ".github/actions/build-artefacts/action.yml:118 -> inputs.binaries",
                ".github/actions/build-artefacts/action.yml:235 -> inputs.binaries",
                ".github/actions/embedded-dependency-list/action.yml:83 -> inputs.binaries",
                ".github/workflows/cross-link.yml:246 -> env.BINARIES",
                ".github/workflows/release.yml:291 -> env.BINARIES",
            ],
            "the set of declarations referencing the shipped set has changed"
        );
        // AND THE FOURTH CLASS, pinned EMPTY. Every `binaries:` input under `.github` states a
        // default today, so a live `default:` deleted from one makes a row appear here - the
        // transition #329's symptom survived in, which is a row in the verdict now and a red test
        // rather than a literal count quietly moving from 4 to 3.
        assert!(
            undefaulted.is_empty(),
            "an input declaring the shipped set states no default, which the gate refuses: {undefaulted:?}"
        );
    }

    #[test]
    fn the_real_tree_holds_the_refusal_this_gates_remedy_names() {
        // The pointer the gate PRINTS, against the tree that has to hold it. Its absence is what
        // let two remedies go on naming `ci.yml` for a whole commit after the step left that file.
        let root = crate::repo::root().expect("could not locate the repo");
        let files = super::finder::yaml_files(&root).expect("a file under `.github` could not be read");
        let host = super::refusal::hosting(&files, &super::refusal::PROBE_REFUSAL)
            .expect("no single workflow refuses an EMPTY probe manifest, so the remedies name nothing");
        assert!(host.starts_with(".github/workflows/"), "{host}");
    }
}
