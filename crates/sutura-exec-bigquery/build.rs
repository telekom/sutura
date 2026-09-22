#![forbid(unsafe_code)]
//! Links the ADBC `BigQuery` driver INTO this binary, when a build supplies one.
//!
//! **This exists because of the one runtime a mounted `.so` cannot serve.** A static musl
//! artefact has no dynamic loader, so `ManagedDriver::load_dynamic_from_filename` can never
//! succeed there - the driver has to be part of the link. `nix/bigquery-adbc.nix` builds the same
//! Go source twice, `c-shared` and `c-archive`, and a build that hands this script the archive
//! directory gets `cfg(adbc_driver_linked)` and the static entrypoint in `src/adbc/linked.rs`.
//!
//! **Fail closed, both ways.** A build that names an archive directory holding no archive is a
//! build whose artefact would silently fall back to a path-mounted driver, which is the defect
//! `telekom/sutura#929`'s sixth finding is about - so it panics here instead. A build that names
//! nothing gets no `cfg`, no link directive and the dynamic route: that is the source build every
//! `cargo` gate in this workspace takes, and `sutura doctor` says which route an artefact took.
//!
//! **The limit next to that:** nothing here can tell whether the archive is for the target being
//! built. A wrong-architecture archive is a link error, which is the outcome anyway; a right
//! architecture built from a different driver revision would link and is held by the flake lock
//! alone.

fn main() {
    println!("cargo::rerun-if-env-changed={ARCHIVE_DIR}");
    println!("cargo::rustc-check-cfg=cfg(adbc_driver_linked)");
    // The transport itself is behind a feature, so a build without it has nothing to link the
    // archive for - and linking 50-odd megabytes of Go runtime into an artefact whose ADBC module
    // is not compiled would be dead weight the linker cannot drop.
    if std::env::var_os("CARGO_FEATURE_ADBC").is_none() {
        return;
    }
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

/// The directory a build names to have the driver linked in, holding `lib<ARCHIVE_NAME>.a`.
const ARCHIVE_DIR: &str = "SUTURA_ADBC_BIGQUERY_ARCHIVE_DIR";

/// The archive's link name, without the `lib` prefix or the `.a` suffix.
const ARCHIVE_NAME: &str = "adbc_driver_bigquery";
