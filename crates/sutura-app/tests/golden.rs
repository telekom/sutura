//! The golden suite: one corpus of questions, expanded over every registered adapter.
//!
//! Everything here is arranged so a change to what we compile shows up as a reviewable diff rather
//! than as a different number. **The snapshots are regenerated and read as a diff, never typed.** A
//! hand-written expectation asserts what somebody wished the generator did.
//!
//! **The suite is a matrix over `adapters::registered`, and that is the shape rather than a
//! convenience.** `SemanticCatalog` and `Warehouse` exist so a metadata provider and a data system are
//! choices. A suite that named one of each in every test proved only that those two work, and made a
//! third one a test edit - which is the opposite of what a port is for. So no test below names an
//! adapter: one registry does, and each corpus is expanded once per entry. Registering an adapter is
//! an `impl` of `adapters::CatalogUnderTest` or `adapters::DataSystemUnderTest` plus one line in
//! `adapters::registered`.
//!
//! Three axes, and each carries the artefacts it actually decides:
//!
//! - **The catalog axis.** The definitions and the digest, so also every plan and every compile-side
//!   refusal - a plan is a function of the definitions and of nothing else. Each registered catalog is
//!   compared against `support::oracle_definitions`, which is those same definitions written out in
//!   Rust by hand, so two adapters cannot agree by sharing a parser bug.
//! - **The dialect axis.** The rendered statement: the SQL, its parameters, the parse check, and the
//!   mechanical no-injection and quoting assertions. Per dialect, so a change to identifier quoting or
//!   placeholder style is visible in the target it affects rather than only in the one we execute
//!   against. Rendering `ClickHouse` SQL is not a claim that a `ClickHouse` exists anywhere.
//! - **The data-system axis.** The rows: the anchor check, the executed corpus, and `dry_run`
//!   acceptance. Two entries today and they are two different kinds of thing behind one port - an
//!   engine that generates no SQL, and a data source that renders and pushes down.
//!   `tests/differential.rs` holds the fourth cell of this axis, which is agreement with the engine.
//! - **Off every axis**, in `golden/service.rs`: the assertions about the service and the compiler
//!   rather than about an adapter, which use a fake. A refusal, and a plan for a data system nobody
//!   opened, are decided above the port - so the honest instrument is one fake and not a matrix.
//!
//! **One test target, split by axis.** `tests/*.rs` at the top level is a test target each, so the
//! axes live under `tests/golden/` and are modules of this one: they share the fixtures, the fakes and
//! the compile time of linking an engine and a data source. `no file exceeds 1000 lines` is a gate
//! here and the only way past it is to split, which is what made the seam visible - and one file per
//! axis is where it was always going to be.
//!
//! Wrapped in `#[cfg(test)]` modules, which looks redundant for an integration test that is only ever
//! compiled as one. `clippy::tests_outside_test_module` is on for the whole workspace, and being
//! consistent with it costs one attribute per module.

// `cfg(test)` because an integration test target is compiled with `--test`, so it is true here - and
// clippy only honours `allow-expect-in-tests` for code inside a `#[cfg(test)]` item. Without it every
// `expect` in a fixture builder is a lint error, and writing fixture setup in the `?`-ceremony the ban
// would demand makes the fixtures worse, which is what that exemption exists to avoid.
#[cfg(test)]
mod adapters;
#[cfg(test)]
mod support;

// `#[path]` because a bare `mod catalogs;` at a crate root resolves to `tests/catalogs.rs`, and cargo
// would build that as a test target of its own.
#[cfg(test)]
#[path = "golden/shared.rs"]
mod shared;

#[cfg(test)]
#[path = "golden/catalogs.rs"]
mod catalogs;

#[cfg(test)]
#[path = "golden/dialects.rs"]
mod dialects;

#[cfg(test)]
#[path = "golden/data_systems.rs"]
mod data_systems;

#[cfg(test)]
#[path = "golden/service.rs"]
mod service;
