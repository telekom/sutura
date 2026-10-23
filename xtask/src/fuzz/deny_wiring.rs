//! Is the fuzz crate's own lock judged by `cargo deny` at every site that judges the root's?
//!
//! `fuzz/` is a second cargo workspace with its own `Cargo.lock`, so `cargo deny check` at the root
//! never reads it. Each site below runs a second, fuzz-scoped invocation; delete or comment out
//! one and that venue silently stops judging the fuzz graph while every other gate stays green.
//!
//! A `check-fuzz` member rather than a unit cell over the tree, because `check-fuzz` is in
//! `hygiene`, which CI's `Structural gates` runs on every pull request - `classify` reads a change
//! to `nix/run-gate.sh` alone as `rust=false`, and would skip a test.
//!
//! **The limit.** A line scan: a line that is wholly a `#` comment runs nothing and is skipped; the
//! marker inside a string, a heredoc or a branch that never runs still counts.

use std::path::Path;

const MARKER: &str = "cargo deny --manifest-path fuzz/Cargo.toml";

/// The script that runs `cargo deny` over the vendored `ClickHouse` driver's lock, which the root run
/// and the fuzz run both skip.
const DRIVER_MARKER: &str = "nix/deny-clickhouse-driver.sh";

/// Every site that runs `cargo deny` over the root workspace, and so must over the fuzz crate.
const SITES: [(&str, &str); 3] = [
    ("flake.nix", "apps.deny"),
    ("justfile", "the gates recipe"),
    ("nix/run-gate.sh", "the supply-chain arm"),
];

/// Does `text` run [`MARKER`] on a line that is not wholly a comment?
fn invokes(text: &str) -> bool {
    runs(text, MARKER)
}

/// Does `text` hold `marker` on a line that is not wholly a comment?
fn runs(text: &str, marker: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| line.contains(marker))
}

/// A site's text: the justfile is read with the files it imports.
fn site_text(root: &Path, path: &str) -> Result<String, String> {
    if path == "justfile" {
        return crate::tasks::justfiles::text(root).map_err(|refused| refused.to_string());
    }
    std::fs::read_to_string(root.join(path)).map_err(|error| error.to_string())
}

/// One message per site under `root` that does not run [`MARKER`]. Empty is a pass.
pub(super) fn gaps(root: &Path) -> Vec<String> {
    SITES
        .iter()
        .filter_map(|(path, site)| match site_text(root, path) {
            Ok(text) if invokes(&text) => None,
            Ok(_) => Some(format!(
                "{path} ({site}) does not run `{MARKER}` - that venue never judges fuzz/Cargo.lock"
            )),
            Err(reason) => Some(format!(
                "{path} is unreadable ({reason}) - whether it judges fuzz/Cargo.lock could not be checked"
            )),
        })
        .collect()
}

/// One message per site under `root` that does not run [`DRIVER_MARKER`]. Empty is a pass.
pub(super) fn driver_gaps(root: &Path) -> Vec<String> {
    SITES
        .iter()
        .filter_map(|(path, site)| match site_text(root, path) {
            Ok(text) if runs(&text, DRIVER_MARKER) => None,
            Ok(_) => Some(format!(
                "{path} ({site}) does not run `{DRIVER_MARKER}` - that venue never judges nix/clickhouse-adbc.Cargo.lock"
            )),
            Err(reason) => Some(format!(
                "{path} is unreadable ({reason}) - whether it judges nix/clickhouse-adbc.Cargo.lock could not be checked"
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo;

    #[test]
    fn fuzz_deny_is_wired_everywhere_it_needs_to_be() {
        let Some(root) = repo::root() else { return };
        assert_eq!(gaps(&root), Vec::<String>::new());
    }

    #[test]
    fn the_deny_marker_in_an_imported_gates_recipe_satisfies_the_fuzz_wiring_check() {
        let marker = format!("    {MARKER} check\n");
        let wired = |tag: &str, justfile: &str, gates: &str| {
            crate::scratch_tree::Tree::of(
                &format!("fuzz-deny-{tag}"),
                &[
                    ("flake.nix", marker.as_bytes()),
                    ("nix/run-gate.sh", marker.as_bytes()),
                    ("justfile", justfile.as_bytes()),
                    ("just/gates.just", gates.as_bytes()),
                ],
            )
        };
        let imported = format!("gates:\n{marker}");
        let tree = wired("imported", "import \"just/gates.just\"\n", &imported);
        assert_eq!(gaps(tree.root()), Vec::<String>::new());
        let tree = wired("silent", "import \"just/gates.just\"\n", "gates:\n    echo hi\n");
        assert_eq!(gaps(tree.root()).len(), 1);
    }

    #[test]
    fn a_commented_out_fuzz_deny_line_is_not_an_invocation() {
        let line = "      deny_run cargo deny --manifest-path fuzz/Cargo.toml check\n";
        assert!(invokes(line));
        assert!(!invokes(&line.replacen("deny_run", "# deny_run", 1)));
    }

    #[test]
    fn the_driver_deny_script_is_wired_everywhere_it_needs_to_be() {
        let Some(root) = repo::root() else { return };
        assert_eq!(driver_gaps(&root), Vec::<String>::new());
    }

    #[test]
    fn a_site_that_runs_only_the_fuzz_deny_is_a_gap_for_the_driver_lock() {
        let fuzz = format!("    {MARKER} check\n");
        let driver = format!("    bash {DRIVER_MARKER}\n");
        let both = format!("{fuzz}{driver}");
        let tree = |tag: &str, flake: &str| {
            crate::scratch_tree::Tree::of(
                &format!("driver-deny-{tag}"),
                &[
                    ("flake.nix", flake.as_bytes()),
                    ("nix/run-gate.sh", both.as_bytes()),
                    ("justfile", both.as_bytes()),
                ],
            )
        };
        assert_eq!(driver_gaps(tree("wired", &both).root()), Vec::<String>::new());
        let gaps = driver_gaps(tree("fuzz-only", &fuzz).root());
        assert_eq!(gaps.len(), 1, "{gaps:?}");
        assert!(!runs(&format!("    # bash {DRIVER_MARKER}\n"), DRIVER_MARKER));
    }
}
