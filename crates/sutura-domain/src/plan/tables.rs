//! Every table one statement reads, and the guarantee that the statement can tell them apart.
//!
//! # The defect this type exists for
//!
//! A column in a plan is qualified by a table's BARE name - `PlanColumn` holds a [`TableName`] - and
//! the reason is that `FROM a.b.orders` gives the reference an implicit alias of `orders` in every
//! target this workspace renders for. [`crate::model::qualified`] argues that at length and it is
//! right; what it does not do is say what happens when TWO of the tables in one statement end their
//! paths with the same name.
//!
//! What happened was this, and it was reproduced rather than reasoned about: a fact table at
//! `analytics-prod.sales.orders` joined to a dimension table at `reference-data.crm.orders` rendered
//! a `FROM` and a `LEFT JOIN` whose `ON` clause compared `orders.customer_id` with `orders.id` - one
//! table with itself - and every projected column was qualified by an identifier that named two
//! tables. On a real `DuckDB` 1.5.5 that statement is
//! `Binder Error: Ambiguous reference to table "orders"`; a target that binds it to one side instead
//! returns a number under a certified metric name, which is the failure class this repository is
//! arranged against. **Same-name tables are the normal shape of the estate `docs/adr/0019` exists
//! for** - dev/prod splits, per-tenant datasets, staging copies - so this is reachable rather than
//! exotic.
//!
//! # Why a refusal, and not distinct explicit aliases
//!
//! Distinct aliases are the fix that would keep the question answerable, and they are **not reachable
//! through the SQL builder this workspace renders with**, which was measured rather than assumed
//! against `polyglot-sql` 0.9.2: `SelectBuilder::from_expr` takes an expression, so the `FROM` side
//! could carry an `AS`, but `left_join` and every other join method take a `&str` table name and
//! `join_with_kind` is private - so the JOINED side cannot be aliased without hand-building a select
//! expression with upwards of thirty fields, which `sutura_sql`'s renderer rules out at its own header
//! for a reason. An alias on one side of a join and not the other is not a fix.
//!
//! So the decision is the other one, and it is made where the plan is built rather than where it is
//! rendered: **a statement whose tables cannot be told apart is unrepresentable.** There is no
//! [`QueryPlan`](crate::plan::QueryPlan) and no [`LegPlan::Fact`](crate::plan::LegPlan::Fact) holding
//! such a set, because [`StatementTables`] is the only way to construct either and its canonical
//! constructor refuses the pair. `sutura_semantic::plan` turns
//! that refusal into
//! [`PlanTablesShareAnIdentifier`](crate::query::RefusalReason::PlanTablesShareAnIdentifier), so the
//! question is declined and the metric stays authorable: a question that does NOT reach the colliding
//! table is still answered. The alternative - refusing the metric at load - would make the estate
//! shape unauthorable, and unlike a label a physical table is not something an author can rename.
//!
//! # The limit, and what it used to be
//!
//! **It used to be the fact leg, and that was not theoretical: the change that gave a leg a producer
//! shipped the bypass this section predicted.** `LegPlan::Fact` carried a `table` and a `joins` field
//! and was built by struct literal, so a federated question over a fact table at
//! `analytics_prod.sales.orders` with a same-source dimension table at `reference_data.crm.orders`
//! compiled, and its fact leg rendered
//! `FROM ...sales.orders LEFT JOIN ...crm.orders ON orders.customer_id = orders.customer_id`. Worse
//! than the whole-answer case rather than equal to it, because a leg's rows are combined above it and
//! nothing downstream sees the statement. That variant now takes a [`StatementTables`] as its field
//! instead, pinned by a `compile_fail` doctest with a compiling twin, so a second leg producer cannot
//! reintroduce it - **a prediction in a doc comment is not a mechanism, which is the lesson worth
//! keeping from this.**
//!
//! What remains is narrow and stated so it is not mistaken for the above. A
//! [`Lookup`](crate::plan::LegPlan::Lookup) leg reads ONE table and declares no joins, so it has no
//! pair to compare - the shape is the check. And two LEGS whose tables collide are not this defect:
//! each leg is its own statement on its own data system, so nothing binds one identifier to two
//! tables; what the combiner joins on is a label, and a label that shadowed a table is
//! [`LabelShadowsTable`](crate::catalog::InconsistentDefinitions::LabelShadowsTable)'s refusal at
//! load.

use crate::model::{IdentifierCase, QualifiedTable, TableName};
use crate::plan::PlanJoin;

/// Why the tables one statement reads could not be told apart inside it.
///
/// One variant today, and an enum rather than a struct because a second way for a statement's tables
/// to be indistinguishable - a target that folds more than ASCII case, an alias this workspace starts
/// emitting - is a variant a caller can branch on rather than a change to a message.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AmbiguousTables {
    /// Two of the statement's tables answer to one identifier.
    ///
    /// Carries both paths and the identifier they collapse to, so the refusal says which two tables
    /// rather than sending a reader to count them. `first` is whichever occurs earlier in the
    /// statement: the `FROM` table, then the joins in plan order. Where the two differ only in case,
    /// `alias` is the LATER spelling - the earlier one is readable off `first`.
    ///
    /// **The two paths are their dotted TEXT rather than a [`QualifiedTable`] each, and the reason is
    /// a lint this workspace deliberately does not allow.** `clippy::result_large_err` is on because
    /// a service whose public surface is a refusal wants to know when the error half of a `Result`
    /// grows, and two `QualifiedTable`s plus a [`TableName`] measured 168 bytes against a threshold of
    /// 128. `sutura_sql::generate::GenerateError::QualificationUnsupported` already carries its table
    /// the same way and for the same reason, and the precedent for trimming rather than allowing the
    /// lint is `crate::knowledge`'s `AmbiguousPhrase`, whose own note says two referents were 160
    /// bytes of it. Nothing branches on these two: the one production caller reads [`Self::alias`] and
    /// the transports carry the identifier alone.
    #[error(
        "the statement reads {first} and {second}, which both answer to the identifier {alias}, so a column qualified by it names neither"
    )]
    OneIdentifierTwoTables {
        alias: TableName,
        first: String,
        second: String,
    },
}

impl AmbiguousTables {
    /// The identifier two tables collapsed to.
    #[inline]
    #[must_use]
    pub const fn alias(&self) -> &TableName {
        match *self {
            Self::OneIdentifierTwoTables { ref alias, .. } => alias,
        }
    }
}

/// The tables one statement reads: the `FROM` table, and one per join.
///
/// **If an instance of this type exists, every table in it is distinguishable from every other one
/// inside the statement** - which is the whole return on the newtype, and what lets
/// [`QueryPlan::new`](crate::plan::QueryPlan::new) stay infallible while the plan it builds cannot be
/// the ambiguous one. See this module's header for what the ambiguity does and why it is refused
/// rather than aliased around.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StatementTables {
    table: QualifiedTable,
    joins: Vec<PlanJoin>,
}

impl StatementTables {
    /// The `FROM` table and its joins, or a refusal if two of them answer to one identifier.
    ///
    /// **The canonical constructor.** [`Self::only`] is the no-join spelling of it and repeats no
    /// check, because one table cannot collide with itself.
    ///
    /// Compared under [`IdentifierCase::COARSEST`] rather than by equality, because `GoogleSQL`
    /// resolves an alias case-insensitively and a real `DuckDB` binds `"orders".id` against a table
    /// declared `"Orders"` - so `Orders` beside `orders` is the same defect spelled to look like two
    /// names. That type's own note is where the argument for comparing under the coarsest rule lives.
    ///
    /// The comparison is over POSITIONS and not over distinct paths, so the same table joined twice
    /// through two relationships is refused too: two occurrences under one identifier is a duplicate
    /// alias whether or not they name the same rows.
    pub fn parse(table: impl Into<QualifiedTable>, joins: Vec<PlanJoin>) -> Result<Self, AmbiguousTables> {
        let table = table.into();
        // Quadratic over a list bounded by `MAX_DIMENSIONS`, and a pair of loops rather than a map
        // on purpose: a set keyed on a folded name would have to hold the fold, and the fold is
        // `IdentifierCase`'s to own rather than this function's to copy.
        let mut seen: Vec<&QualifiedTable> = vec![&table];
        for join in &joins {
            let candidate = join.table();
            let collision = seen
                .iter()
                .find(|earlier| IdentifierCase::COARSEST.names_one_thing(earlier.name().as_str(), candidate.name().as_str()));
            if let Some(earlier) = collision {
                return Err(AmbiguousTables::OneIdentifierTwoTables {
                    alias: candidate.name().clone(),
                    first: earlier.to_string(),
                    second: candidate.to_string(),
                });
            }
            seen.push(candidate);
        }
        Ok(Self { table, joins })
    }

    /// One table and no joins.
    ///
    /// Infallible by construction rather than by a skipped check: a set of one has no pair to compare.
    #[inline]
    #[must_use]
    pub fn only(table: impl Into<QualifiedTable>) -> Self {
        Self {
            table: table.into(),
            joins: Vec::new(),
        }
    }

    /// Where the statement's own table lives: the whole path, which is what the `FROM` names.
    #[inline]
    #[must_use]
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }

    /// Every join, in the order the statement will make them.
    #[inline]
    #[must_use]
    pub fn joins(&self) -> &[PlanJoin] {
        &self.joins
    }

    /// The two halves, for the one caller that stores them separately.
    ///
    /// `pub(crate)` deliberately: a plan keeps `table` and `joins` as fields of its own so its
    /// serialized form - which a golden pins - is unchanged by this type existing, and taking them
    /// apart is that constructor's business rather than an operation on a checked set.
    #[inline]
    pub(crate) fn into_parts(self) -> (QualifiedTable, Vec<PlanJoin>) {
        (self.table, self.joins)
    }
}

#[cfg(test)]
mod tests;
