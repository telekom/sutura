//! The semantic compiler: a modelled question becomes one plan for one data system.
//!
//! Two stages, in two modules, and the split is the design rather than tidiness:
//!
//! 1. **Resolve** looks every name up in the pinned snapshot. Its only outputs are references into
//!    that bundle and refusals naming the argument that failed. It cannot reach a live catalog,
//!    because it is handed a [`PinnedDefinitions`] and nothing else.
//! 2. **Plan** settles what nothing else may settle: which single data
//!    system the statement runs against, and which values become bind parameters. It holds no SQL,
//!    and its serialized form is what a golden snapshot pins.
//!
//! **[`compile`] runs both, and stops. There is no third stage in this crate.** The `Warehouse`
//! port takes a plan: an adapter that executes over Arrow renders nothing, and a SQL-speaking
//! adapter renders for the dialect it alone knows. Whoever wants SQL calls `sutura_sql::generate`
//! and names the dialect there.
//!
//! **`sutura-sql` is a separate crate, and this crate does not depend on it.** Rendering used to be
//! a `pub` module here - `generate` and `dialect` - which put `polyglot-sql` in the transitive
//! closure of every consumer of the core, the network binary included: it links the engine, renders
//! nothing, and could reach no line of that code. Moving the two modules out drops the generator
//! from this crate's tree, and `cargo xtask check-boundaries` fails if either edge comes back, so
//! the direction is a gate rather than a sentence in this comment.
//!
//! [`compile`] returns a [`Compiled`] rather than a `Result` of a plan, because a refusal is an
//! answer: a caller must not be able to mistake "you may not ask that" for a transport failure and
//! retry until something works.
//!
//! **Nothing here emits SQL, parses SQL, or names a dialect.** After the split that is a fact about
//! the dependency list rather than a discipline: there is no SQL generator in this crate's tree to
//! call.
pub(crate) mod plan;
mod resolve;
pub use crate::resolve::BundleInconsistent;
use crate::resolve::ResolveError;
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::QueryPlan as DomainPlan;
pub use sutura_domain::plan::{FederatedPlan, QueryPlan};
pub use sutura_domain::plan::{PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin};
use sutura_domain::query::{Query, RefusalReason};
/// What compiling a question produced.
///
/// A refusal is a variant here rather than an `Err`, which is the same choice
/// [`sutura_domain::query::ToolOutcome`] makes and for the same reason.
#[derive(Debug)]
pub enum Compiled {
    /// The question resolved to one data system, and this is what we decided to execute.
    ///
    /// The plan is boxed because it is by far the larger of the payloads, and an enum whose size is
    /// set by its rarest variant makes every refusal carry the cost of an answer.
    Planned { plan: Box<DomainPlan> },
    /// The question resolved to two data systems, split into a fact leg and a lookup leg.
    ///
    /// The two legs execute against their own sources and [`FederatedPlan::combine`] turns the rows
    /// back into one answer above them.
    Federated { plan: Box<FederatedPlan> },
    /// The question was refused, and this is why.
    Refused { reason: RefusalReason },
}
impl Compiled {
    /// The refusal, if there was one.
    #[inline]
    pub const fn refusal(&self) -> Option<&RefusalReason> {
        match *self {
            Self::Refused { ref reason } => Some(reason),
            Self::Planned { .. } | Self::Federated { .. } => None,
        }
    }
    /// The plan, if the question resolved to one data system.
    #[inline]
    pub const fn plan(&self) -> Option<&DomainPlan> {
        match *self {
            Self::Planned { ref plan } => Some(plan),
            Self::Federated { .. } | Self::Refused { .. } => None,
        }
    }
}
/// Resolves and plans. It does not render.
///
/// **The dialect used to be an argument here, and that was a parameter that could lie.** Compiling
/// rendered a statement alongside the plan, and the one caller that answers questions threw the
/// statement away - because the port takes a plan, and a SQL-speaking adapter renders its own. So the
/// dialect decided nothing, while a caller could hand this `Postgres` and a `DuckDB` warehouse and
/// nothing anywhere would notice the disagreement.
///
/// Rendering now lives where the dialect is actually known: `sutura_sql::generate`, in its own
/// crate, called by the adapter that speaks that dialect. Whoever wants SQL asks for it, and
/// linking this crate no longer links a SQL generator.
///
/// The error type is [`BundleInconsistent`] rather than an enum, because after the split that is the
/// only way this can fail. A refused question is not a failure and comes back as [`Compiled`].
pub fn compile(query: &Query, pinned: &PinnedDefinitions) -> Result<Compiled, BundleInconsistent> {
    let resolution = match resolve::resolve(query, pinned) {
        Ok(resolution) => resolution,
        Err(ResolveError::Refused(reason)) => return Ok(Compiled::Refused { reason }),
        Err(ResolveError::Bundle(cause)) => return Err(cause),
    };
    match plan::plan(&resolution) {
        Ok(plan::Plan::Mono(query)) => Ok(Compiled::Planned { plan: Box::new(query) }),
        Ok(plan::Plan::Federated(federated)) => Ok(Compiled::Federated {
            plan: Box::new(federated),
        }),
        Err(reason) => Ok(Compiled::Refused { reason }),
    }
}
