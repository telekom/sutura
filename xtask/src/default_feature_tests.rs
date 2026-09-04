//! Every package that SHIPS *runs* its tests at the feature set it ships with.
//!
//! **A whole category of test was compiled by a gate and executed by none, which is the worst of
//! the two states a `#[test]` can be in.** `check-default-features` next door compiles the shipped
//! lane and stops at metadata - `cargo check --all-targets` and `cargo clippy --all-targets` both
//! do - while every venue in this repository that RUNS a test passes `--all-features`: `just test`,
//! `just serve-e2e`, `just mcp-e2e`, `just declared-source` and the `nextest` nix check alike. So a
//! `#[cfg(not(feature = "..."))]` test was compiled by the first set and excluded by the second. It
//! reads as coverage in a diff and holds nothing: a refusal that stopped refusing - the wrong
//! message, the wrong remedy, no refusal at all - would have been caught by no venue at all.
//!
//! **MEASURED rather than reasoned about**, by listing both feature sets and differencing them
//! (2026-09-04, `cargo nextest list --workspace` against `cargo nextest list --workspace
//! --all-features`, this workspace):
//!
//! | Tests at the default set | Tests at `--all-features` | Only at the default set | Only at `--all-features` |
//! | --- | --- | --- | --- |
//! | 1718 | 1813 | 2 | 97 |
//!
//! The two in the third column are `sutura-serve`'s
//! `a_bigquery_source_is_refused_by_a_build_that_did_not_link_the_adapter` and `sutura-cli`'s
//! `a_kind_this_build_did_not_link_is_refused_by_name_and_says_what_to_build`, and they were the
//! whole population: every other `#[cfg(not(feature = ...))]` item in the tree is production code,
//! which `check-default-features` does compile. The fourth column is the mirror image and the reason
//! the split is not an accident - 97 tests exist only with a feature ON, and every venue reaches
//! those. The counts are what make this a CATEGORY rather than a pair: a third such test would have
//! joined the dead zone in silence.
//!
//! **Same package list, same declaration, one owner.** The packages are `nix/shipped.nix`'s
//! `binaries` block, read by [`shipped_packages`], which is the parser `check-default-features`
//! already uses - so a binary added there is covered here without anybody remembering, and a rename
//! fails rather than silently dropping out. FAIL CLOSED on parsing none, for that gate's reason: a
//! parser that silently sees half a file is worse than no parser.
//!
//! **ONE INVOCATION PER PACKAGE, and that is the shipped configuration rather than a style.**
//! `nix/shipped.nix` builds each binary with `--package <p>` alone, so features unify across that
//! package's graph and no other. A single `nextest run` naming both packages resolves them together,
//! which can enable a feature neither ships with - a different configuration, checked instead of the
//! one that is published. **The limit of that claim**: the package's own feature SELECTION is the
//! release's, which is what both refusal tests turn on, but a test run also resolves
//! DEV-dependencies (`sutura-serve`'s pull `sutura-dev` with `mock-issuer`) and a release derivation
//! has no dev-dependency graph at all, so unification over a shared dependency can differ from what
//! is published. What this runs is the shipped SELECTION, not a byte-identical build of the artefact.
//!
//! **AN EMPTY RUN IS RED**, because a lane that selects nothing and reports success is the defect
//! this gate exists to end, one level up. `--no-tests fail` is passed rather than inherited, and the
//! limit next to that claim is measured: nextest 0.9.143's own default (`auto`) exits 4 on an empty
//! selection **too**, so this pins a default rather than changing behaviour today. It is written
//! down anyway because a default is not a mechanism - `auto`'s documented job is to *determine* the
//! behaviour, and `NEXTEST_NO_TESTS` in an environment would override it silently.
//!
//! The consequence to read before adding a binary: a shipped package with no test that runs at its
//! default features fails this gate, and that is the direction wanted here - such a package is
//! exactly the hole that was invisible before.
//!
//! **The profile is DERIVED from the target directory, not passed as a flag.** CI reaches this
//! through a flake app that unpacks the dependency closure `checks.nextest` built minutes earlier in
//! the same job; cargo keys artifacts per profile, so compiling there at anything but the profile
//! those artifacts were built at reuses none of them - it rebuilds the whole closure, passes, and
//! nobody attributes the minutes to it. A flag would be one more thing to get wrong in a place where
//! being wrong is silent - `check-warm-start` holds that same seam for the flake apps, and its own
//! test records nextest's `--profile` being taken for cargo's as a measured failure rather than a
//! hypothetical one. So [`profile_for`] reads the stamp `cargoWarmStart` leaves behind - the
//! mechanism, not the directory's name - and `just gates` stays on the developer's default profile
//! with no argument at all.
//!
//! **The two literals the profile is derived from belong to `check-warm-start`.** [`STAMP`] and
//! [`WARM_PROFILE`] are read from `warm_start`, and so is the pair of facts that makes reading them
//! sound - the warmer writes the stamp into the directory it exports, and `flake.nix` builds the
//! artifacts at that profile. Both were `contains` assertions in this module's test module, where a
//! commented-out write satisfied one and a stamp moved out of the exported directory satisfied it
//! anyway; they are a hygiene gate now, which is a venue `just validate` reaches and a unit test
//! here is not.
//!
//! **What it does NOT reach**, and the first one is the limit to read before trusting this. It holds
//! *the shipped lane's tests run*, NOT *the feature-off refusals are tested*: deleting both of them
//! leaves this gate green over the 67 tests that run in both configurations. What covers that is
//! review of a diff removing a `#[cfg(not(feature = ...))]` test, and nothing mechanical. **That
//! number is measured** (2026-09-04, `cargo nextest list` per package, the way this gate invokes
//! them): `sutura-cli` lists 44 tests at the default set and 53 at `--all-features`, `sutura-serve`
//! 25 and 33, with exactly one default-only test each - so 69 run here and 67 of them would still be
//! here with both refusals deleted. An earlier wording said *several hundred*, which was the one
//! unmeasured quantity in a header built on measurements. Then: the shipped packages only, so a feature-off test in a crate that does not ship
//! is run by nothing here either; the HOST triple only, so the four `cross` builds stay the
//! authority on a musl link; and nothing about a feature no shipped binary enables, which is what
//! `--all-features` reaches.
//!
//! **And the VENUE, because `AGENTS.md` calls `just validate` the only thing that counts as
//! verified**: this is neither a nix check nor in that recipe's loop. It reaches `just gates` and one
//! `ci.yml` step - the same shape and the same reason as its sibling, which shells out to cargo and
//! so cannot be a check at all. A green `just validate` says nothing about this lane.

use std::path::Path;

use crate::Verdict;
use crate::default_features::{SOURCE, Shipped, shipped_or_fail};
// Both from their owner. `warm_start` is the module that already FAILS THE BUILD when a
// `${cargoWarmStart}` consumer in `flake.nix` compiles at the wrong profile, so a second spelling of
// either literal here would be a copy that can go stale while that gate stays green. The signal is
// the STAMP and not the directory's NAME, which is the distinction `check-warm-start`'s header draws
// about its own readers: a name can be renamed while a gate matching it keeps passing, whereas the
// stamp exists if and only if the unpack happened.
use crate::warm_start::{STAMP, WARM_PROFILE};

/// The cargo profile to compile at, from the target directory alone.
///
/// `None` is cargo's default and the developer's answer: `just gates` runs in an ordinary `target/`,
/// where a second profile would be a second dependency build bought for a verdict that does not
/// depend on the profile. Inside the warmed directory the answer is the profile the artifacts in it
/// carry, because anything else silently reuses none of them.
fn profile_for(target_dir: Option<&Path>) -> Option<&'static str> {
    if target_dir.is_some_and(|dir| dir.join(STAMP).is_file()) {
        return Some(WARM_PROFILE);
    }
    None
}

/// The words this gate hands cargo, for one package, at one profile.
///
/// `--cargo-profile` and never `--profile`: nextest reads the second as ITS OWN configuration
/// profile, so a run that passed it would build at cargo's default, reuse nothing that was warmed,
/// and still report a verdict. `check-warm-start` holds the same distinction for the flake apps and
/// its own test records it as a real failure rather than a hypothetical one.
fn invocation<'a>(package: &'a str, profile: Option<&'a str>) -> Vec<&'a str> {
    let mut words: Vec<&str> = vec!["nextest", "run", "--no-tests", "fail"];
    if let Some(name) = profile {
        words.extend(["--cargo-profile", name]);
    }
    words.extend(["--package", package]);
    words
}

/// `cargo xtask check-default-feature-tests` - the shipped feature set's tests actually run.
pub(crate) fn run(args: &[String]) -> Verdict {
    if !args.is_empty() {
        eprintln!("usage: check-default-feature-tests - it takes no arguments");
        eprintln!("  The cargo profile is derived from the target directory rather than passed in.");
        return Verdict::Usage;
    }
    // The declaration, the parser and the fail-closed-on-empty rule all come from the gate that owns
    // them, so an empty list cannot mean one thing here and another there.
    let Shipped { root, packages } = match shipped_or_fail("check-default-feature-tests") {
        Ok(read) => read,
        Err(verdict) => return verdict,
    };
    let target_dir = std::env::var_os("CARGO_TARGET_DIR");
    let profile = profile_for(target_dir.as_deref().map(Path::new));
    println!(
        "xtask check-default-feature-tests: {} shipped package(s) from {SOURCE}: {}",
        packages.len(),
        packages.join(", ")
    );
    println!("  cargo's DEFAULT feature set - the one `nix/shipped.nix` publishes and no venue that runs a test uses.");
    if let Some(name) = profile {
        println!("  profile `{name}` - the warmed artifacts were built at it, so this reuses them.");
    }
    let mut failed: Vec<&str> = Vec::new();
    for package in &packages {
        println!("\n=== nextest {package} ===");
        let mut command = std::process::Command::new("cargo");
        command.current_dir(&root).args(invocation(package, profile));
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(_) => failed.push(package.as_str()),
            Err(error) => {
                eprintln!("xtask check-default-feature-tests: could not run cargo: {error}");
                return Verdict::Fail;
            }
        }
    }
    if failed.is_empty() {
        println!(
            "\nxtask check-default-feature-tests: ok - {} package(s) pass their tests at their default features",
            packages.len()
        );
        return Verdict::Pass;
    }
    eprintln!(
        "\nxtask check-default-feature-tests: FAILED - {}: {}",
        failed.len(),
        failed.join(", ")
    );
    eprintln!("  Every other venue that runs a test passes --all-features, so a `cfg(not(feature = ...))`");
    eprintln!("  test is invisible to all of them. What ships is what this ran.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{STAMP, WARM_PROFILE, invocation, profile_for};

    /// The flake output CI reaches this gate through.
    const APP: &str = "default-feature-tests";

    /// The name `main.rs` registers this gate under.
    const TASK: &str = "check-default-feature-tests";

    /// The recipe that is the developer's lane.
    const RECIPE: &str = "gates";

    /// The workflow the CI lane lives in.
    const WORKFLOW: &str = ".github/workflows/ci.yml";

    /// The job it has to be in, which is the one required context.
    const JOB: &str = "ci";

    /// The condition every rust step in that job is gated on.
    const CLASSIFIED: &str = "steps.classify.outputs.rust == 'true'";

    /// A directory that looks like the one the warm start filled.
    fn stamped() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-default-feature-tests-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a writable temp directory");
        std::fs::write(dir.join(STAMP), "/nix/store/whatever-artifacts").expect("a writable stamp");
        dir
    }

    #[test]
    fn the_profile_is_nextests_cargo_profile_and_never_its_own() {
        // `cargo nextest run --profile ci` selects NEXTEST's configuration profile. It builds at
        // cargo's default, reuses none of the warmed artifacts, and reports a verdict anyway - so
        // the only symptom is a CI step quietly paying for a whole dependency build.
        let words = invocation("sutura-serve", Some("ci"));
        assert!(
            words.windows(2).any(|pair| pair == ["--cargo-profile", "ci"]),
            "the profile must be cargo's: {words:?}"
        );
        assert!(
            !words.contains(&"--profile"),
            "`--profile` is nextest's own configuration profile: {words:?}"
        );
    }

    #[test]
    fn an_empty_selection_is_red_rather_than_green() {
        // The defect this whole gate exists to end, one level up: a lane that selects no test and
        // reports success. Written down rather than inherited - nextest 0.9.143's `auto` exits 4
        // here too, measured, so what this holds is that the value stops depending on a default and
        // on `NEXTEST_NO_TESTS` not being set in whatever environment the gate runs in.
        let words = invocation("sutura-cli", None);
        assert!(
            words.windows(2).any(|pair| pair == ["--no-tests", "fail"]),
            "an empty run must exit non-zero: {words:?}"
        );
    }

    #[test]
    fn the_profile_is_derived_from_the_stamp_and_not_from_a_flag() {
        assert_eq!(
            profile_for(None),
            None,
            "no target directory is the developer's default profile"
        );
        let root = crate::repo::root().expect("the repo root");
        assert_eq!(
            profile_for(Some(&root)),
            None,
            "a directory the warm start never filled holds no ci artifacts to reuse"
        );
        let warmed = stamped();
        assert_eq!(profile_for(Some(&warmed)), Some(WARM_PROFILE));
        std::fs::remove_dir_all(&warmed).expect("the temp directory is removable");
    }

    #[test]
    fn both_lanes_still_invoke_this_gate() {
        // "A test pass belongs in the same place or it is a second thing to keep wired" -
        // `github.com/telekom/sutura#264`'s own words about this gate. Two lanes, so two readers:
        // the developer's `just gates` and the required CI job. `check-workflows` holds the other
        // direction, that the app this names is declared.
        //
        // NEITHER READER IS `contains` OVER RAW TEXT, and that is this test rather than a detail
        // of it. `#     cargo run -q -p xtask -- check-default-feature-tests` in the recipe and
        // `# run: nix run .#default-feature-tests` in the workflow each satisfy a substring while
        // no lane invokes anything - the dead-check shape this gate exists to remove, in the test
        // that says the gate is wired. So a comment line of the recipe body is dropped, and the
        // workflow is read through `workflows::step`, whose `collect` is the reader
        // `a_comment_is_not_a_reference` holds to skipping a `#` line.
        let root = crate::repo::root().expect("the repo root");
        assert!(
            crate::TASKS.iter().any(|task| task.name == TASK),
            "{TASK} is not a registered task"
        );
        let body = crate::tasks::recipe_body(&root, RECIPE).expect("a `gates` recipe in the justfile");
        assert!(
            body.iter()
                .any(|line| !line.trim_start().starts_with('#') && line.contains(TASK)),
            "`just {RECIPE}` no longer runs {TASK} on a line that is not a comment"
        );
        let workflow = std::fs::read_to_string(root.join(WORKFLOW)).expect("ci.yml");
        let step = crate::workflows::step::app_step(&workflow, JOB, APP)
            .unwrap_or_else(|| panic!("a live `nix run .#{APP}` step inside {WORKFLOW}'s `{JOB}` job"));
        let declared = step.join("\n");
        // Two ways the step stays in the file and stops being a gate. Both cheap to read once the
        // step's own lines are in hand, and neither reachable from a whole-file scan.
        assert!(
            !declared.contains("continue-on-error"),
            "the {APP} step tolerates its own failure, so the CI half reports rather than gates:\n{declared}"
        );
        assert!(
            declared.contains(CLASSIFIED),
            "the {APP} step is not gated on `{CLASSIFIED}`, which is the condition the rust steps around it use:\n{declared}"
        );
        // WHAT IS STILL NOT HELD HERE. That `{JOB}` is a REQUIRED context is a branch-ruleset
        // setting no file in this tree states, and whether the classification FIRES for a change
        // under `xtask/` is `check-changed`'s question - it fails open, which is the safe
        // direction and not a proof.
    }
}
