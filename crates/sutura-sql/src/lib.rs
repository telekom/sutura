#![forbid(unsafe_code)]
//! Rendering: a `QueryPlan` becomes one statement in one dialect.
//!
//! Three modules and one type. `dialect` names the data systems we render for and owns the two
//! decisions the dialect layer does not make for us; `generate` turns a plan into a statement and
//! its bind parameters; `expression` compiles the one thing a catalog is allowed to author as SQL,
//! at load, for every dialect at once. [`GeneratedQuery`] is what `generate` returns, and it is at
//! the crate root because it is this crate's output rather than any one module's detail.
//!
//! **Two entry points, one output type.** [`generate`](fn@crate::generate) renders a whole answer
//! from a `QueryPlan`; [`generate_leg`] renders one leg of a federated question from a `LegPlan`.
//! They share every decision that could drift - the quoting, the placeholder style, the bucket, the
//! joins, how a term renders - and differ in the four ways `generate_leg`'s own documentation
//! lists. **A RELEASE now calls the second one, and that sentence used to say the opposite.** It
//! read *nothing a release runs calls it, because the one leg-executing adapter a release links is
//! the engine, which renders no SQL*; `sutura-exec-postgres` declares `Warehouse::EXECUTES_LEGS`
//! and `nix/shipped.nix` carries the `postgres` feature in the shipped artefact, so a deployment
//! holding two Postgres sources renders both legs of a federated answer here. **The limit:** the
//! leg goldens under `crates/sutura-app/tests/golden` still pin five dialects and only the Postgres
//! statements among them are what a release executes - and what establishes that one is EXECUTED is
//! `crates/sutura-exec-postgres/tests/conformance.rs`, a real tier answering a leg, not a golden.
//! Oracle's leg renders here too and no venue any gate reaches can run it.
//!
//! # Why this is its own crate and not the compiler's last stage
//!
//! It used to be `sutura_semantic::generate` and `sutura_semantic::dialect`, and rendering was
//! already off the compile path - `sutura_semantic::compile` stops at a plan, because the
//! `Warehouse` port's currency is a `QueryPlan` and an adapter that executes over Arrow renders
//! nothing. What did not follow was the dependency: rendering was a `pub` module of the core, so
//! `polyglot-sql` sat in the transitive closure of every consumer of `sutura-semantic` - including
//! the network binary, which links the engine, renders nothing, and can reach no code in here.
//!
//! Sharing the quoting and the placeholder decisions between SQL adapters is a good reason for this
//! code to be shared. It is not a reason for it to sit in the core. A separate crate gives the same
//! sharing, keeps the generator out of the core's closure, and makes the direction a **gate** rather
//! than a comment: `cargo xtask check-boundaries` fails if `sutura-semantic` can reach either this
//! crate or `polyglot-sql`.
//!
//! So the direction is one way only. This crate depends on `sutura-domain` - a plan comes in, a
//! [`GeneratedQuery`] goes out - and on the dialect layer. It does **not** depend on
//! `sutura-semantic`, and `sutura-semantic` does not depend on it. Neither needs the other: one
//! decides what to execute, the other writes it down for a data system that speaks SQL.
//!
//! # Who calls it
//!
//! An adapter that pushes a statement down: `sutura-exec-duckdb` renders for its own dialect and no
//! other. `sutura-cli` calls it too, because `sutura compile` exists to print the statement for a
//! dialect somebody named. The engine adapter calls nothing here, which is the whole point of the
//! port taking a plan.
//!
//! **Nothing here translates between dialects, and exactly one thing here parses SQL.** The
//! statement is generated from a model, so there is no foreign SQL on the *query* path to parse.
//! Translation is banned separately: the `transpile` feature is not even compiled - see the feature
//! list in the workspace manifest for why not calling it was judged too weak.
//!
//! The one exception is [`expression`], and it is an exception with a stated shape. A catalog may
//! author a SQL fragment for a metric the closed measure vocabulary cannot express, and that
//! fragment is parsed - **at catalog-compile time, once, never on the query path** - checked against
//! a list of constructs this build refuses, qualified against the model's columns, and rendered for
//! every dialect. What reaches a statement afterwards is our own generator's output. `docs/adr/0004`
//! is the decision, and its amendment is the state of the tree: **nothing published calls
//! [`expression::compile`] today.** An authored fragment is loaded, pinned as written and refused at
//! boot; a catalog adapter may not reach this crate (`cargo xtask check-boundaries` forbids the edge,
//! for the closure reason above), so the caller, when it exists, is the execution adapter that
//! declares `Warehouse::EXECUTES_AUTHORED_SQL` - and it compiles beside the renderer it needs.
use sutura_domain::model::SourceName;
use sutura_domain::warehouse::ParamValue;

pub mod dialect;
pub mod expression;
pub mod generate;

pub use crate::dialect::{Dialect, PlaceholderStyle};
pub use crate::expression::refusal::{Construct, ExpressionError};
pub use crate::expression::{CompiledExpression, Rendering, compile};
pub use crate::generate::{GenerateError, generate, generate_key_probe, generate_leg};

/// A statement, its parameters, and the one data system it runs against.
///
/// **Parameters are a separate field and there is no constructor that merges them.** That is the
/// mechanism behind "no value from a question reaches the statement as text": to inline a value an
/// adapter would have to build the string itself, which is a diff rather than an oversight.
///
/// `source` rides along because a plan resolves to exactly one data system, and carrying it here is
/// what lets the composition root check that the adapter it is about to call is the one the plan
/// named.
///
/// **It is in this crate rather than in `sutura-domain`, and that is the same argument this crate
/// exists for.** It sat beside the `Warehouse` port while the port took a rendered statement, on the
/// reasoning that the port had to hand one to something. The port takes a `QueryPlan` now - an
/// adapter that executes over Arrow renders nothing and never sees one of these - and once the port
/// changed, nothing in the domain constructed or read one: the type was a concept the domain named
/// and did not use. Its producer is `generate` one module over, and every consumer - the SQL
/// adapters, and the CLI so `sutura compile` can print a statement - already depends on this crate.
/// So the move added an edge nowhere and made the domain smaller by exactly the part of it that was
/// not domain.
///
/// **A6: a caller outside this crate cannot pair an arbitrary `sql` with an arbitrary `params`.**
/// [`GeneratedQuery::new`] is `pub(crate)`; the only public constructor for a caller elsewhere is
/// [`GeneratedQuery::literal`], which takes no parameters at all.
///
/// ```compile_fail
/// use sutura_domain::model::SourceName;
/// let source = SourceName::parse("local").expect("a test source is a source");
/// // `new` is `pub(crate)`; a caller in another crate has no way to pair `sql` with `params`.
/// let _query = sutura_sql::GeneratedQuery::new(source, String::from("SELECT ?"), Vec::new());
/// ```
///
/// The compiling twin: the one constructor a caller outside this crate may reach.
///
/// ```
/// use sutura_domain::model::SourceName;
/// let source = SourceName::parse("local").expect("a test source is a source");
/// let query = sutura_sql::GeneratedQuery::literal(source, String::from("SELECT 1"));
/// assert!(query.params().is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GeneratedQuery {
    source: SourceName,
    sql: String,
    params: Vec<ParamValue>,
}

impl GeneratedQuery {
    /// `pub(crate)`, deliberately: [`generate()`] is the one place that derives `sql` and `params`
    /// together, and a `pub` constructor let any caller pair an arbitrary parameter list with
    /// arbitrary text - A6's finding. [`Self::literal`] is the one shape a caller outside this
    /// crate legitimately needs, and it cannot mismatch because it has no parameters to mismatch.
    pub(crate) const fn new(source: SourceName, sql: String, params: Vec<ParamValue>) -> Self {
        Self { source, sql, params }
    }

    /// A statement with no bind parameters, for a caller that is not this crate's own generator.
    ///
    /// **The one external use today is a test fixture** (`sutura-exec-duckdb`) exercising a literal
    /// `SELECT` with nothing to bind - exactly what this constructor can build and nothing else.
    /// The general two-list constructor stays crate-private, so no external caller can pair a
    /// nonempty `params` with text it did not derive from them.
    #[must_use]
    pub const fn literal(source: SourceName, sql: String) -> Self {
        Self::new(source, sql, Vec::new())
    }

    #[inline]
    pub const fn source(&self) -> &SourceName {
        &self.source
    }

    #[inline]
    pub fn sql(&self) -> &str {
        &self.sql
    }

    #[inline]
    pub fn params(&self) -> &[ParamValue] {
        &self.params
    }
}

#[cfg(test)]
mod tests {
    use super::GeneratedQuery;
    use sutura_domain::calendar::Date;
    use sutura_domain::model::SourceName;
    use sutura_domain::warehouse::ParamValue;

    #[test]
    fn a_generated_query_keeps_its_parameters_out_of_its_text() {
        // The mechanism behind the no-injection claim, asserted at the type level: an adapter
        // receives the statement and the values separately, so inlining one would be its own code
        // rather than an accident here.
        let query = GeneratedQuery::new(
            SourceName::parse("local").expect("a test source is a source"),
            String::from("SELECT 1 WHERE d >= ? AND d < ?"),
            vec![
                ParamValue::Date(Date::parse("2026-06-01").expect("a test date is a date")),
                ParamValue::Date(Date::parse("2026-07-01").expect("a test date is a date")),
            ],
        );
        assert!(!query.sql().contains("2026"), "{}", query.sql());
        assert_eq!(query.params().len(), 2);
        assert_eq!(query.source().as_str(), "local");
    }
}
