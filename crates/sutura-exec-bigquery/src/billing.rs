//! Which project a job bills, in the order the deployment declared.

use std::collections::BTreeSet;

use sutura_domain::identity::{BillTo, BillingAccount};
use sutura_domain::model::QualifiedTable;
use sutura_domain::plan::{Executable, PlanJoin};

use crate::transport::ProjectId;

/// Who this source's jobs bill, as the composition root read it from the source's `billing` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobBilling {
    /// Each job runs as its caller and bills, in order: the project of the caller's mapped group,
    /// else the one project every table is in, else the payer the caller named where
    /// `caller_may_name`.
    CallerRun { caller_may_name: bool },
    /// Every job runs as the deployment's service account and bills this project.
    ServiceAccount(ProjectId),
}

/// Why no project was chosen, so the job does not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoPayer {
    /// The leg states a payer this source does not bill.
    NotTaken,
    /// No mapped group, not one project, and nothing named.
    Undecided,
}

/// A table path with no project, which nothing here resolves against a default.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the table `{table}` names no project: write it as `project.dataset.table`")]
pub struct TableNamesNoProject {
    table: String,
}

impl TableNamesNoProject {
    /// The path as written.
    #[inline]
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }
}

/// The distinct projects `tables` live in.
///
/// # Errors
///
/// [`TableNamesNoProject`] for the first path without one.
pub(crate) fn projects<'p>(
    tables: impl IntoIterator<Item = &'p QualifiedTable>,
) -> Result<BTreeSet<&'p str>, TableNamesNoProject> {
    tables
        .into_iter()
        .map(|table| {
            table
                .qualifier()
                .and_then(|qualifier| qualifier.project())
                .map(sutura_domain::model::ProjectName::as_str)
                .ok_or_else(|| TableNamesNoProject {
                    table: table.to_string(),
                })
        })
        .collect()
}

/// The distinct projects one statement's tables live in: its own table and every join's.
///
/// # Errors
///
/// [`TableNamesNoProject`] for the first path without one.
pub(crate) fn executable_projects(executable: Executable<'_>) -> Result<BTreeSet<&str>, TableNamesNoProject> {
    let (table, joins) = match executable {
        Executable::Query(plan) => (plan.table(), plan.joins()),
        Executable::Leg(leg) => (leg.tables().table(), leg.tables().joins()),
    };
    projects(core::iter::once(table).chain(joins.iter().map(PlanJoin::table)))
}

/// The project a job over `projects` bills.
///
/// # Errors
///
/// [`NoPayer::NotTaken`] for a payer stated where this source takes none, or one that is not a
/// project id; [`NoPayer::Undecided`] when no step of the order answers.
pub(crate) fn payer(billing: &JobBilling, bill_to: &BillTo, projects: &BTreeSet<&str>) -> Result<ProjectId, NoPayer> {
    let caller_may_name = match *billing {
        JobBilling::ServiceAccount(ref project) => {
            return match *bill_to {
                BillTo::Unstated => Ok(project.clone()),
                BillTo::Group(_) | BillTo::Named(_) => Err(NoPayer::NotTaken),
            };
        }
        JobBilling::CallerRun { caller_may_name } => match *bill_to {
            BillTo::Group(ref account) => return project_of(account),
            BillTo::Named(_) | BillTo::Unstated => caller_may_name,
        },
    };
    if let BillTo::Named(_) = *bill_to
        && !caller_may_name
    {
        return Err(NoPayer::NotTaken);
    }
    let mut each = projects.iter();
    if let (Some(only), None) = (each.next(), each.next())
        && let Ok(project) = ProjectId::parse(only)
    {
        return Ok(project);
    }
    match *bill_to {
        BillTo::Named(ref account) => project_of(account),
        BillTo::Group(_) | BillTo::Unstated => Err(NoPayer::Undecided),
    }
}

fn project_of(account: &BillingAccount) -> Result<ProjectId, NoPayer> {
    ProjectId::parse(account.as_str()).map_err(|_unusable| NoPayer::NotTaken)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sutura_domain::identity::{BillTo, BillingAccount};

    use super::{JobBilling, NoPayer, payer};
    use crate::transport::ProjectId;

    const CALLER_RUN: JobBilling = JobBilling::CallerRun { caller_may_name: false };
    const MAY_NAME: JobBilling = JobBilling::CallerRun { caller_may_name: true };

    fn account(name: &str) -> BillingAccount {
        BillingAccount::parse(name).expect("a test account is an account")
    }

    fn billed(billing: &JobBilling, bill_to: &BillTo, projects: &[&str]) -> Result<String, NoPayer> {
        let projects: BTreeSet<&str> = projects.iter().copied().collect();
        payer(billing, bill_to, &projects).map(|project| String::from(project.as_str()))
    }

    #[test]
    fn a_mapped_group_bills_its_project_before_the_tables_one() {
        let group = BillTo::Group(account("sales-dev-1234"));
        assert_eq!(
            billed(&CALLER_RUN, &group, &["acme-data"]),
            Ok(String::from("sales-dev-1234"))
        );
    }

    #[test]
    fn with_no_group_a_one_project_question_bills_that_project_even_when_a_payer_is_named() {
        assert_eq!(
            billed(&CALLER_RUN, &BillTo::Unstated, &["acme-data"]),
            Ok(String::from("acme-data"))
        );
        let named = BillTo::Named(account("caller-pays-1"));
        assert_eq!(billed(&MAY_NAME, &named, &["acme-data"]), Ok(String::from("acme-data")));
    }

    #[test]
    fn a_two_project_question_bills_the_named_payer_else_is_refused() {
        let named = BillTo::Named(account("caller-pays-1"));
        assert_eq!(
            billed(&MAY_NAME, &named, &["acme-a", "acme-b"]),
            Ok(String::from("caller-pays-1"))
        );
        assert_eq!(
            billed(&MAY_NAME, &BillTo::Unstated, &["acme-a", "acme-b"]),
            Err(NoPayer::Undecided)
        );
    }

    #[test]
    fn a_payer_named_where_the_source_takes_none_is_refused_never_dropped() {
        let named = BillTo::Named(account("caller-pays-1"));
        assert_eq!(billed(&CALLER_RUN, &named, &["acme-data"]), Err(NoPayer::NotTaken));
        let shared = JobBilling::ServiceAccount(ProjectId::parse("ops-1234").expect("a test project"));
        assert_eq!(billed(&shared, &named, &["acme-data"]), Err(NoPayer::NotTaken));
        let not_a_project = BillTo::Named(account("Not_A_Project"));
        assert_eq!(
            billed(&MAY_NAME, &not_a_project, &["acme-a", "acme-b"]),
            Err(NoPayer::NotTaken)
        );
    }

    #[test]
    fn a_shared_source_bills_its_service_account_project_whatever_the_tables() {
        let shared = JobBilling::ServiceAccount(ProjectId::parse("ops-1234").expect("a test project"));
        assert_eq!(
            billed(&shared, &BillTo::Unstated, &["acme-a", "acme-b"]),
            Ok(String::from("ops-1234"))
        );
    }
}
