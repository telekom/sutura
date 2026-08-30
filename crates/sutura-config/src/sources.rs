//! The sources this deployment declares: one entry per data system, keyed by the alias a model names.
//!
//! **Beside `catalog.data_dir` rather than instead of it, and the two answer different questions.**
//! `catalog.dir` and `catalog.data_dir` are the *catalog*: authored definitions, and the directory the
//! `sutura` command reads. A `sources:` entry is a *data system*: what kind it is, where it is, which
//! identity a query reaches it as, and which identity re-ran its anchors at boot. A model's `source:`
//! is the key that selects one.
//!
//! **The service reads this tree and not `catalog.data_dir`**, which is the one operator-facing break
//! worth stating at the top: a deployment that pointed the service at its files with
//! `catalog.data_dir` has to declare a source instead, and one that declares none does not serve -
//! the catalog names a source with no entry, and the composition root refuses before a listener is
//! bound.
//!
//! # What is refused here, and what is refused later
//!
//! This module is the **parse**: an alias that is not a name, two entries whose aliases are one name,
//! a kind this build has no adapter for, an entry with no file location, a relative path, a word that
//! is not a posture, and a pairing of declarations that contradict each other. Every one of those is a
//! value or a combination this type cannot hold, so it is a
//! [`SettingsError`](crate::SettingsError) naming the key.
//!
//! Two checks are deliberately **not** here, and they are not here for two different reasons.
//!
//! - **The shared-identity acknowledgement** is a [`NotFitToServe`](crate::NotFitToServe) out of
//!   `Settings::refusals`, because whether it is required depends on the *declared deployment mode* -
//!   a fact about the tree as a whole rather than about this entry. A single-user deployment holds
//!   every source under one static credential legitimately: that credential *is* the one user's.
//! - **Whether the linked adapter can carry a per-subject credential at all** is a startup refusal in
//!   the composition root, because it is a property of the BUILD. This crate cannot see which adapters
//!   were linked and must not pretend to.
//!
//! And one is not a parse check on purpose: **whether the directory exists.** `CatalogSettings::parse`
//! declines the same check for the reason that applies here unchanged - a directory that disappears
//! between reading the configuration and opening the engine would make an existence check a claim that
//! is already stale, and it would make configuration validation depend on the filesystem. A missing
//! *file* is refused where it is discovered, at boot, by the composition root that tries to attach it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use sutura_domain::model::{InvalidIdentifier, SourceName};
use sutura_domain::source::{
    AcknowledgementReason, ConflictingSourceIdentity, InvalidOperatorText, SharedIdentityDeclared, SourceIdentity, SourcePosture,
    VerificationIdentity,
};

use crate::security::DeploymentIdentity;
use crate::sources::placement::{BillingProject, DatasetId, InvalidResourceName, SourcePlacement};

/// Where a source's data is, per kind, plus the two `BigQuery` resource newtypes.
pub mod placement;

/// What kind of data system a source is.
///
/// **A closed set of typed declarations rather than something discovered**, which is the whole of
/// *pluggable by declaration*: a capability nobody declared cannot be used, and a new kind is a
/// compile error in every place that has to decide about it. Two variants today, and only one of them
/// can be OPENED by a shipped binary - [`Self::BigQuery`] says which and why.
///
/// **It replaced a comparison against a hard-coded source NAME**, and that is the change worth reading
/// rather than the enum. The composition root used to refuse any source not called `local`, on the
/// argument that the engine has its own identity and does not borrow the catalog's. That argument was
/// right while the catalog was the only signal - a catalog naming `production_warehouse` said nothing
/// about what the deployment held - and it stops being right once the DEPLOYMENT declares each source:
/// an operator who writes `sources.production_warehouse.kind: files` with a directory beside it has
/// stated that this source is a directory of files, which is the statement the name comparison was
/// standing in for. Under the old rule that deployment could not be served at all, and it is a
/// legitimate one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A directory of CSV or Parquet files, read by the in-process engine.
    Files,
    /// A `BigQuery` dataset, queried by rendering the plan into `GoogleSQL` and pushing it down.
    ///
    /// **A declarable kind that no shipped binary can open yet, and that is deliberate rather than an
    /// oversight.** The vocabulary of kinds is the vocabulary of adapters *this repository has*, and
    /// `sutura-exec-bigquery` exists; what does not exist is a composition root that links it, so
    /// `sutura-serve` refuses this kind by name. The alternative was to leave the word out, which
    /// would refuse the same deployment with `kind` does not name a data system this build can open -
    /// a message that sends an operator looking for a typo instead of telling them the truth.
    ///
    /// It is here now rather than with the impersonation step because the billing project has to be
    /// declared somewhere, and putting the declaration one step early is what keeps the per-subject
    /// step to one change: how a connection is authenticated.
    BigQuery,
}

/// The configured word did not name a kind of data system.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a kind of data system - one of: {}", SourceKind::NAMES.join(", "))]
pub struct UnknownSourceKind {
    found: String,
}

impl SourceKind {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["files", "bigquery"];

    /// Reads the configured word.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownSourceKind> {
        match raw.as_ref().trim() {
            "files" => Ok(Self::Files),
            "bigquery" => Ok(Self::BigQuery),
            other => Err(UnknownSourceKind {
                found: String::from(other),
            }),
        }
    }

    /// The spelling, for the startup log.
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::BigQuery => "bigquery",
        }
    }
}

/// One declared data system.
///
/// The identity is an `Option`, and its `None` is **fail-closed rather than permissive**: it means
/// this entry declared the shared posture and nobody acknowledged it, which
/// `Settings::refusals` refuses. A `Settings` obtained through `Settings::load` therefore has `Some`
/// for every source. It stays an `Option` rather than being unwrapped here because a composition root
/// that treated the absence as permission is a bug the type should not be able to hide, and because
/// `Settings::parse` is reachable from this crate's own tests without the refusal having run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredSource {
    placement: SourcePlacement,
    identity: Option<SourceIdentity>,
}

impl ConfiguredSource {
    /// What kind of data system this is, which is what decides which adapter opens it.
    ///
    /// Read off the placement rather than stored beside it - see [`SourcePlacement::kind`].
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        self.placement.kind()
    }

    /// Where this source's data is, in the terms its own kind uses.
    ///
    /// **This replaced a `data_dir()` that every kind had to have.** A `BigQuery` source has no
    /// directory, so a path accessor on the shared shape would have had to return something - an
    /// empty path, or an `Option` whose `None` every caller re-interprets. Matching on the placement
    /// makes the composition root say which kind it is opening, which is the same thing the kind's
    /// exhaustive match there already asks of it.
    #[inline]
    #[must_use]
    pub const fn placement(&self) -> &SourcePlacement {
        &self.placement
    }

    /// How this source establishes identity, once the deployment-level refusal has passed.
    ///
    /// `None` only for a shared source nobody acknowledged - see the type's own note.
    #[inline]
    #[must_use]
    pub const fn identity(&self) -> Option<&SourceIdentity> {
        self.identity.as_ref()
    }

    /// The posture this source was declared with, if it is one a deployment may be served with.
    #[inline]
    #[must_use]
    pub fn posture(&self) -> Option<&SourcePosture> {
        self.identity.as_ref().map(SourceIdentity::posture)
    }
}

/// The configured word did not name a posture.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name how a source establishes identity - one of: {}", SourcePosture::NAMES.join(", "))]
pub struct UnknownPosture {
    found: String,
}

/// Why a `sources:` tree is not usable.
///
/// Every variant names the alias, because a refusal that does not say which entry to change is a
/// support request - and a deployment with several sources is exactly the deployment where "one of
/// your sources is wrong" is useless.
///
/// No `Clone`, and the reason is worth a line rather than a shrug: one variant's cause is
/// `sutura_domain::model::InvalidIdentifier`, which is not `Clone` either. Deriving it here would mean
/// either a second copy of that error's shape or a `Clone` added to a domain type for a config crate's
/// convenience, and nothing needs to clone a startup refusal.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidSourceRegistry {
    /// The key is not a name a model could write in its `source:` field.
    #[error("`sources.{written}` is not a source name")]
    Alias {
        written: String,
        #[source]
        cause: InvalidIdentifier,
    },
    /// Two entries name one source.
    ///
    /// **Reachable, and that is why the check exists.** A source name is trimmed when it is parsed -
    /// case is preserved, whitespace is not - so `local` and `" local"` are two distinct keys in a YAML
    /// mapping and one `SourceName`. Whichever entry lost would be the one nobody opened, with the
    /// deployment configured as though it had been, and nothing anywhere would say so.
    #[error(
        "`sources.{written}` and an earlier entry both name the source `{alias}`. Source names are \
         trimmed, so two keys differing only in surrounding whitespace are one source - and one of \
         these two entries would be the one nothing reads"
    )]
    DuplicateAlias { alias: SourceName, written: String },
    /// The entry names no file location.
    ///
    /// Refused rather than defaulted to `catalog.data_dir`: a source that inherited the catalog's
    /// data directory would be a second source reading the first one's files, which is a
    /// configuration nobody wrote and cannot see.
    #[error("`sources.{alias}.data_dir` is missing or empty - write the directory the files behind this source live in")]
    NoDataDirectory { alias: SourceName },
    /// The path is relative, so it resolves against the process working directory.
    ///
    /// A different directory on every host and never the one the operator meant. `catalog.data_dir`
    /// tolerates a relative path because it is resolved beside a command somebody typed; a source in a
    /// registry is read by a service whose working directory is whatever its supervisor chose.
    #[error(
        "`sources.{alias}.data_dir` is `{}`, which is relative and resolves against this process's \
         working directory - a different directory on every host. Write an absolute path",
        path.display()
    )]
    RelativeDataDirectory { alias: SourceName, path: PathBuf },
    /// The `posture:` word is not one of the two.
    #[error("`sources.{alias}.posture` does not say how this source establishes identity")]
    Posture {
        alias: SourceName,
        #[source]
        cause: UnknownPosture,
    },
    /// The `kind:` word does not name a data system this build has an adapter for.
    ///
    /// **This is where "a source this build cannot open" is refused now**, and it is a parse error
    /// rather than a startup one because the vocabulary is closed: the set of kinds is the set of
    /// adapters, so a word outside it is a value no build could honour rather than a value this build
    /// cannot. Which adapter opens a declared kind is then an exhaustive match in the composition root,
    /// so a second kind is a compile error there rather than a case that falls through.
    #[error("`sources.{alias}.kind` does not name a data system this build can open")]
    Kind {
        alias: SourceName,
        #[source]
        cause: UnknownSourceKind,
    },
    /// A piece of operator-written text on this entry is not usable.
    #[error("`sources.{alias}` carries text that is not usable")]
    Text {
        alias: SourceName,
        #[source]
        cause: InvalidOperatorText,
    },
    /// The entry's two identity declarations contradict each other.
    #[error("`sources.{alias}` declares an identity pairing that does not go together")]
    Conflict {
        alias: SourceName,
        #[source]
        cause: ConflictingSourceIdentity,
    },
    /// A key this kind requires was not written.
    #[error("`sources.{alias}` is `kind: {}` and declares no `{key}`, which that kind cannot be opened without", kind.as_str())]
    MissingForKind {
        alias: SourceName,
        kind: SourceKind,
        key: &'static str,
    },
    /// A key was written that means nothing for this kind.
    ///
    /// **Refused rather than ignored**, because a key an operator wrote and a deployment reads past is
    /// a configuration nobody can see - see `parse_placement` for the argument in full.
    #[error(
        "`sources.{alias}` is `kind: {}` and declares a `{key}`, which that kind has no use for - \
         remove it, or write the kind you meant",
        kind.as_str()
    )]
    KeyNotForKind {
        alias: SourceName,
        kind: SourceKind,
        key: &'static str,
    },
    /// A declared cloud resource name is not usable.
    #[error("`sources.{alias}.{key}` is not a usable name")]
    ResourceName {
        alias: SourceName,
        key: &'static str,
        #[source]
        cause: InvalidResourceName,
    },
}

/// Every source this deployment declares, keyed by the alias a model's `source:` names.
///
/// A newtype over the map rather than the map, so [`Self::parse`] is the only way one comes into
/// existence and the duplicate-alias refusal cannot be skipped by building the map directly.
///
/// **May be empty, and that is not a refusal here.** A deployment configuring no source is one that
/// has not said where its data is; what refuses it is the composition root, which finds the catalog
/// naming a source with no declaration and stops before a listener is bound. Refusing an empty tree in
/// this crate would mean `Settings::load` on the embedded defaults could not produce a `Settings` at
/// all, and the defaults are what the `prompt` command and every settings test read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRegistry {
    by_alias: BTreeMap<SourceName, ConfiguredSource>,
}

/// One entry as it was read, before anything is parsed.
///
/// Named here rather than in `crate::raw` because every field of it is this module's to interpret, and
/// because the alias arrives as the map key rather than as a field.
///
/// `pub(crate)`, with `pub(crate)` fields, for the reason `crate::raw`'s shapes are private: it is the
/// *unparsed* form, so a public one would be a second door into [`SourceRegistry`] that skips nothing
/// and proves nothing. That makes [`SourceRegistry::parse`] crate-visible too - `Settings::parse` is
/// the only caller, which is what makes the refusals unskippable.
#[derive(Debug, Clone)]
pub(crate) struct RawSourceEntry<'raw> {
    /// The key, exactly as written, so a refusal can quote what the operator typed.
    pub(crate) written: &'raw str,
    pub(crate) kind: &'raw str,
    pub(crate) data_dir: Option<&'raw str>,
    pub(crate) billing_project: Option<&'raw str>,
    pub(crate) dataset: Option<&'raw str>,
    pub(crate) posture: &'raw str,
    pub(crate) acknowledged_because: Option<&'raw str>,
    pub(crate) verification_identity: Option<&'raw str>,
}

impl SourceRegistry {
    /// Reads the whole `sources:` tree.
    ///
    /// **`mode` is an input rather than something derived from the entries**, and that direction is
    /// load-bearing: it decides where a shared source's witness may come from. In single-user mode the
    /// witness is the mode's own declaration - the configured credential *is* the one user's, and the
    /// operator wrote a reason for the mode - so a source needs no second acknowledgement. In
    /// multi-user mode the witness must be written against that source's own entry, so a deployment
    /// cannot acknowledge one source and inherit it for the next.
    ///
    /// Deriving the mode from the postures instead is unsound in exactly the configuration that most
    /// needs the check: a genuinely multi-tenant deployment whose sources are all shared would derive
    /// to single-user and be exempted from the acknowledgement it most needs.
    pub(crate) fn parse(
        entries: &[RawSourceEntry<'_>],
        mode: Option<&DeploymentIdentity>,
    ) -> Result<Self, InvalidSourceRegistry> {
        let mut by_alias: BTreeMap<SourceName, ConfiguredSource> = BTreeMap::new();
        for entry in entries {
            let alias = SourceName::parse(entry.written).map_err(|cause| InvalidSourceRegistry::Alias {
                written: String::from(entry.written),
                cause,
            })?;
            if by_alias.contains_key(&alias) {
                return Err(InvalidSourceRegistry::DuplicateAlias {
                    alias,
                    written: String::from(entry.written),
                });
            }
            let configured = parse_entry(&alias, entry, mode)?;
            drop(by_alias.insert(alias, configured));
        }
        Ok(Self { by_alias })
    }

    /// The source declared under `alias`, if there is one.
    #[must_use]
    pub fn get(&self, alias: &SourceName) -> Option<&ConfiguredSource> {
        self.by_alias.get(alias)
    }

    /// Every declared source, in alias order.
    pub fn each(&self) -> impl Iterator<Item = (&SourceName, &ConfiguredSource)> {
        self.by_alias.iter()
    }

    /// Did this deployment declare any source at all?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_alias.is_empty()
    }

    /// How many sources are declared.
    #[must_use]
    pub fn count(&self) -> usize {
        self.by_alias.len()
    }
}

/// Reads one entry, given the alias it was written under.
///
/// Split out of [`SourceRegistry::parse`] so each half stays under the complexity threshold, and
/// because the two halves are two questions: the loop asks *which sources are there*, and this asks
/// *what does one of them say*.
fn parse_entry(
    alias: &SourceName,
    entry: &RawSourceEntry<'_>,
    mode: Option<&DeploymentIdentity>,
) -> Result<ConfiguredSource, InvalidSourceRegistry> {
    let kind = SourceKind::parse(entry.kind).map_err(|cause| InvalidSourceRegistry::Kind {
        alias: alias.clone(),
        cause,
    })?;
    let placement = parse_placement(alias, kind, entry)?;
    let acknowledgement = match entry.acknowledged_because {
        None | Some("") => None,
        Some(text) => Some(
            AcknowledgementReason::parse(text).map_err(|cause| InvalidSourceRegistry::Text {
                alias: alias.clone(),
                cause,
            })?,
        ),
    };
    let verification = match entry.verification_identity {
        None | Some("") => None,
        Some(text) => Some(
            VerificationIdentity::parse(text).map_err(|cause| InvalidSourceRegistry::Text {
                alias: alias.clone(),
                cause,
            })?,
        ),
    };

    // The witness, and where it may come from. A per-source acknowledgement always counts; the
    // single-user mode declaration counts only because that mode's own reason is a key an operator
    // wrote, and in that mode there is one identity and it is the right one.
    let witness = acknowledgement
        .or_else(|| mode.and_then(DeploymentIdentity::shared_witness).cloned())
        .map(SharedIdentityDeclared::of);

    let posture = match entry.posture.trim() {
        "impersonation-at-source" => Some(SourcePosture::ImpersonationAtSource),
        "shared-service-user" => witness.map(|declared| SourcePosture::SharedServiceUser { declared }),
        other => {
            return Err(InvalidSourceRegistry::Posture {
                alias: alias.clone(),
                cause: UnknownPosture {
                    found: String::from(other),
                },
            });
        }
    };

    let identity = match posture {
        // A shared source with no witness. Representable, because the refusal for it is a
        // `NotFitToServe` and a `NotFitToServe` is a list `Settings::refusals` returns rather than an
        // error that stops the parse - which is what lets a test assert on the whole set.
        None => None,
        Some(posture) => {
            Some(
                SourceIdentity::declared(alias, posture, verification).map_err(|cause| InvalidSourceRegistry::Conflict {
                    alias: alias.clone(),
                    cause,
                })?,
            )
        }
    };
    Ok(ConfiguredSource { placement, identity })
}

/// Reads the fields that belong to this entry's kind, and refuses the ones that do not.
///
/// **Both directions are refused, and the second one is the reason this is a function rather than two
/// lines at the call site.** A `bigquery` entry with no `billing_project` cannot be served, so it is
/// refused - that direction is obvious. A `files` entry that also carries a `billing_project` is
/// refused too, because the key would otherwise sit in the file doing nothing: an operator who wrote
/// it believes it is in effect, and a deployment that reads past it has a configuration nobody can
/// see. Fail-closed on a key that means nothing is the same argument `deny_unknown_fields` makes one
/// level up, applied to a key that IS known and is known to the wrong kind.
fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    // Read as "was anything meaningful written", so an empty string is the same as an absent key -
    // which is what the rest of this module already does with operator-written text.
    let written = |value: Option<&str>| value.is_some_and(|text| !text.trim().is_empty());
    match kind {
        SourceKind::Files => {
            for (key, present) in [
                ("billing_project", written(entry.billing_project)),
                ("dataset", written(entry.dataset)),
            ] {
                if present {
                    return Err(InvalidSourceRegistry::KeyNotForKind {
                        alias: alias.clone(),
                        kind,
                        key,
                    });
                }
            }
            Ok(SourcePlacement::Files {
                data_dir: parse_data_dir(alias, entry.data_dir)?,
            })
        }
        SourceKind::BigQuery => {
            if written(entry.data_dir) {
                return Err(InvalidSourceRegistry::KeyNotForKind {
                    alias: alias.clone(),
                    kind,
                    key: "data_dir",
                });
            }
            let billing_project = BillingProject::parse(required(alias, kind, "billing_project", entry.billing_project)?)
                .map_err(|cause| InvalidSourceRegistry::ResourceName {
                    alias: alias.clone(),
                    key: "billing_project",
                    cause,
                })?;
            let dataset = DatasetId::parse(required(alias, kind, "dataset", entry.dataset)?).map_err(|cause| {
                InvalidSourceRegistry::ResourceName {
                    alias: alias.clone(),
                    key: "dataset",
                    cause,
                }
            })?;
            Ok(SourcePlacement::BigQuery {
                billing_project,
                dataset,
            })
        }
    }
}

/// One kind-specific key that has to be there, trimmed, or the refusal that says it is not.
///
/// It returns the TEXT rather than taking the newtype's `parse` as an argument, and that is a
/// deliberate retreat from a tidier shape: the `parse` functions here are generic over
/// `impl AsRef<str>`, so passing one as a `FnOnce(&str)` needs a higher-ranked bound the fn item does
/// not satisfy. Two steps at the call site read better than a `for<'a>` bound whose only job is to
/// make a one-line helper accept a generic function.
fn required<'raw>(
    alias: &SourceName,
    kind: SourceKind,
    key: &'static str,
    written: Option<&'raw str>,
) -> Result<&'raw str, InvalidSourceRegistry> {
    written
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| InvalidSourceRegistry::MissingForKind {
            alias: alias.clone(),
            kind,
            key,
        })
}

/// Reads one entry's file location.
fn parse_data_dir(alias: &SourceName, written: Option<&str>) -> Result<PathBuf, InvalidSourceRegistry> {
    let Some(raw) = written.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(InvalidSourceRegistry::NoDataDirectory { alias: alias.clone() });
    };
    let path = PathBuf::from(raw);
    if path.is_relative() {
        return Err(InvalidSourceRegistry::RelativeDataDirectory {
            alias: alias.clone(),
            path,
        });
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
