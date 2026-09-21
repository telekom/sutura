//! Tasks that judge what the build PULLS IN and what it ships: pins, platform, features,
//! unused dependencies, the binaries declared, and the major version of a shared wire type.
//!
//! The seam is the dependency graph rather than the source: these rows read manifests, lockfiles
//! and nix, and a change to them is a supply-chain change rather than a refactor.

use crate::registry::{Falsifier, Kind, Reads, Task};
use crate::{
    arrow_major, default_feature_tests, default_features, feature_remedies, nix_platform, pins, shipped, unused_deps,
    vendor_count, vendor_expiry, warm_start,
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
        // Beside `check-arrow` for the same reason: a comparison between a named row and the
        // named files repeating its number, the same shape as the allowlist-versus-lock check
        // one row up. `Reads::Code`, per that payload's own rule: `VENDOR.md`, `Cargo.toml` and
        // `REUSE.toml` are prose, but none of them sits under `docs/`, and a `docs/*.md` diff
        // cannot change any of the three - "prose OUTSIDE docs/ is on this side too".
        name: "check-vendor-count",
        description: "VENDOR.md's mimalloc row cardinal matches its own enumeration and Cargo.toml/REUSE.toml's repeated count",
        kind: Kind::Hygiene(Reads::Code),
        falsifier: Falsifier {
            // A genuine cardinal-versus-enumeration mismatch (2 said, 1 enumerated) - the
            // own-rule seed `registry::Falsifier`'s doc asks for. Seeding `VENDOR.md` ALONE
            // was reviewed and measured wrong: `falsifier_tree`'s own `Cargo.toml` carries no
            // mimalloc mention and writes no `REUSE.toml` at all, so BOTH repeaters land on the
            // absent-input floor (`SiteFinding::Absent`) rather than the substantive comparison
            // - measured, deleting the comparison outright left the sweep's own
            // `Verdict::Fail` assertion PASSING, exactly the masked-refusal shape
            // `github.com/telekom/sutura#371` exists to catch. Seeding a matching-but-wrong
            // cardinal into both repeaters removes the mask: each reads a real value (two), so
            // the row's own mismatch is the only thing left standing between this tree and a
            // clean bill.
            seeds: &[
                (
                    "VENDOR.md",
                    "| `vendor/mimalloc_rust/**` | up | MIT | abc | 2026-01-01 | Two local changes. (1) one thing. |\n",
                ),
                ("Cargo.toml", "[workspace]\nmembers = []\n# two local changes\n"),
                ("REUSE.toml", "# two local changes\n"),
            ],
            in_scope: Some("VENDOR.md"),
        },
        run: vendor_count::run,
    },
    Task {
        // Beside `check-vendor-count` because the subject is the same vendored trees, and
        // `Kind::Standalone` because it needs EGRESS. `just validate`'s nix checks are hermetic
        // and must stay that way, and a gate that reddens whichever branch is open at the moment
        // upstream publishes is worse than the rot it closes - the finding is about the
        // repository, not about that diff. `just update` is its caller, which is where somebody
        // is already deciding about versions. The hermetic half - `devco/vendor-expiry` names
        // every `vendor/` child and nothing else - is a rule inside `check-workflows`, so a new
        // vendored tree with no expiry declaration fails there rather than here.
        name: "check-vendor-expiry",
        description: "no vendored crate has a newer release published than the version it was vendored at",
        kind: Kind::Standalone,
        falsifier: Falsifier::declared_in_programme(),
        run: vendor_expiry::run,
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
