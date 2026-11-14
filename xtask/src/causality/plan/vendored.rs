//! Vendor dependencies implement workspace behavior without contributing workspace tests.

use super::super::provenance::{Reach, is_build_input};

pub(super) fn reconstruction(reach: Reach, path: &str) -> Reach {
    if reach != Reach::Outside || !path.starts_with("vendor/") {
        return reach;
    }
    // This package is built by Nix, not resolved by the workspace's Cargo invocation. Its
    // feature names must come from the same image as its source when the mounted driver rebuilds.
    if path == "vendor/adbc_clickhouse/Cargo.toml" {
        return Reach::Revertible;
    }
    if is_build_input(path) {
        return Reach::BuildInput;
    }
    if std::path::Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
    {
        return Reach::Revertible;
    }
    Reach::Outside
}
