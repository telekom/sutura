---
title: A closed enum over the shipped warehouse kinds
description: Warehouses<W> stays generic in one adapter and takes no adapter dependency; a composition root builds the concrete per-kind registry it always built and erases it behind a closed enum over the adapters that BUILD linked, so a deployment can hold two kinds of source instead of refusing at the kind mismatch - and why the leg-capability gate had to move from a type-level constant to an instance method to make that safe.
---

# A closed enum over the shipped warehouse kinds

Status: **accepted and built.** `github.com/telekom/sutura#112`, blocker 1: `sutura_app::Warehouses<W>`
is generic in one `W`, `Warehouse::IMPERSONATION` is a required associated constant with no default,
so the port is not object-safe and `crate::serve::one_kind` refused any catalog whose declared
sources spanned more than one kind. This record decides where the remedy lives, what it costs, and
what it deliberately still does not fix. Amends [0007](0007-federating-across-different-data-systems.md)
and [0009](0009-the-plan-from-one-source-to-many.md), each at their own *Second amendment*.

## What this decides

**The enum is `crate::serve::kind::AnyWarehouse` in `sutura-cli`, not a type in `sutura-app`.**
This thread's own comments disagreed with each other about that before either side was measured
against the tree: `sutura-app`'s `[dependencies]` names `sutura-domain`, `sutura-semantic` and
`thiserror` and nothing else, and `crates/sutura-cli/src/serve.rs`'s own doc comment already
carried the reason - "which adapters a process holds is a property of the BUILD" - naming
`sutura_app::warehouses` as the record this decision would land in. Putting the enum in
`sutura-app` would have been the first normal edge that crate ever took onto an adapter.

**`sutura_app::Warehouses` gains two adapter-agnostic methods rather than an adapter type.**
`into_mapped<U>(self, wrap: impl FnMut(W) -> U) -> Warehouses<U>` erases a concrete registry into
whichever enum the caller declares; `merge(self, other: Self) -> Result<Self, SourceAlreadyOpen>`
is `and`'s whole-registry sibling. Both are generic in the adapter type and import none, so
`sutura-app` still composes the port over whatever a root hands it rather than knowing what a root
built it from. `crate::serve::kind::open_mixed` is the caller: for each of the (at most three)
kinds a catalog declares, it calls the SAME `open_files`/`bigquery::open_bigquery`/
`postgres::open_postgres` the single-kind path always called - so a mixed deployment is refused by
the identical message a single-kind one would be, for whichever half is wrong - then wraps each
concrete registry and merges them into one `Warehouses<AnyWarehouse>`.

**The missing boundary is now a gate, not a convention.** `AGENTS.md`'s layout table, the
`crate-map` skill and `sutura-app`'s own module doc all said `-app` holds no adapter, and none of
`check-boundaries`' eight existing halves could see a `sutura-app -> sutura-exec-*` edge:
`FORBIDDEN_EDGES` is a denylist naming two crates and had no row naming `sutura-app`; the
intra-class rule compares members of the SAME class against each other, and `sutura-app` belongs
to none of the three. `xtask/src/boundaries/application.rs` is the ninth half - one named crate
against a class, the shape neither sibling rule is - proven both ways: a fake `sutura-exec-datafusion`
dependency added to `sutura-app/Cargo.toml` and reverted, `cargo run -p xtask -- check-boundaries`
read `FAILED - the application reaches a data-system adapter` naming the edge, then `ok` again.

**The leg-capability gate moved from the type to the instance, and that is the one change that
could not be avoided.** `Warehouse::EXECUTES_LEGS` is an associated constant: one value for a
whole TYPE. An enum wrapping a leg-capable adapter and a leg-declining one cannot answer this
constant truthfully for both variants at once - the safe direction is the declining one, which
would silently refuse every question a mixed build could otherwise answer on its capable leg.
`Warehouse::executes_legs(&self) -> bool`, defaulted to `Self::EXECUTES_LEGS`, is the escape: an
adapter that never overrides it keeps its one type-level fact for free, and the enum overrides it
to match on `self` and delegate to whichever concrete adapter it holds. `sutura_app::federated::answer_federated`
reads it per LEG now - `fact_warehouse.executes_legs() && lookup_warehouse.executes_legs()` - rather
than once per build, so a mixed registry with one capable kind and one declining kind refuses only
the answer that would have needed the declining leg, exactly as a build linking only the capable
kind always did.

**`IMPERSONATION` stays a lie the enum's own callers never ask.** It is fixed at
`NoPlaceForASubject` for every variant - the restrictive direction, because the permissive one
would let a `files` source silently pass a check meant for a source that can actually carry a
subject. Nothing reads it generically: `SourcePosture::deliverable_by` is called at each
composition site against the source's own CONCRETE adapter constant, before that adapter is ever
wrapped. `EXECUTES_AUTHORED_SQL`, `ACCEPTS_RAW_STATEMENTS` and `PRICES_DRY_RUN` take the same
conservative default and are a real, stated limit rather than an inert one - see *What this does
not decide*.

## Consequences

- `crate::serve::OpenedSources` gains a fourth variant, `Mixed(kind::Mixed)`, reached the moment
  more than one of `kind::group_by_kind`'s three groups is non-empty; the three single-kind
  variants and their call sites are untouched.
- `boot::refuse_absent_tables` lost its `#[cfg(feature = "bigquery")]` gate: the mixed arm calls it
  unconditionally over the whole heterogeneous registry, so a `files` entry now logs the same
  informational `NotReported` line a `BigQuery` entry always has - `dead_code` no longer depends on
  that one feature having a caller.
- `RefusalReason::FederationNotExecutable`'s own doc moved from "a fact about the BUILD, read once"
  to "a fact about the two adapters this answer would run on, read per leg" - the same correction
  this record makes to `docs/adr/0009`'s `test/startup-source-refusals` row.
- Proof, not assertion: `AsymmetricLegWarehouse` (`sutura-app`'s own test support) is one TYPE whose
  `EXECUTES_LEGS` constant is `true` and whose `executes_legs()` override answers a runtime field;
  a registry of two instances, one built `false`, still refuses the whole answer under the new
  gate and would have RUN both legs under the old one - measured directly, by temporarily
  restoring the old `if !W::EXECUTES_LEGS` gate and watching that exact test fail with an `Answer`
  rather than a `Refusal`.

## What this does not decide

- **Whether a heterogeneous or leg-2 build actually SHIPS.** This is a publishing and cost call
  belonging to the maintainer - the musl/closure/cost surface `github.com/telekom/sutura#112`'s own
  body names - and nothing here makes it. `nix/shipped.nix` is unchanged.
- **Promoting `sutura-exec-bigquery`'s `EXECUTES_LEGS`.** It stays at the port's default. A mixed
  build combining `files` and `bigquery` sources today opens both and refuses only a question that
  would need the `bigquery` leg to run - `FederationNotExecutable`, unchanged in shape, now scoped
  to the leg that cannot run rather than to the whole build.
- **A mixed-posture answer's reachability.** `RefusalReason::LegsDecideIdentityDifferently` is
  unaffected and unexercised by this record: every adapter this workspace ships is either
  `NoPlaceForASubject` or, for `BigQuery`, `PerSubjectCredential` without `EXECUTES_LEGS`, so no
  shipped combination can put two genuinely different postures on the same federated answer yet.
- **The three type-level consts this enum still lies about the conservative direction on.** A
  mixed build cannot serve the raw-SQL tool against a `Postgres` source it holds
  (`ACCEPTS_RAW_STATEMENTS`), read a real dry-run byte estimate off a `BigQuery` source it holds
  (`PRICES_DRY_RUN`), or execute an authored-SQL metric through any wrapped adapter
  (`EXECUTES_AUTHORED_SQL`), even where the underlying adapter could. Each would need the same
  instance-method escape `executes_legs` got, asked for by name when a deployment needs it.
- **A live, end-to-end served test of two kinds answering over HTTP.** The boot path is proven at
  the composition root (`crates/sutura-cli/src/serve/tests.rs`) and the per-leg combining
  mechanism is proven at the domain/application boundary (`crates/sutura-app/tests/differential/federated/two_kinds.rs`,
  mixing two DEV-ONLY adapters that both execute a leg, since no two SHIPPED kinds both do). A
  served deployment mixing real linked kinds and answering a federated question over the wire is
  not exercised here.
