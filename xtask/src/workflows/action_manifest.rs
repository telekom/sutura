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
//! # What this does not hold
//!
//! * **Not that the fetched sha is the one a workflow actually pins.** `with_keys::problems`
//!   still refuses an unrecorded `uses:`; this only makes filing the record for a new pin one
//!   command instead of a hand transcription.
//! * **Not removal of a stale entry.** `with_keys::problems` already refuses one - an action no
//!   `uses:` names any more - so a bump that moves every reference off an old sha will fail there
//!   until the old entry is deleted by hand; this command never deletes.

use crate::Verdict;
use crate::repo;

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

    let manifest = match fetch(repo_path, sha) {
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

/// `curl -fsSL` against the raw manifest, `action.yml` then `action.yaml` - the two names GitHub
/// actions ship theirs under.
fn fetch(repo_path: &str, sha: &str) -> Result<String, String> {
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

/// The keys directly under a top-level `inputs:` block - not their `description`, `default` or
/// `required` fields, which sit one indent deeper. Same rule `with_keys::step_with_keys` applies
/// to a `with:` block: latch the first child indent below the block's own key, and only a line at
/// that column is a key of it.
fn declared_inputs(manifest: &str) -> Option<Vec<String>> {
    let lines: Vec<&str> = manifest.lines().collect();
    let inputs_at = lines.iter().position(|line| line.trim_end() == "inputs:")?;
    let mut child = None;
    let mut keys = Vec::new();
    let Some(body) = lines.get(inputs_at.saturating_add(1)..) else {
        return Some(Vec::new());
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
        if column != child_column {
            continue; // deeper: a field of the key above, not a key of `inputs:` itself.
        }
        if let Some((key, _)) = line.trim().split_once(':') {
            keys.push(key.trim().to_owned());
        }
    }
    Some(keys)
}

#[cfg(test)]
mod tests {
    use super::declared_inputs;

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
}
