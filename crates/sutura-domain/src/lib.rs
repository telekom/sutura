//! The hexagon's interior: the types the business rules are written in, and the port traits it
//! names its dependencies by.
//!
//! Nothing here may depend on a framework: no async runtime, no web server, no query engine.
//! `cargo xtask check-boundaries` enforces it over the whole transitive tree, because the rule
//! is worth more as a check than as a sentence in a design document. The allowlist is `serde` and
//! `thiserror` and their proc-macro support, and nothing else - which is why there is a hand-written
//! calendar in [`calendar`] and no `serde_json` in any test here.
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
//! - [`catalog`] is what a catalog says, and where its cross-references are checked.
//! - [`pinned`] is the hashed snapshot a question resolves against, plus the catalog port.
//! - [`query`] is the tool surface, defined mostly by what it has no field for.
//! - [`warehouse`] is the execution port, and the one place a generated statement is named.
//! - [`definitions`] and [`identity`] hold the digest and the credential-shaped newtypes.

pub mod calendar;
pub mod catalog;
pub mod definitions;
pub mod identity;
pub mod model;
pub mod pinned;
pub mod query;
pub mod warehouse;
