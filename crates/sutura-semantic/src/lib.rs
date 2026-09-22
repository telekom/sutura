//! The semantic compiler: a modelled question becomes one plan for one data system.
//!
//! Two stages, in two modules, and the split is the design rather than tidiness:
//!
//! 1. **Resolve** looks every name up in the caller's [`ScopedView`](sutura_domain::pinned::view::ScopedView)
//!    of the pinned snapshot. Its only outputs are references into that bundle and refusals naming
//!    the argument that failed. It cannot reach a live catalog, because it is handed the view and
//!    nothing else - and a metric outside that view resolves as unknown, not merely undescribed.
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
use crate::plan::PlanError;
pub use crate::resolve::BundleInconsistent;
use crate::resolve::ResolveError;
use sutura_domain::model::MetricName;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::QueryPlan as DomainPlan;
pub use sutura_domain::plan::{FederatedPlan, QueryPlan};
use sutura_domain::plan::{FederatedPlanError, IncoherentBindings};
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
    /// The two legs execute against their own sources and the combiner behind
    /// [`FederationCombiner`](sutura_domain::plan::FederationCombiner) turns the batches
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
/// Why compiling failed, which is never why a question was refused.
///
/// **Several arms rather than one bundle error, and `telekom/sutura#338` is the report.** A question
/// the deployment declines comes back as [`Compiled::Refused`]; what reaches this type is our own
/// side being wrong. Each arm below is one way that can happen: the pinned bundle names something it
/// does not hold, the bundle reaches this compiler with authored SQL its plan cannot carry, the
/// splitter built a two-source plan that
/// [`FederatedPlan::new`](sutura_domain::plan::FederatedPlan::new) then rejected, a producer
/// built a plan whose predicates and parameters did not resolve each other, the splitter ran with no
/// remote dimension, and a chain reached the plan stage having left the metric's data system.
/// `NotAssembled` used to
/// be flattened into [`RefusalReason::FederationNotExecutable`], which is what a build whose adapter
/// type does not declare `Warehouse::EXECUTES_LEGS` is told - so a wiring defect and a statement
/// about the build's own capability arrived as one value, and a caller could not tell which it had.
///
/// The arms are deliberately NOT counted in this prose: a number here is a second thing to keep
/// true, and it had already stopped being true once - it said four while the enum held five.
///
/// **The limit, next to the claim:** nothing provokes [`NotAssembled`](CompileFailure::NotAssembled)
/// or [`NotBound`](CompileFailure::NotBound) today. Every `FederatedPlanError` variant is
/// structurally unreachable from the splitter as it stands, and every
/// [`IncoherentBindings`](sutura_domain::plan::IncoherentBindings) variant is unreachable from the
/// two functions that build a binding set, because both mint each parameter index from the position
/// the value was pushed to - `crate::plan::PlanError` enumerates why, one variant at a time - so what
/// these arms buy is that a future edit which makes one reachable surfaces as a failure rather than
/// as a refusal a caller would retry.
#[derive(Debug, thiserror::Error)]
pub enum CompileFailure {
    /// The pinned bundle names a model or a relationship it does not hold.
    #[error(transparent)]
    Bundle(#[from] BundleInconsistent),
    /// The bundle carries authored SQL, while the domain plan deliberately carries no SQL.
    #[error("metric {metric} uses authored SQL, which the semantic plan cannot carry")]
    AuthoredSqlNotPlanned { metric: MetricName },
    /// A two-source plan this workspace compiled and could not then assemble.
    #[error("this deployment compiled a two-source question it could not assemble")]
    NotAssembled(#[from] FederatedPlanError),
    /// A plan this workspace compiled whose predicates and parameters did not resolve each other.
    #[error("this deployment compiled a question whose parameters did not bind")]
    NotBound(#[from] IncoherentBindings),
    /// A2: the splitter ran with no remote dimension to join the fact leg through.
    ///
    /// Same limit as `NotAssembled` and `NotBound`: `plan` calls the splitter only when exactly one
    /// remote source exists and a remote dimension has a join by construction, so nothing provokes
    /// this either. It replaces a fabricated `RefusalReason::PlanSpansTooManySources { sources: 1,
    /// limit: 2 }` a caller could not have narrowed their way out of.
    #[error("the federated splitter found no remote dimension to join the fact leg through")]
    NoRemoteJoin,
    /// A chain that leaves the metric's data system after its first hop reached the plan stage.
    ///
    /// The bundle should not have assembled: `sutura_domain::catalog` refuses such a chain at load,
    /// naming the dimension and the hop. So nothing provokes this from a catalog either, and what it
    /// buys is the thing the load check alone did not have - a second reader of the same rule, on
    /// the path where getting it wrong renders another data system's table into one statement under
    /// a certified metric name. `crate::plan::PlanError::ChainLeavesItsSource` carries the report.
    #[error("metric {metric} reaches dimension {dimension} through a chain that leaves its data system at hop {hop}")]
    ChainLeavesItsSource {
        metric: MetricName,
        dimension: sutura_domain::model::DimensionName,
        hop: usize,
    },
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
/// The error type is [`CompileFailure`] and a refused question is not one of its arms: a refusal is
/// an answer and comes back as [`Compiled`].
///
/// **A broken bundle is no longer the only way this can fail**, which is the change
/// `telekom/sutura#338` asked for and the one thing about it a test can hold:
///
/// ```compile_fail
/// use sutura_domain::pinned::PinnedDefinitions;
/// use sutura_domain::pinned::view::ScopedView;
/// use sutura_domain::query::Query;
/// use sutura_semantic::{BundleInconsistent, compile};
///
/// fn _only_a_broken_bundle(query: &Query, pinned: &PinnedDefinitions) -> Option<BundleInconsistent> {
///     compile(query, &ScopedView::everything(pinned), sutura_domain::plan::RowCeiling::DEFAULT).err()
/// }
/// ```
///
/// And the twin, so a rename cannot make that block pass vacuously:
///
/// ```
/// use sutura_domain::pinned::PinnedDefinitions;
/// use sutura_domain::pinned::view::ScopedView;
/// use sutura_domain::query::Query;
/// use sutura_semantic::{CompileFailure, compile};
///
/// fn _either_way(query: &Query, pinned: &PinnedDefinitions) -> Option<CompileFailure> {
///     compile(query, &ScopedView::everything(pinned), sutura_domain::plan::RowCeiling::DEFAULT).err()
/// }
/// ```
pub fn compile(
    query: &Query,
    view: &ScopedView<'_>,
    row_ceiling: sutura_domain::plan::RowCeiling,
) -> Result<Compiled, CompileFailure> {
    let resolution = match resolve::resolve(query, view, row_ceiling) {
        Ok(resolution) => resolution,
        Err(ResolveError::Refused(reason)) => return Ok(Compiled::Refused { reason }),
        Err(ResolveError::Bundle(cause)) => return Err(cause.into()),
    };
    match plan::plan(&resolution) {
        Ok(plan::Plan::Mono(query)) => Ok(Compiled::Planned { plan: query }),
        Ok(plan::Plan::Federated(federated)) => Ok(Compiled::Federated { plan: federated }),
        Err(PlanError::Refused(reason)) => Ok(Compiled::Refused { reason }),
        Err(PlanError::AuthoredSqlNotPlanned { metric }) => Err(CompileFailure::AuthoredSqlNotPlanned { metric }),
        Err(PlanError::NotAssembled(cause)) => Err(cause.into()),
        Err(PlanError::NotBound(cause)) => Err(cause.into()),
        Err(PlanError::NoRemoteJoin) => Err(CompileFailure::NoRemoteJoin),
        Err(PlanError::ChainLeavesItsSource { metric, dimension, hop }) => {
            Err(CompileFailure::ChainLeavesItsSource { metric, dimension, hop })
        }
    }
}
