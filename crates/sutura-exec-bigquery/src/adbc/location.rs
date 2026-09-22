//! Where this process reaches the `BigQuery` ADBC driver, parsed once.
//!
//! **This replaced a `String` that was an arbitrary filesystem path**, which is
//! `telekom/sutura#929`'s sixth finding: a release artefact that ships the `BigQuery` adapter and no
//! driver is not an ADBC-enabled product, and a free-form variable naming any file on the host is
//! not a declaration. The two readings a deployment can now have are the two variants below, and
//! a build that links the driver in cannot be asked for the other one.

use core::fmt;
use std::path::{Path, PathBuf};

/// Where the driver is, once something has decided that it is reachable at all.
///
/// **There is no third state and no `Option`.** A source cannot be opened without one of these,
/// so a composition root either resolved a driver or refused to serve - the shape
/// `crate::transport::JobIdentity` uses for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverLocation(Reached);

/// The two ways a driver is reachable.
///
/// **[`Self::LinkedIn`] is unconstructible in a build that linked no archive** - not by a `cfg` on
/// the variant, which made every match arm and this enum's own shape depend on the build, but
/// because [`DriverLocation::linked_in`] is the only constructor and it answers `None` there. So a
/// source build cannot claim a carried driver and a linked artefact has no path to fall back to,
/// and the two arms stay readable in both builds. `../../build.rs` owns the `cfg`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reached {
    /// Part of this artefact's own link, through `super::linked`.
    LinkedIn,
    /// A shared object this deployment mounted, at an absolute path.
    Mounted(PathBuf),
}

/// Why a named driver path is not one this process will open.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UnusableDriverPath {
    /// There was nothing there.
    #[error("the BigQuery ADBC driver path is empty")]
    Empty,
    /// Relative, so what it names depends on the working directory the process happened to start in.
    ///
    /// **A refusal and not a canonicalisation.** Resolving it here would make the driver a
    /// deployment loads depend on how its supervisor launched it, and the driver is the code that
    /// then executes every question - so the one thing this must not do is guess.
    #[error(
        "the BigQuery ADBC driver path `{named}` is relative, and what it names would depend on this process's working directory"
    )]
    Relative {
        /// As given, so an operator can see what they set.
        named: String,
    },
}

impl DriverLocation {
    /// The driver this BUILD carries, or `None` where it carries none.
    ///
    /// **`None` is the source build**, which is every `cargo` invocation in this workspace: the
    /// archive is supplied by `nix/shipped.nix` for the triples a release publishes and by nothing
    /// else. A caller that gets `None` has to find a driver some other way or refuse.
    #[must_use]
    pub fn linked_in() -> Option<Self> {
        cfg!(adbc_driver_linked).then_some(Self(Reached::LinkedIn))
    }

    /// Parses a mounted driver's path.
    ///
    /// # Errors
    ///
    /// [`UnusableDriverPath::Empty`] and [`UnusableDriverPath::Relative`]. Whether the file is
    /// THERE is deliberately not decided here: a path that exists at parse time and not at load
    /// time is the same outcome, so [`super::AdbcBigQuery::probe`] loading it is the check, and a
    /// second one here would only move the message.
    pub fn parse(named: &str) -> Result<Self, UnusableDriverPath> {
        if named.is_empty() {
            return Err(UnusableDriverPath::Empty);
        }
        let path = Path::new(named);
        if !path.is_absolute() {
            return Err(UnusableDriverPath::Relative { named: named.to_owned() });
        }
        Ok(Self(Reached::Mounted(path.to_path_buf())))
    }

    /// Whether this is the driver the artefact itself carries.
    ///
    /// Read by `sutura doctor` and by `nix/bigquery-driver-check.sh` through it, because "the
    /// release artefact carries a working driver" is a claim about WHICH route loaded, and a
    /// success message that did not say so would read the same for a mounted one.
    #[must_use]
    pub const fn is_linked_in(&self) -> bool {
        matches!(self.0, Reached::LinkedIn)
    }

    /// The mounted path, for the one caller that has to hand it to the driver manager.
    pub(super) fn mounted(&self) -> Option<&Path> {
        match &self.0 {
            Reached::LinkedIn => None,
            Reached::Mounted(path) => Some(path),
        }
    }
}

impl fmt::Display for DriverLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Reached::LinkedIn => f.write_str("linked into this binary"),
            Reached::Mounted(path) => write!(f, "mounted at {}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DriverLocation, UnusableDriverPath};

    #[test]
    fn an_empty_driver_path_is_refused_rather_than_carried_to_the_loader() {
        assert_eq!(DriverLocation::parse(""), Err(UnusableDriverPath::Empty));
    }

    #[test]
    fn a_relative_driver_path_is_refused_because_what_it_names_depends_on_the_working_directory() {
        assert_eq!(
            DriverLocation::parse("lib/libadbc_driver_bigquery.so"),
            Err(UnusableDriverPath::Relative {
                named: String::from("lib/libadbc_driver_bigquery.so")
            })
        );
    }

    #[test]
    fn an_absolute_driver_path_parses_and_says_it_is_not_the_one_the_artefact_carries() {
        let parsed = DriverLocation::parse("/opt/sutura/lib/libadbc_driver_bigquery.so").expect("an absolute path");
        assert!(!parsed.is_linked_in(), "a mounted driver is not part of this artefact");
        assert_eq!(
            parsed.mounted().map(std::path::Path::to_path_buf),
            Some(std::path::PathBuf::from("/opt/sutura/lib/libadbc_driver_bigquery.so"))
        );
        assert_eq!(parsed.to_string(), "mounted at /opt/sutura/lib/libadbc_driver_bigquery.so");
    }

    /// The route this BUILD took, asserted in whichever direction the `cfg` puts it.
    ///
    /// **Both arms are real assertions rather than one arm and a skip.** A source build must not
    /// be able to claim a linked driver, and a build that linked the archive must not answer
    /// `None` and send a composition root looking for a path - `nix/bigquery-driver-check.sh`
    /// reads the second half out of the shipped artefact, and this is the half a cargo gate can
    /// see.
    #[test]
    fn the_linked_route_is_offered_exactly_when_this_build_linked_the_archive() {
        let linked = DriverLocation::linked_in();
        if cfg!(adbc_driver_linked) {
            let carried = linked.expect("a build that linked the archive offers the linked route");
            assert!(carried.is_linked_in());
            assert_eq!(carried.mounted(), None, "the linked driver has no path to hand a loader");
            assert_eq!(carried.to_string(), "linked into this binary");
        } else {
            assert_eq!(linked, None, "a source build must not claim a driver it did not link");
        }
    }
}
