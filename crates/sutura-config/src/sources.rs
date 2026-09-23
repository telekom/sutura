//! The sources this deployment declares: one entry per data system, keyed by the alias a model names.
//!
//! **Beside `catalogs[].data_dir` rather than instead of it, and the two answer different questions.**
//! `catalogs[].dir` and `catalogs[].data_dir` are the *catalog*: authored definitions, and the directory the
//! `sutura` command reads. A `sources:` entry is a *data system*: what kind it is, where it is, which
//! identity a query reaches it as, and which identity re-ran its anchors at boot. A model's `source:`
//! is the key that selects one.
//!
//! **The service reads this tree and not `catalogs[].data_dir`**, which is the one operator-facing break
//! worth stating at the top: a deployment that pointed the service at its files with
//! `catalogs[].data_dir` has to declare a source instead, and one that declares none does not serve -
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

use crate::security::DeploymentIdentity;
use crate::sources::placement::{InvalidHostName, InvalidOracleServiceName, InvalidResourceName, SourcePlacement};
use crate::sources::transport::InvalidTransport;
use crate::sources::workload_identity::{InvalidWorkloadIdentity, WorkloadIdentityConfig};
use sutura_domain::model::{InvalidIdentifier, SourceName};
use sutura_domain::source::{
    AcknowledgementReason, ConflictingSourceIdentity, InvalidOperatorText, SharedIdentityDeclared, SourceIdentity, SourcePosture,
    VerificationIdentity,
};

/// The `clickhouse` entry's own keys, read and refused.
///
/// **Its own file because `cargo xtask max-lines` fails at 1000 lines rather than warning**, and
/// this one crosses it if a fourth kind's key reading lives here. The split follows the seam the
/// composition roots already use - one file per kind - so the arm in [`parse_placement`] stays one
/// call and the reading stays beside the kind it is about.
mod clickhouse;
/// The `oracle` entry's own keys, read and refused - split out for the reason `clickhouse` is.
mod oracle;
/// Where a source's data is, per kind, plus the two `BigQuery` resource newtypes.
pub mod placement;
/// How the channel to a source is secured, per source and never globally.
pub mod transport;
/// The token-exchange setup one `impersonation-at-source` source declares.
pub mod workload_identity;

/// The per-entry reader - see its header for why it is its own file.
mod entry;
use entry::{parse_absolute, parse_placement, refuse_foreign_keys, refuse_remote_plaintext, required};

/// What kind of data system a source is.
///
/// **A closed set of typed declarations rather than something discovered**, which is the whole of
/// *pluggable by declaration*: a capability nobody declared cannot be used, and a new kind is a
/// compile error in every place that has to decide about it. Which of them a given BINARY can open
/// is a separate question, answered by that binary's features rather than here - [`Self::BigQuery`]
/// says why the word is in the vocabulary either way.
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
    /// A directory of Parquet, CSV or NDJSON files - each text format plain or compressed - read
    /// by the in-process engine.
    Files,
    /// A `BigQuery` dataset, queried by rendering the plan into `GoogleSQL` and pushing it down.
    ///
    /// **A declarable kind that no shipped binary can open yet, and that is deliberate rather than an
    /// oversight.** The vocabulary of kinds is the vocabulary of adapters *this repository has*, and
    /// `sutura-exec-bigquery` exists; what does not exist is a composition root that links it, so
    /// `sutura` refuses this kind by name. The alternative was to leave the word out, which
    /// would refuse the same deployment with `kind` does not name a data system this build can open -
    /// a message that sends an operator looking for a typo instead of telling them the truth.
    ///
    /// It is here now rather than with the impersonation step because the billing project has to be
    /// declared somewhere, and putting the declaration one step early is what keeps the per-subject
    /// step to one change: how a connection is authenticated.
    BigQuery,
    /// A `PostgreSQL` database, queried by rendering the plan into that dialect and pushing it down.
    ///
    /// **A declarable kind that no shipped binary opens yet, and the refusal is at startup rather
    /// than at parse, naming the `postgres` feature** - the same shape `BigQuery`'s entry uses the
    /// other way around. The vocabulary of kinds is the vocabulary of adapters this repository has;
    /// which adapter a given BUILD linked is a property of its features, so an entry for a kind whose
    /// adapter is not linked is a startup refusal naming the `--features` that would link it, not a
    /// spelling error.
    ///
    /// The static-credential half: one connection under the deployment's declared identity. Per-subject
    /// Postgres over SASL OAUTHBEARER is `telekom/sutura#126` and is deliberately not this shape.
    Postgres,
    /// A `ClickHouse` database, queried by rendering the plan into that dialect and pushing it down
    /// over its HTTP interface.
    ///
    /// **Declarable, and openable only by a build carrying the `clickhouse` feature** - the same
    /// shape `Postgres` above describes, for the same reason: the vocabulary of kinds is the
    /// vocabulary of adapters this repository has, and which one a given BUILD linked is a property
    /// of its features.
    ///
    /// The static-credential half, and the only half that exists: `sutura_exec_clickhouse`'s
    /// `Warehouse::IMPERSONATION` is `NoPlaceForASubject`, so an `impersonation-at-source` entry on
    /// this kind is refused at the composition root's own posture cross-check. Per-subject
    /// `ClickHouse` identity is wanted and not built.
    ClickHouse,
    /// An Oracle Database, queried by rendering the plan into that dialect and pushing it down.
    ///
    /// Declarable and openable behind the `oracle` feature - `ClickHouse`'s shape, identity half
    /// included. [`SourcePlacement::Oracle`] carries what the driver cannot be told about TLS.
    Oracle,
}

/// The configured word did not name a kind of data system.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{found}` does not name a kind of data system - one of: {}", SourceKind::NAMES.join(", "))]
pub struct UnknownSourceKind {
    found: String,
}

impl SourceKind {
    /// Every accepted spelling, so a message and the parser cannot disagree.
    pub const NAMES: &'static [&'static str] = &["files", "bigquery", "postgres", "clickhouse", "oracle"];

    /// Reads the configured word.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownSourceKind> {
        match raw.as_ref().trim() {
            "files" => Ok(Self::Files),
            "bigquery" => Ok(Self::BigQuery),
            "postgres" => Ok(Self::Postgres),
            "clickhouse" => Ok(Self::ClickHouse),
            "oracle" => Ok(Self::Oracle),
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
            Self::Postgres => "postgres",
            Self::ClickHouse => "clickhouse",
            Self::Oracle => "oracle",
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
    workload_identity: Option<WorkloadIdentityConfig>,
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

    /// The token-exchange setup this `impersonation-at-source` source declared.
    ///
    /// `Some` exactly when the source is impersonating: the parse refuses an impersonating entry with
    /// none, and refuses a non-impersonating entry with one, so an accessor's shape and a deployment's
    /// posture cannot disagree about which sources exchange a subject's token.
    #[inline]
    #[must_use]
    pub const fn workload_identity(&self) -> Option<&WorkloadIdentityConfig> {
        self.workload_identity.as_ref()
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
    /// Refused rather than defaulted to `catalogs[].data_dir`: a source that inherited the catalog's
    /// data directory would be a second source reading the first one's files, which is a
    /// configuration nobody wrote and cannot see.
    #[error("`sources.{alias}.data_dir` is missing or empty - write the directory the files behind this source live in")]
    NoDataDirectory { alias: SourceName },
    /// The path is relative, so it resolves against the process working directory.
    ///
    /// A different directory on every host and never the one the operator meant. `catalogs[].data_dir`
    /// tolerates a relative path because it is resolved beside a command somebody typed; a source in a
    /// registry is read by a service whose working directory is whatever its supervisor chose.
    #[error(
        "`sources.{alias}.data_dir` is `{}`, which is relative and resolves against this process's \
         working directory - a different directory on every host. Write an absolute path",
        path.display()
    )]
    RelativeDataDirectory { alias: SourceName, path: PathBuf },
    /// A path a kind requires is relative, so it resolves against the process working directory.
    ///
    /// The same fact as [`Self::RelativeDataDirectory`] about a different key, and a second variant
    /// rather than a widened first one: `data_dir` is the only key whose absence has a refusal of its
    /// own - [`Self::NoDataDirectory`] - so folding them would make one message stand for two checks
    /// that are not the same. This one names the key.
    #[error(
        "`sources.{alias}.{key}` is `{}`, which is relative and resolves against this process's \
         working directory - a different directory on every host. Write an absolute path",
        path.display()
    )]
    RelativePath {
        alias: SourceName,
        key: &'static str,
        path: PathBuf,
    },
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
    /// A declared Oracle `service_name` the driver would not read as written.
    #[error("`sources.{alias}.service_name` is not a usable Oracle service name")]
    OracleServiceName {
        alias: SourceName,
        #[source]
        cause: InvalidOracleServiceName,
    },
    /// A declared `host` cannot be dialled at all - a shape refusal, not a reachability one.
    #[error("`sources.{alias}.host` is not a usable host")]
    Host {
        alias: SourceName,
        #[source]
        cause: InvalidHostName,
    },
    /// An `impersonation-at-source` source declared no token-exchange setup.
    ///
    /// A source that executes as the asking subject has to say WHICH account each subject becomes -
    /// there is nothing this build could guess, and a per-caller identity has to come out of a
    /// declaration rather than a default that pretends one exists.
    ///
    /// **Corrected: `audience` is load-bearing again.** A round of this record said it "is read by
    /// no transport in this build", which was true while the shipped mechanism was a principal
    /// switch on the deployment's own credentials - and stopped being true when workload-identity
    /// federation replaced it. The pool's own exchange needs the audience, and Google's library
    /// refuses an empty one outright.
    ///
    /// **Corrected twice, and the second correction is narrower than the first.** The same round
    /// said `audience` *and* `scope` become the credential document. Only the audience does: the
    /// document shape has no `scopes` member and the driver's own scope option means
    /// service-account impersonation, so `scope` is declared and sent by nothing - measured against
    /// the pinned sources at `WorkloadIdentity::scope`. So of the three keys: `audience` is read,
    /// `impersonate`'s KEYS decide which callers may be served at all, its VALUES name the account
    /// each caller's questions execute as, and `scope` alone is read by nothing - see
    /// `sutura_exec_bigquery::DeclaredPrincipals::target` for the values.
    #[error(
        "`sources.{alias}` is `impersonation-at-source` and declares no `workload_identity` block - write the `audience` of the identity pool the asker's own assertion is exchanged against, the `scope` (declared for a future transport, and sent by none in this build - the driver applies its own), and the `impersonate` map naming which subjects may be served here"
    )]
    MissingWorkloadIdentity { alias: SourceName },
    /// A workload-identity block was declared on a source that is not impersonating.
    ///
    /// Refused rather than ignored, for the reason every key a kind has no use for is refused: a
    /// declaration that does nothing is a configuration nobody can see.
    #[error(
        "`sources.{alias}` declares `workload_identity` and is not `impersonation-at-source`, so no request will be exchanged against it - remove the block, or write the posture you meant"
    )]
    WorkloadIdentityNotImpersonating { alias: SourceName },
    /// The declared workload-identity value is not usable.
    #[error("`sources.{alias}.workload_identity` is not usable")]
    WorkloadIdentity {
        alias: SourceName,
        #[source]
        cause: InvalidWorkloadIdentity,
    },
    /// The declared transport of a source was not usable.
    ///
    /// The transport is the whole channel a source is reached over, so its refusals (an unknown
    /// `transport_mode` word, TLS with no anchors, a key the declared mode would not read, a
    /// partial client certificate, a relative path) surface here as a single parse refusal naming
    /// the source. The `cause` names the key.
    #[error("`sources.{alias}` declares a transport sutura cannot use")]
    Transport {
        alias: SourceName,
        #[source]
        cause: InvalidTransport,
    },
    /// A source a network can reach was declared with no transport security.
    ///
    /// Issue 124's fail-closed rule, and the reason `plaintext` is a written WORD rather than the
    /// absence of a setting: a unix socket or a loopback host may declare it, and anything reachable
    /// from another machine may not - so a deployment cannot send a password and a whole result set
    /// in clear text by leaving a key out. The key named is `transport_mode`, because declaring a TLS
    /// mode and its anchors is the remedy.
    #[error(
        "`sources.{alias}.host` is `{host}`, which is not a loopback address, and \
         `sources.{alias}.transport_mode` is `plaintext` - a password and every row would cross the \
         network in clear text. Write `transport_mode: verified` with `transport_anchors`, or \
         `transport_mode: mutual` with a `client_certificate`/`client_key` pair"
    )]
    RemoteWithoutTls { alias: SourceName, host: String },
    /// A `verified` or `mutual` transport declared on a `unix_socket` dial.
    ///
    /// A parse-time refusal rather than the connect-time one the driver would otherwise give: the
    /// driver has no TLS handshake to perform over a local socket, so the failure it produces there
    /// is a confusing one that names neither key. Refusing here says which two keys disagree.
    #[error(
        "`sources.{alias}.unix_socket` is set and `sources.{alias}.transport_mode` is `{mode}` - \
         there is no TLS handshake over a local socket to perform. Write `transport_mode: plaintext`, \
         or dial over `host` instead of `unix_socket`"
    )]
    TlsOverUnixSocket { alias: SourceName, mode: &'static str },
    /// A TLS mode on a kind whose driver cannot be handed the declared trust store (`oracle` - see
    /// `SourcePlacement::Oracle`), so `transport_anchors` could not be what it verifies against.
    #[error(
        "`sources.{alias}` is `kind: {}` and `sources.{alias}.transport_mode` is `{mode}` - its driver \
         trusts the public certificate authorities compiled into it, and no declared \
         `transport_anchors` can replace them. Write `transport_mode: plaintext` with a loopback `host`",
        kind.as_str()
    )]
    TlsNotDeliverable {
        alias: SourceName,
        kind: SourceKind,
        mode: &'static str,
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
    pub(crate) credential_file: Option<&'raw str>,
    pub(crate) max_bytes_billed: Option<u64>,
    pub(crate) posture: &'raw str,
    pub(crate) acknowledged_because: Option<&'raw str>,
    pub(crate) verification_identity: Option<&'raw str>,
    pub(crate) workload_identity: Option<crate::raw::RawWorkloadIdentity>,
    pub(crate) host: Option<&'raw str>,
    pub(crate) unix_socket: Option<&'raw str>,
    pub(crate) port: Option<u16>,
    pub(crate) database: Option<&'raw str>,
    pub(crate) service_name: Option<&'raw str>,
    pub(crate) user: Option<&'raw str>,
    pub(crate) password_file: Option<&'raw str>,
    pub(crate) transport_mode: Option<&'raw str>,
    pub(crate) transport_anchors: Option<&'raw str>,
    pub(crate) client_certificate: Option<&'raw str>,
    pub(crate) client_key: Option<&'raw str>,
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

    // The token-exchange setup follows the POSTURE and not the identity's presence: it belongs to the
    // impersonating shape, and only it. An impersonating entry must name the provider and scope its
    // subject's credential is exchanged against; a non-impersonating entry may not carry one at all.
    let workload_identity = match entry.workload_identity.as_ref() {
        None if matches!(entry.posture.trim(), "impersonation-at-source") => {
            return Err(InvalidSourceRegistry::MissingWorkloadIdentity { alias: alias.clone() });
        }
        None => None,
        Some(_) if !matches!(entry.posture.trim(), "impersonation-at-source") => {
            return Err(InvalidSourceRegistry::WorkloadIdentityNotImpersonating { alias: alias.clone() });
        }
        Some(raw) => Some(
            WorkloadIdentityConfig::parse_with_expectations(
                &raw.audience,
                &raw.scope,
                &raw.impersonate,
                raw.expected_issuer.as_deref(),
                raw.expected_audience.as_deref(),
            )
            .map_err(|cause| InvalidSourceRegistry::WorkloadIdentity {
                alias: alias.clone(),
                cause,
            })?,
        ),
    };
    Ok(ConfiguredSource {
        placement,
        identity,
        workload_identity,
    })
}

#[cfg(test)]
mod tests;
