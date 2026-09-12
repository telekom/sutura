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
//! **Four ports live here now, and each arrived with the adapter that implements it.** A port exists
//! to invert a dependency on something outside the hexagon, so a trait with no implementor is a
//! guess at a signature that only the first real adapter can settle, and in a library crate `pub`
//! hides such a guess from `dead_code`. [`pinned::SemanticCatalog`] arrived with the local catalog
//! adapter, [`warehouse::Warehouse`] with the `DuckDB` one, [`audit::AuditSink`] with the structured
//! writer in `sutura-runtime` - the sink a deployment that attaches nothing else gets - and
//! [`identity::CredentialBroker`] with `sutura_config::StaticCredentialBroker`, which is in the
//! settings crate because the identity provider it reads *is* the settings tree.
//!
//! **What the credential port did and did not buy, said here because the count above invites the
//! wrong reading.** There is no longer a signature that reaches a data system with a question and no
//! credential, and a subject with no credential at a source is refused rather than answered as the
//! process. What is absent is the other end: no adapter in this build has anywhere for a per-subject
//! credential to arrive, so a leg runs under the identity an operator declared for that source and
//! [`pinned::Provenance`] records which. The one method that still executes with no credential is
//! [`warehouse::Warehouse::verify_anchor`], the boot path's; `clippy.toml` bans it everywhere else and
//! [`plan::AnchorPlan`] says plainly that it is a self-check on that path rather than a barrier.
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
//!   into a leg, which one descends decomposed, and which needs its rows pulled up. The splitter
//!   and the combiner ([`plan::FederatedPlan::combine`]) both call it, and since
//!   `sutura-exec-datafusion` declares `Warehouse::EXECUTES_LEGS` a published build answers a
//!   two-source question end to end - so this is a classification on the answer path rather than
//!   one with no production caller, which is what this line used to say.
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
//! - [`source`] is what a deployment declares about one source: which identity a query reaches it as,
//!   which identity re-ran its anchors at boot, and - separately, because a different party declares
//!   it - whether the linked adapter can carry a per-subject credential at all. It also holds the
//!   per-leg execution record [`pinned::Provenance`] carries, which is read off what the adapter was
//!   handed rather than off a settings tree.
//! - [`definitions`] and [`identity`] hold the digest and the credential-shaped newtypes. The
//!   principal chain a call is attributed to lives in [`identity`] as well, beside the redaction and
//!   the credential port, because all three are properties of who is asking rather than of what was
//!   asked.
//! - [`audit`] is the record one call is written to, and the port it goes through. It is not a
//!   store: sutura writes a record before the outcome returns and retains nothing, so what the
//!   sink does with it is the deployment's.
//!
//! One module is private, and it is the only one: `text` holds the set of invisible and
//! direction-changing code points that a phrase, a note body, a version label and an authored SQL
//! fragment all refuse. It exists because that set was written down twice, in two files, and the two
//! had already drifted.

pub mod audit;
pub mod calendar;
pub mod capabilities;
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
#[cfg(test)]
mod serialized_form_tests;
pub mod source;
mod text;
pub mod warehouse;

// probe(#639 slice I): deliberate rustfmt violation, measured then reverted
pub const   PROBE_STAGE_ONE : u8   =   1 ;
