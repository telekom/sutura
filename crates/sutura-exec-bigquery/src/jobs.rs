//! Read the labels and the principal of this project's jobs back from `INFORMATION_SCHEMA.JOBS`: the
//! live check of what a request-time job carried and whom it ran as. Behind `fixtures`, so no shipped build holds it.

use std::collections::BTreeSet;

use sutura_domain::identity::BillTo;
use sutura_domain::warehouse::labels::{ACTOR, CLIENT, CORRELATION};
use sutura_domain::warehouse::{ParamValue, RowSet};

use crate::transport::{JobDeadline, JobIdentity, JobRequest, JobTransport};
use crate::{BigQueryError, BigQueryWarehouse, Mapped};

/// The `REGION` of a `region-REGION` qualifier: the location of the jobs to read, such as `eu` or
/// `europe-west3`. Lowercase letters, digits and `-`, because it is written into the statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobsRegion(String);

/// Why a value is not a [`JobsRegion`]. Carries nothing of the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotAJobsRegion {
    /// Nothing but whitespace.
    #[error("a jobs region is empty")]
    Empty,
    /// A character outside lowercase letters, digits and `-`, after lowercasing.
    #[error("a jobs region is lowercase letters, digits and `-`, such as `eu` or `europe-west3`")]
    UnacceptableCharacter,
}

impl JobsRegion {
    /// A dataset location, case-insensitively, as `BigQuery` names one; a `region-` prefix is
    /// dropped, so `EU` and `region-eu` are one region.
    ///
    /// # Errors
    ///
    /// [`NotAJobsRegion`] for an empty value or any other character.
    pub fn parse(raw: &str) -> Result<Self, NotAJobsRegion> {
        let lowered = raw.trim().to_ascii_lowercase();
        let region = lowered.strip_prefix("region-").unwrap_or(&lowered);
        if region.is_empty() {
            return Err(NotAJobsRegion::Empty);
        }
        if !region
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(NotAJobsRegion::UnacceptableCharacter);
        }
        Ok(Self(String::from(region)))
    }
}

pub(super) fn labels<T>(warehouse: &BigQueryWarehouse<T>, region: &JobsRegion, correlation: &str) -> Mapped<RowSet, T::Error>
where
    T: JobTransport,
{
    let columns = format!(
        "(SELECT value FROM UNNEST(labels) WHERE key = '{CLIENT}') AS client, \
         (SELECT value FROM UNNEST(labels) WHERE key = '{ACTOR}') AS actor"
    );
    by_correlation(warehouse, region, correlation, &columns)
}

/// The principal `BigQuery` recorded each job under: one nullable `user_email` cell per job.
pub(super) fn principals<T>(warehouse: &BigQueryWarehouse<T>, region: &JobsRegion, correlation: &str) -> Mapped<RowSet, T::Error>
where
    T: JobTransport,
{
    by_correlation(warehouse, region, correlation, "user_email")
}

/// `columns` of each job in the last day whose correlation label is `correlation`.
fn by_correlation<T>(
    warehouse: &BigQueryWarehouse<T>,
    region: &JobsRegion,
    correlation: &str,
    columns: &str,
) -> Mapped<RowSet, T::Error>
where
    T: JobTransport,
{
    let payer = crate::billing::payer(&warehouse.billing, &BillTo::Unstated, &BTreeSet::new()).map_err(|_no_payer| {
        BigQueryError::NoPayer {
            at: String::from(warehouse.source.as_str()),
            projects: Vec::new(),
        }
    })?;
    // The project and the region are parsed values, the columns this module's own constants; the
    // correlation id is bound, never written.
    let statement = format!(
        "SELECT {columns} \
         FROM `{project}`.`region-{region}`.INFORMATION_SCHEMA.JOBS \
         WHERE creation_time > TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 1 DAY) \
         AND EXISTS (SELECT 1 FROM UNNEST(labels) WHERE key = '{CORRELATION}' AND value = ?)",
        project = payer.as_str(),
        region = region.0,
    );
    let params = [ParamValue::Text(String::from(correlation))];
    let request = JobRequest::new(&statement, &params, &payer, JobIdentity::Transport, JobDeadline::Boot);
    let answered = warehouse
        .transport
        .run(&request)
        .map_err(|cause| BigQueryError::Endpoint { cause })?;
    answered.to_rows().map_err(|cause| BigQueryError::Unreadable { cause })
}

#[cfg(test)]
mod tests {
    use super::{JobsRegion, NotAJobsRegion};

    #[test]
    fn a_jobs_region_is_a_lowercased_location_and_nothing_that_could_leave_the_identifier() {
        assert_eq!(JobsRegion::parse(" EU "), Ok(JobsRegion(String::from("eu"))));
        assert_eq!(JobsRegion::parse("region-eu"), Ok(JobsRegion(String::from("eu"))));
        assert_eq!(
            JobsRegion::parse("europe-west3"),
            Ok(JobsRegion(String::from("europe-west3")))
        );
        assert_eq!(JobsRegion::parse("  "), Err(NotAJobsRegion::Empty));
        for refused in ["eu`.x", "eu.x", "region eu"] {
            assert_eq!(
                JobsRegion::parse(refused),
                Err(NotAJobsRegion::UnacceptableCharacter),
                "{refused:?}"
            );
        }
    }
}
