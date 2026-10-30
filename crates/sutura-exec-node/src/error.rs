//! Why a node did not answer a leg, typed so the leg runner can tell a refusal from an outage.
//!
//! **No peer text is kept.** A gRPC status carries the receiving node's message; this crate reads
//! it only to compare it with the closed codes it maps ([`RemoteCode`]) and never stores or renders
//! it, so nothing the other node wrote reaches a caller or a log line from here (ADR 0022).

use sutura_domain::identity::PresentedDisagreesWithPosture;
use sutura_domain::plan::remote::NotSendable;
use sutura_domain::warehouse::arrow::UnannouncedBatch;

/// The receiving node's refusal, as the closed code it crossed as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteCode {
    /// The node does not certify the leg against its own definitions, or will not serve it.
    NotCertified,
    /// The node's data system refused the leg as the token's subject.
    SourceRefused,
    /// The node's own deadline ran out.
    DeadlineExceeded,
    /// The node's answer would not fit its own bound.
    ResultTooLarge,
    /// Any other code: a failure here, never a refusal this node names.
    Other,
}

impl RemoteCode {
    /// Reads a `failed_precondition` message, which the receiving node sets to exactly one
    /// [`RefusalReason::code`](sutura_domain::query::RefusalReason::code).
    #[must_use]
    pub fn of(message: &str) -> Self {
        match message {
            "remote_leg_not_certified" => Self::NotCertified,
            "source_refused" => Self::SourceRefused,
            "deadline_exceeded" => Self::DeadlineExceeded,
            "result_too_large" => Self::ResultTooLarge,
            _ => Self::Other,
        }
    }
}

/// Why this adapter did not return a leg's rows.
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error(
        "source `{at}` was handed {presented}, which this adapter cannot send to another node. This is a \
         wiring defect between the credential broker and the source declaration"
    )]
    Undeliverable { at: String, presented: &'static str },
    #[error("the credential broker presented a leg that disagrees with how this source is declared")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    #[error("a node source runs legs of a federated answer, and was handed a whole plan over {table}")]
    WholePlan { table: String },
    #[error("the leg could not be written for the wire")]
    NotSendable {
        #[source]
        cause: NotSendable,
    },
    #[error("the leg could not be framed")]
    Framing,
    #[error("the exchanged token is not a value a request header carries")]
    Bearer,
    #[error("the deadline ran out before the node answered")]
    DeadlineExceeded,
    #[error("the node did not take this deployment's token for the leg")]
    Unauthenticated,
    #[error("the node could not read the leg this node sent")]
    Rejected,
    #[error("the node refused the leg as {code:?}")]
    Refused { code: RemoteCode },
    #[error("the node did not answer the leg ({code:?})")]
    Unreachable { code: tonic::Code },
    #[error("the node's answer was not a stream of Arrow batches")]
    Answer,
    #[error("the node answered under labels this leg does not project")]
    Labels,
    #[error("the node's answer does not fit what this node holds for a leg")]
    Result {
        #[source]
        cause: UnannouncedBatch,
    },
}

impl NodeError {
    /// A gRPC status, read as its code and, for a refusal, its closed message - never kept.
    pub(crate) fn of_status(status: &tonic::Status) -> Self {
        match status.code() {
            tonic::Code::FailedPrecondition => Self::Refused {
                code: RemoteCode::of(status.message()),
            },
            tonic::Code::Unauthenticated | tonic::Code::PermissionDenied => Self::Unauthenticated,
            tonic::Code::InvalidArgument => Self::Rejected,
            tonic::Code::DeadlineExceeded => Self::DeadlineExceeded,
            code => Self::Unreachable { code },
        }
    }
}

/// Why a node source could not be built.
#[derive(Debug, thiserror::Error)]
pub enum NotBuilt {
    #[error("the runtime the node client runs on could not start")]
    Runtime {
        #[source]
        cause: std::io::Error,
    },
    #[error("the node's trust anchors could not be used")]
    Anchors {
        #[source]
        cause: tokio_rustls::rustls::Error,
    },
    #[error("the node endpoint's host is not a name a certificate can be checked against")]
    ServerName,
}
