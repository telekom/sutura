//! Rendering: a `QueryPlan` becomes one statement in one dialect.
//!
//! Two modules, and they are the two halves of what a SQL-speaking adapter needs: `dialect` names
//! the data systems we render for and owns the two decisions the dialect layer does not make for us,
//! `generate` turns a plan into a statement and its bind parameters.
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
//! `GeneratedQuery` goes out - and on the dialect layer. It does **not** depend on
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
//! **Nothing here parses SQL, and nothing here translates between dialects.** There is no foreign
//! SQL on this path to parse: the statement is generated from a model. Translation is banned
//! separately, in `clippy.toml`, and the `transpile` feature is not even compiled - see the feature
//! list in the workspace manifest for why not calling it was judged too weak.
pub mod dialect;
pub mod generate;

pub use crate::dialect::{Dialect, PlaceholderStyle};
pub use crate::generate::{GenerateError, generate};
