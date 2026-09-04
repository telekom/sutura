//! What ships is declared in `nix/shipped.nix`, and every workflow that builds it spells the same
//! set again as a literal. This is the gate that makes the two agree.
//!
//! # Why the arrival check in `release.yml` cannot do this
//!
//! `Check that every build arrived` counts one `facts/` file per binary per target and fails on a
//! shortfall, which reads like the mechanism and is not: both `want` and the files it counts are
//! driven by the SAME `BINARIES` string, and `build-artefacts` loops over that same input. Add a
//! third record to `nix/shipped.nix` and touch nothing else, and that check still sees
//! `want == have`, every job is green, and the new binary is in no release. `ci.yml`'s own
//! `BINARIES` literal has the same shape: the cross matrix proves that the names IT holds link,
//! and says nothing about the names nix ships.
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
//!   and is wrong, in either direction.
//! * **The FEATURES each binary ships with.** That is `checks.shipped-features`, which reads them
//!   out of the built artifact rather than out of any text.
//! * **A `probeFeatures` entry no page documents.** A probe nobody asked for costs a job and
//!   breaks no claim, so it is not a failure. The direction that matters is the other one.
//! * **A documented `cargo run --features ...`.** Deliberately out of scope: it builds for the
//!   host and executes, and the risk a probe exists for is the CROSS link. `docs/serving.md`
//!   documents one, and demanding a probe for it would fail a correct tree.
//!
//! # The second rule: a documented feature build is a probe, or it is unproven
//!
//! `nix/shipped.nix`'s `probeFeatures` decides which feature-on builds the four `cross` jobs LINK,
//! and `docs/adr/0017` claims on that basis that *the documented feature-on source build is linked
//! on every pull request*. The coupling that claim rests on was prose: a page tells a reader to run
//! `cargo build --release -p sutura-cli --features bigquery`, and nothing said that `bigquery` had
//! to appear in `probeFeatures`.
//!
//! **The step's own refusal cannot cover it, and that is why this rule is here.** `ci.yml` refuses
//! an EMPTY probe manifest, which is equivalent to *the probe is gone* only because one binary
//! declares no probe features. Declare one for `sutura-serve` and delete `"bigquery"` from
//! `sutura-cli`'s, and the manifest is still non-empty: the job goes green, and the claim reverts
//! to *assumed* with no signal at all. Found in review of this rule's own absence.

use std::collections::BTreeMap;

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
    /// `probeFeatures`: which feature-on source builds the `cross` jobs link.
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

/// A `cargo build` line in published prose that names a package AND a feature list.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DocumentedBuild {
    /// Repo-relative page, for the message.
    page: String,
    /// One-based line, so a failure names something a reader can open.
    line: usize,
    /// The `-p` / `--package` value.
    package: String,
    /// The `--features` value, split on commas.
    features: Vec<String>,
}

/// Every documented feature build in one page.
///
/// **`cargo build` and not `cargo run`**, for the reason the module header gives. Both the spaced
/// and the `=` form of each flag are read, because a page may legitimately write either and a gate
/// that sees one shape silently passes the other.
fn documented_builds(page: &str, text: &str) -> Vec<DocumentedBuild> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if !line.contains("cargo build") {
            continue;
        }
        let (Some(package), Some(features)) = (flag_value(line, "-p", "--package"), flag_value(line, "", "--features")) else {
            continue;
        };
        out.push(DocumentedBuild {
            page: String::from(page),
            line: index.saturating_add(1),
            package,
            features: features
                .split(',')
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(String::from)
                .collect(),
        });
    }
    out
}

/// The value of `short` or `long` on this line, in either the spaced or the `=` form.
///
/// An empty `short` means the flag has no short form. The value stops at whitespace and at the
/// punctuation prose wraps a command in - a backtick, a quote, a line-continuation backslash - so
/// an inline citation yields the same value a fenced block does.
fn flag_value(line: &str, short: &str, long: &str) -> Option<String> {
    for flag in [long, short] {
        if flag.is_empty() {
            continue;
        }
        for sep in [' ', '='] {
            let needle = format!("{flag}{sep}");
            let Some(at) = line.find(&needle) else {
                continue;
            };
            // A boundary before the flag, so `--no-features` is not read as `--features`.
            let boundary = line
                .get(..at)
                .and_then(|s| s.chars().next_back())
                .is_none_or(char::is_whitespace);
            if !boundary {
                continue;
            }
            let tail = line.get(at.saturating_add(needle.len())..)?.trim_start();
            let value: String = tail
                .chars()
                .take_while(|c| !c.is_whitespace() && !matches!(c, '\\' | '`' | '"' | '\''))
                .collect();
            if !value.is_empty() && !value.starts_with('-') {
                return Some(value);
            }
        }
    }
    None
}

/// A set of names spelled out in one file, with the line it was spelled on.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Spelled {
    line: usize,
    names: Vec<String>,
}

/// Is every word a plain name rather than an expression?
///
/// This is what tells `BINARIES: sutura sutura-serve` from `binaries: ${{ env.BINARIES }}`. The
/// second is a REFERENCE to a literal declared elsewhere, so comparing it to anything would be
/// comparing an expression to a list; the first is the literal itself.
fn is_literal_set(value: &str) -> bool {
    !value.is_empty()
        && value
            .split_whitespace()
            .all(|word| !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_'))
}

/// Every literal set of shipped-binary names one YAML file spells out.
///
/// Two shapes, because a workflow and a composite action declare the same thing differently:
///
/// * `BINARIES: sutura sutura-serve` - a workflow's `env`, at any indent.
/// * an input called `binaries:` whose block carries `default: sutura sutura-serve` - an action's
///   input. The block is the lines indented deeper than the key, which is the only thing about
///   YAML this needs to know.
fn spelled(text: &str) -> Vec<Spelled> {
    let mut found = Vec::new();
    // The indent of an open `binaries:` input block, while one is open.
    let mut block: Option<usize> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let indent = line.len().saturating_sub(line.trim_start().len());

        if let Some(open) = block {
            if trimmed.is_empty() {
                continue;
            }
            if indent <= open {
                block = None;
            } else if let Some(value) = trimmed.strip_prefix("default:")
                && is_literal_set(value.trim())
            {
                found.push(Spelled {
                    line: index.saturating_add(1),
                    names: value.split_whitespace().map(String::from).collect(),
                });
                block = None;
                continue;
            } else {
                continue;
            }
        }

        for key in KEYS {
            let Some(rest) = trimmed.strip_prefix(key) else {
                continue;
            };
            let Some(value) = rest.strip_prefix(':') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                // A key with no value opens a block - an action's input declaration.
                block = Some(indent);
            } else if is_literal_set(value) {
                found.push(Spelled {
                    line: index.saturating_add(1),
                    names: value.split_whitespace().map(String::from).collect(),
                });
            }
            break;
        }
    }
    found
}

/// Every workflow and every local composite action, as `(repo-relative path, contents)`.
///
/// The same two directories `check-workflows` reads, and for the same reason: since #111 a build
/// step is a composite action, so a literal can live in either place.
fn yaml_files(root: &std::path::Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let github = root.join(".github");
    if let Ok(entries) = std::fs::read_dir(github.join("workflows")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let yaml = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "yml" || e == "yaml");
            if !yaml {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                let name = path
                    .file_name()
                    .map_or_else(String::new, |n| format!(".github/workflows/{}", n.to_string_lossy()));
                files.insert(name, text);
            }
        }
    }
    // One level deep, which is not an approximation: an action IS
    // `.github/actions/<name>/action.yml` by GitHub's own resolution rules.
    if let Ok(dirs) = std::fs::read_dir(github.join("actions")) {
        for dir in dirs.flatten() {
            for leaf in ["action.yml", "action.yaml"] {
                let path = dir.path().join(leaf);
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let name = dir
                    .path()
                    .file_name()
                    .map_or_else(String::new, |n| format!(".github/actions/{}/{leaf}", n.to_string_lossy()));
                files.insert(name, text);
            }
        }
    }
    files
}

/// Every documented feature build in `docs/**`, page by page.
fn documented_pages(root: &std::path::Path) -> Vec<DocumentedBuild> {
    let mut pages = Vec::new();
    repo::collect_files(root, &root.join("docs"), &["md"], &mut pages);
    pages.sort();
    let mut found = Vec::new();
    for page in pages {
        if let Ok(text) = std::fs::read_to_string(root.join(&page)) {
            found.extend(documented_builds(&page, &text));
        }
    }
    found
}

/// The second rule: a documented `cargo build --features` of a SHIPPED package is probed.
///
/// Returns the failures, and how many documented builds named a shipped package at all - the
/// second is what keeps the first from running over an empty set, the failure mode a
/// text-scanning check is most prone to.
fn unprobed(records: &[Record], documented: &[DocumentedBuild]) -> (Vec<String>, usize) {
    let mut problems = Vec::new();
    let mut reconciled = 0_usize;
    for build in documented {
        let Some(record) = records.iter().find(|r| r.package == build.package) else {
            // A page documenting a feature build of something this repository does not ship has
            // no `probeFeatures` field to disagree with. Not this gate's business.
            continue;
        };
        reconciled = reconciled.saturating_add(1);
        for feature in &build.features {
            if !record.probe_features.iter().any(|p| p == feature) {
                problems.push(format!(
                    "{}:{} documents `cargo build -p {} --features {feature}`, and {SOURCE} does \
                     not list `{feature}` in {}'s probeFeatures - so no `cross` job links it",
                    build.page, build.line, build.package, record.bin
                ));
            }
        }
    }
    (problems, reconciled)
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

    let mut mismatches = Vec::new();
    let mut checked = 0_usize;
    for (name, text) in yaml_files(&root) {
        for set in spelled(&text) {
            checked = checked.saturating_add(1);
            if set.names != expected {
                mismatches.push((name.clone(), set));
            }
        }
    }

    if checked == 0 {
        eprintln!("xtask check-shipped-binaries: FAILED - no shipped-binary literal in any workflow or action");
        eprintln!("  `release.yml` and `ci.yml` each declare `BINARIES`, and the two build actions");
        eprintln!("  carry it as an input default. Finding none means this gate is reading nothing.");
        return Verdict::Fail;
    }

    let documented = documented_pages(&root);
    let (unprobed_problems, reconciled) = unprobed(&records(&source), &documented);

    if !mismatches.is_empty() {
        eprintln!(
            "xtask check-shipped-binaries: FAILED - {} literal(s) disagree with {SOURCE}",
            mismatches.len()
        );
        eprintln!("  {SOURCE} ships: {}", expected.join(" "));
        for (name, set) in &mismatches {
            eprintln!("  {name}:{} spells: {}", set.line, set.names.join(" "));
        }
        eprintln!();
        eprintln!("  These are compared IN ORDER, because the order is read: `sutura` is what an");
        eprintln!("  unqualified download and an unqualified `docker pull` mean, and it is the first");
        eprintln!("  row of every table in the release notes.");
        eprintln!();
        eprintln!("  A binary added to {SOURCE} and not here is built by nothing and released as");
        eprintln!("  nothing - which is what #111 was. A name here that {SOURCE} does not ship is a");
        eprintln!("  `nix build` of an attribute that does not exist, minutes into a tagged run.");
        return Verdict::Fail;
    }

    // FAIL CLOSED, and this is the half `ci.yml`'s empty-manifest refusal cannot cover. If no
    // page documents a feature build of a shipped package, `probeFeatures` is reconciled against
    // nothing and `docs/adr/0017`'s claim that *the documented feature-on source build is linked*
    // has no referent left. A page reworded out of existence is a verdict, not a pass.
    if reconciled == 0 {
        eprintln!("xtask check-shipped-binaries: FAILED - no page under docs/ documents a `cargo build`");
        eprintln!("  with both a shipped package and `--features`, so {SOURCE}'s probeFeatures is");
        eprintln!("  compared to nothing. docs/adr/0017 claims the documented feature-on build is");
        eprintln!("  linked on every pull request; either a page states that build, or the claim goes.");
        return Verdict::Fail;
    }

    if !unprobed_problems.is_empty() {
        eprintln!(
            "xtask check-shipped-binaries: FAILED - {} documented feature build(s) are linked by nothing",
            unprobed_problems.len()
        );
        for problem in &unprobed_problems {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("  `ci.yml`'s probe step refuses an EMPTY manifest, which is not this: with one");
        eprintln!("  binary still declaring a probe the manifest is non-empty, the four `cross` jobs");
        eprintln!("  are green, and the build a reader is told to run is linked by nothing.");
        return Verdict::Fail;
    }

    println!(
        "xtask check-shipped-binaries: ok - {} literal(s) agree with {SOURCE} ({}), {reconciled} documented feature build(s) probed",
        checked,
        expected.join(" ")
    );
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::{declared, spelled};

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
        assert!(records[1].probe_features.is_empty());
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
            "cargo build --release -p sutura-cli --features bigquery\n",
            "cargo build --package sutura-cli --features=bigquery,postgres\n",
            "run `cargo build -p sutura-cli --features bigquery` to get one\n",
            // Neither of these is a documented feature BUILD, and both must be ignored: the first
            // has no feature list, the second is a `cargo run`, which builds for the host and is
            // not what a cross probe answers for.
            "cargo build --release -p sutura-cli\n",
            "cargo run -p sutura-serve --features tls\n",
        );
        let found = super::documented_builds("docs/p.md", page);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].package, "sutura-cli");
        assert_eq!(found[0].features, vec![String::from("bigquery")]);
        assert_eq!(found[1].features, vec![String::from("bigquery"), String::from("postgres")]);
        // The backticked citation yields the same value the fenced block does, rather than
        // `bigquery\``, which is what a naive scan to the next space returns.
        assert_eq!(found[2].line, 3);
        assert_eq!(found[2].features, vec![String::from("bigquery")]);
    }

    #[test]
    fn a_documented_feature_the_probe_set_omits_is_linked_by_nothing() {
        // The scenario `ci.yml`'s empty-manifest refusal CANNOT catch: a second binary keeps the
        // manifest non-empty, so the job stays green while the documented build is probed by
        // nothing. RED before this rule existed, because nothing compared the two.
        let nix = concat!(
            "  binaries = [\n",
            "    { bin = \"sutura\"; package = \"sutura-cli\"; probeFeatures = [ ]; }\n",
            "    { bin = \"sutura-serve\"; package = \"sutura-serve\"; probeFeatures = [ \"tls\" ]; }\n",
            "  ];\n",
        );
        let documented = super::documented_builds(
            "docs/getting-started.md",
            "cargo build --release -p sutura-cli --features bigquery\n",
        );
        let (problems, reconciled) = super::unprobed(&super::records(nix), &documented);
        assert_eq!(reconciled, 1);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("docs/getting-started.md:1"));
        assert!(problems[0].contains("bigquery"));
        assert!(problems[0].contains("sutura"));
    }

    #[test]
    fn a_documented_build_of_something_we_do_not_ship_is_not_this_gates_business() {
        // No `probeFeatures` field exists to disagree with, and `reconciled` must not count it -
        // or the fail-closed test below would pass on a tree where nothing was reconciled.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; probeFeatures = [ ]; }\n  ];\n";
        let documented = super::documented_builds("docs/p.md", "cargo build -p some-other-crate --features whatever\n");
        let (problems, reconciled) = super::unprobed(&super::records(nix), &documented);
        assert!(problems.is_empty());
        assert_eq!(reconciled, 0);
    }

    #[test]
    fn a_probe_no_page_documents_is_not_a_failure() {
        // Deliberate: a probe nobody asked for costs a job and breaks no claim. The direction
        // this gate holds is the other one.
        let nix = "  binaries = [\n    { bin = \"sutura\"; package = \"sutura-cli\"; probeFeatures = [ \"bigquery\" \"postgres\" ]; }\n  ];\n";
        let documented = super::documented_builds("docs/p.md", "cargo build -p sutura-cli --features bigquery\n");
        let (problems, reconciled) = super::unprobed(&super::records(nix), &documented);
        assert!(problems.is_empty());
        assert_eq!(reconciled, 1);
    }

    #[test]
    fn the_repositorys_own_documented_feature_build_is_probed() {
        // The gate over the real tree, so the reconciliation is not only exercised on fixtures.
        // `docs/getting-started.md` tells a reader to build `sutura-cli --features bigquery`, and
        // `nix/shipped.nix` must therefore probe it. This is the assertion that goes red if
        // somebody deletes the probe and keeps the page.
        let root = crate::repo::root().expect("could not locate the repo");
        let source = std::fs::read_to_string(root.join(super::SOURCE)).expect("could not read nix/shipped.nix");
        let documented = super::documented_pages(&root);
        let (problems, reconciled) = super::unprobed(&super::records(&source), &documented);
        assert!(reconciled > 0, "no page documents a feature build of a shipped package");
        assert!(problems.is_empty(), "{problems:?}");
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

    #[test]
    fn a_workflow_env_literal_is_a_spelled_set() {
        let yaml = "env:\n  IMAGE: ghcr.io/x\n  BINARIES: sutura sutura-serve\n";
        let found = spelled(yaml);
        assert_eq!(found.len(), 1, "got {found:?}");
        assert_eq!(found[0].names, vec![String::from("sutura"), String::from("sutura-serve")]);
        assert_eq!(found[0].line, 3);
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
        let found = spelled(yaml);
        assert_eq!(found.len(), 1, "only the binaries input's default counts: {found:?}");
        assert_eq!(found[0].names, vec![String::from("sutura"), String::from("sutura-serve")]);
    }

    #[test]
    fn a_reference_to_the_literal_is_not_itself_a_literal() {
        // A call site passes `binaries: ${{ env.BINARIES }}`, which is a reference to the set
        // declared elsewhere. Comparing an expression to a list would fail on a correct tree,
        // which is how a gate gets disabled.
        let yaml = "      - uses: ./.github/actions/build-artefacts\n        with:\n          binaries: ${{ env.BINARIES }}\n";
        assert!(spelled(yaml).is_empty());
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
        let found = spelled(yaml);
        assert_eq!(expected.len(), 3);
        assert_eq!(found.len(), 1);
        assert_ne!(found[0].names, expected, "the drift must be visible");
    }

    #[test]
    fn order_is_part_of_the_comparison() {
        let nix = "  binaries = [\n    { bin = \"sutura\"; }\n    { bin = \"sutura-serve\"; }\n  ];\n";
        let yaml = "env:\n  BINARIES: sutura-serve sutura\n";
        assert_ne!(spelled(yaml)[0].names, declared(nix));
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
        let mut seen = 0_usize;
        for (name, text) in super::yaml_files(&root) {
            for set in spelled(&text) {
                seen = seen.saturating_add(1);
                assert_eq!(set.names, expected, "{name}:{} disagrees", set.line);
            }
        }
        assert!(seen >= 2, "found {seen} literal(s); the release path declares more than that");
    }
}
