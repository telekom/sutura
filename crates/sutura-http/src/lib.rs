//! The HTTP surface. Transport only.
//!
//! # What this crate is allowed to do
//!
//! Turn bytes into a [`sutura_domain::query::Query`], hand it to a port, and turn what comes back
//! into bytes. Nothing here decides whether a question may be answered - that is
//! `sutura-semantic` and `sutura-app`, behind the [`Surface`] port - and nothing here reads a
//! catalog directory or opens a data system. The one place either port is touched is
//! [`surface::LocalService::start`], which is called by the composition root.
//!
//! # The three properties a reader should check first
//!
//! **A refusal says so three ways.** `POST /v1/query` answers a question the caller may not have
//! with an explicit status - `403`, `404`, `409`, `413`, `422` or `503` depending on why - plus the
//! stable `code` and the sentence it has always carried in `outcome: refusal`. It was a `200`, on the
//! argument that an error status invites a client library to retry; the retry premise does not
//! survive checking, and a `200` made a governance refusal indistinguishable from an answer to
//! anything reading a status alone. `wire::refusal` holds the mapping, the citations and the reason
//! for each status. The domain invariant is untouched: `ToolOutcome::Refusal` is still a result and
//! not an `Err`.
//!
//! **A caller's identity is established, and it is still not per-caller access.** Where a deployment
//! declares `security.inbound`, [`inbound`] verifies the caller's own token - signature against a
//! pinned asymmetric algorithm, issuer, expiry, and an audience matching this deployment's own
//! resource identifier - and the request runs under a
//! [`sutura_domain::identity::Subject::Verified`]. Where it declares none, the answer is
//! [`sutura_domain::identity::Subject::TheDeploymentItself`] and an access token, if one is
//! configured, proves only that the caller holds a secret an operator distributed.
//!
//! **What neither shape does is make a data system execute as the asking subject.** That is leg 2, and
//! the half of it that is built is the credential port: a question cannot execute without a credential
//! minted for the source it reads, and a subject with no credential there is refused rather than
//! answered as this process. What no adapter in this build can do is CARRY a per-subject credential, so
//! every question is still answered with whatever access this process already had. The startup log
//! prints that limit on every boot, and [`inbound`] lists the four things `docs/adr/0014` describes
//! and this does not build.
//!
//! `crate::principal` is the one place a [`sutura_domain::identity::RequestContext`] is constructed,
//! and there are exactly two ways in: one takes no argument, and the other takes a
//! [`VerifiedCaller`], whose only constructor is a signature check and which implements no
//! `Deserialize`. So no *field* of a request can contribute to a chain either way.
//!
//! **`/health` carries nothing.** It is the one path an unauthenticated caller can always reach, so
//! every field it might have is a field handed to anybody who can route a packet. No version, no
//! build, no configuration, no catalog. A test asserts the body byte for byte.
//!
//! # What is deliberately absent
//!
//! * **No CORS layer.** A browser is not a client of this surface. An allow-list nobody needs is an
//!   allow-list somebody widens.
//! * **No request identifier on the wire.** One is minted now - [`correlation::CorrelationId`], on
//!   the request span, so every line of one request carries it - and it is deliberately **not** in
//!   the failure body. Putting it there is a change to the response contract and to the generated
//!   document, and it buys nothing until somebody is asked to quote it; the honest state is that an
//!   operator can find a request in the log and a caller cannot yet name one. If a caller ever
//!   needs to, that is an additive field and this bullet is where it changes.
//! * **No audit sink *here*, and no store anywhere.** There is a sink now - the port is
//!   `sutura_domain::audit::AuditSink` and `sutura_app::LocalService` writes one record per outcome
//!   through it, before the outcome returns - but this crate neither implements it nor chooses it.
//!   The composition root attaches one; the writer a deployment gets for free is
//!   `sutura_runtime::TracingAuditSink`, which puts the record on the log this crate already emits
//!   into. **Nothing retains a record**: sutura writes and keeps nothing, so what happens after the
//!   write belongs to the deployment's log pipeline, including the case where that is nothing. The
//!   per-outcome `tracing::info!` this handler used to write was replaced by that record rather than
//!   joined by it - see `routes::v1::query`.
//! * **No readiness route.** The module documentation on the liveness route says why: there is
//!   nothing it could report that is not
//!   already true of a process that is listening.
//!
//! # Assembling it
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use sutura_config::{Environment, Settings, Sources};
//! use sutura_http::{ServiceState, router, serve};
//! use sutura_runtime::Shutdown;
//!
//! # async fn wire(surface: Arc<dyn sutura_http::Surface>) -> Result<(), Box<dyn core::error::Error>> {
//! let settings = Settings::load(&Sources::defaults(Environment::Development))?;
//! let address = settings.server().bind().socket();
//! let state = ServiceState::new(surface, Arc::new(settings));
//! serve(router(&state)?, address, Shutdown::new()).await?;
//! # Ok(())
//! # }
//! ```

pub mod capability;
pub mod client_address;
pub mod constants;
pub mod correlation;
pub mod inbound;
pub mod middleware;
pub mod openapi;
pub(crate) mod principal;
pub mod problem;
pub mod router;
pub(crate) mod routes;
pub mod server;
pub mod state;
pub mod surface;
#[cfg(feature = "tls")]
pub mod tls;
pub mod wire;

#[cfg(test)]
mod testing;

#[cfg(test)]
mod harness;

pub use crate::capability::{capability_of, governed, permitted_for, require_capability};
pub use crate::client_address::ClientAddress;
pub use crate::correlation::{CorrelationId, NotACorrelationId};
pub use crate::inbound::{InboundGate, InboundNotUsable, VerifiedCaller};
pub use crate::problem::{Failure, ProblemBody};
pub use crate::router::{Assembled, RouterNotBuilt, assemble, router};
#[cfg(feature = "tls")]
pub use crate::server::serve_tls;
pub use crate::server::{ServeFailed, serve};
pub use crate::state::ServiceState;
pub use crate::surface::{ErasedCause, LocalService, ServiceNotStarted, Surface, SurfaceFailure, cause_chain};
#[cfg(feature = "tls")]
pub use crate::tls::{Renewal, Renewed, Termination, TlsListener, TlsNotUsable};
