//! Why Foundry did not answer a plan.

use sutura_domain::identity::PresentedDisagreesWithPosture;
use sutura_domain::model::TableName;
use sutura_domain::warehouse::arrow::UnannouncedBatch;
use sutura_sql::GenerateError;

use crate::{DirectoryFailed, TokenFailed};

/// Why Foundry did not answer a plan.
#[derive(Debug, thiserror::Error)]
pub enum FoundryError<E>
where
    E: std::error::Error + 'static,
{
    #[error("the Foundry endpoint did not answer")]
    Endpoint {
        #[source]
        cause: E,
    },
    #[error("the plan could not be rendered as Foundry Ontology SQL")]
    Render {
        #[source]
        cause: GenerateError,
    },
    #[error("the source credential does not agree with the declared posture")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    #[error("the Foundry Arrow result could not be decoded")]
    Arrow {
        #[source]
        cause: arrow_schema::ArrowError,
    },
    #[error("the Foundry answer deadline was spent while decoding Arrow")]
    DeadlineSpent,
    #[error("the Foundry Arrow result did not match its announced schema")]
    Unannounced {
        #[source]
        cause: UnannouncedBatch,
    },
    #[error("a Foundry result cell could not be read as a domain value")]
    Unreadable {
        #[source]
        cause: sutura_domain::warehouse::UnreadableCell,
    },
    #[error("the Foundry source identifier is not the source selected by this plan")]
    WrongSource,
    #[error("the credential broker presented {presented}, which this Foundry source cannot send")]
    Undeliverable { presented: &'static str },
    /// An `impersonation-at-source` source has no identity of its own, so nothing at boot can run
    /// as anybody here: an anchor on it cannot be verified.
    #[error("this Foundry source runs each question as its caller and holds no identity to verify an anchor with")]
    NoBootIdentity,
    #[error("the deployment's Foundry token could not be obtained")]
    Token {
        #[source]
        cause: TokenFailed,
    },
    /// The object type behind `table`, named by API name, is in no fresh read of the object-type
    /// list - so it has no resource identifier to be queried by. Named by the table the catalog
    /// gives it, never by the API name.
    #[error("the object type behind {table} is not in the object-type list this token reads")]
    UnknownObjectType { table: TableName },
    /// The object type behind `table` was listed with no usable resource identifier.
    #[error("the object type behind {table} was listed with no usable resource identifier")]
    NoResourceId { table: TableName },
    /// `table` names an object type this source's `exclude_object_types` leaves out.
    #[error("{table} names an object type this source leaves out")]
    ExcludedObjectType { table: TableName },
    /// Foundry found no object type behind a resource identifier the statement named; the names
    /// resolved for it were dropped from the directory, so the next question reads them again.
    #[error("Foundry found no object type behind a resource identifier the statement named")]
    StaleObjectType {
        #[source]
        cause: E,
    },
    #[error("the object-type list that resolves API names to resource identifiers was not read")]
    Directory {
        #[source]
        cause: DirectoryFailed,
    },
    #[error("the Foundry endpoint does not support this plan shape")]
    UnsupportedPlan,
}
