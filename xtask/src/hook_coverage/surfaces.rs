//! The surface table: what kinds of file a change can touch, and what inspects each.
//!
//! A module of its own because it grows by ROW while [`super`] grows by rule, and the 1000-line
//! cap under `xtask/` cannot be exempted - so the file holding the table is the one that has to
//! have room. Nothing moved across this boundary changed, and the readers stayed: `touched`,
//! `surface_gaps` and `unknown_hook_ids` all ask questions OF this table rather than being part
//! of it.

/// A kind of file a change can touch, and what inspects it.
pub(super) struct Surface {
    /// What a reader would call it.
    pub(super) label: &'static str,
    /// Path globs, matched by [`repo::matches`].
    pub(super) paths: &'static [&'static str],
    /// The hook IDs that claim it. **EMPTY is the sharp case**: nothing a diff-scoped hook run
    /// invokes reaches this surface at all.
    pub(super) hooks: &'static [&'static str],
    /// The `just` task that reaches it when no hook did.
    pub(super) reached_by: &'static str,
}

/// Every surface, and the reason each row is where it is.
pub(super) const SURFACES: &[Surface] = &[
    Surface {
        // The extension, not a directory: `crates/`, `xtask/` and `examples/` all carry Rust, and
        // a directory list here is a list to forget the day a fourth appears.
        // `rust-tests` and `rust-doctests` are NOT claimed, and their absence is the honest
        // reading rather than an omission. Neither declares a `files:` or a `types:` filter, so
        // prek runs both on every diff and their rows always appear - a claim on them can never
        // report a gap, exactly as `hygiene`'s could not. They still run, and the exit code is
        // what says whether they passed; what they cannot be is evidence about THIS diff, which is
        // the only thing this module measures.
        label: "Rust source",
        paths: &["*.rs"],
        hooks: &["rust-fmt", "rust-clippy", "rust-check-changed"],
        reached_by: "lint",
    },
    Surface {
        label: "shell script",
        paths: &["*.sh"],
        hooks: &["shellcheck"],
        reached_by: "lint-workflows",
    },
    Surface {
        // `zizmor`'s own `files:` is `^\.github/workflows/.*\.ya?ml$`, so this row and that
        // regex agree by construction rather than by coincidence.
        label: "workflow YAML",
        paths: &[".github/workflows/*.yml", ".github/workflows/*.yaml"],
        hooks: &["zizmor"],
        reached_by: "lint-workflows",
    },
    Surface {
        // NO HOOK, and that is the finding rather than an omission here. `actionlint` cannot read
        // a composite action at the pinned version, `zizmor` is pointed elsewhere, and a `run:`
        // block is not a `.sh` file - so this surface is invisible to every hook in the config.
        label: "composite-action shell",
        paths: &[".github/actions/*/action.yml", ".github/actions/*/action.yaml"],
        hooks: &[],
        reached_by: "lint-workflows",
    },
    Surface {
        // The same shape one file over, and it had no row at all: a 74-line change to `devenv.nix`
        // used to produce a surface list that said nothing about it. The shell in this file is the
        // `scripts.<name>.exec` bodies and `enterShell`, none of which is a tracked `*.sh` file, a
        // workflow or a composite action - so every row above filters it out.
        //
        // `hooks` IS EMPTY, and it was `["hygiene"]` for one release - a claim that could never
        // report a gap, because that hook is `always_run: true`. No hook's `files:` filter reaches
        // `devenv.nix` at all, so empty is the honest value.
        //
        // It does NOT mean nothing checks this file: `check-devenv-shell` runs inside `hygiene` on
        // every commit and every pull request. What no hook reaches is the LINTER - ShellCheck
        // runs when the dev shell is BUILT, and nothing a diff-scoped run invokes builds one - so
        // `reached_by` is that task and `just ship-check` runs it rather than describing the gap.
        label: "devenv script shell",
        paths: &["devenv.nix"],
        hooks: &[],
        reached_by: "devenv-linter",
    },
];

#[cfg(test)]
mod tests {
    use super::SURFACES;

    // A `#[test]` OF ITS OWN, for `xtask/src/falsifier.rs`'s reason: a moved file that adds none is
    // revertible, the parent declaring `mod surfaces;` is held at HEAD for ITS tests, and `E0583`
    // then turns a real causal verdict into `INCONCLUSIVE`.

    #[test]
    fn the_table_can_answer_the_questions_its_readers_ask_of_it() {
        // Three properties every reader in `super` assumes and none of them checks, because each
        // is about the TABLE rather than about a run. A duplicate label makes two rows of the
        // coverage report indistinguishable; a row with no path is matched by `touched` never, so
        // it is a claim over nothing; and a row with no `reached_by` leaves `--surface-tasks` with
        // nothing to print for the empty-hook case, which is the whole point of an empty hook set.
        assert!(!SURFACES.is_empty(), "an empty table makes every reader vacuous");
        let mut labels: Vec<&str> = SURFACES.iter().map(|surface| surface.label).collect();
        let count = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), count, "two rows share a label: {labels:?}");
        for surface in SURFACES {
            assert!(!surface.paths.is_empty(), "`{}` matches nothing", surface.label);
            assert!(!surface.reached_by.is_empty(), "`{}` names no task", surface.label);
        }
    }
}
