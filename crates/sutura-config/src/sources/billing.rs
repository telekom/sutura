//! Who a `bigquery` source's jobs are billed to, read and refused.

use crate::raw::RawBilling;
use crate::sources::placement::{BillingProject, InvalidResourceName};
use sutura_domain::identity::{BillingAccount, UnusableBillingAccount};

/// Who pays for a `bigquery` source's jobs, by posture.
///
/// An enum, so a shared source with a group map, or a caller-run source with a fixed payer, cannot
/// be written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceBilling {
    /// Each job runs as its caller and bills, in order: the first `by_group` entry whose group the
    /// caller holds, else the one project the tables are in, else - where `caller_may_name` - the
    /// project the caller named.
    CallerRun {
        by_group: Vec<(String, BillingAccount)>,
        caller_may_name: bool,
    },
    /// Every job runs as the deployment's service account and bills this project.
    ServiceAccount { project: BillingProject },
}

impl SourceBilling {
    /// The account the first `by_group` entry the caller's groups hold maps onto.
    #[must_use]
    pub fn for_groups(&self, groups: &sutura_domain::identity::CallerGroups) -> Option<&BillingAccount> {
        match *self {
            Self::CallerRun { ref by_group, .. } => by_group
                .iter()
                .find(|&(group, _)| groups.holds(group))
                .map(|(_, account)| account),
            Self::ServiceAccount { .. } => None,
        }
    }

    /// Whether a caller may name the project a job bills.
    #[must_use]
    pub const fn caller_may_name(&self) -> bool {
        matches!(
            *self,
            Self::CallerRun {
                caller_may_name: true,
                ..
            }
        )
    }
}

/// Why a source's `billing` block, or a removed payer key on it, is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidBilling {
    #[error(
        "`billing_project` is removed: a caller-run source bills `billing.by_group`, then the tables' \
         project; a shared source bills `billing.service_account_project`"
    )]
    BillingProjectRemoved,
    #[error("`dataset` is removed: name every table as `project.dataset.table`")]
    DatasetRemoved,
    #[error("`billing` is read only on a `bigquery` source")]
    BillingOnOtherKind,
    #[error("`billing.by_group[{index}].group` is empty")]
    EmptyGroup { index: usize },
    #[error("`billing.by_group[{index}].project` is not a project id: {cause}")]
    GroupProject {
        index: usize,
        #[source]
        cause: InvalidResourceName,
    },
    #[error("`billing.by_group[{index}].project` is not a billing account: {cause}")]
    GroupAccount {
        index: usize,
        #[source]
        cause: UnusableBillingAccount,
    },
    #[error("`billing.by_group` lists the group `{group}` twice")]
    GroupTwice { group: String },
    #[error("`billing.{key}` is refused on a shared-service-user source: its jobs run, and bill, as the service account")]
    CallerBillingOnSharedSource { key: &'static str },
    #[error("a shared-service-user `bigquery` source declares `billing.service_account_project`")]
    ServiceAccountProjectMissing,
    #[error("`billing.service_account_project` is read only on a shared-service-user source")]
    ServiceAccountProjectOnCallerRun,
    #[error("`billing.service_account_project` is not a project id: {cause}")]
    ServiceAccountProject {
        #[source]
        cause: InvalidResourceName,
    },
}

/// Reads a `bigquery` source's `billing` block for its posture.
///
/// # Errors
///
/// [`InvalidBilling`] for a key the posture refuses, a missing service account project, or an
/// unusable group or project.
pub(super) fn parse(raw: Option<&RawBilling>, shared: bool) -> Result<SourceBilling, InvalidBilling> {
    let empty = RawBilling::default();
    let raw = raw.unwrap_or(&empty);
    if shared {
        if !raw.by_group.is_empty() {
            return Err(InvalidBilling::CallerBillingOnSharedSource { key: "by_group" });
        }
        if raw.caller_may_name {
            return Err(InvalidBilling::CallerBillingOnSharedSource { key: "caller_may_name" });
        }
        let written = raw
            .service_account_project
            .as_deref()
            .ok_or(InvalidBilling::ServiceAccountProjectMissing)?;
        let project = BillingProject::parse(written).map_err(|cause| InvalidBilling::ServiceAccountProject { cause })?;
        return Ok(SourceBilling::ServiceAccount { project });
    }
    if raw.service_account_project.is_some() {
        return Err(InvalidBilling::ServiceAccountProjectOnCallerRun);
    }
    let mut by_group: Vec<(String, BillingAccount)> = Vec::with_capacity(raw.by_group.len());
    for (index, entry) in raw.by_group.iter().enumerate() {
        if entry.group.trim().is_empty() {
            return Err(InvalidBilling::EmptyGroup { index });
        }
        if by_group.iter().any(|(group, _)| *group == entry.group) {
            return Err(InvalidBilling::GroupTwice {
                group: entry.group.clone(),
            });
        }
        let project = BillingProject::parse(&entry.project).map_err(|cause| InvalidBilling::GroupProject { index, cause })?;
        let account = BillingAccount::parse(project.as_str()).map_err(|cause| InvalidBilling::GroupAccount { index, cause })?;
        by_group.push((entry.group.clone(), account));
    }
    Ok(SourceBilling::CallerRun {
        by_group,
        caller_may_name: raw.caller_may_name,
    })
}
