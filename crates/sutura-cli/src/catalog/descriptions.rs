//! Explicitly referenced catalogs contribute prose only. Unrelated entries retain their role.

use super::{OpenedCatalogs, native};
use sutura_catalog_rdbms::descriptions::{DescriptionOverlay, MappedDescriptions};
use sutura_catalog_rdbms::{AnyDictionaryReader, Dictionary, DictionaryReader, RdbmsError};
use sutura_config::catalog::rdbms::{DescriptionMapping, DescriptionSource};
use sutura_domain::model::SourceName;

#[derive(Debug)]
pub(crate) struct Reader {
    base: AnyDictionaryReader,
    sources: Vec<Input>,
}

#[derive(Debug)]
enum Input {
    Catalog(Box<OpenedCatalogs>),
    /// A table mapping's reader, with whether its key matches the main dictionary
    /// case-insensitively, and what an unusable matched row's prose does.
    Mapping(
        AnyDictionaryReader,
        bool,
        sutura_catalog_rdbms::descriptions::InvalidDescriptionPolicy,
    ),
}

impl Reader {
    pub(super) const fn new(base: AnyDictionaryReader) -> Self {
        Self {
            base,
            sources: Vec::new(),
        }
    }
}

impl DictionaryReader for Reader {
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
        let dictionary = self.base.read_dictionary()?;
        if self.sources.is_empty() {
            return Ok(dictionary);
        }
        let bounds = self.base.bounds();
        let overlays = self
            .sources
            .iter()
            .map(|input| match input {
                Input::Catalog(catalog) => {
                    let bundle = super::load(catalog).map_err(|message| RdbmsError::Read(message.into()))?;
                    DescriptionOverlay::from_definitions(bundle.definitions(), bounds)
                        .map_err(|cause| RdbmsError::Read(Box::new(cause)))
                }
                Input::Mapping(reader, case_insensitive, on_invalid_description) => DescriptionOverlay::from_dictionary(
                    &reader.read_dictionary()?,
                    reader.bounds(),
                    *case_insensitive,
                    *on_invalid_description,
                )
                .map_err(|cause| RdbmsError::Read(Box::new(cause))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (dictionary, ignored_columns) = dictionary.overlay_descriptions(&overlays)?;
        tracing::info!(ignored_columns, "description overlay load completed");
        Ok(dictionary)
    }
}

pub(super) fn open(
    catalogs: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    let structural = catalogs.structural_catalogs().map_err(|cause| cause.to_string())?;
    // Open the original structural set by its existing kind/mixed rules, without recursively
    // treating its reference declarations as another structural set.
    let mut opened = super::open_catalog_without_descriptions(&structural, outbound)?;
    match &mut opened {
        OpenedCatalogs::Rdbms(entries) | OpenedCatalogs::RdbmsWithMarkdown { rdbms: entries, .. } => {
            let mut replacement = Vec::with_capacity(entries.len());
            for catalog in std::mem::take(entries) {
                let settings = structural
                    .each()
                    .filter(|entry| entry.kind() == sutura_config::CatalogKind::Rdbms)
                    .nth(replacement.len())
                    .ok_or_else(|| String::from("description catalog structural order is inconsistent"))?;
                replacement.push(attach(catalog, settings, catalogs, outbound)?);
            }
            *entries = replacement;
        }
        _ => return Err(String::from("description overlays require an RDBMS structural catalog")),
    }
    Ok(opened)
}

fn attach(
    catalog: native::Rdbms,
    settings: &sutura_config::CatalogSettings,
    all: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<native::Rdbms, String> {
    let sources = inputs(settings, all, outbound)?;
    Ok(catalog.map_reader(|mut reader| {
        reader.sources = sources;
        reader
    }))
}

fn referenced(
    name: &SourceName,
    all: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<OpenedCatalogs, String> {
    let settings = all
        .each()
        .find(|entry| entry.name() == name)
        .ok_or_else(|| format!("unknown description catalog {name}"))?;
    match settings.kind() {
        sutura_config::CatalogKind::Rdbms => {
            let catalog = super::open_one_rdbms_catalog(settings)?;
            Ok(OpenedCatalogs::Rdbms(vec![attach(catalog, settings, all, outbound)?]))
        }
        #[cfg(feature = "datahub")]
        sutura_config::CatalogKind::Datahub => Ok(OpenedCatalogs::Datahub(vec![super::open_one_datahub_catalog(
            settings, outbound,
        )?])),
        _ => Err(format!(
            "description catalog {name} requires a linked RDBMS or DataHub reader"
        )),
    }
}

fn inputs(
    settings: &sutura_config::CatalogSettings,
    all: &sutura_config::Catalogs,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<Vec<Input>, String> {
    let rdbms = settings
        .rdbms()
        .ok_or_else(|| String::from("description source owner is not RDBMS"))?;
    rdbms
        .descriptions_from()
        .iter()
        .map(|source| match source {
            DescriptionSource::Catalog(name) => referenced(name, all, outbound).map(|catalog| Input::Catalog(Box::new(catalog))),
            DescriptionSource::Table(mapping) => mapped_reader(settings, rdbms, mapping).map(|reader| {
                Input::Mapping(
                    reader,
                    mapping.case_insensitive(),
                    super::rdbms::map_on_invalid_description(mapping.on_invalid_description()),
                )
            }),
        })
        .collect()
}

fn mapped_reader(
    settings: &sutura_config::CatalogSettings,
    rdbms: &sutura_config::RdbmsSettings,
    mapping: &DescriptionMapping,
) -> Result<AnyDictionaryReader, String> {
    use sutura_catalog_rdbms::descriptions::{MappedKeySource, MappedPredicate, MappedTable, MappedVia};
    let key = match mapping.key_source() {
        sutura_config::catalog::rdbms::DescriptionKeySource::Direct { schema, table } => MappedKeySource::Direct {
            schema: schema.clone(),
            table: table.clone(),
        },
        sutura_config::catalog::rdbms::DescriptionKeySource::Via(via) => MappedKeySource::Via(MappedVia::new(
            via.table().clone(),
            via.on().to_vec(),
            via.schema().clone(),
            via.table_name().clone(),
        )),
    };
    let predicates = mapping
        .predicates()
        .iter()
        .map(|declared| {
            let table = match declared.table() {
                sutura_config::catalog::rdbms::PredicateTable::Main => MappedTable::Main,
                sutura_config::catalog::rdbms::PredicateTable::Via => MappedTable::Via,
            };
            super::rdbms::predicate(Some(declared.predicate())).map(|predicate| MappedPredicate::new(table, predicate))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let projection = MappedDescriptions::new(
        mapping.table().clone(),
        mapping.catalog_key().cloned(),
        key,
        mapping.column_key().clone(),
        mapping.description().clone(),
        predicates,
    );
    match mapping.connection() {
        sutura_config::CatalogConnection::Postgres(connection) => {
            let reader = super::rdbms::postgres_dictionary_reader(
                settings,
                connection,
                String::from("descriptions"),
                String::from("descriptions"),
                sutura_catalog_rdbms::postgres_reader::RowPredicate::None,
                rdbms,
            )?;
            reader
                .with_description_mapping(projection, mapping.max_rows(), mapping.max_bytes())
                .map(|reader| AnyDictionaryReader::Postgres(Box::new(reader)))
                .map_err(|cause| cause.to_string())
        }
        #[cfg(feature = "oracle")]
        sutura_config::CatalogConnection::Oracle(connection) => {
            let password =
                crate::password_file::read_key("description source connection.password_file", connection.password_file())?;
            let login = crate::oracle::dictionary_login(settings.name(), connection, password)?;
            let reader = sutura_catalog_rdbms::oracle_reader::OracleReader::new(
                login,
                "descriptions",
                String::from("descriptions"),
                sutura_catalog_rdbms::postgres_reader::RowPredicate::None,
                mapping.max_rows(),
                mapping.max_bytes(),
            )
            .map_err(|cause| cause.to_string())?;
            reader
                .with_description_mapping(projection, mapping.max_rows(), mapping.max_bytes())
                .map(|reader| AnyDictionaryReader::Oracle(Box::new(reader)))
                .map_err(|cause| cause.to_string())
        }
        #[cfg(not(feature = "oracle"))]
        sutura_config::CatalogConnection::Oracle(_) => Err(super::oracle_dictionary_not_linked(settings.name())),
    }
}
