//! The `bigquery` registration: the provisioned dataset, the tables it holds, and the catalog its
//! cells read - every table named `project.dataset.table`, because this source resolves no bare path.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sutura_domain::model::{DatasetName, ProjectName, QualifiedTable, SourceName, TableName, TableQualifier};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_exec_bigquery::BigQueryWarehouse;
use sutura_exec_bigquery::adbc::{AdbcBigQuery, BytesBilledCeiling, DriverLocation, Impersonation};
use sutura_exec_bigquery::transport::{DatasetId, ProjectId};

use super::{DIM_CALENDAR, DataSystemUnderTest, FCT_TICKET_MONTHLY, local_bundle, posture, schema_counter, set};

/// One table the `bigquery` dataset holds, and what the provisioning cell loads into it: `None` for an
/// example-corpus table, which loads its committed CSV, and the rows of a two-fact table the corpus
/// lacks.
pub(crate) type DatasetTable = (&'static str, Option<&'static str>);

/// Every table the `bigquery` dataset holds. The derived corpus's APPENDED rows are not here: the
/// dataset holds one `fct_subscription_monthly`, the committed one every other cell reads.
pub(crate) const BIGQUERY_TABLES: [DatasetTable; 7] = [
    ("dim_customer", None),
    ("dim_product", None),
    ("dim_region", None),
    ("fct_subscription_monthly", None),
    ("fct_usage_daily", None),
    ("dim_calendar", Some(DIM_CALENDAR)),
    ("fct_ticket_monthly", Some(FCT_TICKET_MONTHLY)),
];

/// `BigQuery`: cloud-only, so its venue is the dataset the `bigquery-conformance` CI job loads
/// [`BIGQUERY_TABLES`] into before this axis runs. Every cell READS those tables and none writes, so
/// an open over any other table is refused rather than loaded: it would replace a table other cells
/// are reading, and one dataset has no per-open schema to give it. **Its limit:** a table is matched
/// by NAME, so a derived corpus's edit to an example table is not what runs here - the two-fact
/// differential's appended `fct_subscription_monthly` rows fall outside its question's range, which
/// is what lets that cell compare. It executes as the job's one CI identity under the matrix's
/// `shared-service-user` posture, so it says nothing about impersonation.
impl DataSystemUnderTest for BigQueryWarehouse<AdbcBigQuery> {
    const NAME: &'static str = "bigquery";

    fn available() -> bool {
        bigquery_venue().is_some()
    }

    fn open_on(name: SourceName, tables: Vec<(TableName, PathBuf)>) -> Self {
        for (table, _) in &tables {
            assert!(
                BIGQUERY_TABLES.iter().any(|&(held, _)| held == table.as_str()),
                "{table} is not a table the bigquery dataset holds, and loading it would replace a table \
                 the other cells are reading"
            );
        }
        bigquery_warehouse(name)
    }

    fn bundle_over(catalog: &Path) -> PinnedDefinitions {
        let venue = bigquery_venue().expect("`available()` guards every bigquery bundle");
        qualified_bundle(catalog, &qualifier(&venue.project, &venue.dataset))
    }
}

/// The markdown catalog at `catalog`, copied with every `table:` line named under `qualifier`.
///
/// **It refuses a bundle with a table left bare**, so a model written some other way than one
/// `table: <name>` line fails here rather than as each cell's "names no project". **The limit:** it
/// rewrites text, so a `table:` line outside a model's frontmatter would be rewritten too - none is.
pub(crate) fn qualified_bundle(catalog: &Path, qualifier: &TableQualifier) -> PinnedDefinitions {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("bigquery-catalog-{}-{}", std::process::id(), schema_counter()));
    drop(std::fs::remove_dir_all(&root));
    qualify_tree(catalog, &root, qualifier);
    let pinned = local_bundle(&root);
    for model in pinned.definitions().models().values() {
        assert_eq!(
            model.table().qualifier(),
            Some(qualifier),
            "{} names {}, not a table in the provisioned dataset",
            model.name(),
            model.table()
        );
    }
    pinned
}

fn qualify_tree(from: &Path, to: &Path, qualifier: &TableQualifier) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("could not create {}: {e}", to.display()));
    for entry in std::fs::read_dir(from).unwrap_or_else(|e| panic!("could not read {}: {e}", from.display())) {
        let entry = entry.expect("a directory entry is readable");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("an entry has a type").is_dir() {
            qualify_tree(&entry.path(), &target, qualifier);
            continue;
        }
        let text =
            std::fs::read_to_string(entry.path()).unwrap_or_else(|e| panic!("could not read {}: {e}", entry.path().display()));
        let rewritten: String = text
            .split_inclusive('\n')
            .map(|line| {
                line.strip_prefix("table: ").map_or_else(
                    || line.to_owned(),
                    |bare| {
                        let table = TableName::parse(bare.trim_end())
                            .unwrap_or_else(|e| panic!("{}: {bare:?} is not a bare table: {e}", entry.path().display()));
                        format!("table: {}\n", QualifiedTable::new(Some(qualifier.clone()), table))
                    },
                )
            })
            .collect();
        std::fs::write(&target, rewritten).unwrap_or_else(|e| panic!("could not write {}: {e}", target.display()));
    }
}

/// The provisioned dataset as a warehouse on source `name` - one address for the provisioning cell
/// that loads it and every cell that reads it.
pub(crate) fn bigquery_warehouse(name: SourceName) -> BigQueryWarehouse<AdbcBigQuery> {
    bigquery_provisioned(name).0
}

/// [`bigquery_warehouse`], with the qualifier its tables are named under: there is no default
/// dataset to resolve a bare name in.
pub(crate) fn bigquery_provisioned(name: SourceName) -> (BigQueryWarehouse<AdbcBigQuery>, TableQualifier) {
    let venue = bigquery_venue().expect("`available()` guards every bigquery open");
    (
        warehouse_billing(name, venue.project.clone(), venue.driver.clone()),
        qualifier(&venue.project, &venue.dataset),
    )
}

/// A warehouse on source `name` whose every job bills `project` through the driver at `driver`.
pub(crate) fn warehouse_billing(name: SourceName, project: ProjectId, driver: DriverLocation) -> BigQueryWarehouse<AdbcBigQuery> {
    BigQueryWarehouse::over_adbc(
        name,
        posture(),
        sutura_exec_bigquery::JobBilling::ServiceAccount(project),
        driver,
        Impersonation::Disabled,
        BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a ceiling"),
    )
}

/// `project.dataset`, as a table path's qualifier.
pub(crate) fn qualifier(project: &ProjectId, dataset: &DatasetId) -> TableQualifier {
    TableQualifier::in_project(
        ProjectName::parse(project.as_str()).expect("a project id is a project name"),
        DatasetName::parse(dataset.as_str()).expect("a dataset id is a dataset name"),
    )
}

/// Where a provisioned `BigQuery` dataset is, read once from the job's environment.
struct BigQueryVenue {
    project: ProjectId,
    dataset: DatasetId,
    driver: DriverLocation,
}

/// The `BigQuery` venue, or `None` on a host that names none.
///
/// **`SUTURA_BQ_DATASET` is the switch, and past it nothing is optional**: a half-configured venue
/// panics naming the variable, because one that skipped would be a green run over nothing. No value
/// is printed - a public log should not learn a resource name from a refusal.
///
/// **Past the switch the run must also be in CI**, so a live cell never runs from a developer's shell
/// even when every variable is set. **The limit:** `GITHUB_ACTIONS` is an environment value, so a shell
/// that exports `true` passes; and a cell that reads its own environment instead of this venue is not
/// held by it.
fn bigquery_venue() -> Option<&'static BigQueryVenue> {
    static CACHED: OnceLock<Option<BigQueryVenue>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let dataset = set("SUTURA_BQ_DATASET")?;
            assert!(
                set("GITHUB_ACTIONS").is_some_and(|ci| ci == "true"),
                "a live bigquery cell runs only in CI (GITHUB_ACTIONS=true); this host names a bigquery venue outside CI"
            );
            let required = |name: &str| {
                set(name).unwrap_or_else(|| panic!("SUTURA_BQ_DATASET names a bigquery venue and {name} is unset or empty"))
            };
            drop(required("GOOGLE_APPLICATION_CREDENTIALS"));
            Some(BigQueryVenue {
                project: ProjectId::parse(required("SUTURA_BQ_PROJECT"))
                    .unwrap_or_else(|_| panic!("SUTURA_BQ_PROJECT is not a project id")),
                dataset: DatasetId::parse(dataset).unwrap_or_else(|_| panic!("SUTURA_BQ_DATASET is not a dataset id")),
                driver: DriverLocation::parse(&required("SUTURA_BIGQUERY_ADBC_DRIVER"))
                    .unwrap_or_else(|_| panic!("SUTURA_BIGQUERY_ADBC_DRIVER is not an absolute path")),
            })
        })
        .as_ref()
}
