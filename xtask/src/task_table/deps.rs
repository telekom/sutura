//! Tasks that judge what the build PULLS IN and what it ships: pins, platform, features,
//! unused dependencies, the binaries declared, and the major version of a shared wire type.
//!
//! The seam is the dependency graph rather than the source: these rows read manifests, lockfiles
//! and nix, and a change to them is a supply-chain change rather than a refactor.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{
    arrow_major, default_feature_tests, default_features, feature_remedies, nix_platform, pins, shipped, unused_deps, warm_start,
};

pub(crate) const TASKS: &[Task] = &[
    Task {
        name: "check-pins",
        description: "no tool is pinned by both nix and pixi",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: pins::run,
    },
    Task {
        // Beside `check-pins` because the subject is the same tree of nix expressions, read as
        // text. It exists because a deprecated `stdenv.is<Platform>` STILL EVALUATES: nixpkgs
        // warns and carries on, so one site survived here while every other had moved and nothing
        // failed. `Reads::Code`: no `docs/*.md` diff can change a nix file.
        name: "check-nix-platform",
        description: "no nix file reads a platform predicate off the deprecated stdenv alias",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            seeds: &[],
            in_scope: Some("nix/platform.nix"),
        },
        run: nix_platform::run,
    },
    Task {
        // Beside `check-pins` because it is the same shape of gate: two files, read as text
        // rather than evaluated, one value that has to be the same in both.
        name: "check-warm-start",
        description: "the warm start's directory, stamp, profile and sweep agree with what reads them",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: warm_start::run,
    },
    Task {
        name: "unused-deps",
        description: "every declared dependency is actually used",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: unused_deps::run,
    },
    Task {
        name: "check-arrow",
        description: "one Arrow major in Cargo.lock, or an explained exception",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: arrow_major::run,
    },
    Task {
        // Beside `check-shared-client` because it is the same shape again: one declaration, read
        // as text, and every place that had to spell it a second time. Here the declaration is
        // `nix/shipped.nix`'s `binaries` list and the copies are `BINARIES` in two workflows and
        // an input default in two composite actions - which cannot be derived from it, because a
        // matrix takes literals and a job cannot evaluate a flake before installing nix.
        //
        // `Reads::Prose` because its second rule reads `docs/**` - the documented `cargo build
        // --features` a `probeFeatures` entry has to cover. Declared here rather than left at
        // `Code`, which is the failure `docs/implementation-plan-identity-and-services.md` records
        // for `check-crap`: a gate whose inputs grew into `docs/` while its classification did not.
        name: "check-shipped-binaries",
        description: "every release-path binary literal equals nix/shipped.nix, and every documented feature build is probed",
        kind: Kind::Hygiene(Reads::Prose),
        falsifier: Falsifier::declared_in_programme(),
        run: shipped::run,
    },
    Task {
        // Beside `check-shipped-binaries` because it reads the same declaration, and STANDALONE
        // rather than hygiene for `check-attribution`'s reason: it invokes cargo, so it
        // needs a resolvable registry and a target directory the nix sandbox has not got, so `just
        // gates` is its caller. The lane it covers is the one every other compiling gate is blind
        // to; CI reaches it as `nix run .#default-features` inside the one required job, and it
        // takes no argument there - the profile is derived, not passed. The module's header says why.
        name: "check-default-features",
        description: "every shipped package compiles and lints at cargo's default features",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: default_features::run,
    },
    Task {
        // The other half of the line above, and it is a separate task because the two cost
        // different amounts: that one stops at metadata, this one links and RUNS. Standalone for
        // the same reason, and it reads the same declaration. What it closes is a whole category
        // of test that was compiled by that gate and executed by no venue at all - the module's
        // header carries the count and the measurement.
        name: "check-default-feature-tests",
        description: "every shipped package runs its tests at cargo's default features",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: default_feature_tests::run,
    },
    Task {
        // Beside `check-refusal-coverage` because it is the other half of the same subject: that
        // one asks whether a refusal is named, this one whether the REMEDY it prints can be
        // acted on. `github.com/telekom/sutura#246` made a gate's own remedy resolve; this is the
        // same rule where the claim is about the manifest rather than about the justfile.
        name: "check-feature-remedies",
        description: "a refusal that says to rebuild names a feature the crate declares",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier::declared_in_programme(),
        run: feature_remedies::run,
    },
];
