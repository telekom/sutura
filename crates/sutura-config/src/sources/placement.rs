//! Where a declared source's data actually is, in the terms its own kind uses.
//!
//! **One enum rather than a struct of optional fields, and the difference is unrepresentable versus
//! checked.** A files source has a directory and no billing project; a `BigQuery` source has a
//! billing project and a dataset and no directory. Carried as `Option` fields on one struct, the
//! wrong combination would be representable - a `BigQuery` source with a data directory and no
//! project - and every reader would have to decide for itself what an absence meant. As two variants
//! there is nothing to decide: a reader matches, and the compiler asks about a third kind.
//!
//! This module is also where the two `BigQuery` newtypes live, and their `parse` functions carry an
//! argument that is **ours rather than the provider's format rules restated** - see
//! [`BillingProject`].

use std::path::PathBuf;

use super::SourceKind;

/// The project a `BigQuery` query job is billed to.
///
/// **Declared, never inferred.** For this data system that is structural rather than a policy we
/// chose: the project is a PATH SEGMENT of the request URL that submits a job, so there is no field
/// it could be omitted from and nothing it could be defaulted from. The reason it is declared HERE,
/// one step before anything impersonates, is that a federated identity has no project of its own to
/// bill - so the per-subject step needs this declaration to already exist rather than introducing it
/// alongside a credential exchange.
///
/// # What `parse` enforces, and why the argument is ours
///
/// The value is interpolated into a URL path segment. So what has to be impossible is a value that
/// LEAVES that segment: a `/`, a `?`, a `#`, a `%`-escape, whitespace, a control character, anything
/// non-ASCII. The accepted set is therefore `[a-z0-9-]`, starting with a letter, not ending with a
/// hyphen, and 6 to 30 characters.
///
/// That happens to be the documented shape of a project id, and it is deliberately not justified
/// that way: **the argument for the character set is the path segment**, which holds whether or not
/// the provider widens its own rules later. If the provider ever narrows them further, a value we
/// accept and they reject is a startup failure against a real endpoint - the safe direction. If they
/// widen them, this refuses a legal id and the fix is a considered change here rather than a value
/// that silently escapes a URL.
///
/// **One shape this knowingly refuses, stated because it is a real deployment and not a hypothetical:**
/// a LEGACY domain-scoped project identifier carries a colon - the provider's own SQL reference uses
/// `google.com:my_project` as its example and tells an author to wrap it in backticks. A colon in a
/// URL path segment is legal, so this is a narrowing we are choosing rather than one escaping forces,
/// and it is chosen because such an id also has to survive being a path segment, a JSON field and a
/// backticked SQL identifier, and nothing here has ever been exercised against one. A deployment that
/// needs it gets a considered change with a test, not a widened character set.
///
/// No `Default`: a default project is a project somebody else pays for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BillingProject(String);

/// The dataset unqualified table names in a generated statement resolve within.
///
/// **Why this is configuration and not catalog:** a model in the catalog names a bare `table:`, and
/// which dataset that table lives in is a property of the deployment's connection rather than of the
/// metric's definition. The same catalog served against a staging dataset and a production one is one
/// catalog and two deployments, which is exactly the split this type keeps.
///
/// The generated statement therefore stays a bare, quoted table name in every dialect - the request
/// carries the dataset beside the SQL rather than the generator qualifying it - so nothing about
/// `sutura-sql` has to know this exists.
///
/// `parse` accepts `[A-Za-z0-9_]`, 1 to 1024 characters. Unlike [`BillingProject`] this one does not
/// reach a URL path, so the constraint is not an escaping argument: it is that a dataset id which is
/// not an identifier is a misconfiguration worth refusing when the file is read rather than on the
/// first question. Case is PRESERVED, because a dataset id is case-sensitive and folding it here
/// would turn a working declaration into a dataset that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DatasetId(String);

/// Why a declared name for a cloud resource was not usable.
///
/// One type for both newtypes above, with the offending key named by the caller rather than by the
/// variant: the shapes differ and the *reasons* do not, so two near-identical enums would be two
/// places to keep one set of sentences.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidResourceName {
    /// Nothing was written, or only whitespace was.
    #[error("it is empty")]
    Empty,
    /// Outside the length the name may be.
    #[error("it is {found} characters, and the length must be {least} to {most}")]
    Length { found: usize, least: usize, most: usize },
    /// A character that is not in the accepted set.
    ///
    /// **The position is carried and the VALUE is not**, which is this workspace's habit for a
    /// refusal about operator-written text: a message that quoted the whole value would put a
    /// project id into a log, and a project id is one of the things this repository does not print.
    #[error("the character at position {at} is not allowed here; {accepted}")]
    Character { at: usize, accepted: &'static str },
    /// The first or last character is one the shape does not allow there.
    #[error("{position} character is not allowed there; {accepted}")]
    Boundary { position: &'static str, accepted: &'static str },
}

/// What a project id may be built from, quoted in a refusal so the message is actionable.
const PROJECT_ACCEPTED: &str = "a project id is 6 to 30 characters of lowercase letters, digits and \
                                hyphens, starting with a letter and not ending with a hyphen";

/// What a dataset id may be built from.
const DATASET_ACCEPTED: &str = "a dataset id is 1 to 1024 characters of letters, digits and underscores";

impl BillingProject {
    /// The shortest and longest a project id may be.
    const LEAST: usize = 6;
    const MOST: usize = 30;

    /// Parses a declared project id.
    ///
    /// The canonical constructor: every other way in delegates here, so there is one copy of the
    /// checks. Trims first, because a trailing space in a configuration file is a typo rather than a
    /// different project - and trimming BEFORE measuring is what stops the length refusal reporting a
    /// number that counts whitespace the value does not have.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidResourceName> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidResourceName::Empty);
        }
        let length = trimmed.chars().count();
        if !(Self::LEAST..=Self::MOST).contains(&length) {
            return Err(InvalidResourceName::Length {
                found: length,
                least: Self::LEAST,
                most: Self::MOST,
            });
        }
        for (at, character) in trimmed.chars().enumerate() {
            if !matches!(character, 'a'..='z' | '0'..='9' | '-') {
                return Err(InvalidResourceName::Character {
                    at,
                    accepted: PROJECT_ACCEPTED,
                });
            }
        }
        // The boundary rules, after the character set, so "starts with a digit" is reported as a
        // boundary rather than as an allowed character in the wrong place.
        if !trimmed.starts_with(|c: char| c.is_ascii_lowercase()) {
            return Err(InvalidResourceName::Boundary {
                position: "the first",
                accepted: PROJECT_ACCEPTED,
            });
        }
        if trimmed.ends_with('-') {
            return Err(InvalidResourceName::Boundary {
                position: "the last",
                accepted: PROJECT_ACCEPTED,
            });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The id, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl DatasetId {
    const MOST: usize = 1024;

    /// Parses a declared dataset id.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidResourceName> {
        let trimmed = raw.as_ref().trim();
        if trimmed.is_empty() {
            return Err(InvalidResourceName::Empty);
        }
        let length = trimmed.chars().count();
        if length > Self::MOST {
            return Err(InvalidResourceName::Length {
                found: length,
                least: 1,
                most: Self::MOST,
            });
        }
        for (at, character) in trimmed.chars().enumerate() {
            if !matches!(character, 'a'..='z' | 'A'..='Z' | '0'..='9' | '_') {
                return Err(InvalidResourceName::Character {
                    at,
                    accepted: DATASET_ACCEPTED,
                });
            }
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The id, for building a request.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where one declared source's data is.
///
/// The module header carries why this is an enum. What is worth repeating at the type is that
/// [`SourceKind`] is DERIVED from it - see [`Self::kind`] - rather than stored beside it, so the two
/// cannot disagree about what a source is.
///
/// # The limit, because an enum variant's fields are always public
///
/// There is no way to make these private, so **a placement is constructible in-process by any crate
/// that can name the type** - including one carrying a relative `data_dir`, which `parse_data_dir`
/// refuses when it reads a file. That is a real gap in this type and it is not the one that matters,
/// for the reason AGENTS.md already states about the other constructors here: what is closed is the
/// path from a **configuration file**. [`ConfiguredSource`](crate::ConfiguredSource) holds its
/// placement in a private field and has no public constructor, so a
/// [`SourceRegistry`](crate::SourceRegistry) can still only come into existence through
/// `Settings::parse`, and that is the only door a deployment goes through.
///
/// Written down rather than left to be re-derived, because "the fields are public" and "the checks can
/// be skipped" look like the same sentence and are not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourcePlacement {
    /// A directory of CSV or Parquet files, read by the in-process engine.
    Files {
        /// Absolute, because a service's working directory is whatever its supervisor chose.
        data_dir: PathBuf,
    },
    /// A `BigQuery` dataset, plus the project its jobs are billed to.
    ///
    /// **The limit, worth stating because the deployment it cannot describe is the one this file's
    /// other argument names:** the dataset's OWN project is unrepresentable. The endpoint's
    /// `defaultDataset` resolves an omitted project in the request's project - the billing one - so
    /// this placement can only describe a dataset that lives inside the project paying for the job. A
    /// dataset owned by a different project than the payer is precisely the case where payer and data
    /// owner are different parties because they are different projects, which is the same argument
    /// [`BillingProject`] makes for existing. `location` is the same question one size smaller: a
    /// dataset outside the two multi-regions needs it on the endpoint's result-paging call. Neither a
    /// `project` nor a `location` field is added yet because this seam has no transport consuming
    /// them; the limit is also stated in `docs/adr/0017`, and the change that adds the wire is the one
    /// that decides the fields.
    BigQuery {
        /// Declared, never inferred. See [`BillingProject`].
        billing_project: BillingProject,
        /// Where an unqualified table name resolves. See [`DatasetId`].
        dataset: DatasetId,
        /// The credential file this source is reached with. Absolute, checked at parse.
        ///
        /// **A path rather than a credential**, so nothing in this tree holds token material and
        /// `Secret` has nothing to redact here. Reading it is the adapter's job and happens once, at
        /// the line that opens the source.
        credential_file: PathBuf,
        /// The most one job may be billed for scanning.
        ///
        /// **A bare number and not a newtype, and that is deliberate rather than an omission.** The
        /// RANGE belongs to the adapter - `sutura_exec_bigquery::wire::BytesBilledCeiling::parse`
        /// refuses a zero and refuses a value above the largest ceiling it will send - and a second
        /// copy of those two bounds here would be the drifting duplicate this repository's
        /// *canonical sources* rule exists to prevent. So there is exactly one parse of this number,
        /// in the composition root, and a value outside the range is a startup refusal naming
        /// `sources.<alias>.max_bytes_billed`. What this crate owns is that the key was WRITTEN, which
        /// is the half a settings tree can see.
        max_bytes_billed: u64,
    },
}

impl SourcePlacement {
    /// Which kind of data system this placement describes.
    ///
    /// **Derived rather than stored**, so `kind:` in a file and the fields beside it cannot describe
    /// two different data systems. The parse reads the word to decide which variant to build; from
    /// then on the variant is the answer.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        match *self {
            Self::Files { .. } => SourceKind::Files,
            Self::BigQuery { .. } => SourceKind::BigQuery,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{BillingProject, DatasetId, InvalidResourceName, SourcePlacement};
    use crate::sources::SourceKind;

    #[test]
    fn a_project_id_that_could_escape_a_url_path_is_refused() {
        // **The reason this newtype exists**, and each of these is a value that would otherwise be
        // interpolated straight into a request path. Asserted as a set rather than one example,
        // because what is claimed is the character SET and not that one slash was thought of.
        for hostile in [
            "acme/../other",
            "acme?alt=json",
            "acme#frag",
            "acme%2f",
            "acme project",
            "acme\nproject",
            "ACMEPROJECT",
            "acme\u{00e9}proj",
        ] {
            assert!(
                BillingProject::parse(hostile).is_err(),
                "{hostile:?} was accepted as a project id"
            );
        }
    }

    #[test]
    fn a_refusal_carries_a_position_and_never_the_value() {
        // A project id is one of the things this repository does not print, so the refusal says
        // where the problem is and what is accepted, and does not quote what was written.
        let err = BillingProject::parse("acme/one").expect_err("a slash is refused");
        let rendered = err.to_string();
        assert!(!rendered.contains("acme"), "{rendered}");
        assert!(rendered.contains("position"), "{rendered}");
        match err {
            InvalidResourceName::Character { at, .. } => assert_eq!(at, 4),
            other => panic!("expected a character refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_boundary_rules_are_reported_as_boundaries() {
        // Ordered after the character set so a leading digit is a boundary refusal rather than
        // "position 0 is not allowed", which would be false - a digit is allowed, just not there.
        assert!(matches!(
            BillingProject::parse("1project").expect_err("a leading digit is refused"),
            InvalidResourceName::Boundary {
                position: "the first",
                ..
            }
        ));
        assert!(matches!(
            BillingProject::parse("project-").expect_err("a trailing hyphen is refused"),
            InvalidResourceName::Boundary {
                position: "the last",
                ..
            }
        ));
    }

    #[test]
    fn a_project_id_is_trimmed_before_it_is_measured() {
        // A trailing space in a file is a typo rather than a different project, and trimming before
        // measuring is what keeps the length refusal from counting whitespace the value has not got.
        let parsed = BillingProject::parse("  acme-analytics  ").expect("surrounding space is trimmed");
        assert_eq!(parsed.as_str(), "acme-analytics");
        // Six characters is the shortest allowed, and five is refused - the boundary rather than a
        // value comfortably inside it.
        assert_eq!(
            BillingProject::parse("abcdef")
                .expect("six characters is the shortest allowed")
                .as_str(),
            "abcdef"
        );
        assert!(matches!(
            BillingProject::parse("abcde").expect_err("five is too short"),
            InvalidResourceName::Length { found: 5, .. }
        ));
    }

    #[test]
    fn a_dataset_id_keeps_its_case_because_a_dataset_id_is_case_sensitive() {
        // Folding here would turn a working declaration into a dataset that does not exist, which is
        // the one normalisation this workspace's "sanitize then validate" habit must NOT do.
        assert_eq!(
            DatasetId::parse("Analytics_Prod")
                .expect("mixed case is a dataset id")
                .as_str(),
            "Analytics_Prod"
        );
        // A hyphen is legal in a project id and not in a dataset id, which is why the two newtypes
        // are not one.
        assert!(matches!(
            DatasetId::parse("analytics-prod"),
            Err(InvalidResourceName::Character { .. })
        ));
        assert_eq!(
            BillingProject::parse("analytics-prod")
                .expect("a hyphen IS in a project id")
                .as_str(),
            "analytics-prod"
        );
    }

    #[test]
    fn a_placement_decides_its_own_kind() {
        // Derived rather than stored, so `kind:` in a file and the fields beside it cannot describe
        // two different data systems.
        assert_eq!(
            SourcePlacement::Files {
                data_dir: std::path::PathBuf::from("/srv/data")
            }
            .kind(),
            SourceKind::Files
        );
        assert_eq!(
            SourcePlacement::BigQuery {
                billing_project: BillingProject::parse("acme-analytics").expect("a test project is one"),
                dataset: DatasetId::parse("warehouse").expect("a test dataset is one"),
                credential_file: PathBuf::from("/etc/sutura/bq.json"),
                max_bytes_billed: 1024 * 1024 * 1024,
            }
            .kind(),
            SourceKind::BigQuery
        );
    }
}
