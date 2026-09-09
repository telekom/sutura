//! The HARNESS for running the real sweep over a real directory - and only the harness.
//!
//! `nix/purge-baked-out-dirs.sh` is executed for its SIDE EFFECT inside a build, and `just
//! lint-workflows` shellchecks it. Neither asks what it removes. So [`unit`] builds a synthetic
//! target directory - one unit directory per branch of the script's decision - and [`sweep`] runs
//! the real script over it, because **an `Ok` from a subprocess is not evidence that the side
//! effect happened** and neither is a text scan.
//!
//! THE ASSERTIONS ARE NOT HERE, AND THAT IS THE WHOLE POINT OF THIS FILE. They sit in
//! [`super::tests`], beside the rest of this gate's tests, and the reason is a MEASURED trap in
//! `test-causality`: a NEW file whose only `#[test]` is a characterization of behaviour this diff
//! does not change can never be red against base, and offering it as the one measurable test file
//! makes the whole diff look *separable*. The gate then reverts the implementation, cannot compile
//! the modules it kept at HEAD, retries with them at base - which deletes the `mod` declaration
//! this file is reached through - and ends `FAILED - the tests this diff added did not run on
//! base`, exit 1, over `0 tests run`. CI treats exit 3 as a warning and exit 1 as red, so that
//! shape blocks a merge.
//!
//! **Move the harness, not the assertions.** With the `#[test]` in [`super`] - a file that changes
//! behaviour and adds tests together - no changed file is separable, the verdict is the honest
//! `NOT MECHANICALLY SEPARABLE`, and the substitute evidence is the mutation run in the pull
//! request. Test-only, and its own file for the 1000-line cap.

use std::path::{Path, PathBuf};

/// A synthetic unit directory: what cargo leaves behind for one build script.
///
/// `ran_in` is what went into `root-output` - the absolute `$OUT_DIR` the script ran with -
/// and `baked` is written into `out/` and into `output` separately, because whether the sweep
/// reads the second one is a STATED LIMIT and a stated limit wants a test.
pub(super) fn unit(profile: &Path, crate_name: &str, hash: &str, ran_in: &str, baked_in_out: &str, baked_in_output: &str) {
    let dir = profile.join("build").join(format!("{crate_name}-{hash}"));
    std::fs::create_dir_all(dir.join("out")).expect("the unit directory");
    std::fs::write(dir.join("root-output"), ran_in).expect("the record");
    std::fs::write(dir.join("out/embed.rs"), baked_in_out).expect("the generated file");
    std::fs::write(dir.join("output"), baked_in_output).expect("the directives file");
    std::fs::create_dir_all(profile.join(".fingerprint").join(format!("{crate_name}-{hash}")))
        .expect("the fingerprint directory");
}

/// The pinned nightly's layout: `root-output` under `run/`, `out/` its sibling, and the crate
/// split into its own `build/<crate>/<hash>/` directory that also holds the fingerprints.
///
/// This is the second face of ONE build-script run. [`unit`] writes the older layout - the record
/// beside `out/` - and the sweep identifies `out/` by looking where it actually is, so both faces
/// are exercised. The purge target is the whole `build/<crate>/` directory.
pub(super) fn unit_run(profile: &Path, crate_name: &str, hash: &str, ran_in: &str, baked_in_out: &str) -> PathBuf {
    let dir = profile.join("build").join(crate_name).join(hash);
    std::fs::create_dir_all(dir.join("run")).expect("the run subdirectory");
    std::fs::create_dir_all(dir.join("out")).expect("the out directory");
    std::fs::create_dir_all(dir.join("fingerprint")).expect("the in-unit fingerprint directory");
    std::fs::write(dir.join("run/root-output"), ran_in).expect("the record");
    std::fs::write(dir.join("out/embed.rs"), baked_in_out).expect("the generated file");
    profile.join("build").join(crate_name)
}

/// Is this new-layout crate directory still present?
pub(super) fn present_run(crate_dir: &Path) -> bool {
    crate_dir.try_exists().expect("the new-layout crate directory is readable")
}

/// Run the real script over `target`, and hand back its output.
///
/// An `Ok` from a subprocess is not evidence that a side effect happened, so every assertion
/// beside this is over the FILESYSTEM and this only supplies the sentence printed with it.
pub(super) fn sweep(target: &Path) -> String {
    let root = crate::repo::root().expect("the repo root");
    let out = std::process::Command::new("bash")
        .arg(root.join(super::pairing::SWEEP))
        .env("CARGO_TARGET_DIR", target)
        .current_dir(&root)
        .output()
        .expect("bash runs the sweep");
    assert!(out.status.success(), "{out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Does this unit directory still exist, and does its fingerprint?
///
/// Two `try_exists` calls rather than one, because the purge has to take BOTH: the build script
/// must rerun (that is what rewrites the path) and the library must be recompiled against what it
/// wrote, and a purge that took only the unit would leave cargo believing the crate was fresh.
pub(super) fn present(profile: &Path, crate_name: &str, hash: &str) -> (bool, bool) {
    let unit = profile.join("build").join(format!("{crate_name}-{hash}"));
    let print = profile.join(".fingerprint").join(format!("{crate_name}-{hash}"));
    (
        unit.try_exists().expect("the unit directory is readable"),
        print.try_exists().expect("the fingerprint is readable"),
    )
}
