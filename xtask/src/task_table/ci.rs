//! Tasks that judge what CI AND THE HOOKS do: workflows and the jobs they gate, acceptance
//! venues, which hook runs at which stage, the devenv shells, and how a diff is classified.
//!
//! The seam is the venue rather than the tree: every row here answers *what will run, and
//! where*, reading `.github/**`, `flake.nix` and `devenv.nix` instead of the crates.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{action_shell, changes, devenv_linter, devenv_shell, hook_coverage, hooks, venues, workflows};

pub(crate) const TASKS: &[Task] = &[
    Task {
        // The identity venue map, and the one venue whose limit is a workflow property. `Prose`,
        // because the map is a `docs/*.md` page - so a prose-only pull request DEFERS this verdict
        // to the `main` push, which the plan's second table has to say.
        name: "check-venues",
        description: "every identity claim's venue states its limit, and the acceptance job holds it",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier::declared_in_programme(),
        run: venues::run,
    },
    Task {
        name: "check-workflows",
        description: "every flake output a workflow names exists",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: workflows::run,
    },
    Task {
        // Beside `check-scope` for the same reason it sits beside `check-guidance`: a claim
        // checked against the thing it claims, over a file no other gate reads. `check-scope`
        // owns the `justfile`; this one owns `.pre-commit-config.yaml`, where the tiering
        // decision lives and where deleting one block silently un-tiers it.
        name: "check-hook-tiers",
        description: "the pre-push stage runs only the security checks, and compiles nothing",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: hooks::run,
    },
    Task {
        // The third of that shape, over the one remaining file: `devenv.nix` and every module its
        // `imports` reach. What it holds is that a shell body there goes through the wrapper
        // ShellCheck reads - and it replaces two forbidden LITERALS that
        // `github.com/telekom/sutura#402` walked past six ways, because a needle enumerates one
        // spelling of one attribute in one file. `Reads::Code`: a `docs/*.md` diff can change
        // nothing it reads.
        name: "check-devenv-shell",
        description: "every devenv script body goes through the wrapper ShellCheck reads",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: devenv_shell::run,
    },
    Task {
        // The other half of `check-devenv-shell`, and standalone for `action-shell`'s reason plus
        // one of its own: it takes an ARGUMENT - the store path of a body the wrapper produced,
        // interpolated by nix at the call site - and it reads a derivation, which needs a store
        // the cheap sweep has no nix to query. That gate holds the STRUCTURE; this one reads what
        // the wrapper actually emitted, which is the half `github.com/telekom/sutura#402`'s
        // seventh escape defeated with one line and every textual gate green.
        name: "check-devenv-linter",
        description: "the devenv wrapper's checkPhase still runs bash -n and a store shellcheck; <store-path>",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: devenv_linter::run,
    },
    Task {
        // NOT `Kind::Hygiene`, and for a different reason from `check-api-docs`: it compiles
        // nothing and needs no network, but it is only half a gate. It EXTRACTS; the verdict comes
        // from `shellcheck`, which is a nix-pinned tool the cheap sweep must not require. So the
        // pairing lives in `just lint-actions` and in `ci.yml`'s workflow-analysis step, beside
        // the `actionlint` call that cannot read these files at all.
        name: "action-shell",
        description: "extract every composite action's shell into a directory, for shellcheck",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: action_shell::run,
    },
    Task {
        // Beside `classify` because it reads the same diff, and STANDALONE because it reads a
        // prek LOG - an argument, produced by a run that has already happened, which no
        // argument-free sweep can have. It exists because `just ship-check` said `green` over a
        // diff five of its ten hooks never looked at, and printed neither number.
        //
        // FAILS CLOSED where `classify` fails open, and the two directions are deliberate: that
        // gate widens what runs when it cannot read a diff; this one reports what a run covered,
        // where an unreadable diff would declare every surface untouched and every gap absent.
        name: "hook-coverage",
        description: "what a diff-scoped hook run left uninspected; --since <ref> [--log <stage>:<path>]... [--ran <task>]... [--surface-tasks]",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: hook_coverage::run,
    },
    Task {
        name: "classify",
        description: "what a diff requires; --since <ref>, or paths (fails open)",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: changes::run_classify,
    },
    Task {
        name: "changed-packages",
        description: "the cargo packages owning the given .rs paths",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: changes::run_changed_packages,
    },
    Task {
        name: "check-changed",
        description: "cargo check, narrowed to the packages that changed",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: changes::run_check_changed,
    },
];
