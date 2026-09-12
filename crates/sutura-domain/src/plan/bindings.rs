//! What a plan's predicates bind, and the guarantee that every one of them resolves.
//!
//! # The defect this type exists for
//!
//! A [`PlanPredicate`](crate::plan::PlanPredicate) records the INDEX of the parameter it compares
//! against, and a plan carries the parameter list beside its filters. Two lists, and nothing made
//! them agree: [`QueryPlan::new`](crate::plan::QueryPlan::new) took them as independent arguments
//! and [`LegPlan`](crate::plan::LegPlan) carried them as independent fields, so a predicate naming
//! parameter 5 beside a two-value list was a plan a producer could build, serialize and hand to an
//! adapter.
//!
//! None of the three things downstream then refuses it, which is what makes the pair worth parsing:
//!
//! - [`definitional_params`](crate::plan::QueryPlan::definitional_params) resolves each index with
//!   `get` and DROPS the ones that miss, so the golden asserting that a metric's required filter is
//!   bound rather than written into the statement reads a shorter list and passes.
//! - `sutura_sql`'s numbered-placeholder rendering converts the index with a saturating cast, so an
//!   oversized one renders as a placeholder the statement carries no value for.
//! - the in-process engine does report a missing parameter, but only when a leg is translated -
//!   after the plan has been accepted.
//!
//! # Order is half of it, and that half is a wrong number rather than an error
//!
//! Three of the four dialects `sutura_sql` renders for write a POSITIONAL placeholder, a bare `?`,
//! so the Nth placeholder in the statement takes the Nth value in the list; only Postgres writes a
//! NUMBERED `$n` that names its value. The renderer emits predicates in filter order and a
//! positional adapter binds the list in list order, so those two agree only while the indices run
//! `0, 1, .. n-1` down the filters. Read off the shipped adapters rather than reasoned about:
//! `sutura_exec_duckdb::bind` maps [`QueryPlan::params`](crate::plan::QueryPlan::params) in list
//! order against `?`, `sutura_exec_bigquery` sends the same list as an ordered array under a
//! positional parameter mode, and `sutura_sql`'s `?` placeholder ignores the position it is given.
//! `ClickHouse` is the third `?` dialect and has no executor here yet. `sutura_exec_postgres::bind`
//! maps the same list in the same order, but its `$n` is derived from the predicate's index, so it
//! is the one shipped adapter the ordering cannot mislead.
//!
//! So a plan whose filters name the same parameters in a different order renders correctly on the
//! numbered dialect and binds the wrong values on a positional one - `order_date >= <end> AND
//! order_date < <start>` is an empty result under a certified metric name, arrived at by nothing
//! that reports an error. That is the failure class this repository is arranged against, so the
//! ordering is parsed rather than trusted.
//!
//! # What this buys, and where it stops
//!
//! **No [`QueryPlan`](crate::plan::QueryPlan) and no [`LegPlan`](crate::plan::LegPlan) holds an
//! incoherent set**, because [`PlanBindings`] is the only way to supply one and [`PlanBindings::parse`]
//! is the only way to obtain one that holds a parameter - the argument
//! [`StatementTables`](crate::plan::StatementTables) makes for the tables of one statement, applied
//! to the second pair of fields those constructors used to take independently.
//!
//! **It stops at the pair, and the limit is the reason this is not a newtype over the index.**
//! Coherence is a relation between a predicate and a LIST, so no wrapper around one `usize` can
//! hold it: a `ParamIndex` that only renamed the number would buy nothing, and
//! [`PlanPredicate`](crate::plan::PlanPredicate) therefore still carries an unconstrained one. What
//! changes is that a predicate outside a parsed set reaches no renderer and no executor. The check
//! runs once, here; after `Ok` nothing re-checks.

use crate::plan::PlanFilter;
use crate::warehouse::ParamValue;

/// Why a set of filters and parameters is not a coherent binding.
///
/// Three distinct author mistakes rather than one message, because the field a reader needs differs:
/// a number that is too large, a number in the wrong place, and a value nothing reads.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IncoherentBindings {
    /// A predicate binds a parameter the list does not hold.
    #[error("a predicate binds parameter {index}, and the set carries {params}")]
    OutOfRange { index: usize, params: usize },
    /// A predicate binds a parameter out of placeholder order.
    ///
    /// `position` is how many predicates before it bound one, which is the placeholder a positional
    /// dialect would give it.
    #[error("the predicate at placeholder {position} binds parameter {index}")]
    OutOfPlaceholderOrder { position: usize, index: usize },
    /// The set carries a parameter no predicate binds.
    #[error("{read} of {params} parameters are bound by a predicate")]
    NeverRead { read: usize, params: usize },
}

/// The predicates one statement applies, and the values they bind, in placeholder order.
///
/// **If an instance of this type exists, every predicate in it resolves to a parameter the set
/// holds, and resolves to the one a positional placeholder would give it** - which is the whole
/// return on the newtype, and what lets [`QueryPlan::new`](crate::plan::QueryPlan::new) stay
/// infallible while the plan it builds cannot be the incoherent one. See this module's header for
/// what the incoherence does to each adapter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PlanBindings {
    filters: Vec<PlanFilter>,
    params: Vec<ParamValue>,
}

impl PlanBindings {
    /// The predicates and the values they bind, or a refusal if the two do not resolve each other.
    ///
    /// **The canonical constructor.** [`Self::none`] is the empty spelling of it and repeats no
    /// check, because a set with no parameters has no index to resolve.
    ///
    /// The three checks are ordered for the DIAGNOSTIC and not for cost - the whole walk is linear
    /// over a list bounded by [`MAX_DIMENSIONS`](crate::query::MAX_DIMENSIONS) plus a metric's
    /// required filters plus the two range bounds. An index the set cannot hold is reported as that
    /// rather than as an ordering fault, because the author who wrote the wrong number needs to read
    /// the number, and an out-of-range index is out of order as well.
    pub fn parse(filters: Vec<PlanFilter>, params: Vec<ParamValue>) -> Result<Self, IncoherentBindings> {
        let mut placeholder: usize = 0;
        for filter in &filters {
            if let Some(index) = filter.predicate().param() {
                if index >= params.len() {
                    return Err(IncoherentBindings::OutOfRange {
                        index,
                        params: params.len(),
                    });
                }
                if index != placeholder {
                    return Err(IncoherentBindings::OutOfPlaceholderOrder {
                        position: placeholder,
                        index,
                    });
                }
                placeholder = placeholder.saturating_add(1);
            }
        }
        if placeholder != params.len() {
            return Err(IncoherentBindings::NeverRead {
                read: placeholder,
                params: params.len(),
            });
        }
        Ok(Self { filters, params })
    }

    /// No predicates and no parameters.
    ///
    /// Infallible by construction rather than by a skipped check: there is no index to resolve and
    /// no value to leave unread. It is [`StatementTables::only`](crate::plan::StatementTables::only)'s
    /// argument in the second pair - a lookup leg for a remote dimension the question did not filter
    /// carries exactly this.
    #[inline]
    #[must_use]
    pub const fn none() -> Self {
        Self {
            filters: Vec::new(),
            params: Vec::new(),
        }
    }

    /// The predicates this set applies, in the order a statement emits them.
    #[inline]
    #[must_use]
    pub fn filters(&self) -> &[PlanFilter] {
        &self.filters
    }

    /// The values bound to this set's placeholders, in placeholder order.
    #[inline]
    #[must_use]
    pub fn params(&self) -> &[ParamValue] {
        &self.params
    }

    /// The two lists, for a constructor that stores them apart.
    ///
    /// [`QueryPlan`](crate::plan::QueryPlan) takes this set and keeps the halves as its own fields,
    /// for the reason its constructor gives about [`StatementTables`](crate::plan::StatementTables):
    /// the serialized form a golden pins is unchanged by the guard existing.
    #[inline]
    #[must_use]
    pub fn into_parts(self) -> BoundParts {
        (self.filters, self.params)
    }
}

/// The halves [`PlanBindings::into_parts`] hands back to a constructor that stores them apart.
///
/// Named rather than written out at the signature: two `Vec`s in a tuple is over the workspace's
/// `type_complexity` threshold of 100 (clippy's default is 250). This is not the pair a constructor
/// can take back in - only [`PlanBindings::parse`] does that - so naming it does not reopen the
/// bypass the two deleted tuple aliases were.
type BoundParts = (Vec<PlanFilter>, Vec<ParamValue>);

#[cfg(test)]
mod tests;
