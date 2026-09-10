//! The explicit live driver. Closed failure classes avoid logging resource-bearing error chains.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sutura_domain::catalog::{Definitions, Description, Model};
use sutura_domain::model::{DatasetName, ProjectName, QualifiedTable, SourceName, TableName, TableQualifier};
use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
use sutura_domain::plan::{Executable, QueryPlan};
use sutura_domain::query::Query;
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content, agree_on_order};
use sutura_domain::warehouse::{RowSet, Warehouse as _};
use sutura_exec_bigquery::BigQueryError;
use sutura_exec_bigquery::wire::credential::{AccessTokens, Bearer, Credential, QuotaProject};
use sutura_exec_bigquery::wire::{BytesBilledCeiling, CallDeadline, JobBounds, QueryDeadline, WireError};

use crate::cross_resource_fixture::{Address, Failed, FixturePort, Layout, Mode, Operation, RawPlacement, RawVenue, run};
use crate::naming::{run_token, suffixed_bundle};
use crate::support::{Connection, Wired, bounds, opened, presented};

fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player")
}

fn source() -> SourceName {
    SourceName::parse("local").expect("the committed source name parses")
}

pub(crate) fn bundle() -> PinnedDefinitions {
    sutura_catalog_local::LocalCatalog::new(
        source(),
        example_root().join("catalog"),
        DefinitionVersion::parse("bigquery-cross-resource-1").expect("fixture version"),
    )
    .load()
    .expect("the committed catalog loads")
}

pub(crate) fn remap(pinned: &PinnedDefinitions, table: impl Fn(&Model) -> QualifiedTable) -> PinnedDefinitions {
    let models = pinned
        .definitions()
        .models()
        .values()
        .map(|model| {
            Model::new(
                model.name().clone(),
                model.source().clone(),
                table(model),
                model.columns().clone(),
                Description::parse(model.description()).expect("a committed description reparses"),
            )
        })
        .collect();
    let definitions = Definitions::assemble(
        models,
        pinned.definitions().relationships().values().cloned().collect(),
        pinned.definitions().metrics().values().cloned().collect(),
    )
    .expect("physical placement does not change catalog cross-references");
    PinnedDefinitions::pin(
        pinned.version().clone(),
        definitions,
        pinned.knowledge().clone(),
        pinned.manifest().clone(),
    )
    .expect("the readdressed catalog pins")
}

fn qualified(address: &Address, table: &TableName) -> QualifiedTable {
    QualifiedTable::new(
        Some(TableQualifier::in_project(
            ProjectName::parse(address.project.as_str()).expect("admission checked the domain project"),
            DatasetName::parse(address.dataset.as_str()).expect("admission checked the domain dataset"),
        )),
        table.clone(),
    )
}

fn plan(pinned: &PinnedDefinitions) -> Result<Box<QueryPlan>, Failed> {
    let question: Query = serde_norway::from_str(include_str!(
        "../../../../examples/single-player/questions/recurring-revenue-by-segment.yaml"
    ))
    .map_err(|cause| Failed::during(Operation::Query, cause))?;
    match sutura_semantic::compile(&question, pinned).map_err(|cause| Failed::during(Operation::Query, cause))? {
        sutura_semantic::Compiled::Planned { plan } if plan.source() == &source() && !plan.joins().is_empty() => Ok(plan),
        _ => Err(Failed::Query),
    }
}

fn same_rows(expected: &RowSet, actual: &RowSet) -> bool {
    !expected.rows().is_empty()
        && agree_on_content(expected, actual, RealTolerance::DIFFERENTIAL).is_ok()
        && agree_on_order(expected, actual, RealTolerance::DIFFERENTIAL).is_ok()
}

fn dimension(model: &Model) -> bool {
    model.name().as_str() == "customers"
}

struct BorrowedTokens<'a>(&'a Credential);

impl AccessTokens for BorrowedTokens<'_> {
    type Error = <Credential as AccessTokens>::Error;

    fn quota_project(&self) -> QuotaProject {
        self.0.quota_project()
    }

    fn bearer(&self, now_unix_seconds: u64, within: CallDeadline) -> Result<Bearer, Self::Error> {
        self.0.bearer(now_unix_seconds, within)
    }
}

struct Live {
    committed: PinnedDefinitions,
    local: PinnedDefinitions,
    connection: Option<Connection>,
}

impl Live {
    fn warehouse(&self, address: &Address, job_bounds: JobBounds) -> Result<Wired<BorrowedTokens<'_>>, Failed> {
        let connection = self.connection.as_ref().ok_or(Failed::Credential)?;
        // Every job, including fixture writes, retains the originally admitted billing project.
        Ok(opened(
            source(),
            Connection {
                billing_project: connection.billing_project.clone(),
                dataset: address.dataset.clone(),
                credentials: BorrowedTokens(&connection.credentials),
            },
            job_bounds,
        ))
    }

    fn oracle(&self) -> Result<RowSet, Failed> {
        let declared = || {
            sutura_domain::source::SharedIdentityDeclared::of(
                sutura_domain::source::AcknowledgementReason::parse("committed CSV fixtures read in this process")
                    .expect("fixture acknowledgement"),
            )
        };
        let engine = sutura_exec_datafusion::DataFusionWarehouse::new(
            source(),
            sutura_domain::source::SourcePosture::SharedServiceUser { declared: declared() },
            sutura_exec_datafusion::WorkingSet::of_bytes(
                core::num::NonZeroUsize::new(1 << 30).expect("positive fixture ceiling"),
            ),
        )
        .map_err(|cause| Failed::during(Operation::Query, cause))?;
        for (name, model) in self.local.definitions().models() {
            let original = &self.committed.definitions().models()[name];
            engine
                .attach_csv(
                    model.table_name(),
                    &example_root().join("data").join(format!("{}.csv", original.table_name())),
                )
                .map_err(|cause| Failed::during(Operation::Query, cause))?;
        }
        let presented = sutura_domain::identity::Presented::SharedServiceUser { declared: declared() };
        let query = plan(&self.local)?;
        engine
            .execute(Executable::Query(&query), &presented)
            .map_err(|cause| Failed::during(Operation::Query, cause))
    }
}

impl FixturePort for Live {
    fn open(&mut self, layout: &Layout) -> Result<(), Failed> {
        let connection = Connection::required();
        if connection.billing_project != layout.default.project || connection.dataset != layout.default.dataset {
            return Err(Failed::Credential);
        }
        self.connection = Some(connection);
        Ok(())
    }

    fn load(&mut self, layout: &Layout) -> Result<(), Failed> {
        let loading = || {
            JobBounds::of(
                QueryDeadline::parse(120).expect("fixture deadline"),
                BytesBilledCeiling::parse(1 << 30).expect("fixture ceiling"),
            )
        };
        for (name, model) in self.local.definitions().models() {
            let original = &self.committed.definitions().models()[name];
            let csv = example_root().join("data").join(format!("{}.csv", original.table_name()));
            let rows = self
                .warehouse(&layout.models[name], loading())?
                .load_fixture(model.table_name(), &csv)
                .map_err(|cause| Failed::during(Operation::Load, cause))?;
            if rows == 0 {
                return Err(Failed::Load);
            }
            if dimension(original) {
                self.warehouse(&layout.default, loading())?
                    .load_fixture(
                        model.table_name(),
                        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cross_resource_fixture/cross_resource_shadow.csv"),
                    )
                    .map_err(|cause| Failed::during(Operation::Load, cause))?;
            }
        }
        Ok(())
    }

    fn compare(&mut self, layout: &Layout) -> Result<(), Failed> {
        let expected = self.oracle()?;
        let located = remap(&self.local, |model| {
            qualified(&layout.models[model.name()], model.table_name())
        });
        let warehouse = self.warehouse(&layout.default, bounds())?;
        let positive = plan(&located)?;
        let first = warehouse
            .execute(Executable::Query(&positive), &presented())
            .map_err(|cause| Failed::during(Operation::Query, cause))?;
        if !same_rows(&expected, &first) {
            return Err(Failed::Query);
        }

        // Dropping a qualifier must not accidentally read equivalent data in the default dataset.
        let shadow = plan(&self.local)?;
        let wrong = warehouse
            .execute(Executable::Query(&shadow), &presented())
            .map_err(|cause| Failed::during(Operation::Query, cause))?;
        if wrong.rows().is_empty() || same_rows(&expected, &wrong) {
            return Err(Failed::Query);
        }

        let absent = remap(&located, |model| {
            if dimension(model) {
                qualified(&layout.absent, model.table_name())
            } else {
                model.table().clone()
            }
        });
        let negative = plan(&absent)?;
        match warehouse.execute(Executable::Query(&negative), &presented()) {
            Err(BigQueryError::Endpoint {
                cause: WireError::Refused { status: 404, named, .. },
            }) if named == "notFound" => {}
            _ => return Err(Failed::Negative),
        }
        let after = warehouse
            .execute(Executable::Query(&positive), &presented())
            .map_err(|cause| Failed::during(Operation::Query, cause))?;
        if !same_rows(&expected, &after) {
            return Err(Failed::Query);
        }
        println!(
            "bigquery-cross-resource: one-source join agreed; shadow differed; endpoint not-found followed by positive control"
        );
        Ok(())
    }

    fn drop_tables(&mut self, layout: &Layout) -> Result<(), Failed> {
        let mut failure = None;
        for (name, model) in self.local.definitions().models() {
            if let Err(cause) = self.warehouse(&layout.models[name], bounds())?.drop_table(model.table_name()) {
                failure = Some(Failed::during(Operation::Drop, cause));
            }
            if dimension(model)
                && let Err(cause) = self.warehouse(&layout.default, bounds())?.drop_table(model.table_name())
            {
                failure = Some(Failed::during(Operation::Drop, cause));
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

pub(crate) fn execute(mode: Mode) -> Result<(), Failed> {
    let read = |name| std::env::var(name).unwrap_or_default();
    let billing = read("SUTURA_BQ_CROSS_BILLING_PROJECT");
    let default = read("SUTURA_BQ_DATASET");
    let protected_project = read("SUTURA_BQ_RLS_PROJECT");
    let protected_dataset = read("SUTURA_BQ_RLS_DATASET");
    let (project_key, dataset_key) = match mode {
        Mode::WritableDatasets => ("SUTURA_BQ_CROSS_DATASET_PROJECT", "SUTURA_BQ_CROSS_DATASET"),
        Mode::ReadOnlyProjects => ("SUTURA_BQ_MIRROR_PROJECT", "SUTURA_BQ_MIRROR_DATASET"),
    };
    let other_project = read(project_key);
    let other_dataset = read(dataset_key);
    let token = run_token();
    let absent = format!("sutura_absent_{token}");
    let committed = bundle();
    let local = match mode {
        Mode::WritableDatasets => suffixed_bundle(&committed, &token, "cross"),
        Mode::ReadOnlyProjects => committed.clone(),
    };
    let placements: Vec<_> = committed
        .definitions()
        .models()
        .values()
        .map(|model| RawPlacement {
            model: model.name().as_str(),
            address: if dimension(model) {
                (&other_project, &other_dataset)
            } else {
                (&billing, &default)
            },
        })
        .collect();
    let expected: BTreeSet<_> = committed.definitions().models().keys().cloned().collect();
    let raw = RawVenue {
        billing: &billing,
        default: (&billing, &default),
        protected: (&protected_project, &protected_dataset),
        absent: (&billing, &absent),
    };
    let mut port = Live {
        committed: committed.clone(),
        local,
        connection: None,
    };
    run(&raw, &placements, &expected, mode, &mut port)
}
