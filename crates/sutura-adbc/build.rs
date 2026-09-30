#![forbid(unsafe_code)]
//! Links an ADBC driver archive INTO this crate, when a build supplies one.
//!
//! **This exists because of the one runtime a mounted `.so` cannot serve.** A static musl
//! artefact has no dynamic loader, so `ManagedDriver::load_dynamic_from_filename` can never
//! succeed there - the driver has to be part of the link. `nix/bigquery-adbc.nix` builds the same
//! Go source twice, `c-shared` and `c-archive`, and a build that hands this script the archive
//! directory gets `cfg(adbc_driver_linked)` and the static entrypoint in `src/linked.rs`.
//!
//! **Fail closed, both ways.** A build that names an archive directory holding no archive is a
//! build whose artefact would silently fall back to a path-mounted driver, which is the defect
//! `telekom/sutura#929`'s sixth finding is about - so it panics here instead. A build that names
//! nothing gets no `cfg`, no link directive and the dynamic route: that is the source build every
//! `cargo` gate in this workspace takes, and `sutura doctor` says which route an artefact took.
//!
//! **What keeps this script from running at all for a build that never wants the driver:** this
//! script has no `CARGO_FEATURE_ADBC` early return, unlike the per-adapter one it replaced,
//! because `sutura-adbc` is itself an OPTIONAL dependency behind each adapter's own `adbc`
//! feature (`sutura-exec-bigquery`'s and `sutura-exec-postgres`') - Cargo does not build a crate at all when nothing pulls in the feature that makes it
//! optional, so this script's absence from a build is the workspace manifest's job, not this
//! file's.
//!
//! **The limit next to that:** nothing here can tell whether the archive is for the target being
//! built. A wrong-architecture archive is a link error, which is the outcome anyway; a right
//! architecture built from a different driver revision would link and is held by the flake lock
//! alone.
//!
//! **One driver per artefact, not one driver per crate.** `src/linked.rs` declares exactly one
//! `AdbcDriverInit` symbol, so linking a second archive that exports the same symbol name would
//! silently make it the wrong driver - there is no per-driver entrypoint here, and this crate does
//! not make that generic. A second linked driver in the same binary needs its own entry symbol and
//! its own route; today's one caller (`crates/sutura-cli/src/bigquery_driver.rs`) is the only one
//! `DriverLocation::linked_in()` can mean.

fn main() {
    println!("cargo::rerun-if-env-changed={ARCHIVE_DIR}");
    println!("cargo::rustc-check-cfg=cfg(adbc_driver_linked)");
    let Some(dir) = std::env::var_os(ARCHIVE_DIR) else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let archive = dir.join(format!("lib{ARCHIVE_NAME}.a"));
    assert!(
        archive.is_file(),
        "{ARCHIVE_DIR} names {}, which holds no lib{ARCHIVE_NAME}.a - refusing to build an \
         artefact that would fall back to a mounted driver",
        dir.display()
    );
    println!("cargo::rustc-link-search=native={}", dir.display());
    println!("cargo::rustc-link-lib=static={ARCHIVE_NAME}");
    println!("cargo::rustc-cfg=adbc_driver_linked");
}

/// The directory a build names to have the driver linked in, holding `lib<name>.a`.
const ARCHIVE_DIR: &str = "SUTURA_ADBC_ARCHIVE_DIR";

/// The archive's link name (without `lib`/`.a`), which is the `BigQuery` driver's - the only
/// driver this crate's one [`crate::linked`] entrypoint can be. No env-var override: a second name
/// here would not link a second driver, only a wrong one under the same `AdbcDriverInit` symbol, so
/// there is nothing a build could correctly set it to.
const ARCHIVE_NAME: &str = "adbc_driver_bigquery";
