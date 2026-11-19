//! Scoped query plans use the same identity, billing, deadline and transport bounds as metrics.

use std::collections::BTreeSet;

use sutura_domain::identity::Presented;
use sutura_domain::model::QualifiedTable;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::RawRows;
use sutura_domain::warehouse::deadline::Deadline;

use crate::transport::{JobDeadline, JobRequest, JobTransport};
use crate::{BigQueryWarehouse, Mapped, billing};

pub(super) fn run<T>(
    warehouse: &BigQueryWarehouse<T>,
    statement: &RawStatement,
    tables: &BTreeSet<QualifiedTable>,
    presented: &Presented,
    deadline: Deadline,
) -> Mapped<RawRows, T::Error>
where
    T: JobTransport,
{
    warehouse.deliverable(presented)?;
    let identity = warehouse.job_identity(presented)?;
    let payer = warehouse.unstated_payer(&billing::projects(tables)?)?;
    let rows = warehouse
        .transport
        .run(&JobRequest::new(
            statement.as_str(),
            &[],
            &payer,
            identity,
            JobDeadline::Port(deadline),
        ))
        .map_err(|cause| crate::BigQueryError::Endpoint { cause })?;
    let (columns, rows) = BigQueryWarehouse::<T>::rows(&rows)?.into_parts();
    Ok(RawRows::of(columns, rows))
}
