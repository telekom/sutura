//! Fetch a pinned action's manifest and file its declared inputs into `devco/action-inputs`.
//!
//! **DEVELOPER-INVOKED, NEVER PART OF THE GATE.** `with_keys::problems` runs inside
//! `checks.hygiene`, a nix derivation with no network - a fetch there would fail closed on every
//! run, gate included. This is the tool the gate's own refusal message names, run by hand (or by
//! whoever bumps a pin) from a shell that has one; `problems` stays a pure reader of the record
//! this writes.
//!
//! `cargo xtask refresh-action-inputs <owner>/<repo>@<sha>` reads that sha's `action.yml` off
//! `raw.githubusercontent.com` - the record's own header names that URL as answering 200 for a
//! public action with no credential - and appends its declared top-level `inputs:` keys, in the
//! manifest's own order. An action already filed under that exact `<owner>/<repo>@<sha>` is left
//! untouched: a second run is a no-op, never a silent reorder.
//!
//! `just refresh-moved-action-pins` is the same fetch for a whole Dependabot bump: every pinned
//! sha the record does not name is read at the NEW sha, filed in place of the stale entry of the
//! same action, and the input change is printed for a human to read - names, `required` and
//! `default`, old sha against new. All manifests are fetched before anything is written, so one
//! fetch error leaves the record untouched.
//!
//! # What this does not hold
//!
//! * **Not that the fetched sha is the one a workflow actually pins.** `with_keys::problems`
//!   still refuses an unrecorded `uses:`; this only makes filing the record for a new pin one
//!   command instead of a hand transcription.
//! * **Not removal of a stale entry, for `refresh-action-inputs`.** `with_keys::problems` already
//!   refuses one - an action no `uses:` names any more - so a bump that moves every reference off
//!   an old sha will fail there until the old entry is deleted; only `refresh-moved-action-pins`
//!   deletes, and only the stale entries of an action it has just refilled.
//! * **Not a review of the action.** The diff is the declared inputs and nothing else; the
//!   network's answer for a sha is trusted, and the code behind it is not read.

use crate::Verdict;
use crate::repo;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// Where an action's manifest text comes from: [`Curl`] in production, a fake in the cells.
pub(crate) trait Manifests {
    fn manifest(&self, repo_path: &str, sha: &str) -> Result<String, String>;
}

/// `curl -fsSL` against the raw manifest, `action.yml` then `action.yaml` - the two names GitHub
/// actions ship theirs under.
struct Curl;

impl Manifests for Curl {
    fn manifest(&self, repo_path: &str, sha: &str) -> Result<String, String> {
        for name in ["action.yml", "action.yaml"] {
            let url = format!("https://raw.githubusercontent.com/{repo_path}/{sha}/{name}");
            let output = std::process::Command::new("curl")
                .args(["-fsSL", &url])
                .output()
                .map_err(|error| format!("could not run curl: {error}"))?;
            if output.status.success() {
                return String::from_utf8(output.stdout).map_err(|error| format!("{url} did not answer with UTF-8: {error}"));
            }
        }
        Err(format!(
            "neither action.yml nor action.yaml answered for {repo_path}@{sha} - is the sha correct and public?"
        ))
    }
}

pub(crate) fn run(args: &[String]) -> Verdict {
    let Some(action) = args.first() else {
        eprintln!("xtask refresh-action-inputs: usage: <owner>/<repo>@<sha>");
        return Verdict::Usage;
    };
    let Some((repo_path, sha)) = action.rsplit_once('@') else {
        eprintln!("xtask refresh-action-inputs: `{action}` has no `@<sha>`");
        return Verdict::Usage;
    };
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        eprintln!("xtask refresh-action-inputs: `{sha}` is not a 40-character sha");
        return Verdict::Usage;
    }

    let Some(root) = repo::root() else {
        eprintln!("xtask refresh-action-inputs: could not locate the repo root");
        return Verdict::Fail;
    };
    let record_path = root.join(super::with_keys::RECORD);
    let existing = std::fs::read_to_string(&record_path).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == action.as_str()) {
        println!(
            "xtask refresh-action-inputs: {action} is already filed in {}",
            super::with_keys::RECORD
        );
        return Verdict::Pass;
    }

    let manifest = match Curl.manifest(repo_path, sha) {
        Ok(text) => text,
        Err(reason) => {
            eprintln!("xtask refresh-action-inputs: {reason}");
            return Verdict::Fail;
        }
    };
    let Some(inputs) = declared_inputs(&manifest) else {
        eprintln!("xtask refresh-action-inputs: no top-level `inputs:` block in {repo_path}@{sha}'s action.yml");
        return Verdict::Fail;
    };
    if inputs.is_empty() {
        println!("xtask refresh-action-inputs: {repo_path}@{sha} declares no inputs - nothing to file");
        return Verdict::Pass;
    }

    let mut written = existing.trim_end().to_owned();
    if !written.is_empty() {
        written.push_str("\n\n");
    }
    written.push_str(action);
    written.push('\n');
    for key in &inputs {
        written.push_str("  ");
        written.push_str(key);
        written.push('\n');
    }
    if let Err(error) = std::fs::write(&record_path, written) {
        eprintln!(
            "xtask refresh-action-inputs: could not write {}: {error}",
            super::with_keys::RECORD
        );
        return Verdict::Fail;
    }
    println!(
        "xtask refresh-action-inputs: filed {} input(s) for {action} in {}",
        inputs.len(),
        super::with_keys::RECORD
    );
    Verdict::Pass
}

/// The keys directly under a top-level `inputs:` block - not their `description`, `default` or
/// `required` fields, which sit one indent deeper. Same rule `with_keys::step_with_keys` applies
/// to a `with:` block: latch the first child indent below the block's own key, and only a line at
/// that column is a key of it.
fn declared_inputs(manifest: &str) -> Option<Vec<String>> {
    Some(input_fields(manifest)?.into_iter().map(|(name, _)| name).collect())
}

/// The two fields of an input a reviewer reads in a bump, as written on one line. A value on a
/// block scalar (`default: |`) reads as its indicator (`|`), not its text: a changed block is not
/// seen here.
#[derive(Debug, Default, PartialEq, Eq)]
struct Input {
    required: Option<String>,
    default: Option<String>,
}

/// An input's key and its [`Input`].
type Named = (String, Input);

/// [`declared_inputs`] with each key's `required` and `default`, in the manifest's own order.
fn input_fields(manifest: &str) -> Option<Vec<Named>> {
    let lines: Vec<&str> = manifest.lines().collect();
    let inputs_at = lines.iter().position(|line| line.trim_end() == "inputs:")?;
    let mut child = None;
    let mut field = None;
    let mut inputs: Vec<Named> = Vec::new();
    let Some(body) = lines.get(inputs_at.saturating_add(1)..) else {
        return Some(inputs);
    };
    for line in body {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let column = line.len().saturating_sub(line.trim_start().len());
        if column == 0 {
            break; // back at column zero: `inputs:` block ended.
        }
        let child_column = *child.get_or_insert(column);
        if column == child_column {
            if let Some((key, _)) = line.trim().split_once(':') {
                inputs.push((key.trim().to_owned(), Input::default()));
                field = None;
            }
        } else if column > child_column
            && column == *field.get_or_insert(column)
            && let Some((name, value)) = line.trim().split_once(':')
            && let Some((_, input)) = inputs.last_mut()
        {
            let value = Some(value.trim().trim_matches(['"', '\'']).to_owned());
            match name.trim() {
                "required" => input.required = value,
                "default" => input.default = value,
                _ => {}
            }
        }
    }
    Some(inputs)
}

/// What a bump changed in an action's inputs, one line each; empty when nothing a reviewer reads
/// moved.
fn input_diff(old: &[Named], new: &[Named]) -> Vec<String> {
    fn find<'a>(set: &'a [Named], name: &str) -> Option<&'a Input> {
        set.iter().find(|(key, _)| key == name).map(|(_, input)| input)
    }
    let shown = |value: Option<&String>| value.map_or_else(|| String::from("<unset>"), Clone::clone);
    let mut lines = Vec::new();
    for (name, _) in old.iter().filter(|(name, _)| find(new, name).is_none()) {
        lines.push(format!("- {name}"));
    }
    for (name, input) in new {
        let Some(before) = find(old, name) else {
            lines.push(format!("+ {name}"));
            continue;
        };
        if before.required != input.required {
            lines.push(format!(
                "~ {name}: required {} -> {}",
                shown(before.required.as_ref()),
                shown(input.required.as_ref())
            ));
        }
        if before.default != input.default {
            lines.push(format!(
                "~ {name}: default {} -> {}",
                shown(before.default.as_ref()),
                shown(input.default.as_ref())
            ));
        }
    }
    lines
}

/// One entry of the record: the `<owner>/<repo>@<sha>` line, its input lines, and the `#` comment
/// directly above it - which describes that entry and goes with it.
struct Block {
    action: String,
    lines: Range<usize>,
}

fn blocks(lines: &[&str]) -> Vec<Block> {
    let mut found: Vec<Block> = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let entry = !line.trim().is_empty() && !line.starts_with(char::is_whitespace) && !line.starts_with('#');
        if !entry {
            continue;
        }
        let mut start = at;
        while start > 0 && lines.get(start.saturating_sub(1)).is_some_and(|above| above.starts_with('#')) {
            start = start.saturating_sub(1);
        }
        let end = lines
            .iter()
            .enumerate()
            .skip(at.saturating_add(1))
            .find(|(_, below)| below.trim().is_empty() || !below.starts_with(char::is_whitespace))
            .map_or(lines.len(), |(index, _)| index);
        found.push(Block {
            action: line.trim().to_owned(),
            lines: start..end,
        });
    }
    found
}

fn is_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The record rewritten for the moved pins, and what a human reads before keeping it.
struct Refresh {
    record: String,
    report: Vec<String>,
}

/// Every pinned sha the record does not name is filed from its own manifest in place of the stale
/// entries of the same action - those the tree no longer pins. `Ok(None)` is nothing to do.
/// Everything is fetched before anything is built, so an error returns no record at all.
fn refresh(record: &str, pinned: &BTreeSet<String>, manifests: &impl Manifests) -> Result<Option<Refresh>, String> {
    let lines: Vec<&str> = record.lines().collect();
    let entries = blocks(&lines);
    let filed: BTreeSet<&str> = entries.iter().map(|block| block.action.as_str()).collect();
    let moved: Vec<(&str, &str)> = pinned
        .iter()
        .filter(|action| !filed.contains(action.as_str()))
        .filter_map(|action| action.rsplit_once('@'))
        .filter(|(_, sha)| is_sha(sha))
        .collect();
    if moved.is_empty() {
        return Ok(None);
    }

    let mut report = Vec::new();
    let mut fresh: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (repo_path, sha) in &moved {
        let manifest = manifests.manifest(repo_path, sha)?;
        let new = input_fields(&manifest)
            .filter(|inputs| !inputs.is_empty())
            .ok_or_else(|| format!("{repo_path}@{sha} declares no `inputs:`, which the record cannot hold"))?;
        let mut entry = format!("{repo_path}@{sha}");
        for (name, _) in &new {
            entry.push_str("\n  ");
            entry.push_str(name);
        }
        fresh.entry(repo_path).or_default().push(entry);

        let stale: Vec<&str> = entries
            .iter()
            .map(|block| block.action.as_str())
            .filter(|action| !pinned.contains(*action))
            .filter(|action| action.rsplit_once('@').is_some_and(|(old_repo, _)| old_repo == *repo_path))
            .collect();
        let short = |sha: &str| sha.get(..12).unwrap_or(sha).to_owned();
        if stale.is_empty() {
            report.push(format!("{repo_path}: new at {}", short(sha)));
            report.extend(new.iter().map(|(name, _)| format!("  + {name}")));
        }
        for old_action in stale {
            let (_, old_sha) = old_action.rsplit_once('@').unwrap_or((old_action, ""));
            let old = input_fields(&manifests.manifest(repo_path, old_sha)?).unwrap_or_default();
            report.push(format!("{repo_path}: {} -> {}", short(old_sha), short(sha)));
            let diff = input_diff(&old, &new);
            if diff.is_empty() {
                report.push(String::from("  no input change"));
            }
            report.extend(diff.into_iter().map(|line| format!("  {line}")));
        }
    }

    let stale_at = |index: usize| {
        entries.iter().find(|block| {
            block.lines.start == index
                && !pinned.contains(&block.action)
                && block
                    .action
                    .rsplit_once('@')
                    .is_some_and(|(repo_path, _)| fresh.contains_key(repo_path))
        })
    };
    let mut out: Vec<String> = Vec::new();
    let mut placed: BTreeSet<&str> = BTreeSet::new();
    let mut resume = 0;
    for (index, line) in lines.iter().enumerate() {
        if index < resume {
            continue;
        }
        let Some(block) = stale_at(index) else {
            out.push((*line).to_owned());
            continue;
        };
        let repo_path = block.action.rsplit_once('@').map_or("", |(repo_path, _)| repo_path);
        resume = block.lines.end;
        if placed.insert(repo_path) {
            out.push(fresh.get(repo_path).map_or_else(String::new, |entries| entries.join("\n\n")));
        } else if lines.get(resume).is_some_and(|line| line.trim().is_empty()) {
            resume = resume.saturating_add(1); // a further replaced entry's blank separator.
        }
    }
    let mut record = out.join("\n").trim_end().to_owned();
    for (repo_path, entries) in &fresh {
        if !placed.contains(repo_path) {
            record.push_str("\n\n");
            record.push_str(&entries.join("\n\n"));
        }
    }
    record.push('\n');
    Ok(Some(Refresh { record, report }))
}

/// `just refresh-moved-action-pins`: the record brought up to the pins the tree now holds.
pub(crate) fn run_moved_pins(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("xtask refresh-moved-action-pins: takes no argument");
        return Verdict::Usage;
    }
    let Some(root) = repo::root() else {
        eprintln!("xtask refresh-moved-action-pins: could not locate the repo root");
        return Verdict::Fail;
    };
    let Some(sources) = super::sources::ci_sources(&root) else {
        eprintln!("xtask refresh-moved-action-pins: the CI source walk failed");
        return Verdict::Fail;
    };
    let pinned: BTreeSet<String> = sources
        .iter()
        .flat_map(|source| super::with_keys::third_party_uses(&source.text))
        .map(|(_, action)| action)
        .collect();
    let record_path = root.join(super::with_keys::RECORD);
    let record = match std::fs::read_to_string(&record_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!(
                "xtask refresh-moved-action-pins: could not read {}: {error}",
                super::with_keys::RECORD
            );
            return Verdict::Fail;
        }
    };
    match refresh(&record, &pinned, &Curl) {
        Ok(None) => println!("xtask refresh-moved-action-pins: every pin is filed, nothing to do"),
        Ok(Some(Refresh { record, report })) => {
            if let Err(error) = std::fs::write(&record_path, record) {
                eprintln!(
                    "xtask refresh-moved-action-pins: could not write {}: {error}",
                    super::with_keys::RECORD
                );
                return Verdict::Fail;
            }
            println!(
                "xtask refresh-moved-action-pins: rewrote {}; read the input change:",
                super::with_keys::RECORD
            );
            for line in report {
                println!("{line}");
            }
        }
        Err(reason) => {
            eprintln!(
                "xtask refresh-moved-action-pins: {reason}; {} is untouched",
                super::with_keys::RECORD
            );
            return Verdict::Fail;
        }
    }
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::{Input, Manifests, declared_inputs, input_fields, refresh};
    use std::collections::{BTreeMap, BTreeSet};

    /// A trimmed real shape: a scalar default, a multi-line `description: |`, a required flag,
    /// a blank line and a comment between keys, and a sibling top-level block ending it.
    #[test]
    fn top_level_keys_only_not_their_fields_or_a_sibling_block() {
        let manifest = concat!(
            "name: Example\n",
            "inputs:\n",
            "  listen:\n",
            "    description: The host and port to listen on.\n",
            "    default: 127.0.0.1:37515\n",
            "  use-flakehub:\n",
            "    description: |\n",
            "      Whether to upload build results to FlakeHub Cache.\n",
            "      Multiple lines, each deeper than the key.\n",
            "    default: null\n",
            "    required: false\n",
            "\n",
            "  # a comment between two keys\n",
            "  source-binary:\n",
            "    required: false\n",
            "runs:\n",
            "  using: node24\n",
        );
        assert_eq!(
            declared_inputs(manifest),
            Some(vec![
                String::from("listen"),
                String::from("use-flakehub"),
                String::from("source-binary")
            ])
        );
    }

    /// No `inputs:` at column zero at all - a manifest with no configurable inputs.
    #[test]
    fn no_inputs_block_is_none_not_empty() {
        let manifest = "name: Example\nruns:\n  using: node24\n";
        assert_eq!(declared_inputs(manifest), None);
    }

    /// An `inputs:` block that declares nothing is `Some(empty)`, distinct from absent.
    #[test]
    fn an_empty_inputs_block_is_some_empty() {
        let manifest = "inputs:\nruns:\n  using: node24\n";
        assert_eq!(declared_inputs(manifest), Some(Vec::new()));
    }

    const REPO: &str = "owner/repo";
    const OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const OLDER: &str = "cccccccccccccccccccccccccccccccccccccccc";

    /// The manifests a fixture answers with, keyed `<owner>/<repo>@<sha>`, and how often it was asked.
    struct Fake {
        texts: BTreeMap<String, String>,
        calls: std::cell::Cell<usize>,
    }

    impl Manifests for Fake {
        fn manifest(&self, repo_path: &str, sha: &str) -> Result<String, String> {
            self.calls.set(self.calls.get().saturating_add(1));
            self.texts
                .get(&format!("{repo_path}@{sha}"))
                .cloned()
                .ok_or_else(|| format!("no fixture for {repo_path}@{sha}"))
        }
    }

    fn fake(texts: &[(&str, String)]) -> Fake {
        Fake {
            texts: texts
                .iter()
                .map(|(action, text)| ((*action).to_owned(), text.clone()))
                .collect(),
            calls: std::cell::Cell::new(0),
        }
    }

    /// A manifest's inputs: `(name, required, default)`.
    type Fixture<'a> = (&'a str, &'a str, &'a str);

    /// A manifest declaring those inputs.
    fn manifest_text(inputs: &[Fixture]) -> String {
        let declared = inputs
            .iter()
            .map(|(name, required, default)| {
                format!("  {name}:\n    description: |\n      Words.\n    required: {required}\n    default: {default}\n")
            })
            .collect::<Vec<_>>()
            .concat();
        format!("name: Example\ninputs:\n{declared}runs:\n  using: node24\n")
    }

    fn pins(shas: &[&str]) -> BTreeSet<String> {
        shas.iter().map(|sha| format!("{REPO}@{sha}")).collect()
    }

    fn record_for_old() -> String {
        format!("# header\n\nother/tool@{OLD}\n  keep\n\n# read on a day\n{REPO}@{OLD}\n  one\n  two\n\nz/last@{OLD}\n  end\n")
    }

    fn refreshed(old: &str, new: &str) -> Result<Option<super::Refresh>, String> {
        let fake = fake(&[
            (&format!("{REPO}@{OLD}"), old.to_owned()),
            (&format!("{REPO}@{NEW}"), new.to_owned()),
        ]);
        let pinned = pins(&[NEW])
            .into_iter()
            .chain([format!("other/tool@{OLD}"), format!("z/last@{OLD}")])
            .collect();
        refresh(&record_for_old(), &pinned, &fake)
    }

    /// Not measured by causality: shares a file with its code; held by kill `rewrite`.
    #[test]
    fn a_moved_pin_with_the_same_inputs_is_swapped_in_place_and_reports_no_change() {
        let same = manifest_text(&[("one", "true", "x"), ("two", "false", "y")]);
        let done = refreshed(&same, &same).expect("fetches answer").expect("a pin moved");
        let expected = format!("# header\n\nother/tool@{OLD}\n  keep\n\n{REPO}@{NEW}\n  one\n  two\n\nz/last@{OLD}\n  end\n");
        assert_eq!(
            done.record, expected,
            "new sha in, old sha and its comment out, neighbours untouched"
        );
        assert_eq!(
            done.report.last().map(String::as_str),
            Some("  no input change"),
            "{:?}",
            done.report
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `added-removed`.
    #[test]
    fn an_added_and_a_removed_input_are_named_in_the_diff() {
        let old = manifest_text(&[("one", "true", "x"), ("two", "false", "y")]);
        let new = manifest_text(&[("one", "true", "x"), ("three", "false", "z")]);
        let done = refreshed(&old, &new).expect("fetches answer").expect("a pin moved");
        assert!(done.report.contains(&String::from("  - two")), "{:?}", done.report);
        assert!(done.report.contains(&String::from("  + three")), "{:?}", done.report);
        assert!(
            done.record.contains(&format!("{REPO}@{NEW}\n  one\n  three\n")),
            "{}",
            done.record
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `required`.
    #[test]
    fn a_changed_required_flag_is_named_in_the_diff() {
        let old = manifest_text(&[("one", "false", "x")]);
        let new = manifest_text(&[("one", "true", "x")]);
        let done = refreshed(&old, &new).expect("fetches answer").expect("a pin moved");
        assert_eq!(
            done.report,
            vec![
                format!("{REPO}: {OLD:.12} -> {NEW:.12}"),
                String::from("  ~ one: required false -> true")
            ]
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `default`.
    #[test]
    fn a_changed_default_is_named_in_the_diff() {
        let old = manifest_text(&[("one", "true", "\"x\"")]);
        let new = manifest_text(&[("one", "true", "y")]);
        let done = refreshed(&old, &new).expect("fetches answer").expect("a pin moved");
        assert_eq!(
            done.report,
            vec![
                format!("{REPO}: {OLD:.12} -> {NEW:.12}"),
                String::from("  ~ one: default x -> y")
            ]
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `fetch-new`.
    #[test]
    fn a_failed_fetch_of_the_new_manifest_returns_no_record() {
        let old = manifest_text(&[("one", "true", "x")]);
        let fake = fake(&[(&format!("{REPO}@{OLD}"), old)]);
        let result = refresh(&record_for_old(), &pins(&[NEW]), &fake);
        assert!(
            result
                .as_ref()
                .is_err_and(|reason| reason.contains("no fixture for") && reason.contains(NEW)),
            "{:?}",
            result.map(|done| done.map(|d| d.record))
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `fetch-old`.
    #[test]
    fn a_failed_fetch_of_the_stale_manifest_returns_no_record() {
        let new = manifest_text(&[("one", "true", "x")]);
        let fake = fake(&[(&format!("{REPO}@{NEW}"), new)]);
        let result = refresh(&record_for_old(), &pins(&[NEW]), &fake);
        assert!(
            result
                .as_ref()
                .is_err_and(|reason| reason.contains("no fixture for") && reason.contains(OLD)),
            "{:?}",
            result.map(|done| done.map(|d| d.record))
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `filed`.
    #[test]
    fn a_pin_the_record_already_names_is_not_fetched() {
        let fake = fake(&[]);
        let result = refresh(&record_for_old(), &pins(&[OLD]), &fake);
        assert!(matches!(result, Ok(None)), "nothing to do");
        assert_eq!(fake.calls.get(), 0, "no fetch for a filed pin");
    }

    /// Not measured by causality: shares a file with its code; held by kills `unpinned`.
    #[test]
    fn a_ref_that_is_not_a_sha_is_not_fetched() {
        let fake = fake(&[]);
        let pinned: BTreeSet<String> = [String::from("fresh/act@v4"), format!("{REPO}@{OLD}")].into_iter().collect();
        let result = refresh(&record_for_old(), &pinned, &fake);
        assert!(
            matches!(result, Ok(None)),
            "the gate refuses a ref; this tool does not file one"
        );
        assert_eq!(fake.calls.get(), 0);
    }

    /// Not measured by causality: shares a file with its code; held by kills `still-pinned-a`, `still-pinned-b`.
    #[test]
    fn an_entry_the_tree_still_pins_is_kept_beside_the_new_one() {
        let new = manifest_text(&[("one", "true", "x")]);
        let fake = fake(&[(&format!("{REPO}@{NEW}"), new.clone()), (&format!("{REPO}@{OLD}"), new)]);
        let done = refresh(&record_for_old(), &pins(&[OLD, NEW]), &fake)
            .expect("fetch answers")
            .expect("a pin moved");
        assert!(
            done.record.contains(&format!("{REPO}@{OLD}\n  one\n  two\n")),
            "{}",
            done.record
        );
        assert!(done.record.ends_with(&format!("{REPO}@{NEW}\n  one\n")), "{}", done.record);
        assert_eq!(
            done.report,
            vec![format!("{REPO}: new at {NEW:.12}"), String::from("  + one")]
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `new-action`.
    #[test]
    fn a_pin_with_no_stale_entry_is_appended_as_new() {
        let new = manifest_text(&[("one", "true", "x")]);
        let fake = fake(&[(&format!("fresh/act@{NEW}"), new)]);
        let pinned: BTreeSet<String> = [format!("fresh/act@{NEW}"), format!("{REPO}@{OLD}")].into_iter().collect();
        let done = refresh(&record_for_old(), &pinned, &fake)
            .expect("fetch answers")
            .expect("a pin moved");
        assert!(
            done.record
                .ends_with(&format!("z/last@{OLD}\n  end\n\nfresh/act@{NEW}\n  one\n")),
            "{}",
            done.record
        );
        assert_eq!(
            done.report,
            vec![format!("fresh/act: new at {NEW:.12}"), String::from("  + one")]
        );
    }

    /// Not measured by causality: shares a file with its code; held by kills `issha-len`, `issha-hex`.
    #[test]
    fn a_short_hex_or_non_hex_ref_is_not_a_sha() {
        let fake = fake(&[]);
        let pinned: BTreeSet<String> = [
            String::from("fresh/act@abc1234"),
            format!("fresh/act@{}", "g".repeat(40)),
            format!("{REPO}@{OLD}"),
        ]
        .into_iter()
        .collect();
        let result = refresh(&record_for_old(), &pinned, &fake);
        assert!(
            matches!(result, Ok(None)),
            "neither a short hex ref nor 40 non-hex characters is a sha"
        );
        assert_eq!(fake.calls.get(), 0);
    }

    /// Not measured by causality: shares a file with its code; held by kill `field-latch`.
    #[test]
    fn a_deeper_line_in_a_description_block_is_not_a_field() {
        let manifest = "inputs:\n  one:\n    required: true\n    default: x\n    description: |\n      default: deeper\n      required: false\nruns:\n  using: node24\n";
        assert_eq!(
            input_fields(manifest),
            Some(vec![(
                String::from("one"),
                Input {
                    required: Some(String::from("true")),
                    default: Some(String::from("x")),
                }
            )])
        );
    }

    /// Not measured by causality: shares a file with its code; held by kills `unset-required`, `unset-default`.
    #[test]
    fn an_input_that_gains_or_loses_its_fields_is_named_with_unset() {
        let bare = "inputs:\n  one:\n    description: Words.\nruns:\n  using: node24\n";
        let set = manifest_text(&[("one", "true", "x")]);
        let gained = refreshed(bare, &set).expect("fetches answer").expect("a pin moved");
        assert_eq!(
            gained.report,
            vec![
                format!("{REPO}: {OLD:.12} -> {NEW:.12}"),
                String::from("  ~ one: required <unset> -> true"),
                String::from("  ~ one: default <unset> -> x"),
            ]
        );
        let lost = refreshed(&set, bare).expect("fetches answer").expect("a pin moved");
        assert_eq!(
            lost.report,
            vec![
                format!("{REPO}: {OLD:.12} -> {NEW:.12}"),
                String::from("  ~ one: required true -> <unset>"),
                String::from("  ~ one: default x -> <unset>"),
            ]
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `blank-arm`.
    #[test]
    fn two_stale_entries_of_one_action_leave_one_blank_separator() {
        let record = format!("# header\n\n{REPO}@{OLD}\n  one\n\n{REPO}@{OLDER}\n  one\n\nz/last@{OLD}\n  end\n");
        let manifest = manifest_text(&[("one", "true", "x")]);
        let fake = fake(&[
            (&format!("{REPO}@{OLD}"), manifest.clone()),
            (&format!("{REPO}@{OLDER}"), manifest.clone()),
            (&format!("{REPO}@{NEW}"), manifest),
        ]);
        let pinned = pins(&[NEW]).into_iter().chain([format!("z/last@{OLD}")]).collect();
        let done = refresh(&record, &pinned, &fake)
            .expect("fetches answer")
            .expect("a pin moved");
        assert_eq!(
            done.record,
            format!("# header\n\n{REPO}@{NEW}\n  one\n\nz/last@{OLD}\n  end\n")
        );
    }

    /// Not measured by causality: shares a file with its code; held by kill `block-end`.
    #[test]
    fn an_entry_ends_at_a_column_zero_line_with_no_blank_after_it() {
        let record = format!("{REPO}@{OLD}\n  one\n# next\nz/last@{OLD}\n  end\n");
        let manifest = manifest_text(&[("one", "true", "x")]);
        let fake = fake(&[
            (&format!("{REPO}@{OLD}"), manifest.clone()),
            (&format!("{REPO}@{NEW}"), manifest),
        ]);
        let pinned = pins(&[NEW]).into_iter().chain([format!("z/last@{OLD}")]).collect();
        let done = refresh(&record, &pinned, &fake)
            .expect("fetches answer")
            .expect("a pin moved");
        assert_eq!(done.record, format!("{REPO}@{NEW}\n  one\n# next\nz/last@{OLD}\n  end\n"));
    }

    /// Not measured by causality: shares a file with its code; held by kill `strict`.
    #[test]
    fn the_rewritten_record_passes_the_strict_gate_that_refused_the_moved_pin() {
        let root = std::env::temp_dir().join(format!("sutura-moved-pins-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(root.join("devco")).expect("temp devco dir");
        std::fs::create_dir_all(root.join(".github").join("workflows")).expect("temp workflows");
        let record = format!("{REPO}@{OLD}\n  one\n  two\n");
        std::fs::write(root.join(crate::workflows::with_keys::RECORD), &record).expect("write the record");
        let workflow = format!("jobs:\n  one:\n    steps:\n      - uses: {REPO}@{NEW} # v2\n        with:\n          one: x\n");
        std::fs::write(root.join(".github").join("workflows").join("x.yml"), &workflow).expect("write the workflow");
        let before = crate::workflows::with_keys::problems(&root);
        let manifest = manifest_text(&[("one", "true", "x"), ("two", "false", "y")]);
        let fake = fake(&[
            (&format!("{REPO}@{OLD}"), manifest.clone()),
            (&format!("{REPO}@{NEW}"), manifest),
        ]);
        let done = refresh(&record, &pins(&[NEW]), &fake)
            .expect("fetches answer")
            .expect("a pin moved");
        std::fs::write(root.join(crate::workflows::with_keys::RECORD), done.record).expect("write the new record");
        let after = crate::workflows::with_keys::problems(&root);
        std::fs::remove_dir_all(&root).expect("the temp tree this test created is removable");
        assert_eq!(before.len(), 2, "the unrecorded pin and the stale entry: {before:?}");
        assert!(
            after.is_empty(),
            "the rewritten record is the one the gate accepts: {after:?}"
        );
    }
}
