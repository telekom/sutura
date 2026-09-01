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

use std::collections::BTreeMap;

use crate::Verdict;
use crate::repo;

/// The declaration every literal is compared against.
const SOURCE: &str = "nix/shipped.nix";

/// The key a workflow spells the set under, and the key an action's input carries it as.
const KEYS: [&str; 2] = ["BINARIES", "binaries"];

/// Every `bin = "..."` inside `nix/shipped.nix`'s `binaries = [ ... ]`, in declaration order.
///
/// SCOPED TO THAT LIST rather than grepping the file: `bin` is also a parameter name in `ociFor`'s
/// signature and a field read as `b.bin` in four places, and a whole-file scan would answer for
/// lines that declare nothing. The list runs from `binaries = [` to the first `];` at the same
/// indent, which is what `nixpkgs-fmt` guarantees about the file it formats.
///
/// FOUND ANYWHERE ON THE LINE, not only at its start, and that is a correction its own tests
/// forced. The first version matched a trimmed line beginning `bin = `, which is the shape
/// `nixpkgs-fmt` produces and not the only legal one: `{ bin = "sutura"; package = "sutura-cli"; }`
/// is one record on one line, and it was read as no record at all - so a gate whose whole job is
/// to notice a missing binary would have silently missed one. That is the failure mode
/// `workflows::declared_block` records twice, in the same words: a parser that silently sees half
/// a file is worse than no parser.
fn declared(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some(open) = indent else {
            if trimmed.starts_with("binaries = [") {
                indent = Some(line.len().saturating_sub(line.trim_start().len()));
            }
            continue;
        };
        if trimmed == "];" && line.len().saturating_sub(line.trim_start().len()) == open {
            break;
        }
        names.extend(bins_in(line).map(String::from));
    }
    names
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

    println!(
        "xtask check-shipped-binaries: ok - {} literal(s) agree with {SOURCE} ({})",
        checked,
        expected.join(" ")
    );
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::{declared, spelled};

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
