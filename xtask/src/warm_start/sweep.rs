//! The sweep's own behaviour, over a real directory, because nothing else in this tree runs it.
//!
//! `nix/purge-baked-out-dirs.sh` is executed for its SIDE EFFECT inside a build, and `just
//! lint-workflows` shellchecks it. Neither asks what it removes. So this runs the real script over
//! a synthetic target directory - one unit directory per branch of its decision - and every
//! assertion is a `try_exists` on the unit AND its fingerprint rather than a line of its output,
//! because **an `Ok` from a subprocess is not evidence that the side effect happened.**
//!
//! Test-only, and its own file for the 1000-line cap. It sits under [`super`] rather than beside
//! [`super::pairing`] because it is not about the pairing: that module holds every taking against
//! the sweep, and this one holds the sweep against a filesystem.

use std::path::Path;

/// A synthetic unit directory: what cargo leaves behind for one build script.
///
/// `ran_in` is what went into `root-output` - the absolute `$OUT_DIR` the script ran with -
/// and `baked` is written into `out/` and into `output` separately, because whether the sweep
/// reads the second one is a STATED LIMIT and a stated limit wants a test.
fn unit(profile: &Path, crate_name: &str, hash: &str, ran_in: &str, baked_in_out: &str, baked_in_output: &str) {
    let dir = profile.join("build").join(format!("{crate_name}-{hash}"));
    std::fs::create_dir_all(dir.join("out")).expect("the unit directory");
    std::fs::write(dir.join("root-output"), ran_in).expect("the record");
    std::fs::write(dir.join("out/embed.rs"), baked_in_out).expect("the generated file");
    std::fs::write(dir.join("output"), baked_in_output).expect("the directives file");
    std::fs::create_dir_all(profile.join(".fingerprint").join(format!("{crate_name}-{hash}")))
        .expect("the fingerprint directory");
}

/// Run the real script over `target`, and hand back its output.
///
/// An `Ok` from a subprocess is not evidence that a side effect happened, so every assertion
/// below is over the FILESYSTEM and this only supplies the sentence beside it.
fn sweep(target: &Path) -> String {
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

#[test]
fn the_sweep_removes_what_baked_a_directory_it_no_longer_sits_in() {
    // THE SCRIPT'S OWN BEHAVIOUR, over a real directory, because nothing else in this
    // repository runs it: `just lint-workflows` shellchecks it and every venue that executes
    // it does so for its side effect inside a build. Four units, one per branch of its
    // decision, and each assertion is a `try_exists` rather than a line of its output.
    let target = std::env::temp_dir().join(format!("sutura-sweep-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&target));
    let profile = target.join("ci");
    let elsewhere = "/nix/var/nix/builds/nix-74462-1743377963/source/target/ci/build/moved-aaaa/out";

    // 1. MOVED, and what it generated names the directory it ran in. The one purge.
    unit(
        &profile,
        "moved",
        "aaaa",
        elsewhere,
        &format!("#[folder = \"{elsewhere}\"]"),
        "",
    );
    // 2. MOVED, and nothing it generated names that directory. Relocation alone is true of
    //    every build script in an unpacked closure; purging on it would cost the closure.
    unit(&profile, "relocated", "bbbb", elsewhere, "pub const N: u8 = 1;", "");
    // 3. RAN WHERE IT SITS, which is every build script in an ordinary target directory.
    let own = profile.join("build/local-cccc/out");
    unit(
        &profile,
        "local",
        "cccc",
        &own.to_string_lossy(),
        &format!("#[folder = \"{}\"]", own.display()),
        "",
    );
    // 4. THE STATED LIMIT: the baked path is in `output` - cargo's record of the `cargo::`
    //    directives - which is a SIBLING of `out/` and not inside it, so the search never
    //    reads it. This asserts the limit rather than trusting the paragraph that states it.
    unit(
        &profile,
        "directives",
        "dddd",
        elsewhere,
        "pub const N: u8 = 2;",
        &format!("cargo:rustc-link-search=native={elsewhere}"),
    );

    let said = sweep(&target);

    let gone = |crate_name: &str, hash: &str| {
        let unit = profile.join("build").join(format!("{crate_name}-{hash}"));
        let print = profile.join(".fingerprint").join(format!("{crate_name}-{hash}"));
        (
            unit.try_exists().expect("the unit directory is readable"),
            print.try_exists().expect("the fingerprint is readable"),
        )
    };
    assert_eq!(
        gone("moved", "aaaa"),
        (false, false),
        "the baked unit and its fingerprint both go: {said}"
    );
    assert_eq!(
        gone("relocated", "bbbb"),
        (true, true),
        "relocation alone is not a reason: {said}"
    );
    assert_eq!(
        gone("local", "cccc"),
        (true, true),
        "a script that ran here baked nothing stale: {said}"
    );
    assert_eq!(
        gone("directives", "dddd"),
        (true, true),
        "the `output` file is the STATED LIMIT: {said}"
    );
    assert!(said.contains("1 inherited build script output(s) regenerated here"), "{said}");
    drop(std::fs::remove_dir_all(&target));
}
