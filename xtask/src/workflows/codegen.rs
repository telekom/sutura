//! Does a workflow or action SET a codegen-backend environment variable? A gate that says no.
//!
//! `CARGO_UNSTABLE_CODEGEN_BACKEND` and `CARGO_PROFILE_DEV_CODEGEN_BACKEND` name Cargo's
//! codegen backend at build time - cranelift is the one this tree uses - and both are
//! nightly-only. There is ONE toolchain now: the dev shell, every gate, every build and every
//! shipped artifact run the single pinned nightly under `devco/rust-toolchain-nightly.toml`, so
//! the pinned compiler is the ONLY thing that may choose a backend. A CI step that sets one of
//! these variables would override that pinned decision for every later compile in its job, and
//! the diff reads as a plumbing detail unless a reader happens to know the variable - which is
//! the drift the whole build was just consolidated away from. So a step that NAMES a backend is
//! the only path left for an unremarked compiler-backend switch to reach CI.
//!
//! **The rule is a name refusal over text**, the `cache_scope::retired` substituter's shape: CI
//! configurations stay reviewable when *which compiler backend a build uses* is decided by the
//! toolchain a reviewer pinned, and not by a variable a step set. A step setting one of these
//! changes the codegen backend for every later compile in its job, and the diff reads as a
//! plumbing detail unless a reader happens to know the variable. Refusing it by name is the
//! stronger form of keeping CI configurations reviewable.
//!
//! # The name list, and why it is exact
//!
//! [`CODEGEN_BACKEND`] holds the EXACT variable names, deliberately, rather than a single
//! `codegen-backend` substring: a future backend env arrives under a name this list does not hold
//! concerns (say `CARGO_UNSTABLE_FOO_BACKEND`), and matching a substring would catch it only if it
//! also contained `codegen-backend`. Keeping the exact names means a new backend env FAILS to be
//! refused until somebody adds it to this list - and this module's header says that is the thing to
//! do, which is how a review stops it slipping past. Cargo recognises no other `*_CODEGEN_BACKEND`
//! env today, so the two entries are the whole of the surface.
//!
//! # What this does NOT hold, stated at the limit
//!
//! Text can see a variable NAMED on one non-comment line. A value assembled from pieces, a backend
//! reached through some other action's input, `RUSTFLAGS` or a `.cargo/config.toml` `codegen-backend`
//! key all pass unseen, and no text rule closes that - `zizmor` and review cover the rest. An
//! `env:` line is a SETTING; a line that only MENTIONS the name in prose, or a commented-out
//! variant, is not - the first is what a header explaining the rule needs, the second is how a
//! reviewer documents the decision. Comments excluded is the same load-bearing half the substituter
//! rule's is: a gate that fired on prose about itself would make its own header unwritable.

use std::path::Path;

use super::sources;

/// The codegen-backend environment variables, refused in CI outright.
///
/// Cargo's codegen-backend selection, both nightly-only. **A future backend env belongs HERE, and
/// the refusal is the mechanism that forces it to be**: matched as exact names, a new one is green
/// until this list names it, which is the failure mode the header above records.
const CODEGEN_BACKEND: [&str; 2] = ["CARGO_UNSTABLE_CODEGEN_BACKEND", "CARGO_PROFILE_DEV_CODEGEN_BACKEND"];

/// Every refusal in this module, over one repository root.
///
/// Reuses [`sources::ci_sources`] - the one walk of what CI reads - and skips the `nix/*.sh`
/// half, because the rule deliberately does not cover it: the decision is that a WORKFLOW or
/// ACTION must not name a backend. The pinned nightly toolchain is the only thing that may
/// choose a backend, and that choice lives in `devco/rust-toolchain-nightly.toml`, not in a job
/// or in a shared shell script.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let Some(sources) = sources::ci_sources(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for source in &sources {
        if source.label.starts_with("nix/") {
            continue;
        }
        out.extend(codegen_backend(&source.label, &source.text));
    }
    out
}

/// Every line of one workflow or action that SETS a codegen-backend env variable.
///
/// A LINE rule, for `cache_scope::retired::trusted_stores`'s reason: the variable can be set in an
/// `env:` mapping at any level, or inline in a `run:` shell body, and no step-shaped scanner sees
/// both. The name must carry an assignment adjacency (`:` in an `env:` map, `=` in a shell body),
/// so a prose MENTION of the name is not a refusal; a line whose trimmed head is `#` is excluded
/// before that runs, so a commented-out or documented variant is not either.
fn codegen_backend(label: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with('#') {
            continue;
        }
        for name in CODEGEN_BACKEND {
            if sets(code, name) {
                out.push(format!(
                    "{label}:{}  sets `{name}` - a codegen-backend environment variable, nightly-only, and a workflow or action must not select the compiler backend itself: CI stays reviewable when the toolchain a reviewer pinned is the only thing that chooses one. A future backend env belongs in this refusal's list, not in CI",
                    index.saturating_add(1)
                ));
            }
        }
    }
    out
}

/// Whether one non-comment line SETS `name` as an environment variable.
///
/// The name opens a whitespace-separated token and is followed by `:` or `=`. Requiring the
/// adjacency is what keeps a bare mention - prose, or a `${{ env.NAME }}` read - from firing, and
/// it is prefix-safe: a sibling variable whose name merely STARTS with this one is not caught,
/// which is the direction [`CODEGEN_BACKEND`]'s exact-name limit depends on.
fn sets(line: &str, name: &str) -> bool {
    line.split_whitespace().any(|token| {
        token
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with(':') || rest.starts_with('='))
    })
}

#[cfg(test)]
mod tests {
    /// The two exact names, borrowed so the rows below stay in step with the refusal.
    fn names() -> &'static [&'static str] {
        &super::CODEGEN_BACKEND
    }

    #[test]
    fn setting_a_codegen_backend_env_variable_is_refused_by_name() {
        // One fixture per spelling of a SET. The `env:` mapping at workflow, job and step level,
        // and the inline shell form a `run:` body reaches for - all of them survive a step-shaped
        // scan, and all must name the variable.
        for (why, line) in [
            ("an env: mapping key", "      CARGO_UNSTABLE_CODEGEN_BACKEND: cranelift\n"),
            (
                "a sibling env: mapping key",
                "      CARGO_PROFILE_DEV_CODEGEN_BACKEND: cranelift\n",
            ),
            (
                "an inline shell assignment",
                "        run: CARGO_UNSTABLE_CODEGEN_BACKEND=cranelift cargo build\n",
            ),
            (
                "an inline shell export",
                "          export CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift\n",
            ),
            (
                "a step env: block",
                "      - env:\n          CARGO_UNSTABLE_CODEGEN_BACKEND: cranelift\n",
            ),
        ] {
            let found = super::codegen_backend("ci.yml", line);
            assert!(!found.is_empty(), "not refused - {why}: {line}");
            let problem = found.first().expect("the refusal");
            let named = names().iter().find(|name| problem.contains(**name));
            assert!(named.is_some(), "the refusal must name the variable: {problem}");
            assert!(problem.contains("codegen-backend environment variable"), "{problem}");
        }
    }

    #[test]
    fn a_clean_workflow_sets_none_and_passes() {
        // THE OTHER DIRECTION, and it is what keeps this from passing by refusing everything: a
        // workflow that carries a toolchain job and no backend variable is untouched.
        let clean = concat!(
            "jobs:\n",
            "  ci:\n",
            "    steps:\n",
            "      - uses: actions/checkout@aaaa # v7\n",
            "      - run: nix run .#xtask check-workflows\n",
        );
        assert!(super::codegen_backend("ci.yml", clean).is_empty(), "{clean}");
        for name in names() {
            assert!(!super::sets(clean, name), "{name} must not be seen");
        }
    }

    #[test]
    fn a_mention_in_prose_or_a_comment_is_not_a_setting() {
        // Comment-lines are excluded outright, and a prose line that only MENTIONS the name is
        // refused by the adjacency half of `sets`. Both directions are load-bearing: a header that
        // explains the rule, and a comment that documents the decision, would otherwise make the
        // gate refuse its own explanation - the substituter rule's documented trap.
        for line in [
            "          # CARGO_UNSTABLE_CODEGEN_BACKEND selects the backend, and CI names none\n",
            "          # CARGO_PROFILE_DEV_CODEGEN_BACKEND was weighed and refused\n",
            "          CARGO_UNSTABLE_CODEGEN_BACKEND is documented in the gate's header\n",
            "          ${{ env.CARGO_PROFILE_DEV_CODEGEN_BACKEND }}\n",
        ] {
            let found = super::codegen_backend("ci.yml", line);
            assert!(found.is_empty(), "over-fired on: {line}\n{found:#?}");
        }
    }

    #[test]
    fn the_committed_tree_names_no_codegen_backend_env_in_a_workflow_or_action() {
        // THE LIVE ASSERTION, and the one that goes red if a step anywhere in CI ever sets a
        // backend. Scoped to workflows and actions, which is what the rule refuses; `nix/*.sh` is
        // deliberately outside it.
        let Some(root) = crate::repo::root() else { return };
        let problems = super::problems(&root);
        assert!(problems.is_empty(), "{problems:#?}");
    }
}
