//! The semantic compiler: a modelled question becomes one statement for one data system.
//!
//! Three stages, in three modules, and the split is the design rather than tidiness:
//!
//! 1. **Resolve** looks every name up in the pinned snapshot. Its only outputs are references into
//!    that bundle and refusals naming the argument that failed. It cannot reach a live catalog,
//!    because it is handed a [`PinnedDefinitions`] and nothing else.
//! 2. **Plan** ([`crate::plan`]) settles what nothing else may settle: which single data
//!    system the statement runs against, and which values become bind parameters. It holds no SQL,
//!    and its serialized form is what a golden snapshot pins.
//! 3. **Generate** ([`generate`]) renders the plan for one dialect. It is the only module that
//!    produces SQL and the only one that names the dialect layer.
//!
//! [`compile`] runs all three. It returns a [`Compiled`] rather than a `Result` of a statement,
//! because a refusal is an answer: a caller must not be able to mistake "you may not ask that" for
//! a transport failure and retry until something works.
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
pub use sutura_domain::plan::QueryPlan;
pub use sutura_domain::plan::{PlanFilter, PlanMeasure, PlanPredicate, PredicateOrigin};

use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::plan::QueryPlan as DomainPlan;
use sutura_domain::query::{Query, RefusalReason};
use sutura_domain::warehouse::GeneratedQuery;

use crate::resolve::ResolveError;

/// What compiling a question produced.
///
/// A refusal is a variant here rather than an `Err`, which is the same choice
/// [`sutura_domain::query::ToolOutcome`] makes and for the same reason.
#[derive(Debug)]
pub enum Compiled {
    /// The question resolved, and here is the statement and the plan behind it.
    ///
    /// The plan is boxed because it is by far the larger of the two payloads, and an enum whose
    /// size is set by its rarest variant makes every refusal carry the cost of an answer.
    Statement { plan: Box<DomainPlan>, query: GeneratedQuery },
    /// The question was refused, and this is why.
    Refused { reason: RefusalReason },
}

impl Compiled {
    /// The refusal, if there was one.
    #[inline]
    pub const fn refusal(&self) -> Option<&RefusalReason> {
        match *self {
            Self::Refused { ref reason } => Some(reason),
            Self::Statement { .. } => None,
        }
    }

    /// The generated statement, if the question resolved.
    #[inline]
    pub const fn query(&self) -> Option<&GeneratedQuery> {
        match *self {
            Self::Statement { ref query, .. } => Some(query),
            Self::Refused { .. } => None,
        }
    }

    /// The plan, if the question resolved.
    #[inline]
    pub const fn plan(&self) -> Option<&DomainPlan> {
        match *self {
            Self::Statement { ref plan, .. } => Some(plan),
            Self::Refused { .. } => None,
        }
    }
}

/// Why compilation failed, as opposed to being refused.
///
/// Neither variant is something a caller did. A broken bundle is an operator's problem and a
/// generator failure is ours, so neither is offered to the caller as a refusal they might retry
/// differently.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("the pinned bundle does not hold together")]
    Bundle {
        #[source]
        cause: BundleInconsistent,
    },
    #[error("the statement could not be rendered")]
    Generate {
        #[source]
        cause: GenerateError,
    },
}

/// Resolves, plans and generates.
pub fn compile(query: &Query, pinned: &PinnedDefinitions, dialect: Dialect) -> Result<Compiled, CompileError> {
    let resolution = match resolve::resolve(query, pinned) {
        Ok(resolution) => resolution,
        Err(ResolveError::Refused(reason)) => return Ok(Compiled::Refused { reason }),
        Err(ResolveError::Bundle(cause)) => return Err(CompileError::Bundle { cause }),
    };
    let plan = match plan::plan(&resolution) {
        Ok(plan) => plan,
        Err(reason) => return Ok(Compiled::Refused { reason }),
    };
    let query = generate::generate(&plan, dialect).map_err(|cause| CompileError::Generate { cause })?;
    Ok(Compiled::Statement {
        plan: Box::new(plan),
        query,
    })
}
