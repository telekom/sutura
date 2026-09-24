//! The surface table: what kinds of file a change can touch, and what inspects each.
//!
//! A module of its own because it grows by ROW while [`super`] grows by rule, and the 1000-line
//! cap under `xtask/` cannot be exempted - so the file holding the table is the one that has to
//! have room. Nothing moved across this boundary changed, and the readers stayed: `touched`,
//! `surface_gaps` and `unknown_hook_ids` all ask questions OF this table rather than being part
//! of it.

/// A kind of file a change can touch, and what inspects it.
pub(crate) struct Surface {
    /// What a reader would call it.
    pub(super) label: &'static str,
    /// Path globs, matched by [`crate::repo::matches`].
    ///
    /// `pub(crate)` because `crate::fuzz` reads the "fuzzed tree" row's globs for a question this
    /// module does not ask: whether a fuzz target's own `sutura_*` crates are all reachable
    /// through it.
    pub(crate) paths: &'static [&'static str],
    /// The hook IDs that claim it. **EMPTY is the sharp case**: nothing a diff-scoped hook run
    /// invokes reaches this surface at all.
    ///
    /// `pub(crate)` for the same reason `paths` is - it is how `crate::fuzz` finds the "fuzzed
    /// tree" row rather than matching on its label text.
    pub(crate) hooks: &'static [&'static str],
    /// The `just` task that reaches it when no hook did.
    pub(super) reached_by: &'static str,
}

/// Every surface, and the reason each row is where it is.
pub(crate) const SURFACES: &[Surface] = &[
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
        hooks: &["rust-fmt", "rust-clippy", "rust-check-changed", "jscpd"],
        reached_by: "lint",
    },
    Surface {
        // NO HOOK, deliberately, and a SEPARATE row from "Rust source" above rather than a second
        // task on it - the table has one `reached_by` per row, and these two gates are the pair
        // #987 measured against 87 red PR/queue runs: 11 stale-API-page failures and 7
        // default-feature failures, both legs a Rust diff reaches that no commit hook and no
        // `SUTURA-#866`-scoped `just gates` run reaches either (`AGENTS.md`'s own line: "It does
        // not run check-default-features or check-default-feature-tests - just gates does").
        // Overlapping `*.rs` with the row above is the same all-must-run shape "workflow YAML" and
        // "text and manifests" already use.
        label: "Rust source (api-docs and default-feature lanes)",
        paths: &["*.rs"],
        hooks: &[],
        reached_by: "check-api-docs",
    },
    Surface {
        // Same pair, second half - kept as its own row rather than folded into the one above so a
        // coverage report can name which of the two ran and which did not, instead of one line
        // standing for both.
        label: "Rust source (default-feature lane)",
        paths: &["*.rs"],
        hooks: &[],
        reached_by: "check-default-features",
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
    Surface {
        // The surface the `fuzz` pre-commit hook claims: the `fuzz/` harness tree plus the crates
        // the six targets' headers name. It is a separate row from "Rust source" on purpose - the
        // hook's `files:` never inspects all `*.rs`, only this reach, so claiming the broader row
        // would report a permanent gap there. A change at the boundary of both surfaces is covered
        // when every hook claiming EACH ran, which is the same all-must-run rule.
        label: "fuzzed tree",
        paths: &[
            "fuzz/**",
            "nix/fuzz.nix",
            "nix/run-fuzz.sh",
            "crates/sutura-domain/src/query.rs",
            "crates/sutura-domain/src/model/**",
            "crates/sutura-domain/src/identity.rs",
            "crates/sutura-domain/src/identity/**",
            "crates/sutura-domain/src/catalog.rs",
            "crates/sutura-domain/src/catalog/**",
            "crates/sutura-domain/src/knowledge.rs",
            "crates/sutura-domain/src/knowledge/**",
            "crates/sutura-domain/src/pinned.rs",
            "crates/sutura-domain/src/pinned/**",
            "crates/sutura-http/src/wire.rs",
            "crates/sutura-http/src/wire/**",
            "crates/sutura-http/src/inbound/token.rs",
            "crates/sutura-http/src/inbound/keys.rs",
            "crates/sutura-http/src/inbound/keys/**",
            "crates/sutura-exec-bigquery/src/wire.rs",
            "crates/sutura-exec-bigquery/src/wire/document.rs",
            "crates/sutura-config/src/inbound.rs",
            "crates/sutura-config/src/inbound/**",
            "crates/sutura-catalog-local/**",
            "crates/sutura-sql/**",
            "crates/sutura-semantic/**",
        ],
        hooks: &["fuzz"],
        reached_by: "fuzz-smoke",
    },
    Surface {
        // The chart tree, whose own gate is `checks.helm-chart` - four legs nothing else here has:
        // `helm lint`, the no-values refusal's wording, every values file against its committed
        // golden, and `kubeconform`. A row of its own rather than an extension of "text and
        // manifests" below, for two reasons a `*.yaml` claim cannot reach: `templates/_helpers.tpl`
        // and `templates/NOTES.txt` are not YAML at all, and dprint EXCLUDES
        // `charts/*/templates/**` and `charts/*/testdata/golden/**` (`dprint.json`, pinned by
        // `xtask/tests/dprint_config.rs`) - so over the half of this tree that decides what renders,
        // `format-text` runs and inspects nothing. The overlap on `charts/**/*.yaml` is the one this
        // table permits, safe by its all-must-run rule.
        //
        // `flake.nix` is a glob here because it is where this check's `src` is chosen - `wholeTree`
        // rather than the filtered source - and where the filter's `charts` arm lives, without
        // which the directory is present in git and EMPTY to every filtered-src derivation.
        label: "Helm chart",
        paths: &["charts/**", "nix/helm-chart.nix", "nix/run-gate.sh", "flake.nix"],
        hooks: &["chart"],
        reached_by: "chart",
    },
    Surface {
        // The text this repository ships that is neither Rust nor shell: prose, the manifests, the
        // compose files and the maintenance scripts. `*.yml` and `*.yaml` OVERLAP the workflow row
        // above on purpose - `zizmor` reads a workflow for a template injection and `format-text`
        // reads it for its shape, so a workflow change is covered when both ran, which is the
        // all-must-run rule the fuzzed-tree row relies on too.
        label: "text and manifests",
        paths: &["*.md", "*.yml", "*.yaml", "*.toml", "*.py"],
        hooks: &["format-text"],
        reached_by: "lint-text",
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

    #[test]
    fn every_chart_file_that_decides_a_render_lands_on_a_hooked_row() {
        // The globs are asserted to MATCH, because a row whose paths reach nothing is a claim over
        // nothing - and the files below are the ones no other row could reach. `*.yaml`, `*.md` and
        // `*.toml` are the whole of "text and manifests", so before the `Helm chart` row a diff of
        // `templates/_helpers.tpl` alone - the file every `fail()` refusal lives in - matched no
        // surface at all: `--surface-tasks` printed nothing and the coverage report named no gap.
        //
        // NON-EMPTY hooks is the assertion rather than the row's label, because that is the whole
        // difference between a surface a commit hook reaches and one only CI does. Which hook IDs
        // are real is held next door by `unknown_hook_ids`, over the config rather than here.
        for path in [
            "charts/sutura/Chart.yaml",
            "charts/sutura/values.yaml",
            "charts/sutura/templates/_helpers.tpl",
            "charts/sutura/templates/NOTES.txt",
            "charts/sutura/templates/deployment.yaml",
            "charts/sutura/.helmignore",
            "charts/sutura/testdata/values/minimal.yaml",
            "charts/sutura/testdata/golden/minimal.yaml",
            "nix/helm-chart.nix",
        ] {
            let reached = SURFACES
                .iter()
                .any(|surface| !surface.hooks.is_empty() && crate::repo::matches_any(surface.paths, path));
            assert!(reached, "`{path}` is reached by no surface that names a hook");
        }
    }

    #[test]
    fn the_uncovered_rows_are_exactly_this_set() {
        // MOVED from `hook_coverage::tests::every_surface_the_real_config_claims_still_exists`
        // (#987 widened the set): a claim about this TABLE belongs beside it, not beside the
        // reader. The day a hook covers one of these this assertion is what says so - a row
        // claiming a hook that cannot report a gap is what `github.com/telekom/sutura#402`
        // measured on `devenv script shell`.
        let uncovered: Vec<&str> = SURFACES.iter().filter(|s| s.hooks.is_empty()).map(|s| s.label).collect();
        assert_eq!(
            uncovered,
            vec![
                "Rust source (api-docs and default-feature lanes)",
                "Rust source (default-feature lane)",
                "composite-action shell",
                "devenv script shell",
            ]
        );
    }

    #[test]
    fn a_rust_diff_surfaces_the_api_docs_and_default_feature_lanes() {
        // #987: 11 of 87 red PR/queue runs were a stale API page and 7 were a default-feature
        // failure, and neither leg is reached by a commit hook or by `just gates`'s cheaper
        // sibling - `ship-check` prints a surface's `reached_by` task only when its `hooks` is
        // EMPTY, so a Rust diff has to match a no-hook row naming each one, or `--surface-tasks`
        // stays silent about both and a developer's `ship-check` run is green over the same gap
        // that turned those 18 runs red.
        for task in ["check-api-docs", "check-default-features"] {
            let row = SURFACES
                .iter()
                .find(|surface| surface.reached_by == task)
                .unwrap_or_else(|| panic!("no surface names `{task}` as its `reached_by` task"));
            assert!(
                row.hooks.is_empty(),
                "`{task}`'s row must have no claiming hook, or ship-check never prints it"
            );
            assert!(
                crate::repo::matches_any(row.paths, "crates/sutura-domain/src/lib.rs"),
                "`{task}`'s row does not match a Rust source path"
            );
        }
    }
}
