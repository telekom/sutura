//! Which fuzz targets' crates the two local hook surfaces NAME.
//!
//! **Why this exists rather than a sentence in `check-fuzz`'s own header.** Issue #146's argument
//! (nothing would notice one arriving or disappearing) applies to the pre-commit `fuzz` hook and
//! to `hook_coverage`'s "fuzzed tree" row as surely as it does to the manifest, and #867 measured
//! it working in the losing direction: `bigquery_answer` arrived, was declared, seeded and put in
//! the workflow matrix - so `check-fuzz` passed - but `crates/sutura-exec-bigquery/` appeared in
//! NEITHER hook file, so a contributor editing the file that target most depends on
//! (`wire/document.rs`, where `decode_answer` itself lives) fired no local hook at all.
//! `check-fuzz` held three of a target's five obligations and stayed silent on the two that gate
//! the local regression half. This file IS the two ungated obligations, read at the same seams as
//! the `checks.fuzz` gate this module feeds.
//!
//! **What is correlated, and how.** For every target, this reader names the first-party crates
//! that target's source actually reaches, then requires each such crate to appear in BOTH of the
//! two local surfaces: the pre-commit `fuzz` hook's `files:` regex (`.pre-commit-config.yaml`,
//! the one thing `prek` scopes the hook by) and the `"fuzzed tree"` `Surface.paths`
//! (`xtask/src/hook_coverage/surfaces.rs`, read live by the caller and injected, so the fixtures drive both halves). A crate
//! in only one of the two is a defect as surely as one in neither: the two surfaces are the same
//! reach written in two venues, and they must not disagree about what the fuzzer inspects.
//!
//! **Where each side of the correlation comes from - nothing prose.** A target's crates are read
//! off its own `use sutura_*` lines (the very thing it compiles against), narrowed to the crates
//! the harness manifest declares as path dependencies; the surfaces are read off the two existing
//! seams rather than transcribed. This sidesteps #867's own "(2) would need to decide where a
//! target's named source paths come from": the answer is the target source itself, which cannot
//! rot the way a hand table or a prose header can - a target that stops reaching a crate drops out
//! of that target's set on its own the moment its `use` stops naming it. What the gate PROVES is
//! the direction that matters: every crate a target's source reaches is named by both hook
//! files. The reverse direction - a surface naming a crate no target reaches - is not a defect
//! this module claims, because the shared `sutura-` crates are reached by whole target SETS and
//! the surface is written once for all of them.
//!
//! **The crate universe is the manifest, not the surfaces.** [`manifest_crates`] reads each path
//! dependency out of the fuzz manifest's `[dependencies]`, so a crate a target binds to but the
//! manifest never declares cannot be BUILT by that target at all - which is `check-boundaries`'
//! second-workspace question, not this one. Deriving the universe from the surfaces instead would
//! let a surface that omits a crate also hide that the crate exists, which is exactly the blind
//! spot this gate exists to close.
//!
//! **The spelling seam.** A target writes `use sutura_sql::..`; the surfaces write
//! `crates/sutura-sql/`. [`SourceCrate`] owns the underscore->hyphen mapping, so the two spellings
//! meet in exactly one place and a third spelling cannot be invented by a caller.
//!
//! **The limit, next to the claim.** "Crate appears in the surface" is a textual containment
//! check - the string `crates/<name>` inside the hook regex, a path starting with `crates/<name>`
//! in the surface - not a regex parse, because `xtask` carries no regex dependency and runs inside
//! a nix sandbox. A surface that rewrote a crate as a non-contiguous character class would be
//! invisible to the check while still matching the paths; that is a review question, and the
//! failure mode the check DOES catch - a surface that simply omits the crate, the exact shape
//! #867 measured - is the one that actually happened.

/// A first-party crate a target reaches: its source spelling and its surface spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceCrate {
    /// As the target's `use` writes it, e.g. `sutura_sql`.
    source: &'static str,
    /// As the surfaces write it, e.g. `crates/sutura-sql`.
    tree: &'static str,
}

/// The universe of crates the harness can bind: manifest-declared path deps, as their directory
/// trees, mapped from the spelling a target `use`s. Declared here rather than read at runtime so
/// the mapping is one place; [`manifest_crates`] still READS the manifest to learn which of these
/// the harness actually declares, so a crate this list names but the manifest drops is not a
/// target's obligation.
const FIRST_PARTY: &[SourceCrate] = &[
    SourceCrate {
        source: "sutura_domain",
        tree: "crates/sutura-domain",
    },
    SourceCrate {
        source: "sutura_http",
        tree: "crates/sutura-http",
    },
    SourceCrate {
        source: "sutura_sql",
        tree: "crates/sutura-sql",
    },
    SourceCrate {
        source: "sutura_config",
        tree: "crates/sutura-config",
    },
    SourceCrate {
        source: "sutura_catalog_local",
        tree: "crates/sutura-catalog-local",
    },
    SourceCrate {
        source: "sutura_exec_bigquery",
        tree: "crates/sutura-exec-bigquery",
    },
];

/// The crate trees the fuzz manifest declares as path dependencies.
///
/// Narrowed to [`FIRST_PARTY`] so a third-party `serde_json = "1"` (its own line, no `path =`) is
/// not a fuzz obligation - only the crates under `crates/` the harness binds to are.
pub(super) fn manifest_crates(manifest: &str) -> Vec<&'static str> {
    FIRST_PARTY
        .iter()
        .filter(|crate_| {
            // No trailing slash: the manifest writes `path = "../crates/sutura-sql"`. Matching
            // the whole quoted value keeps `sutura-config` from matching a sibling prefix.
            manifest.contains(&format!("path = \"../{}\"", crate_.tree))
        })
        .map(|crate_| crate_.tree)
        .collect()
}

/// A crate a target's source reaches, found by the `use` line that binds it.
pub(super) fn source_crates(source: &str, universe: &[&'static str]) -> Vec<&'static str> {
    FIRST_PARTY
        .iter()
        .filter(|crate_| {
            universe.contains(&crate_.tree)
                && source
                    .lines()
                    .any(|line| line.trim_start().starts_with(&format!("use {}::", crate_.source)))
        })
        .map(|crate_| crate_.tree)
        .collect()
}

/// Whether the fuzz hook's raw `files:` regex names `crate_tree`.
fn regex_names(regex: &str, crate_tree: &str) -> bool {
    regex.contains(crate_tree)
}

/// Whether the "fuzzed tree" surface's path list claims `crate_tree`.
fn surface_names(paths: &[&str], crate_tree: &str) -> bool {
    paths.iter().any(|path| path.starts_with(crate_tree))
}

/// One surface omitting one crate, named in the direction a reader can fix.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Gap {
    /// The repository-relative crate directory, e.g. `crates/sutura-exec-bigquery`.
    pub(super) crate_tree: &'static str,
    /// The surface that omits it: `fuzz hook files:` or `fuzzed tree surface`.
    pub(super) surface: &'static str,
    /// The target that reaches the crate - the obligation that makes the omission a defect.
    pub(super) target: String,
}

/// A crate's two surfaces in one check: NONE means it is in neither, ONE means the reach is
/// written in one venue only.
///
/// The narrow (one-of-two) case is reported as a gap because the two surfaces must agree about
/// what the fuzzer inspects - a crate `check-changed`/prek would run the `fuzz` hook on (via the
/// regex) but `hook_coverage` would not call the `fuzz` hook for (surface missing it) is a diff
/// that fires one venue and not the other, and either half being wrong is the defect.
///
/// `targets` is the per-target crate set [`source_crates`] built from the target sources, each
/// target named by the file that declares it - the key the verdict names so a reader can find
/// the obligation in the tree the way every other `check-fuzz` line names its subject.
/// One target's reach: its name (as the gate's failures name it) and the crates its source uses.
///
/// A type alias because the pair appears in `gaps` and at every construction site in `fuzz::run`,
/// and the 1000-line cap means the reader is called from a second file.
pub(super) type Reach<'a> = (&'a str, Vec<&'static str>);

/// `fuzzed_tree` is injected (not read from the live table) so the fixtures can drive both
/// halves of the check independently - the same seam `hook_coverage`'s own `touched` draws
/// between the table and the question asked of it. The live row is passed by the caller:
/// [`crate::fuzz::run`] hands `crate::hook_coverage::surfaces::fuzzed_tree_paths()`.
pub(super) fn gaps(fuzz_hook_files: &str, fuzzed_tree: &[&str], targets: &[Reach<'_>]) -> Vec<Gap> {
    let mut gaps = Vec::new();
    let mut seen = Vec::new();
    for (target, crates) in targets {
        for crate_tree in crates {
            if seen.contains(crate_tree) {
                continue;
            }
            seen.push(*crate_tree);
            if !regex_names(fuzz_hook_files, crate_tree) {
                gaps.push(Gap {
                    crate_tree,
                    surface: "fuzz hook files:",
                    target: String::from(*target),
                });
            }
            if !surface_names(fuzzed_tree, crate_tree) {
                gaps.push(Gap {
                    crate_tree,
                    surface: "fuzzed tree surface",
                    target: String::from(*target),
                });
            }
        }
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(uses: &[&str]) -> String {
        let mut out = String::new();
        for u in uses {
            out.push_str("use ");
            out.push_str(u);
            out.push_str("::something;\n");
        }
        out
    }

    /// A controlled "fuzzed tree" path list; the live table lives on the real surfaces module
    /// and is passed by `run()`, so these fixtures drive the check by injection.
    fn surface(paths: &[&'static str]) -> Vec<&'static str> {
        paths.to_vec()
    }

    #[test]
    fn manifest_crates_keeps_only_path_dependencies() {
        let manifest =
            "[dependencies]\nlibfuzzer-sys = \"0.4\"\nsutura-sql = { path = \"../crates/sutura-sql\" }\nserde_json = \"1\"\n";
        assert_eq!(manifest_crates(manifest), vec!["crates/sutura-sql"]);
    }

    #[test]
    fn a_targets_crates_are_its_use_lines_narrowed_to_the_manifest() {
        let universe = vec!["crates/sutura-domain", "crates/sutura-sql"];
        let crates = source_crates(&source(&["sutura_domain::x", "sutura_sql::y"]), &universe);
        assert_eq!(crates, vec!["crates/sutura-domain", "crates/sutura-sql"]);
    }

    #[test]
    fn a_use_of_a_crate_not_in_the_manifest_is_not_an_obligation() {
        let universe = vec!["crates/sutura-domain"];
        let crates = source_crates(&source(&["sutura_exec_bigquery::x"]), &universe);
        assert!(
            crates.is_empty(),
            "a crate the harness cannot build is not a target's obligation"
        );
    }

    #[test]
    fn a_crate_in_both_surfaces_has_no_gaps() {
        let regex = "^(fuzz/|crates/sutura-domain/|crates/sutura-sql/)";
        let targets = vec![("sql_expression", vec!["crates/sutura-domain", "crates/sutura-sql"])];
        let tree = surface(&["crates/sutura-domain", "crates/sutura-sql"]);
        assert!(
            gaps(regex, &tree, &targets).is_empty(),
            "both crates are in both surfaces here"
        );
    }

    #[test]
    fn a_crate_in_only_one_surface_is_a_gap_naming_that_surface() {
        let regex = "^(fuzz/|crates/sutura-domain/)";
        let targets = vec![("sql_expression", vec!["crates/sutura-sql"])];
        // sql is in the SURFACE but not in the regex - a change to sutura-sql fires `hook_coverage`
        // (which would hold the surface) but not prek's own `fuzz` hook, so the reach disagrees.
        let tree = surface(&["crates/sutura-sql"]);
        assert_eq!(
            gaps(regex, &tree, &targets),
            vec![Gap {
                crate_tree: "crates/sutura-sql",
                surface: "fuzz hook files:",
                target: String::from("sql_expression")
            }],
        );
    }

    #[test]
    fn a_crate_in_neither_surface_is_reported_for_both() {
        let regex = "^(fuzz/|crates/sutura-domain/)";
        let targets = vec![("bigquery_answer", vec!["crates/sutura-exec-bigquery"])];
        let tree = surface(&[]);
        assert_eq!(
            gaps(regex, &tree, &targets),
            vec![
                Gap {
                    crate_tree: "crates/sutura-exec-bigquery",
                    surface: "fuzz hook files:",
                    target: String::from("bigquery_answer")
                },
                Gap {
                    crate_tree: "crates/sutura-exec-bigquery",
                    surface: "fuzzed tree surface",
                    target: String::from("bigquery_answer")
                },
            ],
        );
    }

    #[test]
    fn a_crate_shared_by_two_targets_is_checked_once() {
        let regex = "^(fuzz/)";
        let targets = vec![
            ("token", vec!["crates/sutura-http"]),
            ("key_set_document", vec!["crates/sutura-http"]),
        ];
        // The surface names http; only the regex omits it, so the one gap is the regex's.
        let tree = surface(&["crates/sutura-http"]);
        let http = gaps(regex, &tree, &targets)
            .iter()
            .filter(|g| g.crate_tree == "crates/sutura-http")
            .count();
        assert_eq!(http, 1, "a shared crate is one obligation, not one per target");
    }
}
