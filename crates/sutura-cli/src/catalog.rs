//! Catalog opening: how the declared `catalogs:` become the one bundle this build serves.
//!
//! Split out of `main.rs` (a file kept under the thousand-line limit by this very split) and of
//! `sutura-config` (which cannot see which adapter a binary links). What kind each entry is and
//! whether this build can open it is a property of the composition root, so it lives here.
//!
//! **It is the ONE opener, shared by both composition roots** - `crate::serve` and `crate::mcp`
//! each call `open_catalog`/`load` rather than keeping a second, parallel version, and this
//! module lives at the crate root (moved out of `serve` in issue #970) so nothing below one root
//! can drift from what the other serves. `sutura mcp` used to open only a directory argument and
//! ignore `catalogs:`.
//!
//! Native RDBMS dictionaries may compose beside Markdown declarations. Other mixed kinds are
//! refused. Each adapter retains its own type-level KIND and capability declaration; the explicit
//! mixed path links declarations in the application before warehouse validation and publication.

use std::path::PathBuf;

#[cfg(feature = "rdbms")]
mod native;

use sutura_app::assemble;
use sutura_catalog_local::LocalCatalog;
use sutura_domain::pinned::{PinnedDefinitions, SemanticCatalog};
use sutura_http::LocalService;

/// The service either composition root builds over one opened catalog kind.
///
/// Named because the concrete type is over `clippy::type_complexity`: the warehouse and the broker
/// stay generic - `crate::serve` erases both behind `Arc<dyn Surface>`, `crate::mcp` keeps the
/// concrete type over one warehouse - and the sink and the combiner are this binary's own fixed
/// choice, the same split `crate::commands::Composed` makes for the broker `serve` never varies.
pub(crate) type Started<W, B> = LocalService<W, sutura_runtime::TracingAuditSink, B, sutura_exec_datafusion::DataFusionCombiner>;

/// Which monomorphic vector of catalogs this build opened - see the module header for why this is
/// not one generic catalog type the way `OpenedSources` is one generic `Warehouse`.
#[derive(Debug)]
pub(crate) enum OpenedCatalogs {
    Markdown(Vec<LocalCatalog>),
    #[cfg(feature = "datahub")]
    Datahub(Vec<sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::http::HttpAspectReader>>),
    /// A directory of OKF Table Schema descriptors - `#970`. `sutura-catalog-okf` is an
    /// unconditional dependency of this build (pure directory read, no TLS - the same shape
    /// `sutura-catalog-local` holds), so `catalog.kind: okf` is openable by every build of this
    /// binary.
    Okf(Vec<sutura_catalog_okf::OkfCatalog>),
    /// An `OpenMetadata` deployment, behind this crate's default-off `openmetadata` feature - see
    /// `Cargo.toml` for why it is default-off (artefact: it links an outbound TLS stack).
    #[cfg(feature = "openmetadata")]
    Openmetadata(Vec<sutura_catalog_openmetadata::OpenMetadataCatalog<sutura_catalog_openmetadata::http::HttpSnapshotReader>>),
    /// A Cube metadata API, behind this crate's default-off `cube` feature, for the `openmetadata`
    /// arm's reason: it links an outbound TLS stack.
    #[cfg(feature = "cube")]
    Cube(Vec<sutura_catalog_cube::CubeCatalog<sutura_catalog_cube::http::HttpMetaReader>>),
    /// EXPERIMENTAL: an `OpenAPI`-described metadata API, behind the default-off `openapi` feature.
    #[cfg(feature = "openapi")]
    OpenApi(Vec<crate::openapi::OpenApiSource>),
    /// A directory of ODCS v3 data-contract documents - `#973`. `sutura-catalog-datacontract` is an
    /// unconditional dependency of this build (pure directory read, no TLS - the same shape
    /// `sutura-catalog-okf` holds), so `catalog.kind: datacontract` is openable by every build of
    /// this binary.
    DataContract(Vec<sutura_catalog_datacontract::DataContractCatalog>),
    /// A live RDBMS dictionary, behind this crate's default-off `rdbms` feature - see
    /// `Cargo.toml` for why it is default-off (artefact: its reader links outbound TLS). Opened over
    /// a Postgres documentation schema, or an Oracle one behind `oracle`, by
    /// [`sutura_catalog_rdbms::AnyDictionaryReader`].
    #[cfg(feature = "rdbms")]
    Rdbms(Vec<native::Rdbms>),
    #[cfg(feature = "rdbms")]
    RdbmsWithMarkdown {
        rdbms: Vec<native::Rdbms>,
        markdown: Vec<LocalCatalog>,
    },
}

/// Opens every catalog the settings declare.
///
/// Every declared entry must share ONE `sutura_config::CatalogKind` - a deployment naming both a
/// `markdown` and a `datahub` catalog is refused here, for the reason the module header gives; the
/// same shape `crate::open_engine`'s `one_kind` already holds for `sources:`.
///
/// **`outbound` is `security.outbound.transport_anchors`, resolved ONCE by `main` at boot and
/// handed here** so the `datahub` catalog reader verifies its endpoint against the SAME CA set the
/// `bigquery` source wire is verified against - the composition-root decision `main.rs` states. A
/// `markdown` catalog reads files and never dials, so only the `Datahub` arm consumes it; `None`
/// leaves the reader on `ureq`'s compiled-in roots, the behaviour before `security.outbound`.
pub(crate) fn open_catalog(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    let mut kinds = catalogs.each().map(sutura_config::CatalogSettings::kind);
    // `Catalogs::parse` refuses an empty list, so there is always a first kind - the `ok_or_else`
    // below is unreachable in practice and named rather than `unwrap`, which this workspace denies.
    let first = kinds
        .next()
        .ok_or_else(|| String::from("this catalog declares no entries, which Catalogs::parse should already have refused"))?;
    if let Some(mixed) = kinds.find(|kind| *kind != first) {
        #[cfg(feature = "rdbms")]
        if catalogs.each().all(|settings| {
            matches!(
                settings.kind(),
                sutura_config::CatalogKind::Rdbms | sutura_config::CatalogKind::Markdown
            )
        }) {
            return native::open(catalogs);
        }
        return Err(format!(
            "this deployment declares catalogs of more than one kind ({} and {}) - one build serves \
             catalogs of one kind today",
            first.as_str(),
            mixed.as_str()
        ));
    }
    match first {
        sutura_config::CatalogKind::Markdown => Ok(OpenedCatalogs::Markdown(
            catalogs
                .each()
                .map(open_one_markdown_catalog)
                .collect::<Result<Vec<_>, String>>()?,
        )),
        // `Okf` and `DataContract` are unconditional dependencies of this build, so their arms are
        // always linked.
        sutura_config::CatalogKind::Okf => Ok(OpenedCatalogs::Okf(
            catalogs
                .each()
                .map(open_one_okf_catalog)
                .collect::<Result<Vec<_>, String>>()?,
        )),
        sutura_config::CatalogKind::DataContract => Ok(OpenedCatalogs::DataContract(
            catalogs
                .each()
                .map(open_one_data_contract_catalog)
                .collect::<Result<Vec<_>, String>>()?,
        )),

        sutura_config::CatalogKind::Datahub => open_datahub_catalogs(catalogs, outbound),
        // `Openmetadata` is openable behind the `openmetadata` feature; a build without it gets
        // the not-linked refusal `open_openmetadata_catalogs` returns.
        sutura_config::CatalogKind::Openmetadata => open_openmetadata_catalogs(catalogs, outbound),
        sutura_config::CatalogKind::Cube => open_cube_catalogs(catalogs, outbound),
        #[cfg(feature = "openapi")]
        sutura_config::CatalogKind::OpenApi => catalogs
            .each()
            .map(|settings| crate::openapi::open(settings, outbound))
            .collect::<Result<Vec<_>, String>>()
            .map(OpenedCatalogs::OpenApi),
        #[cfg(not(feature = "openapi"))]
        sutura_config::CatalogKind::OpenApi => Err(String::from(
            "catalog.kind: openapi names an EXPERIMENTAL metadata adapter this binary was not built to link - \
             build sutura-cli with --features openapi, or declare markdown catalogs",
        )),
        // `Rdbms` is openable behind the `rdbms` feature; a build without it gets the not-linked
        // refusal `open_rdbms_catalogs` returns, which names the feature.
        sutura_config::CatalogKind::Rdbms => {
            refuse_oracle_dictionaries(catalogs, cfg!(feature = "oracle"))?;
            open_rdbms_catalogs(catalogs)
        }
    }
}

/// Loads every opened catalog and composes them into one pinned bundle.
///
/// **The bundle this returns is the bundle `LocalService` validates**, because `main` hands the
/// same opened catalogs to `LocalService::start_composed`, which loads them itself - the two
/// bundles are the same values, which is what "validate what you serve" needs.
pub(crate) fn load(catalogs: &OpenedCatalogs) -> Result<PinnedDefinitions, String> {
    match catalogs {
        OpenedCatalogs::Markdown(catalogs) => load_each(catalogs),
        #[cfg(feature = "datahub")]
        OpenedCatalogs::Datahub(catalogs) => load_each(catalogs),
        OpenedCatalogs::Okf(catalogs) => load_each(catalogs),
        #[cfg(feature = "openmetadata")]
        OpenedCatalogs::Openmetadata(catalogs) => load_each(catalogs),
        #[cfg(feature = "cube")]
        OpenedCatalogs::Cube(catalogs) => load_each(catalogs),
        #[cfg(feature = "openapi")]
        OpenedCatalogs::OpenApi(catalogs) => load_each(catalogs),
        OpenedCatalogs::DataContract(catalogs) => load_each(catalogs),
        #[cfg(feature = "rdbms")]
        OpenedCatalogs::Rdbms(catalogs) => native::load(catalogs, &[]),
        #[cfg(feature = "rdbms")]
        OpenedCatalogs::RdbmsWithMarkdown { rdbms, markdown } => native::load(rdbms, markdown),
    }
}

/// The combiner that runs every stage of a combine on this node.
pub(crate) fn one_node_combiner() -> Result<sutura_exec_datafusion::DataFusionCombiner, String> {
    sutura_exec_datafusion::DataFusionCombiner::new().map_err(|cause| format!("{cause}\ncould not build the federation combiner"))
}

/// The single place either composition root turns [`OpenedCatalogs`] into a serving
/// [`sutura_http::LocalService`].
///
/// **Moved here so `crate::serve` (whose `started` erases the adapter to `Arc<dyn Surface>`)
/// and `crate::mcp` (whose transport wants the concrete type over one warehouse) dispatch the
/// SAME `LocalService::start_composed` match rather than two copies that could diverge.**
/// `sutura-app` genericises over the catalog type parameter; the adapter `W` and the broker `B`
/// stay this fn's own generics, because the two roots differ on the broker exactly where serve's
/// `shared_identity_service` does - a `bigquery` build attaches the principal-presenting broker
/// and every other shape the static one.
///
/// The working-set bytes, the spend ledger and the row ceiling are read from the settings here, the
/// same values each root read before, so neither caller names a number the other could pick
/// differently. The combiner is the caller's, because only `serve` can plan one across nodes, and
/// the sink is always [`sutura_runtime::TracingAuditSink`] - this process installs no subscriber,
/// so a record is written onto a dispatcher that discards it until a composition installs one,
/// the same limit `commands::started` states.
///
/// **It returns the CONCRETE service, not an erased object, and each root decides how far to
/// erase.** `serve` needs `Arc<dyn Surface>` for the transport; `mcp`'s `serve_stdio` is generic
/// over a sized `S: Surface` and keeps the concrete type. What is shared - and what cannot drift -
/// is the `start_composed` match that builds the service from whichever catalog kind was opened.
///
/// # Errors
///
/// Whatever `LocalService::start_composed` refuses while loading the catalogs a second time and
/// re-running the anchors.
#[expect(
    clippy::type_complexity,
    reason = "the lint fires on this function's two-generic SIGNATURE (W and B, each with its own \
              bound), not on the return type the Started<W, B> alias already names - so the alias \
              does not silence it, and the expectation must still be here for -D warnings to pass"
)]
pub(crate) fn start_composed<W, B>(
    catalogs: &OpenedCatalogs,
    engines: sutura_app::Warehouses<W>,
    broker: B,
    combiner: sutura_exec_datafusion::DataFusionCombiner,
    settings: &sutura_config::Settings,
) -> Result<Started<W, B>, String>
where
    W: sutura_domain::warehouse::Warehouse + Send + Sync + 'static,
    W::Error: Send + Sync,
    B: sutura_domain::identity::CredentialBroker + Send + Sync + 'static,
    B::Error: Send + Sync,
{
    if settings.tools().run_sql_enabled() {
        raw_capable(&engines, settings.tools().run_sql_sources())?;
    }
    let working_set_bytes = settings.runtime().working_set().bytes().get() as u64;
    let spend_ledger = sutura_app::SpendLedger::new(
        settings
            .spend_budget()
            .map(|budget| sutura_app::SpendBudget::new(budget.ceiling_bytes(), budget.window())),
    );
    // Every arm is the same call over a different monomorphic catalog vector.
    macro_rules! start {
        ($catalogs:expr) => {
            LocalService::start_composed(
                $catalogs,
                engines,
                sutura_runtime::TracingAuditSink::new(),
                broker,
                combiner,
                working_set_bytes,
            )
        };
    }
    let service = match catalogs {
        OpenedCatalogs::Markdown(catalogs) => start!(catalogs),
        #[cfg(feature = "datahub")]
        OpenedCatalogs::Datahub(catalogs) => start!(catalogs),
        OpenedCatalogs::Okf(catalogs) => start!(catalogs),
        #[cfg(feature = "openmetadata")]
        OpenedCatalogs::Openmetadata(catalogs) => start!(catalogs),
        #[cfg(feature = "cube")]
        OpenedCatalogs::Cube(catalogs) => start!(catalogs),
        #[cfg(feature = "openapi")]
        OpenedCatalogs::OpenApi(catalogs) => start!(catalogs),
        OpenedCatalogs::DataContract(catalogs) => start!(catalogs),
        #[cfg(feature = "rdbms")]
        OpenedCatalogs::Rdbms(_) | OpenedCatalogs::RdbmsWithMarkdown { .. } => LocalService::start_pinned(
            load(catalogs)?,
            engines,
            sutura_runtime::TracingAuditSink::new(),
            broker,
            combiner,
            working_set_bytes,
        ),
    }
    .map(|service| {
        let mut service = service.with_spend_ledger(spend_ledger).with_ceilings(settings.ceilings());
        let tools = settings.tools();
        if let Some(sources) = tools.run_sql_listed() {
            // Cloned once at startup: the transport goes on to read these settings whole.
            service = service.with_run_sql(sutura_app::RunSqlScope::new(
                sources.clone(),
                tools.run_sql_federation_allowed(),
            ));
        }
        service
    });
    service.map_err(|cause| flatten(&cause))
}

/// Refuses a `tools.run_sql.sources` alias this process opened no source for, or whose adapter
/// accepts no statement in this build. Asked of each opened adapter, so a mix answers per variant.
fn raw_capable<W>(engines: &sutura_app::Warehouses<W>, listed: &[sutura_domain::model::SourceName]) -> Result<(), String>
where
    W: sutura_domain::warehouse::Warehouse,
{
    for alias in listed {
        match engines.get(alias) {
            None => {
                return Err(format!(
                    "tools.run_sql.sources lists `{alias}`, and no source of that alias is open"
                ));
            }
            Some(engine)
                if !(if cfg!(feature = "sql") {
                    engine.accepts_planned_statements()
                } else {
                    engine.accepts_raw_statements()
                }) =>
            {
                return Err(format!(
                    "tools.run_sql.sources lists `{alias}`, whose adapter accepts no statement in this build. List \
                     a source that does, or set tools.run_sql.enabled to false"
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// One kind's worth of catalogs, loaded and composed - the body `load` used to be, generic now
/// because it runs over either monomorphic vector [`OpenedCatalogs`] carries.
///
/// `pub(super)` since `#975`: `serve::refresh::Refresher` re-runs exactly this over a declared
/// `refresh_seconds` interval, so a re-read composes the SAME way the boot-time one does.
pub(super) fn load_each<C>(catalogs: &[C]) -> Result<PinnedDefinitions, String>
where
    C: SemanticCatalog,
{
    let bundles = catalogs
        .iter()
        .map(|catalog| catalog.load().map_err(|cause| flatten(&cause)))
        .collect::<Result<Vec<_>, String>>()?;
    assemble::assemble(&bundles).map_err(|cause| flatten(&cause))
}

/// Opens one declared markdown catalog.
fn open_one_markdown_catalog(settings: &sutura_config::CatalogSettings) -> Result<LocalCatalog, String> {
    let dir = settings.dir().ok_or_else(|| {
        format!(
            "`catalogs.{}.dir` is required for catalog.kind: markdown - this adapter reads a directory of documents",
            settings.name()
        )
    })?;
    Ok(LocalCatalog::new(
        settings.name().clone(),
        PathBuf::from(dir),
        settings.version().clone(),
    ))
}

/// Opens one declared `okf` catalog: a directory of Table Schema descriptors, wrapped in
/// `sutura_catalog_okf::OkfCatalog`.
///
/// Refuses a missing directory before constructing the adapter; folder reads happen at `load`.
fn open_one_okf_catalog(settings: &sutura_config::CatalogSettings) -> Result<sutura_catalog_okf::OkfCatalog, String> {
    let dir = settings.dir().ok_or_else(|| {
        format!(
            "`catalogs.{}.dir` is required for catalog.kind: okf - this adapter reads a directory of Table Schema descriptors",
            settings.name()
        )
    })?;
    Ok(sutura_catalog_okf::OkfCatalog::new(
        settings.name().clone(),
        PathBuf::from(dir),
        settings.version().clone(),
    ))
}
/// Opens every declared `openmetadata` catalog, behind this crate's `openmetadata` feature.
#[cfg(feature = "openmetadata")]
fn open_openmetadata_catalogs(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    catalogs
        .each()
        .map(|settings| open_one_openmetadata_catalog(settings, outbound))
        .collect::<Result<Vec<_>, String>>()
        .map(OpenedCatalogs::Openmetadata)
}

/// The refusal for a build that did not link the `OpenMetadata` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body** - `crate::bigquery`'s
/// `open_bigquery` precedent, the same shape `open_datahub_catalogs` holds. The message names the
/// FEATURE, for the same reason: an operator can act on "build with `--features openmetadata`", and
/// `cargo xtask check-feature-remedies` is what keeps that instruction honest.
#[cfg(not(feature = "openmetadata"))]
fn open_openmetadata_catalogs(
    _catalogs: &sutura_config::Catalogs,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    Err(String::from(
        "catalog.kind: openmetadata names a metadata adapter this binary was not built to link - \
         build sutura-cli with --features openmetadata, or declare markdown catalogs",
    ))
}

/// Opens every declared `cube` catalog, behind this crate's `cube` feature.
#[cfg(feature = "cube")]
fn open_cube_catalogs(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    catalogs
        .each()
        .map(|settings| open_one_cube_catalog(settings, outbound))
        .collect::<Result<Vec<_>, String>>()
        .map(OpenedCatalogs::Cube)
}

/// The refusal for a build that did not link the Cube adapter - `open_openmetadata_catalogs`'s shape.
#[cfg(not(feature = "cube"))]
fn open_cube_catalogs(
    _catalogs: &sutura_config::Catalogs,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    Err(String::from(
        "catalog.kind: cube names a metadata adapter this binary was not built to link - \
         build sutura-cli with --features cube, or declare markdown catalogs",
    ))
}

/// Opens one declared `cube` catalog: the reader, bounded and authenticated by the declared
/// deployment token, wrapped in `sutura_catalog_cube::CubeCatalog`.
///
/// Every cube is a model on the source the catalog's own name aliases: one Cube deployment is one
/// source. Endpoint, bounds and anchors resolve as `open_one_openmetadata_catalog`'s do.
#[cfg(feature = "cube")]
fn open_one_cube_catalog(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_catalog_cube::CubeCatalog<sutura_catalog_cube::http::HttpMetaReader>, String> {
    use sutura_catalog_cube::http::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpMetaReader, ReadBounds};

    let endpoint_raw = settings
        .endpoint()
        .ok_or_else(|| format!("`catalogs.{}.endpoint` is required for catalog.kind: cube", settings.name()))?;
    let endpoint = Endpoint::parse(endpoint_raw)
        .map_err(|cause| format!("`catalogs.{}.endpoint` is not a usable endpoint: {cause}", settings.name()))?;
    let token = read_token(settings, "cube")?;
    let deadline_seconds = settings.deadline_seconds().unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let max_response_bytes = settings.max_response_bytes().unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
    let bounds = ReadBounds::parse(deadline_seconds, max_response_bytes).map_err(|cause| {
        format!(
            "`catalogs.{}.deadline_seconds`/`max_response_bytes` are not usable read bounds: {cause}",
            settings.name()
        )
    })?;
    let (reader, rotator) = HttpMetaReader::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (cube reader)", rotator);
    Ok(sutura_catalog_cube::CubeCatalog::new(
        settings.name().clone(),
        settings.version().clone(),
        settings.name().clone(),
        HttpMetaReader::rotating(endpoint, token, bounds, reader),
    ))
}

/// Opens every declared `rdbms` catalog at once, behind this crate's `rdbms` feature.
#[cfg(feature = "rdbms")]
fn open_rdbms_catalogs(catalogs: &sutura_config::Catalogs) -> Result<OpenedCatalogs, String> {
    catalogs
        .each()
        .map(open_one_rdbms_catalog)
        .collect::<Result<Vec<_>, String>>()
        .map(OpenedCatalogs::Rdbms)
}

#[cfg(feature = "rdbms")]
fn open_one_rdbms_catalog(
    settings: &sutura_config::CatalogSettings,
) -> Result<sutura_catalog_rdbms::RdbmsCatalog<sutura_catalog_rdbms::AnyDictionaryReader>, String> {
    use sutura_catalog_rdbms::AnyDictionaryReader;
    use sutura_catalog_rdbms::postgres_reader::RowPredicate;

    let rdbms = settings.rdbms().ok_or_else(|| {
        format!(
            "`catalogs.{}` is `kind: rdbms` yet carries no rdbms settings, which CatalogSettings::parse should have refused",
            settings.name()
        )
    })?;
    // The documentation schema, defaulting to the reader's own documented default.
    let documentation_schema = rdbms.dictionary_schema().map_or_else(
        || String::from(sutura_catalog_rdbms::postgres_reader::DEFAULT_DOCUMENTATION_SCHEMA),
        |schema| String::from(schema.as_str()),
    );
    let environment = rdbms
        .environment()
        .map_or_else(String::new, |environment| environment.as_str().to_owned());

    let predicate = match rdbms.live_row_predicate() {
        None => RowPredicate::None,
        Some(p) => match p.operator() {
            sutura_config::PredicateOperator::IsNull => RowPredicate::IsNull(String::from(p.column().as_str())),
            sutura_config::PredicateOperator::IsNotNull => RowPredicate::IsNotNull(String::from(p.column().as_str())),
            sutura_config::PredicateOperator::Equals => RowPredicate::Equals {
                column: String::from(p.column().as_str()),
                value: p
                    .value()
                    .map(String::from)
                    .ok_or_else(|| format!("`catalogs.{}.live_row_predicate` is `equals` with no value, which CatalogSettings::parse should have refused", settings.name()))?,
            },
        },
    };

    // A catalog read has no caller to run as, so its connection is the catalog's OWN, never
    // borrowed from a `sources:` entry.
    let reader = match rdbms.connection() {
        sutura_config::CatalogConnection::Postgres(connection) => {
            postgres_dictionary_reader(settings, connection, documentation_schema, environment, predicate, rdbms)
                .map(|reader| AnyDictionaryReader::Postgres(Box::new(reader)))
        }
        #[cfg(feature = "oracle")]
        sutura_config::CatalogConnection::Oracle(connection) => {
            let password = crate::password_file::read_key(
                &format!("catalogs.{}.connection.password_file", settings.name()),
                connection.password_file(),
            )?;
            let login = crate::oracle::dictionary_login(settings.name(), connection, password)?;
            let reader = match rdbms.native_selection() {
                Some(selection) => sutura_catalog_rdbms::oracle_reader::OracleReader::native(
                    login,
                    selection,
                    rdbms.max_dictionary_rows(),
                    rdbms.max_dictionary_bytes(),
                ),
                None => sutura_catalog_rdbms::oracle_reader::OracleReader::new(
                    login,
                    &documentation_schema,
                    environment,
                    predicate,
                    rdbms.max_dictionary_rows(),
                    rdbms.max_dictionary_bytes(),
                ),
            };
            reader
                .map(|reader| AnyDictionaryReader::Oracle(Box::new(reader.with_unbounded_number(rdbms.unbounded_number()))))
                .map_err(|cause| {
                    format!(
                        "`catalogs.{}` has an invalid dictionary reader setting: {cause}",
                        settings.name()
                    )
                })
        }
        #[cfg(not(feature = "oracle"))]
        sutura_config::CatalogConnection::Oracle(_) => Err(oracle_dictionary_not_linked(settings.name())),
    }?;

    // The contribution manifest stays under the catalog NAME while the semantic models bind to the
    // declared `source_alias` - see `RdbmsCatalog::with_source_alias`.
    let bounds = reader.bounds();
    let catalog = sutura_catalog_rdbms::RdbmsCatalog::new(settings.name().clone(), settings.version().clone(), reader)
        .with_source_alias(rdbms.source_alias().clone())
        .with_bounds(bounds);
    Ok(match rdbms.audience() {
        Some(audience) => catalog.with_audience(audience.clone()),
        None => catalog,
    })
}

/// The Postgres reader over the catalog's own read-only connection and declared transport.
#[cfg(feature = "rdbms")]
fn postgres_dictionary_reader(
    settings: &sutura_config::CatalogSettings,
    connection: &sutura_config::PostgresCatalogConnection,
    documentation_schema: String,
    environment: String,
    predicate: sutura_catalog_rdbms::postgres_reader::RowPredicate,
    rdbms: &sutura_config::RdbmsSettings,
) -> Result<sutura_catalog_rdbms::postgres_reader::PostgresReader, String> {
    use sutura_adbc_postgres::{Channel, ConnectionTarget, Conninfo, PostgresDriver, UnusableChannel};
    use sutura_catalog_rdbms::postgres_reader::PostgresReader;
    use sutura_config::sources::placement::PostgresDial;
    use sutura_config::sources::transport::{SourceTransport, TrustAnchors};

    let name = settings.name();
    let (target, port) = match *connection.dial() {
        PostgresDial::Tcp { ref host, port } => (ConnectionTarget::Host(host.as_str()), port),
        PostgresDial::UnixSocket { ref directory, port } => (ConnectionTarget::UnixSocket(directory.as_path()), port),
    };
    let password = crate::password_file::read_key(
        &format!("catalogs.{name}.connection.password_file"),
        connection.password_file(),
    )?;
    // The shape `crate::postgres::build` resolves a `sources:` entry's channel in: the transport
    // layer has already refused plaintext to a remote host.
    let anchors = |declared: &TrustAnchors| match *declared {
        TrustAnchors::System => sutura_tls::Anchors::System,
        TrustAnchors::File(ref path) => sutura_tls::Anchors::Bundle(path.clone()),
    };
    let (declared, pair) = match *connection.transport() {
        SourceTransport::Plaintext => (None, None),
        SourceTransport::Verified { anchors: ref declared } => (Some(anchors(declared)), None),
        SourceTransport::Mutual {
            anchors: ref declared,
            ref identity,
        } => (
            Some(anchors(declared)),
            Some(sutura_tls::Identity::new(
                identity.certificate().clone(),
                identity.key().clone(),
            )),
        ),
    };
    let channel = match (declared.as_ref(), pair.as_ref()) {
        (None, _) => Channel::Plaintext,
        (Some(anchors), None) => Channel::Verified(anchors),
        (Some(anchors), Some(pair)) => Channel::Mutual(anchors, pair),
    };
    // The connector's refusal names `sources.<name>`, its own key space; the catalog's key leads.
    let conninfo = Conninfo::new(
        name,
        target,
        port,
        connection.database(),
        connection.user(),
        &password,
        channel,
    )
    .map_err(|cause| match cause {
        UnusableChannel::Material { cause, .. } => format!(
            "`catalogs.{name}.connection` is declared TLS and its material is not usable: {}",
            crate::commands::render(&cause)
        ),
        other => format!(
            "`catalogs.{name}.connection` cannot be dialled as declared: {}",
            crate::commands::render(&other)
        ),
    })?;
    let driver = PostgresDriver::from_host()
        .map_err(|none| format!("`catalogs.{name}` has no driver to open: {}", crate::commands::render(&none)))?;

    let reader = match rdbms.native_selection() {
        Some(selection) => PostgresReader::native(
            driver,
            conninfo,
            selection,
            rdbms.max_dictionary_rows(),
            rdbms.max_dictionary_bytes(),
        ),
        None => PostgresReader::new(
            driver,
            conninfo,
            documentation_schema,
            environment,
            predicate,
            rdbms.max_dictionary_rows(),
            rdbms.max_dictionary_bytes(),
        ),
    };
    reader.map_err(|cause| {
        format!(
            "`catalogs.{}` has an invalid dictionary reader setting: {cause}",
            settings.name()
        )
    })
}

/// Refuses a declared `rdbms` catalog whose dictionary is an Oracle connection, in a build without
/// the `oracle` feature - whether or not `rdbms` is linked, so the shipped feature set and the
/// default one answer the same. The build's feature state arrives as `oracle_built`, so every
/// build compiles and tests this refusal rather than only the one that lacks the feature.
pub(crate) fn refuse_oracle_dictionaries(catalogs: &sutura_config::Catalogs, oracle_built: bool) -> Result<(), String> {
    if oracle_built {
        return Ok(());
    }
    catalogs
        .each()
        .find(|catalog| {
            catalog
                .rdbms()
                .is_some_and(|rdbms| matches!(rdbms.connection(), sutura_config::CatalogConnection::Oracle(_)))
        })
        .map_or(Ok(()), |catalog| Err(oracle_dictionary_not_linked(catalog.name())))
}

/// The refusal for an Oracle dictionary connection in a build that did not enable `oracle`.
fn oracle_dictionary_not_linked(name: &sutura_domain::model::SourceName) -> String {
    format!(
        "`catalogs.{name}` declares an Oracle dictionary connection, which needs a build with the `oracle` feature - \
         build sutura-cli with --features oracle, or declare a Postgres dictionary"
    )
}

/// The refusal for a build that did not link the `rdbms` adapter - the message names the feature.
#[cfg(not(feature = "rdbms"))]
fn open_rdbms_catalogs(_catalogs: &sutura_config::Catalogs) -> Result<OpenedCatalogs, String> {
    Err(String::from(
        "catalog.kind: rdbms names a metadata adapter this binary was not built to link - \
         build sutura-cli with --features rdbms, or declare markdown catalogs",
    ))
}

/// Opens one declared `openmetadata` catalog: the reader, bounded and authenticated, wrapped in
/// `sutura_catalog_openmetadata::OpenMetadataCatalog`.
///
/// **The source mapping is a single fixed alias, the service the recorded corpus and this build's
/// fake serve - `warehouse` - a real limit, not a placeholder**: `CatalogSettings` carries no
/// platform-to-source mapping for an `openmetadata` entry. A deployment whose models live on a different service is
/// not representable by this build's `openmetadata` composition yet - a real gap, named here rather
/// than left for a reader to discover from a refusal at load time.
///
/// The read bounds and endpoint are resolved here, the same single-owner split the `datahub` arm
/// holds: an absent `deadline_seconds`/`max_response_bytes` resolves to the reader's own constants,
/// and the endpoint is parsed through `sutura_catalog_openmetadata::http::Endpoint::parse` because
/// the reader REQUIRES one - this composition step cannot skip the parse even by accident. `outbound`
/// closes the loop from `main`'s ONE boot-time resolution to the reader's own `ureq` agent.
#[cfg(feature = "openmetadata")]
fn open_one_openmetadata_catalog(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_catalog_openmetadata::OpenMetadataCatalog<sutura_catalog_openmetadata::http::HttpSnapshotReader>, String> {
    use sutura_catalog_openmetadata::http::{
        DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpSnapshotReader, ReadBounds,
    };

    let endpoint_raw = settings.endpoint().ok_or_else(|| {
        format!(
            "`catalogs.{}.endpoint` is required for catalog.kind: openmetadata",
            settings.name()
        )
    })?;
    let endpoint = Endpoint::parse(endpoint_raw)
        .map_err(|cause| format!("`catalogs.{}.endpoint` is not a usable endpoint: {cause}", settings.name()))?;
    let token = read_token(settings, "openmetadata")?;
    let deadline_seconds = settings.deadline_seconds().unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let max_response_bytes = settings.max_response_bytes().unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
    let bounds = ReadBounds::parse(deadline_seconds, max_response_bytes).map_err(|cause| {
        format!(
            "`catalogs.{}.deadline_seconds`/`max_response_bytes` are not usable read bounds: {cause}",
            settings.name()
        )
    })?;
    let (reader, rotator) = HttpSnapshotReader::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (openmetadata reader)", rotator);
    let reader = HttpSnapshotReader::rotating(endpoint, token, bounds, reader);
    let mut sources = std::collections::BTreeMap::new();
    drop(sources.insert(String::from("warehouse"), settings.name().clone()));
    Ok(sutura_catalog_openmetadata::OpenMetadataCatalog::new(
        settings.name().clone(),
        settings.version().clone(),
        sources,
        reader,
    ))
}

/// Opens one declared `datacontract` catalog: a directory of ODCS v3 contract documents, wrapped in
/// `sutura_catalog_datacontract::DataContractCatalog`.
///
/// Refuses a missing directory before constructing the adapter; folder reads happen at `load`.
fn open_one_data_contract_catalog(
    settings: &sutura_config::CatalogSettings,
) -> Result<sutura_catalog_datacontract::DataContractCatalog, String> {
    let dir = settings.dir().ok_or_else(|| {
        format!(
            "`catalogs.{}.dir` is required for catalog.kind: datacontract - this adapter reads a directory of contract documents",
            settings.name()
        )
    })?;
    Ok(sutura_catalog_datacontract::DataContractCatalog::new(
        settings.name().clone(),
        PathBuf::from(dir),
        settings.version().clone(),
    ))
}

/// Opens every declared `datahub` catalog, behind this crate's `datahub` feature.
#[cfg(feature = "datahub")]
fn open_datahub_catalogs(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    catalogs
        .each()
        .map(|settings| open_one_datahub_catalog(settings, outbound))
        .collect::<Result<Vec<_>, String>>()
        .map(OpenedCatalogs::Datahub)
}

/// The refusal for a build that did not link the `DataHub` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body** - `crate::bigquery`'s
/// `open_bigquery` precedent - so the dispatcher above has exactly one call and the compiler decides
/// which of these it reaches. The message names the FEATURE, for the same reason
/// `open_bigquery`'s does: an operator can act on "build with `--features datahub`", and
/// `cargo xtask check-feature-remedies` is what keeps that instruction honest.
#[cfg(not(feature = "datahub"))]
fn open_datahub_catalogs(
    _catalogs: &sutura_config::Catalogs,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    Err(String::from(
        "catalog.kind: datahub names a metadata adapter this binary was not built to link - build \
         sutura-cli with --features datahub, or declare markdown catalogs",
    ))
}

/// Reads a `catalog.kind: datahub`/`openmetadata`/`cube` entry's declared token file into a `Secret`, at
/// boot rather than on the first question - the same argument a source's `password_file` is
/// read at boot for. Trimmed, so a file ending in the newline a text editor or `echo` ordinarily writes
/// still reads as one token.
#[cfg(any(feature = "datahub", feature = "openmetadata", feature = "cube"))]
fn read_token(settings: &sutura_config::CatalogSettings, kind: &'static str) -> Result<sutura_domain::identity::Secret, String> {
    let path = settings.token_file().ok_or_else(|| {
        format!(
            "`catalogs.{}.token_file` is required for catalog.kind: {kind}",
            settings.name()
        )
    })?;
    let raw = std::fs::read_to_string(path)
        .map_err(|cause| format!("`catalogs.{}.token_file` could not be read: {cause}", settings.name()))?;
    Ok(sutura_domain::identity::Secret::new(raw.trim().to_owned()))
}

/// Opens one declared `datahub` catalog: the reader, bounded and authenticated, wrapped in
/// `sutura_catalog_datahub::DataHubCatalog`.
///
/// **The source mapping is `catalogs.<name>.platforms`**, each `DataHub` platform to the declared
/// `sources:` alias serving it (`docs/adr/0016` decision 7 leaves that mapping to the deployment;
/// `sutura-config` refuses an alias nobody declared). An entry that writes none keeps the single
/// `bigquery` platform served by the catalog's own name.
///
/// **The read bounds are `catalogs.<name>.deadline_seconds`/`max_response_bytes`, each defaulting
/// to the reader's own recommended constant when absent.** `docs/adr/0016`'s 2026-09-14 revision
/// asks for a deadline and a response-size cap that are settings with defaults, not constants -
/// `sutura_catalog_datahub::http::{DEFAULT_TIMEOUT_SECONDS, DEFAULT_MAX_RESPONSE_BYTES}` stay
/// constants IN THE READER (PR1's own decision: the adapter owns the range), and this composition
/// step is where an absent settings key resolves to them - the same single-owner split
/// `BytesBilledCeiling::parse` holds for `BigQuery`'s ceiling, read here rather than in
/// `sutura-config`, which does not depend on this adapter crate and so cannot name its constants.
///
/// **The endpoint is parsed here too, for the same single-owner reason.** A review on PR1 found
/// `HttpAspectReader::new` took a raw `String` and dialled a bearer over plaintext to any host;
/// PR1's fix is `sutura_catalog_datahub::http::Endpoint::parse` - `https://` for any host, `http://`
/// only for an IP loopback literal - and `HttpAspectReader::new` now REQUIRES one, so this
/// composition step cannot skip the parse even by accident: there is no other way to reach the
/// constructor. `sutura-config` still owns only "was the key written" (`CatalogSettings::endpoint`
/// stays an unparsed `Option<&str>`, since that crate cannot depend on this adapter's `Endpoint`
/// type); the adapter's own refusal (`InvalidEndpoint`) surfaces here as the settings-layer error a
/// deployment sees.
///
/// **`outbound` closes the loop from `main`'s ONE boot-time resolution of
/// `security.outbound.transport_anchors` to the reader's own `ureq` agent** - the same value, cloned,
/// that the `bigquery` wire folds into its `RootCerts::Specific`. `None` (no declared
/// `security.outbound`) hands the reader `ureq`'s compiled-in roots, the behaviour before #125;
/// the reader itself never reads the bundle a second time.
#[cfg(feature = "datahub")]
fn open_one_datahub_catalog(
    settings: &sutura_config::CatalogSettings,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::http::HttpAspectReader>, String> {
    use sutura_catalog_datahub::http::{
        DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT_SECONDS, Endpoint, HttpAspectReader, ReadBounds,
    };

    let endpoint_raw = settings.endpoint().ok_or_else(|| {
        format!(
            "`catalogs.{}.endpoint` is required for catalog.kind: datahub",
            settings.name()
        )
    })?;
    let endpoint = Endpoint::parse(endpoint_raw)
        .map_err(|cause| format!("`catalogs.{}.endpoint` is not a usable endpoint: {cause}", settings.name()))?;
    let property = settings
        .metric_property()
        .ok_or_else(|| {
            format!(
                "`catalogs.{}.metric_property` is required for catalog.kind: datahub",
                settings.name()
            )
        })?
        .to_owned();
    let token = read_token(settings, "datahub")?;
    let deadline_seconds = settings.deadline_seconds().unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let max_response_bytes = settings.max_response_bytes().unwrap_or(DEFAULT_MAX_RESPONSE_BYTES);
    let bounds = ReadBounds::parse(deadline_seconds, max_response_bytes).map_err(|cause| {
        format!(
            "`catalogs.{}.deadline_seconds`/`max_response_bytes` are not usable read bounds: {cause}",
            settings.name()
        )
    })?;
    let (reader, rotator) = HttpAspectReader::rotating_agent(bounds, outbound.cloned())
        .map_err(|cause| format!("`security.outbound.transport_anchors` could not be loaded: {cause}"))?;
    crate::rotation::drive_rotation("security.outbound.transport_anchors (datahub reader)", rotator);
    let reader = HttpAspectReader::rotating(endpoint, property, token, bounds, reader);
    let sources = if settings.platforms().is_empty() {
        std::collections::BTreeMap::from([(String::from("bigquery"), settings.name().clone())])
    } else {
        settings
            .platforms()
            .iter()
            .map(|(platform, source)| (String::from(platform.as_str()), source.clone()))
            .collect()
    };
    Ok(sutura_catalog_datahub::DataHubCatalog::new(
        settings.name().clone(),
        settings.version().clone(),
        sources,
        reader,
    ))
}

/// The catalog adapter's own error and every cause beneath it, on one line each.
fn flatten(error: &dyn core::error::Error) -> String {
    let mut out = error.to_string();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        out.push_str("\n  caused by: ");
        out.push_str(&cause.to_string());
        cursor = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::load_each;
    use sutura_catalog_local::LocalCatalog;
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::DefinitionVersion;

    /// A scratch directory of this test's own, cleared on the way in - same shape
    /// `sutura_catalog_local::tests::scratch` uses, since `tempfile` is not a dependency here either.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-cli-catalog-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// `LocalCatalog::capabilities` declares `MetadataCapabilities::everything()` regardless of a
    /// given directory's content, so a directory holding one model and no relationships composes
    /// to a `CompositionError::Unfaithful` naming `DefinitionKind::Relationships` - the kind
    /// `UnfaithfulDeclaration::Unprovided` already carries as a field. Before this fix,
    /// `cause.to_string()` rendered only `CompositionError::Unfaithful`'s own message, which does not
    /// interpolate that field, and the kind was reachable only by walking the `#[source]` chain.
    #[test]
    fn a_composition_refusal_names_the_kind_it_already_carries() {
        let root = scratch("unfaithful");
        std::fs::write(
            root.join("model.md"),
            "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents]\n---\n\
             Orders, one row per order.\n",
        )
        .expect("a document is writable");
        let catalog = LocalCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.clone(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        );
        let message = load_each(&[catalog]).expect_err("one model with no relationships is not everything");
        drop(std::fs::remove_dir_all(&root));
        assert!(
            message.contains("relationships"),
            "the flattened message should name the undersupplied kind: {message}"
        );
    }

    /// A tree's own `kind: declaration` document is held to its content at composition, so a
    /// hand-written one cannot over-claim (`github.com/telekom/sutura#1278`).
    #[test]
    fn a_declaration_document_that_over_claims_does_not_compose() {
        let root = scratch("over-claims");
        std::fs::write(
            root.join("model.md"),
            "---\nkind: model\nname: orders\nsource: local\ntable: fct_order\ncolumns: [amount_cents]\n---\n\
             Orders, one row per order.\n",
        )
        .expect("a document is writable");
        std::fs::write(
            root.join("declaration.md"),
            "---\nkind: declaration\ndefinitions: [structure, descriptions, relationships]\nknowledge: []\n---\n",
        )
        .expect("a document is writable");
        let catalog = LocalCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.clone(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        );
        let message = load_each(&[catalog]).expect_err("a tree declaring relationships it lacks does not compose");
        drop(std::fs::remove_dir_all(&root));
        assert!(
            message.contains("declares relationships, and the bundle carries none"),
            "the refusal should name the over-claimed kind: {message}"
        );
    }
}

#[cfg(test)]
mod planned_tests;
