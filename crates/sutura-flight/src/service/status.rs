//! What this node's own failure to run a remote leg becomes on the wire.

use sutura_app::surface::{SurfaceFailure, cause_chain};
use tonic::Status;

/// The message both of this node's own defects cross with.
const NOT_RUN: &str = "this node could not run the remote leg";

/// Maps a failure to a status, and logs the cause chain the caller must not be told.
///
/// One arm per variant and no wildcard, so a new variant does not compile here until it chooses a
/// code. The split follows the variants' own reasons: an outage is `unavailable` and worth
/// retrying, with the data system and the credential broker told apart because they are retried
/// and diagnosed differently; an unlinked account is `failed_precondition` with the closed code
/// `identity_not_linked`, because retrying does not help and the caller can act on it; this node's
/// own compile or wiring defect is `internal`. Every message is fixed text.
pub(super) fn failed(failure: &SurfaceFailure) -> Status {
    let chain = cause_chain(failure);
    match *failure {
        SurfaceFailure::Compile { ref cause } => {
            tracing::error!(error = %cause, causes = ?chain, "this node could not compile a remote leg");
            Status::internal(NOT_RUN)
        }
        SurfaceFailure::Warehouse { ref cause } => {
            tracing::error!(error = %cause, causes = ?chain, "the data system did not answer a remote leg");
            Status::unavailable("the data system did not answer")
        }
        SurfaceFailure::Broker { ref cause } => {
            tracing::error!(error = %cause, causes = ?chain, "the credential broker did not answer for a remote leg");
            Status::unavailable("the credential broker did not answer")
        }
        SurfaceFailure::NotLinked { ref cause } => {
            tracing::warn!(error = %cause, causes = ?chain, "a remote leg's caller has no linked account");
            Status::failed_precondition("identity_not_linked")
        }
        SurfaceFailure::Miswired { ref cause } => {
            tracing::error!(error = %cause, causes = ?chain, "the credentials that came back do not fit a remote leg");
            Status::internal(NOT_RUN)
        }
    }
}
