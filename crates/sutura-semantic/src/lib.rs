//! The semantic compiler: a modelled question becomes one plan for one data system.
//!
//! Three stages, in three modules, and the split is the design rather than tidiness:
//!
//! 1. **Resolve** looks every name up in the pinned snapshot. Its only outputs are references into
//!    that bundle and refusals naming the argument that failed. It cannot reach a live catalog,
//!    because it is handed a [`PinnedDefinitions`] and nothing else.
//! 2. **Plan** settles what nothing else may settle: which single data
//!    system the statement runs against, and which values become bind parameters. It holds no SQL,
//!    and its serialized form is what a golden snapshot pins.
//! 3. **Generate** ([`generate`]) renders the plan for one dialect. It is the only module that
//!    produces SQL and the only one that names the dialect layer.
//!
//! **[`compile`] runs the first two, and stops.** Stage 3 is not part of compiling, because the
//! `Warehouse` port takes a plan: an adapter that executes over Arrow renders nothing, and a
//! SQL-speaking adapter renders for the dialect it alone knows. Whoever wants SQL calls
//! [`generate::generate`] and names the dialect there.
//!
//! [`compile`] returns a [`Compiled`] rather than a `Result` of a plan, because a refusal is an
//! answer: a caller must not be able to mistake "you may not ask that" for a transport failure and
//! retry until something works.
//!
//! **Nothing here parses SQL, and nothing here translates between dialects.** There is no foreign
//! SQL on this path to parse: the statement is generated from a model, so the rule holds by
//! construction. Translation is banned separately, in `clippy.toml`, and the `transpile` feature is
//! not even compiled - see the workspace manifest for why not calling it was judged too weak.
pub mod dialect;
pub mod generate;
pub(crate) mod plan;
mod resolve;
pub use crate::dialect::Dialect;
pub use crate::generate::GenerateError;
pub use crate::resolve::BundleInconsistent;
use crate::resolve::ResolveError;
use sutura_domain::pinned::PinnedDefinitions;
pub use sutura_domain::plan::QueryPlan;
use sutura_domain::plan::QueryPlan as DomainPlan;
pub use sutura_domain::plan::{PlanFilter, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin};
use sutura_domain::query::{Query, RefusalReason};
/// What compiling a question produced.
///
/// A refusal is a variant here rather than an `Err`, which is the same choice
/// [`sutura_domain::query::ToolOutcome`] makes and for the same reason.
#[derive(Debug)]
pub enum Compiled {
    /// The question resolved, and this is what we decided to execute.
    ///
    /// The plan is boxed because it is by far the larger of the two payloads, and an enum whose
    /// size is set by its rarest variant makes every refusal carry the cost of an answer.
    Planned { plan: Box<DomainPlan> },
    /// The question was refused, and this is why.
    Refused { reason: RefusalReason },
}
impl Compiled {
    /// The refusal, if there was one.
    #[inline]
    pub const fn refusal(&self) -> Option<&RefusalReason> {
        match *self {
            Self::Refused { ref reason } => Some(reason),
            Self::Planned { .. } => None,
        }
    }
    /// The plan, if the question resolved.
    #[inline]
    pub const fn plan(&self) -> Option<&DomainPlan> {
        match *self {
            Self::Planned { ref plan } => Some(plan),
            Self::Refused { .. } => None,
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
/// Rendering now lives where the dialect is actually known: [`generate::generate`], called by the
/// adapter that speaks that dialect. Whoever wants SQL asks for it.
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
        Ok(plan) => Ok(Compiled::Planned { plan: Box::new(plan) }),
        Err(reason) => Ok(Compiled::Refused { reason }),
    }
}
