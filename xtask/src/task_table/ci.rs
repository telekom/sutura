//! Tasks that judge what CI AND THE HOOKS do: workflows and the jobs they gate, acceptance
//! venues, which hook runs at which stage, the devenv shells, and how a diff is classified.
//!
//! The seam is the venue rather than the tree: every row here answers *what will run, and
//! where*, reading `.github/**`, `flake.nix` and `devenv.nix` instead of the crates.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{action_shell, changes, devenv_linter, devenv_shell, hook_coverage, hooks, pr_title, venues, workflows};

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
        falsifier: Falsifier {
            // A tree that satisfies every `check_gates` sub-rule - the contexts, obligations,
            // cache anchor, SAST stand-in, cross matrix, image-record grammar and action-input
            // record - while carrying a `nix run .#nonexistent` reference in a CI source file.
            // The own rule at the end of `run` is the only arm that fires: `nonexistent` is not in
            // the flake's runnable set, so `missing` is non-empty and the verdict is `Fail`.
            // `nix/run-gate.sh` also carries `nix build .#checks.x86_64-linux.clippy` for
            // `sast::invokes_check` and the six declared image-record lines for `image_records`.
            seeds: &[
                (
                    "flake.nix",
                    "{\n  apps.xtask = { type = \"app\"; };\n\n  packages = {\n    xtask = { };\n  };\n\n  checks = {\n    clippy = { };\n    reuse = { };\n  };\n\n  cargoClippyExtraArgs = [ \"-D warnings\" ];\n}\n",
                ),
                (
                    ".github/workflows/ci.yml",
                    "on:\n  pull_request:\njobs:\n  ci:\n    if: ${{ !startsWith(github.event.head_commit.message || '', 'chore(release):') }}\n    steps:\n      - name: Secrets\n        if: ${{ always() && github.event_name != 'push' }}\n        run: echo secrets\n      - name: Licensing\n        if: ${{ !cancelled() && github.event_name != 'push' }}\n        run: echo licensing\n      - name: Chart\n        if: ${{ !cancelled() && github.event_name != 'push' }}\n        run: echo chart\n      - name: PR title\n        if: ${{ github.event_name == 'pull_request' }}\n        run: echo pr-title\n      - uses: nix-community/cache-nix-action@0123456789abcdef0123456789abcdef01234567\n        save: ${{ github.event_name == 'push' && github.ref == 'refs/heads/main' }}\n        with:\n          primary-key: flake.lock-Cargo.lock-rust-toolchain.toml-flake.nix-nix/**-.cargo/config.toml-**/Cargo.toml\n          restore-prefixes-first-match: |\n            flake.lock-Cargo.lock-rust-toolchain.toml-flake.nix-nix/**-.cargo/config.toml-**/Cargo.toml\n            flake.lock\n  bigquery-driver-check:\n    if: ${{ !startsWith(github.event.head_commit.message || '', 'chore(release):') }}\n    steps:\n      - run: echo bq\n  ci-aggregate:\n    steps:\n      - run: echo aggregate\n        env:\n          CI_RESULT: ${{ needs.ci.result }}\n          KC_RESULT: ${{ needs.keycloak-served-test.result }}\n          KC_SELECTED: ${{ needs.ci.outputs.data_source_bigquery }}\n          BQ_RESULT: ${{ needs.bigquery-driver-check.result }}\n          BQ_SELECTED: ${{ needs.ci.outputs.data_source_bigquery }}\n          E2E_RESULT: ${{ needs.e2e-datahub-adbc.result }}\n          E2E_REQUIRED: ${{ needs.ci.outputs.data_source_bigquery }}\n          ORACLE_RESULT: ${{ needs.oracle-tier.result }}\n          ORACLE_SELECTED: ${{ needs.ci.outputs.data_source_oracle }}\n",
                ),
                (
                    ".github/workflows/cross-link.yml",
                    "on: workflow_call\njobs:\n  link:\n    strategy:\n      matrix:\n        target:\n          - aarch64-unknown-linux-gnu\n          - x86_64-unknown-linux-musl\n    steps:\n      - run: echo link\n",
                ),
                (
                    ".github/workflows/cachix-push.yml",
                    "on:\n  push:\n    branches:\n      - main\njobs:\n  cross-build:\n    strategy:\n      matrix:\n        target:\n          - aarch64-unknown-linux-gnu\n          - x86_64-unknown-linux-musl\n    steps:\n      - run: echo build\n",
                ),
                (
                    ".github/workflows/release.yml",
                    "on: push\njobs:\n  build:\n    strategy:\n      matrix:\n        target:\n          - x86_64-unknown-linux-gnu\n          - aarch64-unknown-linux-gnu\n          - x86_64-unknown-linux-musl\n          - aarch64-unknown-linux-musl\n    steps:\n      - run: echo build\n",
                ),
                (
                    ".github/workflows/release-performance.yml",
                    "on: push\njobs:\n  build:\n    if: ${{ github.event_name == 'workflow_dispatch' }}\n    steps:\n      - run: |\n          nix build .#sutura-x86_64-unknown-linux-gnu-performance\n          nix build .#sutura-aarch64-unknown-linux-gnu-performance\n          nix build .#sutura-x86_64-unknown-linux-musl-performance\n          nix build .#sutura-aarch64-unknown-linux-musl-performance\n",
                ),
                (
                    "nix/run-gate.sh",
                    "#!/usr/bin/env bash\nnix run .#nonexistent\nnix build .#checks.x86_64-linux.clippy\necho \"# header\" > dist/image-digests.txt\ngrep '^leaf ' dist/image-digests.txt\ndigests-file:\ndefault: dist/image-digests.txt\nIMAGE_DIGESTS: ${{ inputs.digests-file }}\ncat dist/image-digests.txt\nif [ -f dist/image-digests.txt ]; then\nnix run .#xtask -- collect-provenance subjects.sha256 \"$IMAGE_DIGESTS\" \\\n",
                ),
                (
                    "REUSE.toml",
                    "SPDX-PackageDownloadLocation = \"https://github.com/test/repo\"\n",
                ),
                ("README.md", "# repo\n"),
                (
                    "devco/required-contexts",
                    "[required]\nci\n\n[advisory]\nci.yml:bigquery-driver-check\nci.yml:ci-aggregate\n",
                ),
                (
                    "docs/adr/0025-what-a-scorecard-zero-says-about-this-repository.md",
                    "# ADR 0025\n\nScorecard's SAST zero is accepted.\n",
                ),
                (
                    "docs/adr/0026-no-third-party-binary-cache.md",
                    "# ADR 0026\n\nNo third-party binary cache.\n",
                ),
                (
                    "devco/action-inputs",
                    "nix-community/cache-nix-action@0123456789abcdef0123456789abcdef01234567\n  primary-key\n  restore-prefixes-first-match\n",
                ),
            ],
            in_scope: Some("nix/run-gate.sh"),
        },
        run: workflows::run,
    },
    Task {
        // The gate's own remedy names this: `with_keys::problems` refuses an action pinned to a
        // sha `devco/action-inputs` does not name, and cannot fetch a manifest to fix that itself
        // - it runs inside `checks.hygiene`, a network-less nix sandbox. `Standalone` because it
        // takes an argument and reaches the network, neither of which the `hygiene` sweep allows.
        name: "refresh-action-inputs",
        description: "file a pinned action's declared inputs into devco/action-inputs; <owner>/<repo>@<sha>",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: workflows::action_manifest::run,
    },
    Task {
        // Beside `check-scope` for the same reason it sits beside `check-guidance`: a claim
        // checked against the thing it claims, over a file no other gate reads. `check-scope`
        // owns the `justfile`; this one owns `.pre-commit-config.yaml`, where the tiering
        // decision lives and where deleting one block silently un-tiers it.
        name: "check-hook-tiers",
        description: "the pre-push stage runs only the security checks, and compiles nothing",
        kind: Kind::Hygiene(Reads::Code),
        // A real own-rule seed, not the placeholder: `default_install_hook_types:` drops
        // `commit-msg` while the push stage stays security-only, so `decide_install_types` is
        // the only thing that can fail this tree - `decide` alone would pass it. Without this,
        // the falsifier tree carries no `.pre-commit-config.yaml` at all, and `run` refuses on
        // the missing-file arm before either rule is reached - the bare exit code says nothing
        // about the two rules this gate exists for, exactly `telekom/sutura#371`'s residue.
        falsifier: Falsifier {
            seeds: &[(
                hooks::CONFIG,
                "default_install_hook_types: [pre-commit, pre-push]\ndefault_stages: [pre-commit]\nrepos:\n  - repo: local\n    hooks:\n      - id: secret-sweep\n        entry: bash nix/run-gate.sh secrets\n        stages: [pre-push]\n      - id: cargo-deny\n        entry: bash nix/run-gate.sh supply-chain\n        stages: [pre-push]\n",
            )],
            in_scope: Some(hooks::CONFIG),
        },
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
        falsifier: Falsifier {
            // The wrapper and one routed body satisfy the builder and discovery floors. The
            // second script body bypasses the wrapper.
            seeds: &[(
                "devenv.nix",
                "{ pkgs, ... }:\nlet\n  linted = name: bashOptions: text:\n    pkgs.writeShellApplication { name = \"sutura-${name}\"; inherit bashOptions text; extraShellCheckFlags = [ \"-x\" ]; };\n  runs = name: body: \"${linted name [ \"errexit\" ] body}/bin/sutura-${name}\";\nin\n{\n  scripts.good.exec = runs \"good\" \"echo good\";\n  scripts.bad.exec = \"echo unchecked\";\n}\n",
            )],
            in_scope: Some("devenv.nix"),
        },
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
        // The venue is the seam, as everywhere in this area: the subject that lands on `main` is
        // composed from the PULL-REQUEST TITLE at merge time, so the only place it can be read is
        // a run that has the event. `Standalone` because it takes that title as an argument - the
        // `hygiene` sweep is argument-free and has no pull request to ask about.
        name: "check-pr-title",
        description: "the title the queue will squash into main's subject keeps the commit vocabulary; <title>",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: pr_title::run,
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
