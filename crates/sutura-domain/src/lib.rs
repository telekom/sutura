//! The hexagon's interior: the types the business rules are written in, and the port traits it
//! names its dependencies by.
//!
//! Nothing here may depend on a framework: no async runtime, no web server, no query engine.
//! `cargo xtask check-boundaries` enforces it over the whole transitive tree, because the rule
//! is worth more as a check than as a sentence in a design document. The allowlist is `serde` and
//! `thiserror` and their proc-macro support, plus the `serde_json` and `sha2` that the definition
//! digest needs, and nothing else - which is why there is a hand-written calendar in [`calendar`]
//! and no SQL parser anywhere in this crate, [`expression`] included.
//!
//! **Two ports live here now, and each arrived with the adapter that implements it.** A port exists
//! to invert a dependency on something outside the hexagon, so a trait with no implementor is a
//! guess at a signature that only the first real adapter can settle, and in a library crate `pub`
//! hides such a guess from `dead_code`. [`pinned::SemanticCatalog`] arrived with the local catalog
//! adapter and [`warehouse::Warehouse`] with the `DuckDB` one. `CredentialBroker` is still absent for
//! the same reason it always was: nothing implements it yet.
//!
//! The modules are grouped by concept rather than named after traits, so a port sits next to the
//! types it speaks in:
//!
//! - [`model`] and [`calendar`] are the vocabulary: names, closed sets, dates.
//! - [`measure`] is what a metric measures, as a closed vocabulary of shapes rather than an
//!   expression language.
//! - [`expression`] is the escape hatch beside it: SQL a catalog author wrote, for the metrics that
//!   vocabulary cannot say. It holds no parser - `sutura_sql` compiles a fragment at load - and
//!   [`expression::Computation`] is what makes "this metric is authored SQL" a word rather than an
//!   absence.
//! - [`plan`] is what we decided to execute, and the artifact the execution port speaks in.
//! - [`federation`] is how a measure survives being computed in pieces: which aggregates descend
//!   into a leg, which one descends decomposed, and which needs its rows pulled up. Nothing executes
//!   it yet - there is no splitter and no combiner - so it is a classification with no production
//!   caller, and its own header says so.
//! - [`catalog`] is what a catalog says, and where its cross-references are checked.
//! - [`knowledge`] is what a catalog says ABOUT what it defines - the glossary, the caveats, the
//!   terms deliberately left undefined, the worked questions - checked against a [`catalog`] and read
//!   by nothing but the agent-facing prompt. It is separate from [`catalog`] because the compiler
//!   must not be able to reach it: descriptive content that could select what executes would not be
//!   descriptive content.
//! - [`pinned`] is the hashed snapshot a question resolves against, plus the catalog port.
//! - [`query`] is the tool surface, defined mostly by what it has no field for.
//! - [`warehouse`] is the execution port. It speaks in plans, so an adapter that executes without
//!   generating any SQL is a first-class implementation of it rather than a special case.
//! - [`definitions`] and [`identity`] hold the digest and the credential-shaped newtypes.
//!
//! One module is private, and it is the only one: `text` holds the set of invisible and
//! direction-changing code points that a phrase, a note body, a version label and an authored SQL
//! fragment all refuse. It exists because that set was written down twice, in two files, and the two
//! had already drifted.

pub mod calendar;
pub mod catalog;
pub mod definitions;
pub mod expression;
pub mod federation;
pub mod identity;
pub mod knowledge;
pub mod measure;
pub mod model;
pub mod pinned;
pub mod plan;
pub mod query;
mod text;
pub mod warehouse;
