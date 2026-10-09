#![forbid(unsafe_code)]
//! The real `cliff.toml` keeps every `feat`, `fix` and breaking subject in the changelog and
//! strips the decision record from it, scope included, and skips any other subject that names a
//! record. The commit-subject guard reads the same pattern and refuses a `feat`, `fix` or breaking
//! subject that names one.
//!
//! The cell runs the file's own `commit_preprocessors` and `commit_parsers` with the `regex` crate,
//! which is what git-cliff compiles them with, over subjects that have each been an entry and over
//! one corpus that it also hands to the real `check-pr-title`. It is not git-cliff: the template and
//! the body rules are not run. A dedicated `tests/` target, so `just causality` can run it against
//! the base tree, where the skip rule drops a feature and the guard passes a record in a scope.

#![cfg(test)]

use regex::Regex;
use std::path::Path;
use std::process::{Command, Output};

/// The one pattern for a record named by number: `cliff.toml` carries it in every rule that names
/// one, and `xtask/src/docs/links.rs` is the guard's copy.
const RECORD: &str = r"(?i)\bADR[\s-]*\d{4}\b";

const FEATURES: &str = "<!-- 01 -->Features";
const FIXES: &str = "<!-- 02 -->Fixes";
const BREAKING: &str = "<!-- 00 -->Breaking changes";
const DOCS: &str = "<!-- 06 -->Documentation";

fn root_file(rel: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the workspace root");
    std::fs::read_to_string(root.join(rel)).expect("a file of the real tree")
}

/// A `commit_parsers` rule: the subject pattern and its group, `None` for a skip rule. A rule with only
/// a `body` matcher is left out: no subject here has one.
type Parser = (Regex, Option<String>);

/// A subject that has been a changelog entry: raw, then the group and the rendered line.
type Kept = (&'static str, &'static str, &'static str);

struct Cliff {
    preprocessors: Vec<(Regex, String)>,
    parsers: Vec<Parser>,
}

fn load() -> Cliff {
    let doc: toml::Table = root_file("cliff.toml").parse().expect("cliff.toml parses");
    let git = doc["git"].as_table().expect("the [git] table");
    let field = |rule: &toml::Value, key: &str| rule.get(key).and_then(toml::Value::as_str).map(str::to_owned);
    let rules = |key: &str| git[key].as_array().unwrap_or_else(|| panic!("{key} is an array"));
    Cliff {
        preprocessors: rules("commit_preprocessors")
            .iter()
            .map(|rule| {
                let pattern = field(rule, "pattern").expect("a pattern");
                (
                    Regex::new(&pattern).expect("a valid pattern"),
                    field(rule, "replace").expect("a replace"),
                )
            })
            .collect(),
        parsers: rules("commit_parsers")
            .iter()
            .filter_map(|rule| {
                let message = Regex::new(&field(rule, "message")?).expect("a valid message pattern");
                Some((message, field(rule, "group")))
            })
            .collect(),
    }
}

impl Cliff {
    /// `raw` after every preprocessor.
    fn strip(&self, raw: &str) -> String {
        self.preprocessors.iter().fold(raw.to_owned(), |text, (pattern, replace)| {
            pattern.replace_all(&text, replace.as_str()).into_owned()
        })
    }

    /// The group and the rendered line of `raw`, or `None` when git-cliff skips it.
    fn render(&self, raw: &str) -> Option<(String, String)> {
        let message = self.strip(raw);
        let (_, line) = message.split_once(": ")?;
        let (_, group) = self.parsers.iter().find(|(pattern, _)| pattern.is_match(&message))?;
        Some((group.clone()?, line.to_owned()))
    }
}

/// Subjects that have each been a changelog entry, a feature among them: raw, then the group and the line.
const KEPT: [Kept; 17] = [
    (
        "feat(identity): per-metric visibility, steps 2-4 of docs/adr/0028 (#825)",
        FEATURES,
        "per-metric visibility, steps 2-4 (#825)",
    ),
    (
        "feat(identity): grant the two principals jobUser+dataViewer and record the cell state in ADR 0017",
        FEATURES,
        "grant the two principals jobUser+dataViewer and record the cell state",
    ),
    (
        "fix(nix): make checks.shipped-features per-artefact, amend ADR-0017 (#821)",
        FIXES,
        "make checks.shipped-features per-artefact (#821)",
    ),
    (
        "fix(docs): ADR-0009's digest arity claim in a third phrasing (#798)",
        FIXES,
        "digest arity claim in a third phrasing (#798)",
    ),
    (
        "fix(docs): ADR 0018's shipped-artifact claims, and a stray merge log (#714)",
        FIXES,
        "shipped-artifact claims, and a stray merge log (#714)",
    ),
    (
        "fix(xtask): narrow the ADR-0025 row to the one sentence it can refuse (#612)",
        FIXES,
        "narrow the row to the one sentence it can refuse (#612)",
    ),
    (
        "feat(datafusion): a compressed CSV or NDJSON source, and ADR 0039",
        FEATURES,
        "a compressed CSV or NDJSON source",
    ),
    ("feat(config): add the knob (ADR 0011)", FEATURES, "add the knob"),
    (
        "feat(xtask): check-shared-client refuses the clients ADR 0023 forbids",
        FEATURES,
        "check-shared-client refuses the clients the decision record forbids",
    ),
    ("refactor!: rename the port of ADR-0015", BREAKING, "rename the port"),
    ("refactor(ADR-0011)!: rename the port", BREAKING, "rename the port"),
    (
        "fix(docs,#760): ADR 0035 leads with WIF, not agent identity",
        FIXES,
        "the decision record leads with WIF, not agent identity",
    ),
    (
        "feat(governance): a headroom gauge for the per-replica spend ledger (#884)",
        FEATURES,
        "a headroom gauge for the per-replica spend ledger (#884)",
    ),
    (
        "feat: ADR 0011 vs ADR 0012 compared",
        FEATURES,
        "the decision record vs the decision record compared",
    ),
    (
        "fix(x): ADR 0011 vs docs/adr/0012-y.md vs ADR-0013 compared",
        FIXES,
        "the decision record vs the decision record vs the decision record compared",
    ),
    (
        "feat: an ADRESS column and an ADR 11 note",
        FEATURES,
        "an ADRESS column and an ADR 11 note",
    ),
    (
        "docs: an ADR 11 note and a headroom note",
        DOCS,
        "an ADR 11 note and a headroom note",
    ),
];

/// Subjects that are neither a `feat`, a `fix` nor breaking and name a record: the skip rule drops them.
const DROPPED: [&str; 5] = [
    "docs: ADR 0008 markers, per-adapter 408 stops and a demo cwd cell (#1186)",
    "test(conformance): the three ADR-0012 federated cases as .case files (#1048)",
    "docs(adr): stop naming an entry count in ADR 0004",
    "test(x): a case that docs/adr/0007-x.md describes",
    "docs(adr-0004): cite the upstream number for the non-ASCII panic (#854)",
];

#[test]
fn a_feat_a_fix_and_a_breaking_subject_keep_their_line_without_the_record_and_the_rest_drop() {
    let cliff = load();
    for (raw, group, line) in KEPT {
        let got = cliff.render(raw);
        assert_eq!(got, Some((group.to_owned(), line.to_owned())), "{raw}");
        let record = Regex::new(&format!("docs/adr/|{RECORD}")).expect("the record pattern");
        assert!(!record.is_match(line), "the rendered line still names a record: {line}");
        let stripped = cliff.strip(raw);
        assert!(
            !record.is_match(&stripped),
            "the stripped subject still names a record: {stripped}"
        );
    }
    for raw in DROPPED {
        assert_eq!(cliff.render(raw), None, "{raw}");
    }
}

/// A text and whether it names a record: the shapes of one spelling each, and three that do not.
const CORPUS: [(&str, bool); 8] = [
    ("ADR-0011", true),
    ("ADR 0011", true),
    ("ADR0011", true),
    ("ADR  0011", true),
    ("adr 0011", true),
    ("headroom for 2024 rows", false),
    ("ADRESS 0011", false),
    ("ADR 11", false),
];

fn check_pr_title(title: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-pr-title")
        .arg(title)
        .output()
        .expect("execute the real xtask binary")
}

#[test]
fn one_pattern_names_a_record_for_the_changelog_and_for_the_commit_subject_guard() {
    let cliff = load();
    for (text, names) in CORPUS {
        for (raw, group, stripped) in [
            (format!("fix: x per {text}"), FIXES, "fix: x"),
            (format!("feat({text}): add the knob"), FEATURES, "feat: add the knob"),
        ] {
            let refused = check_pr_title(&raw);
            let said = format!(
                "{}{}",
                String::from_utf8_lossy(&refused.stdout),
                String::from_utf8_lossy(&refused.stderr)
            );
            assert_eq!(refused.status.code(), Some(i32::from(names)), "{raw}: {said}");
            assert_eq!(said.contains("names a decision record"), names, "{raw}: {said}");
            if names {
                assert_eq!(cliff.strip(&raw), stripped, "{raw}");
                assert_eq!(cliff.render(&raw).map(|(group, _)| group), Some(group.to_owned()), "{raw}");
            } else {
                assert_eq!(cliff.strip(&raw), raw, "{raw}");
            }
        }
        let docs = format!("docs: x per {text}");
        assert!(check_pr_title(&docs).status.success(), "{docs}");
        assert_eq!(
            cliff.render(&docs).map(|(group, _)| group),
            (!names).then(|| DOCS.to_owned()),
            "{docs}"
        );
    }
}

#[test]
fn every_rule_that_names_a_record_and_the_guard_carry_the_same_pattern_string() {
    let text = root_file("cliff.toml");
    let doc: toml::Table = text.parse().expect("cliff.toml parses");
    let git = doc["git"].as_table().expect("the [git] table");
    let patterns = |key: &str, field: &str| -> Vec<String> {
        git[key]
            .as_array()
            .expect("a rule array")
            .iter()
            .filter_map(|rule| rule.get(field)?.as_str().map(str::to_owned))
            .collect()
    };
    let skip = patterns("commit_parsers", "message")
        .into_iter()
        .filter(|pattern| pattern.contains("docs/adr/"))
        .collect::<Vec<_>>();
    let rules = [patterns("commit_preprocessors", "pattern"), skip].concat();
    assert_eq!(rules.len(), 10, "nine preprocessors and the skip rule");
    for rule in &rules {
        assert!(rule.contains(RECORD), "a rule without the one pattern: {rule}");
        assert!(
            !rule.replace(RECORD, "").contains("ADR"),
            "a second spelling of a record in: {rule}"
        );
    }
    let guard = root_file("xtask/src/docs/links.rs");
    assert!(
        guard.contains(&format!("const DECISION_NUMBER: &str = r\"{RECORD}\";")),
        "the guard's pattern differs"
    );
}
