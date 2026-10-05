#![forbid(unsafe_code)]
//! Links ADBC driver archives INTO this crate, when a build supplies them.
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
//! feature (`sutura-exec-bigquery`'s and `sutura-exec-postgres`') - Cargo does not build a crate
//! at all when nothing pulls in the feature that makes it optional, so this script's absence from
//! a build is the workspace manifest's job, not this file's.
//!
//! **The limit next to that:** nothing here can tell whether the archive is for the target being
//! built. A wrong-architecture archive is a link error, which is the outcome anyway; a right
//! architecture built from a different driver revision would link and is held by the flake lock
//! alone.
//!
//! **Three drivers, one archive directory each.** `SUTURA_ADBC_ARCHIVE_DIR` names the `BigQuery`
//! archive, `SUTURA_ADBC_POSTGRES_ARCHIVE_DIR` the PostgreSQL one and
//! `SUTURA_ADBC_DUCKDB_ARCHIVE_DIR` the `DuckDB` one; each turns on its own `cfg` and
//! `src/linked.rs` declares each driver's own init symbol (`AdbcDriverBigqueryInit`,
//! `AdbcDriverPostgresqlInit`, `duckdb_adbc_init`), so no name is shared between the archives -
//! `nix/postgres-adbc.nix` says how the PostgreSQL one stopped defining the ADBC C API, and
//! `nix/duckdb-adbc.nix` refuses a `DuckDB` archive that defines one. The PostgreSQL directory also
//! holds that archive's static link set (libpq, MIT krb5's GSSAPI and OpenSSL), linked here in
//! dependency order with the C++ runtime after it; the `DuckDB` archive is already self-contained,
//! and each directory is the whole contract.

fn main() {
    link(ARCHIVE_DIR, &[ARCHIVE_NAME], "adbc_driver_linked");
    let postgres = link(POSTGRES_ARCHIVE_DIR, POSTGRES_ARCHIVES, "adbc_postgres_driver_linked");
    let duckdb = link(DUCKDB_ARCHIVE_DIR, DUCKDB_ARCHIVES, "adbc_duckdb_driver_linked");
    if postgres || duckdb {
        // Both drivers are C++; a static link has no shared runtime to find it in.
        println!("cargo::rustc-link-lib=stdc++");
    }
}

/// Links every archive in `names` from the directory `var` names, in order, and sets `cfg`.
/// Returns whether it did. Panics where the directory lacks one, for the reason above.
fn link(var: &str, names: &[&str], cfg: &str) -> bool {
    println!("cargo::rerun-if-env-changed={var}");
    println!("cargo::rustc-check-cfg=cfg({cfg})");
    let Some(dir) = std::env::var_os(var) else {
        return false;
    };
    let dir = std::path::PathBuf::from(dir);
    for name in names {
        assert!(
            dir.join(format!("lib{name}.a")).is_file(),
            "{var} names {}, which holds no lib{name}.a - refusing to build an artefact that \
             would fall back to a mounted driver",
            dir.display()
        );
    }
    println!("cargo::rustc-link-search=native={}", dir.display());
    for name in names {
        println!("cargo::rustc-link-lib=static={name}");
    }
    println!("cargo::rustc-cfg={cfg}");
    true
}

/// The directory a build names to have the driver linked in, holding `lib<name>.a`.
const ARCHIVE_DIR: &str = "SUTURA_ADBC_ARCHIVE_DIR";

/// The `BigQuery` archive's link name (without `lib`/`.a`). No env-var override: `src/linked.rs`
/// declares that driver's own init symbol, so another archive here would only fail to link.
const ARCHIVE_NAME: &str = "adbc_driver_bigquery";

/// The directory holding the PostgreSQL archive and its static link set (`nix/postgres-adbc.nix`).
const POSTGRES_ARCHIVE_DIR: &str = "SUTURA_ADBC_POSTGRES_ARCHIVE_DIR";

/// The PostgreSQL archive, then what it needs, in the order a single-pass linker resolves them.
const POSTGRES_ARCHIVES: &[&str] = &[
    "adbc_driver_postgresql",
    "pq",
    "pgcommon",
    "pgport",
    "gssapi_krb5",
    "krb5",
    "k5crypto",
    "com_err",
    "krb5support",
    "ssl",
    "crypto",
];

/// The directory holding the `DuckDB` archive (`nix/duckdb-adbc.nix`).
const DUCKDB_ARCHIVE_DIR: &str = "SUTURA_ADBC_DUCKDB_ARCHIVE_DIR";

/// The `DuckDB` engine, which is its own ADBC driver, merged with everything it links into one
/// archive.
const DUCKDB_ARCHIVES: &[&str] = &["duckdb_adbc"];
