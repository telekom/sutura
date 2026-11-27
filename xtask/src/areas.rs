//! The area table [`classify`](super::classify) matches paths against - its own file because
//! `xtask/src/changes.rs` sits within a few lines of the 1000-line cap `cargo xtask max-lines`
//! enforces (`xtask/` is unexemptable there), and this table is the single largest thing in it.
//!
//! `#[path]` keeps this a sibling of `changes.rs`, exactly as `xtask/src/affected.rs` is: a peer
//! of the classification code, not a step of it.

use super::{DEPS_CLOSURE_INPUTS, DEPS_PATTERNS};

/// One area of the repo, and what depends on it.
pub(super) struct Area {
    /// Output name. CI gates steps on these.
    pub(super) name: &'static str,
    /// Glob patterns, matched by [`repo::matches`](crate::repo::matches).
    pub(super) patterns: &'static [&'static str],
    /// Areas that must also run when this one matches. A dependency edge, not a category:
    /// changing the build definition means the build must run even if no Rust changed.
    pub(super) consumers: &'static [&'static str],
}

/// Every area, and the reason each pattern is where it is.
pub(super) const AREAS: &[Area] = &[
    Area {
        // The code, the manifests, the lint config and the compiler pin: anything that can
        // change what `cargo` produces or what it rejects.
        //
        // `examples/**` is here because THE TESTS READ IT. `checks.nextest` runs against the
        // unfiltered tree precisely so those fixtures are visible, and
        // `crates/sutura-cli/tests/example.rs` reads all of `examples/single-player` - its catalog,
        // its questions AND the numbers its README prints, which is the point of that suite. The
        // catalog is a directory of markdown documents whose `measure:` decides what SQL we
        // generate, so without this pattern `DOCS_ONLY` below swallows a changed measure and every
        // Rust gate is skipped over it. The same reason an area match is tested before `DOCS_ONLY`.
        //
        // `ci.yml`'s path filter re-includes the same documents, and BOTH gates have to agree: the
        // filter decides whether a runner starts, this decides whether the tests run once it has.
        name: "rust",
        patterns: &[
            "crates/**",
            "examples/**",
            "xtask/**",
            "Cargo.toml",
            "Cargo.lock",
            "clippy.toml",
            "rust-toolchain.toml",
            ".cargo/**",
            // Read by Rust tests; the census in `affected/mutation_tests.rs` refuses a missing one.
            "docs/getting-started.md",
            "docs/examples/single-player.md",
            "docs/serving.md",
            "docs/serving-behind-a-gateway.md",
        ],
        consumers: &["build"],
    },
    Area {
        // The build definition itself. A flake edit can break the release without touching
        // a line of Rust, so it pulls in the full chain.
        name: "nix",
        patterns: &["flake.nix", "flake.lock", "devenv.nix", "devenv.yaml", "devenv.lock"],
        consumers: &["rust", "build"],
    },
    Area {
        // Supply-chain policy and the lock it judges. NOT "the dependency derivation moved" -
        // that is `deps_closure` below, a different axis with different patterns. Review found
        // `pr-cache` (`ci.yml`) reading this one for that question: a `crates/*/Cargo.toml`
        // feature edit, `rust-toolchain.toml`, `.cargo/config.toml` and `flake.{nix,lock}` all
        // move `.#deps` while classify wrote `deps=false` for every one of them.
        //
        // The two locks `cargo deny` reads beside the root one: `fuzz/`'s own and the vendored
        // ClickHouse driver's (`nix/deny-clickhouse-driver.sh`), each with the manifest it
        // resolves. `check-workflows` holds `.github/workflows/security-audit.yml`'s path lists to
        // `DEPS_PATTERNS`, so a path dropped from either is refused.
        name: "deps",
        patterns: &DEPS_PATTERNS,
        consumers: &[],
    },
    Area {
        // #980 review: EXACTLY the inputs `nix-store-cache`'s own `primary-key` hashes for the
        // dependency generation (`workflows::cache_scope::KEY_INPUTS`, moved here so the two
        // stay one list rather than two that can drift) - so `pr-cache`'s skip is gated on the
        // same property the cache key already keys on: did `.#deps` move.
        name: "deps_closure",
        patterns: &DEPS_CLOSURE_INPUTS,
        consumers: &[],
    },
    Area {
        // The release artifacts.
        name: "build",
        patterns: &["Dockerfile", "compose*.yaml", "compose*.yml"],
        consumers: &[],
    },
    Area {
        // The API reference, which is GENERATED from the library crates' doc comments and committed
        // nowhere. `cargo xtask check-api-docs` generates it into a temporary directory and scans
        // it; this area is what decides when that runs. It needs its own area rather than riding on `rust`
        // because it needs the NIGHTLY toolchain, so it is the one gate CI cannot fold into
        // the others.
        //
        // `crates/*/src/**` and not one crate name: the gate DERIVES the library crates from
        // `cargo metadata`, and naming a crate here would be a second list to keep in step
        // with it. The pattern is deliberately a SUPERSET - it matches the binary crates too,
        // whose sources cannot change a page - because the error directions are not
        // symmetrical. An unnecessary run generates clean pages and passes; a missing
        // pattern is a doc comment that breaks the publish run after merge.
        //
        // `Cargo.toml` is here for FEATURES and DEPENDENCIES: the pages are generated with
        // `--all-features`, so adding a feature or a dependency can add a public re-export
        // without touching a doc comment. It is NOT here for the version any more - the page
        // stopped printing it, precisely because that made a release bump stale every page
        // (see the note in `docs/.tools/rustdoc_to_markdown.py`).
        //
        // The generator is here too: it decides what a generation produces, and `DOCS_ONLY` below
        // would otherwise read a changed renderer as needing nothing at all. An area match is
        // tested before `DOCS_ONLY`, which is what makes that work. (`ci.yml` additionally ignores
        // `docs/**` for the purpose of STARTING a run, so in CI this pattern bites on a change that
        // also touches something else. It still bites in the hooks and in a local `classify`.)
        //
        // The pages are NOT here: none is committed, so a path under `docs/api/` is a local
        // preview that `.gitignore` keeps out of every diff.
        //
        // NOT here: `devco/rust-toolchain-nightly.toml`. It sets rustdoc's `format_version` and so
        // can change every page - but it matches no area today, which means it fails open to
        // `run_all`, and `run_all` already subsumes this area. Adding it here would NARROW
        // that to `api` alone, which is strictly less checking.
        name: "api",
        patterns: &[
            "crates/*/src/**",
            "Cargo.toml",
            "docs/.tools/rustdoc_to_markdown.py",
            // The gate's own source, so a change to it is judged by running it.
            "xtask/src/api_docs.rs",
        ],
        consumers: &[],
    },
    Area {
        // The chart and what `checks.k3s-autoscaling` and `checks.k3s-worker-pool` install it with:
        // the pinned helm, the image, the operator that sizes the pool and the example it serves.
        // Those VM tests are the one job this area starts. `rust` follows
        // because `crates/sutura-config/tests/chart_renders.rs` and `xtask/tests/chart_gate.rs` read
        // `charts/`. The `Chart` step and `kind-smoke` run on every event, so neither needs it.
        name: "chart",
        patterns: &[
            "charts/**",
            "examples/single-player/**",
            "nix/k3s-autoscaling.nix",
            "nix/k3s-worker-pool.nix",
            "nix/helm-chart.nix",
            "nix/oci.nix",
            "crates/sutura-operator/**",
        ],
        consumers: &["rust"],
    },
];

/// Paths that cannot affect a build. Deliberately short: everything else is either an area
/// or unclassified, and unclassified fails open.
///
/// `slides/**` is the meetup deck's sources: `checks.slides` builds them with nix and `docs.yml`
/// starts on the same paths, so no cargo gate reads them. `nix/slides.nix` needs no line: `nix/**`
/// already lands it in `deps_closure`, which sets no `rust` and only starts the dependency-cache job
/// (a limit - that job still runs for a deck-only diff). `docs/.tools/slides.py` sits under `docs/**`.
pub(super) const DOCS_ONLY: &[&str] = &[
    "**/*.md",
    "docs/**",
    "slides/**",
    "LICENSE",
    "NOTICE",
    ".gitattributes",
    ".gitignore",
];

/// CI configuration changes the pipeline itself, so nothing may be skipped on the strength
/// of a classification the change may have just altered.
pub(super) const RUN_ALL_PATTERNS: &[&str] = &[
    ".github/**",
    ".pre-commit-config.yaml",
    // This file and `changes.rs` itself. A commit that narrows `AREAS` would otherwise be the
    // first thing judged by the narrowed table, which is the same mistake as the two above.
    "xtask/src/changes.rs",
    "xtask/src/areas.rs",
    // `changes::touches_fuzzed_tree` reads the "fuzzed tree" row here to set `fuzz_compile`
    // (#2095) - a table that narrows what selects `ci.yml:fuzz-compile` must not judge its own
    // narrowing diff by the row it just shrank.
    "xtask/src/hook_coverage/surfaces.rs",
];
