//! Vendor dependencies implement workspace behavior without contributing workspace tests.

use super::super::provenance::{Reach, is_build_input};

pub(super) fn reconstruction(reach: Reach, path: &str) -> Reach {
    if reach != Reach::Outside || !path.starts_with("vendor/") {
        return reach;
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
