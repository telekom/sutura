//! The closed enum over the shipped warehouse KINDS, so `open_engine` can hold more than one.
//!
//! **`github.com/telekom/sutura#112`.** `sutura_app::Warehouses<W>` is generic in one `W`, and
//! `Warehouse::IMPERSONATION` is a required associated constant with no default - the port is not
//! object-safe, so a heterogeneous set has to be a closed enum over the adapter types a process
//! LINKED rather than `dyn Warehouse`. That is [`AnyWarehouse`](kind::AnyWarehouse): one variant
//! per adapter this build can compile in, matching [`OpenedSources`]'s own variants exactly,
//! because the same feature that links an adapter for the single-kind arm links it here.
//!
//! **It is here rather than in `sutura-app`, for the reason `sutura_app::warehouses`'s own header
//! states: which adapters a process holds is a property of the BUILD.** `sutura-app`'s manifest
//! still names no adapter - [`sutura_app::Warehouses::into_mapped`] is what lets this module build
//! the concrete, per-kind registry `open_files`/`open_bigquery`/`open_postgres` already know how to
//! build and then erase it, rather than threading this enum through every step that constructs an
//! adapter.
//!
//! # Two things this enum deliberately gets wrong, and why that is safe
//!
//! **`IMPERSONATION` is `NoPlaceForASubject` for every variant, and it is a LIE for whichever
//! variant is `BigQuery`.** An associated constant is fixed for the whole TYPE, not per value, so
//! an enum cannot answer this truthfully per variant the way an instance method could. The lie is
//! safe because nothing reads it: `SourcePosture::deliverable_by` - the only caller - is invoked at
//! every one of this crate's composition sites against each source's own CONCRETE adapter
//! constant, before that adapter is ever wrapped in this enum. Declaring the permissive answer
//! here instead would be the dangerous direction - it would let a `files` source pass a check it
//! should fail, wearing a capability this build's engine does not have - so the restrictive one is
//! the only safe default for a constant this enum's own callers never ask.
//!
//! **`EXECUTES_LEGS`, `EXECUTES_AUTHORED_SQL`, `ACCEPTS_RAW_STATEMENTS` and `PRICES_DRY_RUN` all
//! take the trait's own conservative default, and unlike `IMPERSONATION` that is a real, stated
//! LIMIT rather than an inert one.** `Warehouse::executes_legs` - the one capability this issue is
//! about - is an INSTANCE method precisely so it can be asked per concrete adapter through this
//! enum (see [`AnyWarehouse`](kind::AnyWarehouse)'s `impl Warehouse`); the other four are still associated constants
//! with no instance escape, so a `PostgresWarehouse`'s real `ACCEPTS_RAW_STATEMENTS = true` and a
//! `BigQueryWarehouse`'s real `PRICES_DRY_RUN = true` both read `false` once erased behind this
//! type. A mixed-kind deployment therefore cannot serve `sutura`'s raw-SQL tool against its
//! Postgres source, and cannot get a real dry-run byte estimate off its BigQuery source, even
//! though the underlying adapter could deliver both. Widening either needs the same instance-method
//! escape `executes_legs` got, asked for by name when a deployment needs it.

use std::collections::BTreeSet;

use sutura_domain::identity::Presented;
use sutura_domain::model::{QualifiedTable, SourceName, TableName};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::raw::RawStatement;
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::preflight::TablesPresent;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RawExecution, RowSet, Warehouse};
use sutura_exec_datafusion::DataFusionWarehouse;

/// One adapter, of whichever kind this build linked - see the module header for the two things it
/// cannot say truthfully and why that is safe.
pub(crate) enum AnyWarehouse {
    /// The in-process engine, unconditional the way [`super::OpenedSources::Files`] is.
    Files(DataFusionWarehouse),
    /// A `BigQuery` dataset, boxed for the reason every `#[cfg(feature = "bigquery")]` type in this
    /// crate is sized the way it is: `clippy::large_enum_variant` compares this variant against
    /// [`Self::Files`]'s, and the wire's own connection state is the larger of the two.
    #[cfg(feature = "bigquery")]
    BigQuery(Box<super::BigQuerySource>),
    /// A `PostgreSQL` connection, boxed for [`Self::BigQuery`]'s reason: the driver's own runtime
    /// and client are larger than the engine's handle.
    #[cfg(feature = "postgres")]
    Postgres(Box<super::PostgresSource>),
}

/// The error any linked adapter can fail with, erased behind one type the same way the adapter is.
///
/// `#[error(transparent)]` on every arm: this type adds no message of its own, because the concrete
/// adapter's error already carries the one an operator needs and this enum's whole job is routing,
/// not translation.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AnyWarehouseError {
    #[error(transparent)]
    Files(<DataFusionWarehouse as Warehouse>::Error),
    #[cfg(feature = "bigquery")]
    #[error(transparent)]
    BigQuery(<super::BigQuerySource as Warehouse>::Error),
    #[cfg(feature = "postgres")]
    #[error(transparent)]
    Postgres(<super::PostgresSource as Warehouse>::Error),
}

/// Delegates a `&self` method with no error in its signature to whichever adapter this variant
/// holds.
macro_rules! any {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            AnyWarehouse::Files(warehouse) => warehouse.$method($($arg),*),
            #[cfg(feature = "bigquery")]
            AnyWarehouse::BigQuery(warehouse) => warehouse.$method($($arg),*),
            #[cfg(feature = "postgres")]
            AnyWarehouse::Postgres(warehouse) => warehouse.$method($($arg),*),
        }
    };
}

/// The same delegation for a method returning `Result<_, Self::Error>`, wrapping whichever
/// adapter's own error in the matching [`AnyWarehouseError`] arm.
macro_rules! any_fallible {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            AnyWarehouse::Files(warehouse) => warehouse.$method($($arg),*).map_err(AnyWarehouseError::Files),
            #[cfg(feature = "bigquery")]
            AnyWarehouse::BigQuery(warehouse) => warehouse.$method($($arg),*).map_err(AnyWarehouseError::BigQuery),
            #[cfg(feature = "postgres")]
            AnyWarehouse::Postgres(warehouse) => warehouse.$method($($arg),*).map_err(AnyWarehouseError::Postgres),
        }
    };
}

/// One of the port's four failure PREDICATES: `&self` paired with `&Self::Error`, answering
/// `$default` when the two do not name the same adapter.
///
/// **That pairing cannot arise from this port's own contract**, so the fallback arm is defensive
/// rather than reachable: every error a warehouse hands back is minted by THAT SAME instance's own
/// `execute`/`dry_run`/`preflight`, never by a different variant's. `$default` is always the
/// predicate's own safe direction (`false`, or `None`), so a pairing this port cannot actually
/// produce still answers as if nothing here could tell - never as a capability this adapter never
/// measured.
macro_rules! any_predicate {
    ($self:expr, $error:expr, $method:ident, $default:expr) => {
        match ($self, $error) {
            (AnyWarehouse::Files(warehouse), AnyWarehouseError::Files(cause)) => warehouse.$method(cause),
            #[cfg(feature = "bigquery")]
            (AnyWarehouse::BigQuery(warehouse), AnyWarehouseError::BigQuery(cause)) => warehouse.$method(cause),
            #[cfg(feature = "postgres")]
            (AnyWarehouse::Postgres(warehouse), AnyWarehouseError::Postgres(cause)) => warehouse.$method(cause),
            // Unreachable, and therefore ABSENT, on the shipped feature set: with neither
            // `bigquery` nor `postgres` linked, `AnyWarehouse` and `AnyWarehouseError` each have
            // the one `Files` variant, so the arm above is exhaustive on its own and `-D warnings`
            // (`unreachable_patterns`) refuses a wildcard nothing can reach. Kept only where a
            // second variant exists to make a cross-variant pairing possible in the first place.
            #[cfg(any(feature = "bigquery", feature = "postgres"))]
            _ => $default,
        }
    };
}

impl Warehouse for AnyWarehouse {
    type Error = AnyWarehouseError;

    // The module header's first "deliberately wrong" answer.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        any!(self, source)
    }

    fn posture(&self) -> &SourcePosture {
        any!(self, posture)
    }

    // The one capability this issue is about: delegated per INSTANCE, because the associated
    // constant above cannot be per variant. See `Warehouse::executes_legs`'s own doc.
    fn executes_legs(&self) -> bool {
        any!(self, executes_legs)
    }

    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        any_fallible!(self, dry_run, executable, presented, deadline)
    }

    fn execute(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<RowSet, Self::Error> {
        any_fallible!(self, execute, executable, presented, deadline)
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        // The same boot-only call `sutura_app::verify_and_validate` already makes, one layer of
        // delegation further down: this enum has no anchor logic of its own, it forwards to
        // whichever concrete adapter it holds - so this is not a second caller, it is the first
        // one reached through one more indirection.
        #[expect(
            clippy::disallowed_methods,
            reason = "delegates the boot path's one call to the concrete adapter this variant holds"
        )]
        let result = any_fallible!(self, verify_anchor, plan);
        result
    }

    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64> {
        any_predicate!(self, error, working_set_exhausted, None)
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        any_predicate!(self, error, result_did_not_fit, false)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        any_predicate!(self, error, source_refused, false)
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        any_predicate!(self, error, deadline_exceeded, false)
    }

    fn preflight(&self, tables: &BTreeSet<QualifiedTable>) -> Result<TablesPresent, Self::Error> {
        any_fallible!(self, preflight, tables)
    }

    fn preflight_was_refused(&self, error: &Self::Error) -> bool {
        any_predicate!(self, error, preflight_was_refused, false)
    }

    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        // [`Self::verify_anchor`]'s own reason: delegates the boot cardinality check's one call,
        // one layer of indirection further down.
        #[expect(
            clippy::disallowed_methods,
            reason = "delegates the boot path's one call to the concrete adapter this variant holds"
        )]
        let result = any_fallible!(self, declared_key, key);
        result
    }

    fn execute_raw(&self, statement: &RawStatement, presented: &Presented) -> RawExecution<Self::Error> {
        match self {
            Self::Files(warehouse) => warehouse
                .execute_raw(statement, presented)
                .map(|result| result.map_err(AnyWarehouseError::Files)),
            #[cfg(feature = "bigquery")]
            Self::BigQuery(warehouse) => warehouse
                .execute_raw(statement, presented)
                .map(|result| result.map_err(AnyWarehouseError::BigQuery)),
            #[cfg(feature = "postgres")]
            Self::Postgres(warehouse) => warehouse
                .execute_raw(statement, presented)
                .map(|result| result.map_err(AnyWarehouseError::Postgres)),
        }
    }
}

// --------------------------------------------------------------------------- opening a mix ---

/// Every source `open_engine` was handed, sorted into the kind it declared.
///
/// A struct of three vectors rather than a map keyed on [`sutura_config::SourceKind`], because that
/// type derives no `Ord` - it is a closed set of exactly three, and three fields say so without
/// asking the settings crate for a comparison it has never needed.
#[derive(Default)]
pub(crate) struct Grouped<'a> {
    pub(crate) files: Vec<&'a SourceName>,
    pub(crate) bigquery: Vec<&'a SourceName>,
    pub(crate) postgres: Vec<&'a SourceName>,
}

/// Sorts every declared source into its kind, one [`super::configured_source`] lookup per source -
/// so a source with no entry is still refused by name rather than silently dropped from every group.
pub(crate) fn group_by_kind<'a>(
    declared: &[&'a SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<Grouped<'a>, String> {
    let mut grouped = Grouped::default();
    for source in declared {
        match super::configured_source(source, registry)?.kind() {
            sutura_config::SourceKind::Files => grouped.files.push(source),
            sutura_config::SourceKind::BigQuery => grouped.bigquery.push(source),
            sutura_config::SourceKind::Postgres => grouped.postgres.push(source),
        }
    }
    Ok(grouped)
}

/// What opening more than one kind produced: one heterogeneous registry, and the attached-table
/// evidence `files` sources contributed - `None` when this mix opened none, the same "nothing
/// attaches anything" fact [`super::OpenedSources::BigQuery`]'s own doc states.
pub(crate) struct Mixed {
    pub(crate) engines: sutura_app::Warehouses<AnyWarehouse>,
    pub(crate) attached: Option<BTreeSet<TableName>>,
}

/// Opens every group [`group_by_kind`] found, erases each into [`AnyWarehouse`] and merges them
/// into one registry.
///
/// **Reuses [`super::open_files`], [`super::bigquery::open_bigquery`] and
/// [`super::postgres::open_postgres`] verbatim** - the per-source construction, the posture
/// cross-check and the feature-off refusal all stay exactly what the single-kind arms already run,
/// so a mixed deployment is refused by the SAME message a single-kind one would be for the half
/// that is wrong. This function's only job is the erase-and-merge [`sutura_app::Warehouses`] itself
/// cannot do without importing an adapter type.
pub(crate) fn open_mixed(
    grouped: &Grouped<'_>,
    pinned: &sutura_domain::pinned::PinnedDefinitions,
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Anchors>,
) -> Result<Mixed, String> {
    let mut engines: Option<sutura_app::Warehouses<AnyWarehouse>> = None;
    let mut attached: Option<BTreeSet<TableName>> = None;

    if !grouped.files.is_empty() {
        let opened = super::open_files(pinned, &grouped.files, registry, runtime)?;
        attached = Some(opened.attached);
        engines = Some(accumulate(engines, opened.engines.into_mapped(AnyWarehouse::Files))?);
    }
    if !grouped.bigquery.is_empty() {
        engines = Some(accumulate(
            engines,
            bigquery_group(&grouped.bigquery, registry, request_timeout, outbound)?,
        )?);
        // See this function's own doc for `attached`'s widened meaning: a `bigquery` group has no
        // attach step, so its tables are trusted in rather than verified - the SAME trust
        // `OpenedSources::BigQuery`'s own `None` already extends a single-kind deployment.
        if let Some(files_attached) = attached.as_mut() {
            files_attached.extend(trusted_tables(pinned, &grouped.bigquery));
        }
    }
    if !grouped.postgres.is_empty() {
        engines = Some(accumulate(engines, postgres_group(&grouped.postgres, registry)?)?);
        if let Some(files_attached) = attached.as_mut() {
            files_attached.extend(trusted_tables(pinned, &grouped.postgres));
        }
    }

    // Unreachable: `open_engine` only calls this function when `Grouped::kinds_present` is at
    // least 2, so at least one of the three branches above ran. Written as a fallback rather than
    // an unwrap the workspace denies.
    let engines = engines.ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))?;
    Ok(Mixed { engines, attached })
}

/// The tables behind every model whose declared source is one of `sources`.
///
/// **Why [`Mixed::attached`] needs this, and single-kind `bigquery`/`postgres` do not.** Neither
/// kind has an attach step, so a single-kind deployment passes `None` for `attached` and
/// `serve.rs`'s later `refuse_unattached` skips the comparison entirely - correct there, because
/// [`sutura_app::preflight::served_tables`]'s WHOLE bundle is exactly what that `None` was already
/// excusing. A MIXED deployment with a `files` group still wants that comparison run for the
/// tables `files` actually attached; the bug this closes is comparing `served_tables`'s WHOLE
/// bundle (every kind) against `attached` when it held only `files`'s own set - which read every
/// `bigquery`/`postgres` table as "no table attached" and refused a mix that had opened
/// correctly. Widening `attached` with these, un-verified the same way `None` already is for a
/// single-kind deployment of either, is what keeps the `files` half checked without demanding a
/// verification neither adapter's port can give.
fn trusted_tables(pinned: &sutura_domain::pinned::PinnedDefinitions, sources: &[&SourceName]) -> BTreeSet<TableName> {
    pinned
        .definitions()
        .models()
        .values()
        .filter(|model| sources.contains(&model.source()))
        .map(|model| model.table_name().clone())
        .collect()
}

/// Folds one more group into the registry-in-progress, or starts it.
fn accumulate(
    acc: Option<sutura_app::Warehouses<AnyWarehouse>>,
    next: sutura_app::Warehouses<AnyWarehouse>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    match acc {
        None => Ok(next),
        Some(acc) => acc.merge(next).map_err(super::flatten),
    }
}

/// The `BigQuery` group, opened through [`super::bigquery::open_bigquery`] and erased.
#[cfg(feature = "bigquery")]
fn bigquery_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Anchors>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let super::OpenedSources::BigQuery(engines) = super::bigquery::open_bigquery(sources, registry, request_timeout, outbound)?
    else {
        return Err(String::from(
            "`open_bigquery` returned an arm this dispatcher does not expect",
        ));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::BigQuery(Box::new(engine))))
}

/// The feature-off twin: `open_bigquery`'s own refusal always fires here, naming the missing
/// feature - propagated rather than duplicated, so there is one place that names it.
#[cfg(not(feature = "bigquery"))]
fn bigquery_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Anchors>,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    super::bigquery::open_bigquery(sources, registry, request_timeout, outbound)?;
    // Unreachable: that call's own feature-off twin only ever returns `Err`, propagated above by
    // `?`. Written as a fallback rather than an `unreachable!` the workspace denies.
    Err(String::from(
        "`open_bigquery` returned an open registry on a build with no BigQuery adapter linked",
    ))
}

/// The `Postgres` group, opened through [`super::postgres::open_postgres`] and erased.
#[cfg(feature = "postgres")]
fn postgres_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    let super::OpenedSources::Postgres(engines) = super::postgres::open_postgres(sources, registry)? else {
        return Err(String::from(
            "`open_postgres` returned an arm this dispatcher does not expect",
        ));
    };
    Ok(engines.into_mapped(|engine| AnyWarehouse::Postgres(Box::new(engine))))
}

/// [`bigquery_group`]'s feature-off twin, for the same reason.
#[cfg(not(feature = "postgres"))]
fn postgres_group(
    sources: &[&SourceName],
    registry: &sutura_config::SourceRegistry,
) -> Result<sutura_app::Warehouses<AnyWarehouse>, String> {
    super::postgres::open_postgres(sources, registry)?;
    // Unreachable, for `bigquery_group`'s feature-off twin's exact reason.
    Err(String::from(
        "`open_postgres` returned an open registry on a build with no Postgres adapter linked",
    ))
}

/// Whether this mix needs the exchanging broker - true the moment ANY opened source is `BigQuery`,
/// since `build_broker` already scans the whole `sources:` registry for shared AND impersonating
/// entries rather than only `bigquery`-kind ones.
///
/// **No feature-off twin, unlike its siblings above.** Its one caller - `serve.rs`'s `Mixed` arm -
/// only asks this inside its OWN `#[cfg(feature = "bigquery")]` half; the other half never needed
/// an exchanging broker to begin with and calls neither this nor a stand-in for it. A twin
/// returning `false` unconditionally would therefore have no caller on a build without the
/// feature, which `-D dead-code` catches rather than tolerates.
#[cfg(feature = "bigquery")]
pub(crate) fn needs_exchanging_broker(engines: &sutura_app::Warehouses<AnyWarehouse>) -> bool {
    engines
        .each()
        .any(|(_, warehouse)| matches!(warehouse, AnyWarehouse::BigQuery(_)))
}
