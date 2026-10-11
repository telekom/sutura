#![forbid(unsafe_code)]
//! A [`Warehouse`] over a Cube metrics serving layer's REST API - `github.com/telekom/sutura#1340`.
//!
//! sutura plans the question from the catalog definitions, with every plan-time refusal it makes for
//! any other source. This adapter then asks Cube for the planned members as a JSON query on
//! `/cubejs-api/v1/load`, and Cube computes each metric from its own model. sutura never computes a
//! Cube metric itself: Cube down, or Cube refusing, is a typed error and never a partial answer.
//!
//! # How a plan names Cube members
//!
//! A metric is `<table>.<metric>` and a column `<table>.<column>`, where `<table>` is the model's
//! table in the catalog: the catalog names each table after its cube and each metric after its Cube
//! measure. Every filter of the plan - the range, each required filter and each requested one - is
//! a Cube filter whose values are JSON values, never query text. [`query`] holds the mapping.
//!
//! Before a query is sent, every member it names is looked up in Cube's `/cubejs-api/v1/meta`
//! answer, read with the same credential: a member Cube does not serve refuses the question.
//!
//! # Identity
//!
//! A `shared-service-user` source sends the deployment's declared Cube token on every request. The
//! token is a [`Secret`]: no error of this crate carries it, or Cube's own text.
//!
//! [`Secret`]: sutura_domain::identity::Secret

#[cfg(feature = "fixtures")]
pub mod fixture;
pub mod http;
mod members;
pub mod query;
mod rows;

use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture, Secret};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable, QueryPlan};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::arrow::of_row_set;
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, NotFinite, ResultBatches, RowSet, Warehouse};

use crate::query::{CubeQuery, NotExpressible};

/// The two round trips this adapter makes, as a seam: [`http::HttpCube`] for a running Cube, and a
/// scripted implementor for the unit cells.
pub trait CubeTransport {
    type Error: core::error::Error + Send + Sync + 'static;

    /// One `GET /cubejs-api/v1/meta`, sent as `token`.
    ///
    /// # Errors
    ///
    /// Cube was not reached, refused the request, or answered past the deadline or the size cap.
    fn meta(&self, token: &Secret, deadline: Deadline) -> Answer<Self::Error>;

    /// One `/cubejs-api/v1/load` of `query`, sent as `token`.
    ///
    /// # Errors
    ///
    /// As [`Self::meta`].
    fn load(&self, query: &serde_json::Value, token: &Secret, deadline: Deadline) -> Answer<Self::Error>;

    /// Whether Cube itself refused the credential.
    fn source_refused(&self, error: &Self::Error) -> bool;

    /// Whether the deadline ran out before Cube answered.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool;
}

/// Why Cube did not answer a plan.
#[derive(Debug, thiserror::Error)]
pub enum CubeError<E>
where
    E: core::error::Error + 'static,
{
    #[error("Cube did not answer")]
    Endpoint {
        #[source]
        cause: E,
    },
    #[error("the plan cannot be asked of Cube")]
    NotExpressible {
        #[source]
        cause: NotExpressible,
    },
    #[error("Cube serves no member {member}, so the question is refused rather than answered without it")]
    AbsentMember { member: String },
    #[error("this adapter answers a whole plan, and the leg against {table} needs a combiner above it")]
    LegWithoutCombiner { table: String },
    #[error(
        "source `{at}` was handed {presented}, and this adapter sends only the deployment's declared Cube \
         token. This is a wiring defect between the credential broker and the source declaration"
    )]
    NoPlaceForASubject { at: String, presented: &'static str },
    #[error("the credential broker presented a leg that disagrees with how this source is declared")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    #[error("the metric {metric} declares that a zero denominator fails, and Cube answered no value")]
    ZeroDenominator { metric: String },
    #[error("member {member} came back as a value that is not a finite number")]
    NotFinite {
        member: String,
        #[source]
        cause: NotFinite,
    },
    #[error("member {member} came back as a value that is not a number")]
    NotANumber {
        member: String,
        #[source]
        cause: core::num::ParseFloatError,
    },
    #[error("member {member} came back as an integer beyond 2^53, which a Cube answer does not carry exactly")]
    InexactInteger { member: String },
    #[error("the Cube answer was not the {expected} this adapter reads")]
    MalformedAnswer { expected: &'static str },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
}

pub(crate) type CubeResult<T, E> = Result<T, CubeError<E>>;

/// One answer's bytes, or why the transport did not get them.
pub type Answer<E> = Result<Vec<u8>, E>;

/// One Cube deployment, as one source.
pub struct CubeWarehouse<T> {
    source: SourceName,
    posture: SourcePosture,
    token: Secret,
    transport: T,
}

impl<T> core::fmt::Debug for CubeWarehouse<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CubeWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl<T> CubeWarehouse<T>
where
    T: CubeTransport,
{
    /// A source that sends `token`, the deployment's declared Cube token, on every request.
    #[must_use]
    pub const fn of(source: SourceName, posture: SourcePosture, token: Secret, transport: T) -> Self {
        Self {
            source,
            posture,
            token,
            transport,
        }
    }

    fn deliverable(&self, presented: &Presented) -> CubeResult<&Secret, T::Error> {
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(CubeError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| CubeError::PresentedDisagreesWithPosture { cause })?;
        Ok(&self.token)
    }

    fn run(&self, plan: &QueryPlan, limit: u32, token: &Secret, deadline: Deadline) -> CubeResult<RowSet, T::Error> {
        let query = CubeQuery::of(plan, limit).map_err(|cause| CubeError::NotExpressible { cause })?;
        let served = self
            .transport
            .meta(token, deadline)
            .map_err(|cause| CubeError::Endpoint { cause })?;
        members::refuse_absent(&served, query.members())?;
        let answers = query
            .queries()
            .iter()
            .map(|one| self.transport.load(one, token, deadline))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|cause| CubeError::Endpoint { cause })?;
        rows::read(&answers, &query)
    }
}

impl<T> Warehouse for CubeWarehouse<T>
where
    T: CubeTransport,
{
    type Error = CubeError<T::Error>;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

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
        let plan = match executable {
            Executable::Query(plan) => plan,
            Executable::Leg(leg) => {
                return Err(CubeError::LegWithoutCombiner {
                    table: leg.table().to_string(),
                });
            }
        };
        let rows = self.run(plan, plan.row_limit(), token, deadline)?;
        of_row_set(&rows).map_err(|cause| CubeError::Shape { cause })
    }

    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.run(plan.plan(), plan.plan().row_limit(), &self.token, boot_deadline())
            .map(AnchorRows::of)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(*error, CubeError::Endpoint { ref cause } if self.transport.source_refused(cause))
    }

    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(*error, CubeError::Endpoint { ref cause } if self.transport.deadline_exceeded(cause))
    }
}

#[expect(
    clippy::expect_used,
    reason = "3600 seconds is a literal, non-zero constant - Budget::parse fails only on zero, so this cannot \
              panic; `sutura-exec-clickhouse`'s boot deadline is the same shape"
)]
fn boot_deadline() -> Deadline {
    let budget = Budget::parse(core::time::Duration::from_secs(3600)).expect("3600 seconds is a non-zero duration");
    Deadline::opened_at(std::time::Instant::now(), budget)
}

#[cfg(test)]
mod tests;
