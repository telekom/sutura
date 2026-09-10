//! Is every `with:` key an input the action it is passed to actually declares?
//!
//! **A key an action does not declare is not an error on the runner.** It is
//! `##[warning]Unexpected input(s) '<key>', valid inputs are [...]`, and then the action's own
//! DEFAULT for whatever the author meant to set. The step is green, the job is green, and the only
//! witness is a line in a log nothing reads. `.github/actions/causality-target-cache` passed
//! `path:` to `nix-community/cache-nix-action`, whose input is `paths:`, on every run it ever made
//! - so the action took its default path list (`["/nix"]`) and the entry held a second copy of the
//!   Nix store instead of the directory its key named. Twenty-one runs, one warning each,
//!   multi-gigabyte writes, and every gate in this repository green.
//!
//! Nothing here could have said so. `check-workflows` reads flake references, `cache_scope` reads
//! what a step is allowed to do, `zizmor` reads security shapes, and `actionlint` ships a metadata
//! set for the popular actions - which did not include the one that shipped the defect. So the
//! oracle is committed: `devco/action-inputs` records each pinned sha's declared input list, and
//! this module holds every `with:` block against it.
//!
//! # What this holds
//!
//! * **Every `with:` key of every pinned third-party `uses:`**, in `.github/workflows` and in the
//!   local composite actions, against that sha's declared inputs.
//! * **Fail-closed on an unrecorded action.** A `uses:` with no entry is refused, not skipped -
//!   otherwise adding an action would be the one way past this gate, and a moved pin would silently
//!   inherit the old sha's list.
//! * **Fail-closed on an unpinned one.** A ref that is not a 40-character sha names no fixed input
//!   set, so no record can be filed for it. `zizmor` refuses that shape too; this one refuses it
//!   because the record cannot be true otherwise.
//! * **A stale entry.** A record naming an action no `uses:` names any more is refused, so the file
//!   shrinks when the tree does instead of accumulating shas nobody can check.
//!
//! # What this does not hold
//!
//! * **Not whether a value is right.** `paths: target/causality-target` and `paths: /dev/null` are
//!   the same shape here. The KEY set is the subject; meaning is a review question.
//! * **Not a missing required input.** The runner enforces `required:` loudly, on the run, which is
//!   why the record drops that column.
//! * **Not the local composite actions' own inputs.** Those are declared in this repository, so a
//!   wrong key is a diff a reviewer sees and `actionlint` validates them from the tree. `./` calls
//!   are skipped here rather than covered twice.
//! * **Not reusable workflows** (`<owner>/<repo>/.github/workflows/<file>@<ref>`), whose `with:` is
//!   held against a `workflow_call` input block - a different oracle. This repository calls none,
//!   and one arriving is refused as unrecorded rather than passed over.
//! * **The reader is line-oriented over surface syntax**, for `fuzz`'s reason: `xtask` carries no
//!   YAML dependency and runs inside a nix sandbox. A step written in flow style
//!   (`{ uses: ..., with: { ... } }`) is invisible to it. It holds the shapes this repository
//!   writes, and the tests below fix those shapes - including the production tree itself, which is
//!   what says the reader still sees anything at all.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The record: each pinned sha's declared input names. Its header carries the refresh procedure.
const RECORD: &str = "devco/action-inputs";

/// Above this many declared inputs the failure names the count instead of the list - one action
/// here declares 62, and a wall of names buries the key that is actually wrong.
const NAMES_INLINE: usize = 25;

/// Each pinned sha's declared input names, keyed by `<owner>/<repo>@<sha>` - the shape
/// [`RECORD`] parses into.
type Declared = BTreeMap<String, BTreeSet<String>>;

/// One `with:` key passed somewhere, and where a reader can open it.
struct Passed {
    label: String,
    line: usize,
    action: String,
    key: String,
}

/// Every way a `with:` block can be passing a key its action does not declare.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let declared = match read_record(root) {
        Ok(declared) => declared,
        Err(reason) => return vec![reason],
    };
    let Some(sources) = super::sources::ci_sources(root) else {
        return vec![format!(
            "the CI source walk failed, so no `with:` key was held against {RECORD}"
        )];
    };

    let mut found = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for source in &sources {
        for (line, action) in third_party_uses(&source.text) {
            let Some((_, sha)) = action.rsplit_once('@') else {
                continue;
            };
            if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                found.push(format!(
                    "{}:{line}  `uses: {action}` is not pinned to a 40-character sha, so no input list can be recorded for it - a ref names a different manifest tomorrow",
                    source.label
                ));
                continue;
            }
            seen.insert(action.clone());
            if !declared.contains_key(&action) {
                found.push(format!(
                    "{}:{line}  `uses: {action}` has no entry in {RECORD} - read the top-level `inputs:` keys of that sha's own action.yml and file them, or this gate is passing over the one action nobody checked",
                    source.label
                ));
            }
        }
    }

    for passed in with_keys(&sources) {
        let Some(inputs) = declared.get(&passed.action) else {
            continue; // Already refused above, as an unrecorded action.
        };
        if !inputs.contains(&passed.key) {
            found.push(refusal(&passed, inputs));
        }
    }

    for action in declared.keys() {
        if !seen.contains(action) {
            found.push(format!(
                "{RECORD} files `{action}`, which no `uses:` names any more - a record of an action this repository does not call is one nobody can check"
            ));
        }
    }
    found
}

/// The refusal, shaped like the runner's own warning because that is the line a reader will find
/// in the log when they go looking.
fn refusal(passed: &Passed, inputs: &BTreeSet<String>) -> String {
    let valid = if inputs.len() <= NAMES_INLINE {
        format!("valid inputs are [{}]", inputs.iter().cloned().collect::<Vec<_>>().join(", "))
    } else {
        format!(
            "that sha declares {} inputs, none of them this one - see {RECORD}",
            inputs.len()
        )
    };
    format!(
        "{}:{}  `with: {}:` is not an input of `{}` - {valid}. The runner does not fail on this: it warns and uses the action's default, so the step reads as configured",
        passed.label, passed.line, passed.key, passed.action
    )
}

/// Parse [`RECORD`]: a line at column zero is `<owner>/<repo>@<sha>`, the indented lines under it
/// are that sha's input names.
///
/// An absent, unreadable or EMPTY record is a failure, not an empty rule - for `sast`'s reason: a
/// gate that passes by finding nothing is the failure mode a text-scanning check is most prone to.
fn read_record(root: &Path) -> Result<Declared, String> {
    let text = std::fs::read_to_string(root.join(RECORD))
        .map_err(|error| format!("{RECORD} could not be read: {error} - it is the oracle, so its absence is a refusal"))?;
    let mut declared: Declared = BTreeMap::new();
    let mut current: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            let Some(action) = current.clone() else {
                return Err(format!(
                    "{RECORD}:{}  an input name before any `<owner>/<repo>@<sha>` line",
                    index.saturating_add(1)
                ));
            };
            if let Some(names) = declared.get_mut(&action)
                && !names.insert(trimmed.to_owned())
            {
                return Err(format!(
                    "{RECORD}:{}  `{trimmed}` is filed twice under `{action}`",
                    index.saturating_add(1)
                ));
            }
            continue;
        }
        if !trimmed.contains('@') || !trimmed.contains('/') {
            return Err(format!(
                "{RECORD}:{}  `{trimmed}` is at column zero but is not `<owner>/<repo>@<sha>`",
                index.saturating_add(1)
            ));
        }
        if declared.insert(trimmed.to_owned(), BTreeSet::new()).is_some() {
            return Err(format!("{RECORD}:{}  `{trimmed}` is filed twice", index.saturating_add(1)));
        }
        current = Some(trimmed.to_owned());
    }
    if declared.is_empty() {
        return Err(format!(
            "{RECORD} declares no action - an empty record makes this gate pass by checking nothing"
        ));
    }
    if let Some((action, _)) = declared.iter().find(|(_, names)| names.is_empty()) {
        return Err(format!(
            "{RECORD} files `{action}` with no inputs - an action declaring none would make every `with:` key under it a refusal, so this is the record being half-written"
        ));
    }
    Ok(declared)
}

/// How deep a line is indented.
fn indent(line: &str) -> usize {
    line.len().saturating_sub(line.trim_start().len())
}

/// The key a step line carries, whether it is written on the `-` line or below it - [`super::step`]
/// spells the same thing for a different reader.
fn body(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed.strip_prefix("- ").map_or(trimmed, str::trim_start)
}

/// Every `uses:` naming something outside this repository, as its 1-based line and value.
///
/// `./`-prefixed calls and `docker://` images are skipped: neither takes inputs this record can
/// hold. A value with no `@` is not a versioned reference and cannot be one of ours.
///
/// THE TRAILING YAML COMMENT IS PART OF THE LINE AND NOT PART OF THE REFERENCE. Every `uses:` here
/// carries the human-readable tag - `uses: owner/repo@<sha> # v7.0.2` - and reading the line to its
/// end made every single reference look unpinned. A `#` cannot appear in an action reference, so
/// the value ends at the first one.
fn third_party_uses(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let value = body(line).strip_prefix("uses:")?;
            let value = value.split('#').next().unwrap_or(value).trim();
            let local = value.starts_with("./") || value.starts_with("docker://");
            if local || !value.contains('@') || !value.contains('/') {
                return None;
            }
            Some((index.saturating_add(1), value.to_owned()))
        })
        .collect()
}

/// Every `with:` key passed to a third-party `uses:`, across every source.
///
/// The step is delimited by INDENTATION rather than parsed: the `uses:` key's own column is the
/// step's key column, and the step ends at the first non-empty line shallower than it - which is
/// the `- ` marker of the next step. `with:` at that same column belongs to this step; the keys
/// under it are the lines at the FIRST child indent, so a block scalar's body, which YAML requires
/// to be deeper than its key, is never mistaken for one. Comment lines are skipped before that
/// indent is latched, because a comment sits wherever its author put it.
fn with_keys(sources: &[super::sources::Source]) -> Vec<Passed> {
    let mut out = Vec::new();
    for source in sources {
        let lines: Vec<&str> = source.text.lines().collect();
        for (line, action) in third_party_uses(&source.text) {
            let start = line.saturating_sub(1);
            let Some(text) = lines.get(start) else { continue };
            let key_column = text.len().saturating_sub(body(text).len());
            for key in step_with_keys(&lines, start, key_column) {
                out.push(Passed {
                    label: source.label.clone(),
                    line,
                    action: action.clone(),
                    key,
                });
            }
        }
    }
    out
}

/// The `with:` keys of the step whose key column is `key_column` and whose `uses:` is at `start`.
fn step_with_keys(lines: &[&str], start: usize, key_column: usize) -> Vec<String> {
    let mut with_at = None;
    for (index, line) in lines.iter().enumerate().skip(start.saturating_add(1)) {
        if line.trim().is_empty() {
            continue;
        }
        // Shallower than the step's keys is the next step's `- ` marker, or the end of the block.
        if indent(line) < key_column {
            break;
        }
        if indent(line) == key_column && body(line).trim_end() == "with:" {
            with_at = Some(index);
            break;
        }
    }
    let Some(with_at) = with_at else { return Vec::new() };
    let mut child = None;
    let mut keys = Vec::new();
    for line in lines.iter().skip(with_at.saturating_add(1)) {
        // A COMMENT IS NOT A KEY, and this repository writes a paragraph of them inside a `with:`
        // block. Reading `# no return. Both halves are COMMITTED ... variable:` as an input name
        // was 24 refusals over a correct tree - and it also latched the child indent onto whatever
        // column the comment happened to sit at.
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let column = indent(line);
        if column <= key_column {
            break;
        }
        let child_column = *child.get_or_insert(column);
        // Deeper than the first child indent is a block scalar's body or a nested mapping's value,
        // never a key of `with:` itself.
        if column != child_column {
            continue;
        }
        if let Some((key, _)) = line.trim().split_once(':') {
            keys.push(key.trim().to_owned());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::{Passed, read_record, step_with_keys, third_party_uses};
    use std::collections::BTreeSet;

    const PINNED: &str = "nix-community/cache-nix-action@7df957e333c1e5da7721f60227dbba6d06080569";

    /// THE PRODUCTION TREE, through the production entry point. Every refusal below breaks one
    /// input to a piece of this same call, so neutralising a refusal reddens one of them rather
    /// than none.
    #[test]
    fn the_production_tree_passes_only_recorded_input_names() {
        let root = crate::repo::root().expect("repo root");
        let found = super::problems(&root);
        assert!(found.is_empty(), "a `with:` key is not a declared input: {found:?}");
    }

    /// THE DEFECT, exactly as it shipped: `path:` where the input is `paths:`.
    #[test]
    fn the_singular_path_that_shipped_for_21_runs_is_refused() {
        let root = crate::repo::root().expect("repo root");
        let declared = read_record(&root).expect("the committed record parses");
        let inputs = declared.get(PINNED).expect("the cache action is recorded");
        assert!(!inputs.contains("path"), "the pinned sha declares no `path` input");
        assert!(inputs.contains("paths"), "it declares `paths`");
        let passed = Passed {
            label: String::from("actions/causality-target-cache"),
            line: 81,
            action: String::from(PINNED),
            key: String::from("path"),
        };
        let refusal = super::refusal(&passed, inputs);
        assert!(refusal.contains("`with: path:`"), "the key itself: {refusal}");
        assert!(refusal.contains("actions/causality-target-cache:81"), "where: {refusal}");
        assert!(refusal.contains("paths"), "the valid list: {refusal}");
    }

    /// THE REFUSAL ITSELF, through the production entry point rather than through
    /// [`super::refusal`]: neutralising the `contains` check leaves the message formatter and its
    /// test untouched, so this is the one that dies. A synthetic tree, one recorded action, one
    /// undeclared key.
    #[test]
    fn a_with_key_the_action_does_not_declare_is_refused_end_to_end() {
        let root = std::env::temp_dir().join(format!("sutura-with-e2e-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(root.join("devco")).expect("temp devco dir");
        std::fs::create_dir_all(root.join(".github").join("workflows")).expect("temp workflows");
        let sha = "0123456789012345678901234567890123456789";
        std::fs::write(root.join(super::RECORD), format!("owner/repo@{sha}\n  paths\n  nix\n")).expect("write the record");
        let workflow = format!(
            "jobs:\n  one:\n    steps:\n      - uses: owner/repo@{sha} # v1.2.3\n        with:\n          # a comment, which is not a key\n          paths: target/x\n          path: target/x\n"
        );
        std::fs::write(root.join(".github").join("workflows").join("x.yml"), workflow).expect("write the workflow");
        let found = super::problems(&root);
        std::fs::remove_dir_all(&root).expect("the temp tree this test created is removable");
        assert_eq!(found.len(), 1, "exactly the undeclared key: {found:?}");
        assert!(found[0].contains("`with: path:`"), "{found:?}");
        assert!(found[0].contains("x.yml:4"), "the file and the `uses:` line: {found:?}");
        assert!(found[0].contains("valid inputs are [nix, paths]"), "{found:?}");
    }

    /// Above [`super::NAMES_INLINE`] the failure names the count, not 62 names.
    #[test]
    fn a_wide_input_list_is_summarised_rather_than_dumped() {
        let inputs: BTreeSet<String> = (0..=super::NAMES_INLINE).map(|n| format!("permission-{n}")).collect();
        let passed = Passed {
            label: String::from("ci.yml"),
            line: 1,
            action: String::from("actions/create-github-app-token@0000"),
            key: String::from("nope"),
        };
        let refusal = super::refusal(&passed, &inputs);
        assert!(refusal.contains("declares 26 inputs"), "{refusal}");
        assert!(!refusal.contains("permission-0"), "the list is not dumped: {refusal}");
    }

    /// The step reader, against the shapes this repository writes: the key on the `- ` line, the
    /// key below it, a block scalar whose body must not become a key, and the next step ending it.
    #[test]
    fn the_step_reader_takes_with_keys_and_not_a_block_scalar_body() {
        let text = concat!(
            "    steps:\n",
            "      - uses: owner/one@aaa\n",
            "        with:\n",
            "          extra_nix_config: |\n",
            "            fallback = true\n",
            "            extra-substituters = https://example.invalid\n",
            "          install_url: https://example.invalid/nix\n",
            "      - name: second\n",
            "        uses: owner/two@bbb\n",
            "        with:\n",
            "          paths: target/x\n",
            "        env:\n",
            "          NOT_A_WITH_KEY: 1\n",
        );
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            step_with_keys(&lines, 1, 8),
            vec![String::from("extra_nix_config"), String::from("install_url")],
            "the block scalar's body is not a key and the next step ends the step"
        );
        assert_eq!(
            step_with_keys(&lines, 8, 8),
            vec![String::from("paths")],
            "a sibling `env:` block contributes no `with:` key"
        );
    }

    /// Local calls and images take no recorded input; a pinned third-party one does.
    #[test]
    fn only_third_party_uses_are_collected() {
        let text = concat!(
            "      - uses: ./.github/actions/reclaim-disk\n",
            "      - uses: docker://example.invalid/image@sha256:0\n",
            "      - uses: owner/repo@0123456789012345678901234567890123456789\n",
            "        with:\n",
            "          key: value\n",
        );
        let found = third_party_uses(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found.first().map(|(line, _)| *line), Some(3), "{found:?}");
    }

    /// An empty or half-written record must not read as a rule that passes.
    #[test]
    fn a_record_that_would_check_nothing_is_refused() {
        let root = std::env::temp_dir().join(format!("sutura-with-keys-{}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(root.join("devco")).expect("temp devco dir");
        for (why, body, expected) in [
            ("no record at all", None, "could not be read"),
            ("only comments", Some("# nothing\n"), "declares no action"),
            ("an action with no inputs", Some("owner/repo@aaa\n"), "no inputs"),
            ("an orphan input name", Some("  paths\n"), "before any"),
            (
                "a bare word at column zero",
                Some("nonsense\n  paths\n"),
                "not `<owner>/<repo>@<sha>`",
            ),
            (
                "the same action twice",
                Some("owner/repo@aaa\n  paths\nowner/repo@aaa\n  paths\n"),
                "filed twice",
            ),
        ] {
            match body {
                Some(text) => std::fs::write(root.join(super::RECORD), text).expect("write record"),
                None => drop(std::fs::remove_file(root.join(super::RECORD))),
            }
            let reason = read_record(&root).expect_err(why);
            assert!(reason.contains(expected), "{why}: {reason}");
        }
        std::fs::remove_dir_all(&root).expect("the temp tree this test created is removable");
    }
}
