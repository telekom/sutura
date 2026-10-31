#![forbid(unsafe_code)]
//! A [`Warehouse`] whose legs another sutura node runs - node A's side of
//! `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`.
//!
//! A leg crosses as one Arrow Flight `DoExchange`: the typed leg ([`to_wire`]) in the descriptor,
//! its values as one parameter batch, and the caller's exchanged token as `authorization: Bearer`
//! metadata over server-authenticated TLS. The receiving node certifies the leg against its own
//! definitions and runs it as that token's subject. This adapter runs no SQL and holds no rows
//! beyond one leg's answer.
//!
//! # Identity
//!
//! Only [`Presented::SubjectToken`] is sent - the token the credential broker exchanged the
//! caller's for, with the node's audience. Every other shape is refused by name before anything is
//! dialled, as the Cube adapter refuses one, so a leg never reaches the node as the deployment.
//!
//! # What crosses back
//!
//! A refusal arrives as a closed code ([`RemoteCode`]) and is reported through the port's
//! predicates; the node's own text is never kept. The answer is bounded as it arrives by the
//! [`ResultBudget`] this source was built with, and its labels must be the leg's own.
//!
//! # Limits
//!
//! - **The source's name here must be the real source's name at the receiving node**: the leg
//!   names it and the node looks it up in its own registry. Nothing here checks that; a mismatch is
//!   the node's `RemoteLegNotCertified`.
//! - The node's other refusals (a spend ceiling, an unavailable source) and its `unauthenticated`
//!   leave as a failure, the `503` of an outage, not as the refusal or `identity_unavailable`.
//! - The trust anchors are read once, when the source is built; nothing rotates them.
//! - `execute` blocks its thread on this adapter's own one-worker runtime. Whether the deadline
//!   stops the node's own work is the node's matter.
//! - The gRPC client dials an `https` URI over this crate's own TLS. A build that turns on a TLS
//!   feature of the gRPC client refuses that URI for want of its own TLS configuration: it fails
//!   closed, and no cell here measures it.

mod call;
mod endpoint;
mod error;
mod params;

pub use endpoint::{InvalidEndpoint, NodeEndpoint};
pub use error::{NodeError, NotBuilt, RemoteCode};

use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::SourceName;
use sutura_domain::plan::remote::to_wire;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::ResultBudget;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, Warehouse};
use tokio_rustls::rustls::pki_types::CertificateDer;
use tonic::metadata::{Ascii, MetadataValue};

use crate::call::Call;

/// One `node` source: the legs another sutura node runs for it.
pub struct NodeWarehouse {
    source: SourceName,
    posture: SourcePosture,
    budget: ResultBudget,
    call: Call,
}

impl core::fmt::Debug for NodeWarehouse {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NodeWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl NodeWarehouse {
    /// A source whose legs `endpoint` runs, trusting `anchors` for its certificate and holding at
    /// most `budget` bytes of one leg's answer. Nothing is dialled.
    ///
    /// # Errors
    ///
    /// The runtime does not start, an anchor is not a usable certificate, or the endpoint's host is
    /// not a name a certificate can be checked against.
    pub fn open(
        source: SourceName,
        posture: SourcePosture,
        endpoint: &NodeEndpoint,
        anchors: impl IntoIterator<Item = CertificateDer<'static>>,
        budget: ResultBudget,
    ) -> Result<Self, NotBuilt> {
        Ok(Self {
            source,
            posture,
            budget,
            call: Call::open(endpoint, anchors)?,
        })
    }

    /// The token `presented` sends: a subject's exchanged token, and only where the leg agrees with
    /// this source's declared posture.
    fn deliverable<'leg>(&self, presented: &'leg Presented) -> Result<&'leg Secret, NodeError> {
        let Presented::SubjectToken { ref material } = *presented else {
            return Err(NodeError::Undeliverable {
                at: String::from(self.source.as_str()),
                presented: presented.as_str(),
            });
        };
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| NodeError::PresentedDisagreesWithPosture { cause })?;
        Ok(material)
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "a bearer has to reach the wire as text; this builds the one request header that carries it, \
              marked sensitive so the client never logs it"
)]
fn bearer(token: &Secret) -> Result<MetadataValue<Ascii>, NodeError> {
    let mut value: MetadataValue<Ascii> = format!("Bearer {}", token.expose_secret())
        .parse()
        .map_err(|_cause| NodeError::Bearer)?;
    value.set_sensitive(true);
    Ok(value)
}

impl Warehouse for NodeWarehouse {
    type Error = NodeError;

    /// The caller's exchanged token is the bearer of the leg's one call.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        let token = self.deliverable(presented)?;
        let leg = match executable {
            Executable::Leg(leg) => leg,
            Executable::Query(plan) => {
                return Err(NodeError::WholePlan {
                    table: plan.table().to_string(),
                });
            }
            Executable::Stacked(plan) => {
                return Err(NodeError::WholePlan {
                    table: plan.arms().first().table().to_string(),
                });
            }
        };
        let wire = to_wire(leg).map_err(|cause| NodeError::NotSendable { cause })?;
        let values = params::batch(leg.params()).map_err(|_cause| NodeError::Framing)?;
        let answer = self.call.exchange(&wire, values, bearer(token)?, (deadline, self.budget))?;
        let labels = executable.result_labels();
        if answer.schema().fields().iter().map(|field| field.name()).ne(labels.iter()) {
            return Err(NodeError::Labels);
        }
        Ok(answer)
    }

    /// A node source answers no boot anchor: the receiving node verifies its own.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(NodeError::WholePlan {
            table: plan.plan().table().to_string(),
        })
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(
            *error,
            NodeError::Refused {
                code: RemoteCode::SourceRefused
            }
        )
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(
            *error,
            NodeError::DeadlineExceeded
                | NodeError::Refused {
                    code: RemoteCode::DeadlineExceeded
                }
        )
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(
            *error,
            NodeError::Result { .. }
                | NodeError::Refused {
                    code: RemoteCode::ResultTooLarge
                }
        )
    }

    fn leg_not_certified(&self, error: &Self::Error) -> bool {
        matches!(
            *error,
            NodeError::Refused {
                code: RemoteCode::NotCertified
            }
        )
    }

    fn relays_to_a_node(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
