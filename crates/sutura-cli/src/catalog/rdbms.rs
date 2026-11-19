pub(super) fn open_one_rdbms_catalog(settings: &sutura_config::CatalogSettings) -> Result<super::native::Rdbms, String> {
    use sutura_catalog_rdbms::AnyDictionaryReader;

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

    let predicate = predicate(rdbms.live_row_predicate())?;

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
        sutura_config::CatalogConnection::Oracle(_) => Err(super::oracle_dictionary_not_linked(settings.name())),
    }?;

    // The contribution manifest stays under the catalog NAME while the semantic models bind to the
    // declared `source_alias` - see `RdbmsCatalog::with_source_alias`.
    let reader = reader
        .with_relationships(relationships(rdbms)?)
        .with_relationships_view(rdbms.relationships_view().map(sutura_config::DocumentationSchema::as_str))
        .map_err(|cause| cause.to_string())?;
    let bounds = reader.bounds();
    let catalog = sutura_catalog_rdbms::RdbmsCatalog::new(
        settings.name().clone(),
        settings.version().clone(),
        super::descriptions::Reader::new(reader),
    )
    .with_source_alias(rdbms.source_alias().clone())
    .with_bounds(bounds);
    Ok(match rdbms.audience() {
        Some(audience) => catalog.with_audience(audience.clone()),
        None => catalog,
    })
}

/// The Postgres reader over the catalog's own read-only connection and declared transport.
#[cfg(feature = "rdbms")]
pub(super) fn postgres_dictionary_reader(
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

pub(super) fn predicate(
    value: Option<&sutura_config::LiveRowPredicate>,
) -> Result<sutura_catalog_rdbms::postgres_reader::RowPredicate, String> {
    use sutura_catalog_rdbms::postgres_reader::RowPredicate;
    Ok(match value {
        None => RowPredicate::None,
        Some(p) => match p.operator() {
            sutura_config::PredicateOperator::IsNull => RowPredicate::IsNull(p.column().as_str().to_owned()),
            sutura_config::PredicateOperator::IsNotNull => RowPredicate::IsNotNull(p.column().as_str().to_owned()),
            sutura_config::PredicateOperator::Equals => RowPredicate::Equals {
                column: p.column().as_str().to_owned(),
                value: p
                    .value()
                    .ok_or_else(|| String::from("equals predicate is missing its value"))?
                    .to_owned(),
            },
        },
    })
}

fn relationships(settings: &sutura_config::RdbmsSettings) -> Result<Vec<sutura_catalog_rdbms::DeclaredRelationship>, String> {
    use sutura_catalog_rdbms::{DeclaredRelationship, DictionaryBounds, Relationship, TableAddress};
    let address = |table: &sutura_domain::model::QualifiedTable| {
        let qualifier = table
            .qualifier()
            .ok_or_else(|| String::from("relationship endpoint requires schema"))?;
        Ok::<_, String>(TableAddress::new(
            qualifier.project().map(|name| name.as_str().to_owned()),
            qualifier.dataset().as_str().to_owned(),
            table.name().as_str().to_owned(),
        ))
    };
    settings
        .relationships()
        .iter()
        .map(|declaration| {
            let relationship = Relationship::declared(
                declaration.name().map(ToString::to_string),
                address(declaration.from().table())?,
                address(declaration.to().table())?,
                declaration
                    .from()
                    .columns()
                    .iter()
                    .zip(declaration.to().columns())
                    .map(|(origin, target)| (origin.as_str().to_owned(), target.as_str().to_owned()))
                    .collect(),
            )
            .map_err(|cause| cause.to_string())?;
            Ok(DeclaredRelationship::new(
                relationship,
                declaration
                    .check()
                    .map(|check| DictionaryBounds::new(check.max_rows(), check.max_bytes())),
            ))
        })
        .collect()
}
