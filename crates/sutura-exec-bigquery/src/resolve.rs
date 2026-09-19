//! What a table's own path resolves to, when it does not name its own project.
//!
//! One decision, used by two callers that used to make it separately:
//! [`crate::preflight::addressed`] for the boot check, and
//! [`crate::BigQueryWarehouse::render_query`] for the statement a query actually becomes.
//! `docs/adr/0019` is the decision this closes: a path that omits its project resolves against
//! *whichever project the job happens to run under*, silently - so a table that lives OUTSIDE the
//! connection's own project needs its project written into the statement, never left for the
//! request's `defaultDataset` to guess.
//!
//! **Only [`Qualification::Dataset`] changes, and that is a decision rather than an oversight.** A
//! bare path ([`Qualification::TableOnly`]) is already resolved, unambiguously, by the request's
//! own `defaultDataset` field - both its project and its dataset are the connection's, and nothing
//! about the path could mean anything else. A path naming its own project
//! ([`Qualification::ProjectAndDataset`]) already says everything this function could add. The one
//! case left is a path that names a dataset and no project: which project that resolves in is a
//! real decision, and [`resolve`] is where it is made rather than left to the wire.

use sutura_domain::model::{InvalidIdentifier, ProjectName, QualifiedTable, TableQualifier};

use crate::transport::ProjectId;

/// Why this connection's own billing project could not be read as the domain's project vocabulary.
///
/// **Unreachable in practice, and a typed branch rather than an `.expect()` because a panic here
/// would be reachable from a live query, not just a catalog file.**
/// `sutura_config::sources::placement::BillingProject::parse` accepts 6 to 30 characters of
/// `[a-z0-9-]`, starting with a letter and never ending in a hyphen - a strict subset of what
/// [`ProjectName::parse`] accepts, so a billing project that reached this adapter at all already
/// satisfies it.
#[derive(Debug, thiserror::Error)]
#[error("this connection's own billing project is not a usable project name: {0}")]
pub struct UnresolvableConnection(#[from] InvalidIdentifier);

/// This table's own path, with a missing project filled in from the connection - and left alone
/// otherwise.
///
/// See this module's own header for why a bare path is not touched here, and
/// [`crate::preflight::addressed`] for the sibling caller that makes the identical project
/// decision in its own, transport-native vocabulary.
pub(crate) fn resolve(table: &QualifiedTable, billing_project: &ProjectId) -> Result<QualifiedTable, UnresolvableConnection> {
    let Some(qualifier) = table.qualifier() else {
        return Ok(table.clone());
    };
    if qualifier.project().is_some() {
        return Ok(table.clone());
    }
    let project = ProjectName::parse(billing_project.as_str())?;
    Ok(QualifiedTable::new(
        Some(TableQualifier::in_project(project, qualifier.dataset().clone())),
        table.name().clone(),
    ))
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{DatasetName, ProjectName, QualifiedTable, TableName, TableQualifier};

    use super::resolve;
    use crate::transport::ProjectId;

    fn billing_project() -> ProjectId {
        ProjectId::parse("acme-analytics").expect("a test project is a project")
    }

    #[test]
    fn a_bare_table_is_left_unqualified() {
        let bare = QualifiedTable::from(TableName::parse("orders").expect("a test table is a table"));
        let resolved = resolve(&bare, &billing_project()).expect("a bare path always resolves");
        assert_eq!(
            resolved, bare,
            "the request's own defaultDataset already resolves a bare path"
        );
    }

    #[test]
    fn a_dataset_qualified_table_gains_the_connections_project() {
        let dataset_only = QualifiedTable::new(
            Some(TableQualifier::in_dataset(
                DatasetName::parse("sales").expect("a test dataset is a dataset"),
            )),
            TableName::parse("orders").expect("a test table is a table"),
        );
        let resolved = resolve(&dataset_only, &billing_project()).expect("the connection's project fills the gap");
        assert_eq!(resolved.to_string(), "acme-analytics.sales.orders");
    }

    #[test]
    fn a_project_qualified_table_is_left_exactly_as_written() {
        let already_qualified = QualifiedTable::new(
            Some(TableQualifier::in_project(
                ProjectName::parse("reference-data").expect("a test project is a project"),
                DatasetName::parse("crm").expect("a test dataset is a dataset"),
            )),
            TableName::parse("customers").expect("a test table is a table"),
        );
        let resolved = resolve(&already_qualified, &billing_project()).expect("a fully-qualified path always resolves");
        assert_eq!(
            resolved, already_qualified,
            "a path naming its own project is never overridden by the connection's"
        );
    }
}
