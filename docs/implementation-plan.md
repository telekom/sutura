# The implementation plan

Operational, and expected to churn. The decisions it executes live in
[the ADRs](adr/0009-the-plan-from-one-source-to-many.md) and do not change because a step turned out
harder than it looked. If a step cannot be done as written, the ADR is the thing to argue with.

**Eleven steps, four of which can start at once.** Every step is one branch, one pull request, and green
before the next depends on it. `stax` manages the stack; the `git-ops/stacked-branches` skill has the
mechanics. Nothing in this plan is started.

## The stack

| # | Branch | Depends on | Can start now |
| --- | --- | --- | --- |
| 1 | `feat/federation-decomposability` | nothing | **yes** |
| 2 | `feat/principal-chain` | nothing | **yes** |
| 3 | `feat/query-bounds` | nothing | **yes** |
| 4 | `test/startup-source-refusals` | nothing | **yes** |
| 5 | `feat/source-registry` | 3 | after 3 |
| 6 | `feat/two-source-execution` | 1, 5 | after 5 |
| 7 | `feat/conformance-packs` | 6 for the execute half; nothing for the compile half | partly |
| 8 | `feat/credential-port` | 2, 5 | after 5 |
| 9 | `feat/postgres-oauth` | 8, and the driver question | after 8 |
| 10 | `feat/compose-tier` | 9 | after 9 |
| 11 | `feat/source-mtls` | 5, 9 | after 9 |

**Verification for every step**, and none of it is optional: `just validate` is the only thing that
counts, it now runs seven checks including `api-docs`, and every new test must be RED against the base
behaviour and GREEN with the change. Where impl and tests share a file, `test-causality` says so and
asks for stated evidence instead: the mutation, the command, the failure before, the pass after. Any
new FILE must be `git add -N`'d or the nix checks cannot see it.

---

## 1. Decomposability in the domain

**Goal.** A federated query cannot compute an aggregate that does not survive being computed per leg
and re-aggregated. No plumbing, no adapters, no federation: just the classification and the rule.

**Touches.** `crates/sutura-domain/src/measure.rs`, and the plan type in
`crates/sutura-domain/src/plan.rs` only if the classification needs to be visible there.

**Adds.** A total function from each aggregate to how it federates, as an exhaustive match: pushable as
written (`Sum`, `Count`, `Min`, `Max`), pushable decomposed (`Avg` as a sum and a count), not pushable
(`CountDistinct`). Plus the ratio rule: numerator and denominator are separate pushed aggregates and
the division happens once, above.

**Tests.**
- `every_aggregate_states_how_it_federates` - the exhaustive match compiles and covers the vocabulary.
- `a_ratio_is_decomposed_rather_than_divided_per_leg` - the shape that would divide per leg is not
  constructible.
- `an_average_travels_as_a_sum_and_a_count`.
- `a_distinct_count_is_not_pushable`.

**Done when** a new aggregate cannot compile without stating how it federates, and dividing a ratio per
leg is unrepresentable rather than discouraged. Six of the eleven shipped metrics are affected by this
step, so the fixtures exercise it immediately.

## 2. The principal chain

**Goal.** Human, then agent, then task - ordered - present from the first record, while both tail
positions are always absent. This is the step that cannot be deferred: a stored row naming only the
subject can never later be told apart from one that meant "an agent acting for" them.

**Touches.** `crates/sutura-domain` for the types, `crates/sutura-app` for the request context that
carries one.

**Adds.** A principal that is a subject plus an ordered list of actors, and a task identifier that
exists from day one. Ordered innermost-last, which is the shape a token exchange maps onto rather than
being translated into.

**Tests.**
- `a_principal_with_no_actor_is_a_bare_subject_and_says_so`.
- `an_actor_chain_keeps_its_order`.
- `a_record_naming_a_subject_is_distinguishable_from_one_naming_an_agent_acting_for_them` - the whole
  reason for the step, asserted rather than described.

**Done when** anything that records a call records the chain, and the budget key includes it even
though the tail is always absent.

## 3. The three bounds

**Goal.** A result-size ceiling, a query timeout, and a per-leg row bound. Each a typed refusal, never
a truncation.

**Touches.** `crates/sutura-config/src/limits.rs` for the settings,
`crates/sutura-domain/src/query.rs` for the refusal variants.

**Adds.** Three newtypes with defaults - **1 GB result ceiling, three-minute query timeout** - set
globally and overridable **per source**. Three refusal variants, each provokable.

**Tests.**
- `a_result_over_the_ceiling_is_refused_rather_than_truncated`.
- `a_query_over_its_deadline_is_refused_and_the_execution_is_cancelled`.
- `a_per_source_override_wins_over_the_global_default`.
- `a_ceiling_of_zero_is_refused_at_parse` - the newtype does the work.
- One test per refusal variant, because a variant no test can provoke is one the enum refuses to
  carry.

**Done when** each bound produces its own refusal, the memory bound is shown BITING rather than
described, and a partial answer is impossible. `panic = "abort"` is why the memory one matters: an
allocation failure is process death for every caller, not an error for one.

## 4. The startup refusals that already hold

**Goal.** Test what is already true. The more-than-one-source arm of `open_engine` has no test in
either binary, and it is the refusal that actually protects a deployment today.

**Touches.** `crates/sutura-serve/src/main.rs` and `crates/sutura-cli/src/commands.rs` test modules
only. No production code.

**Tests.** One per binary, each asserting the multi-source arm and asserting it is NOT the neighbouring
wrong-name arm, so it cannot pass on the wrong branch.

**Done when** both are red against a build with the branch removed. This is the cheapest step in the
plan and it closes a real gap.

## 5. The source registry, the mode, and the boot check

**Goal.** More than one source becomes configurable, each declaring its mode and its capabilities.

**Touches.** `crates/sutura-config` (a keyed source structure beside `catalog.data_dir`),
`crates/sutura-app/src/surface.rs` (a service over many warehouses rather than one),
`crates/sutura-app/tests/adapters/mod.rs` (the registry the matrix reads).

**Adds.** A source declaration carrying an alias, a mode (`SharedServiceUser` or
`ImpersonationAtSource`), and declared capabilities. A startup check that refuses a source declaring an
impersonation the deployment cannot perform. Provenance gains the mode per leg.

**Tests.**
- `a_duplicate_alias_is_refused_at_parse`, `a_missing_file_is_refused_at_parse`,
  `a_relative_path_is_refused_at_parse` - asserted on the typed variant, not the message.
- `a_source_declaring_an_impersonation_the_deployment_cannot_perform_refuses_at_boot`.
- `a_mode_is_recorded_in_provenance_per_leg`.
- `an_adapter_with_no_declared_mode_does_not_compile` - a compile-fail doctest, with a compiling twin.

**Done when** two sources can be configured, each says what it is, and an answer says which mode
produced it.

## 6. Two sources, one question, end to end

**Goal.** The real machinery, with two DuckDB files as its first instance: split by source, render each
leg, combine above.

**Touches.** `crates/sutura-semantic` for the split, `crates/sutura-sql` for nothing new (it already
renders a mono-source plan), `crates/sutura-exec-datafusion` for the combine, and the plan type for a
set of legs.

**Adds.** A split that groups models by `SourceName` - so same-source fusion is grouping, not analysis -
one mono-source `QueryPlan` per source, and a combine that joins and finishes the aggregation that
could not descend. The bounds from step 3 are enforced here. Whether a leg was pushed or pulled is
observable.

**Tests.**
- `two_sources_answer_the_same_rows_as_one_source_over_the_same_data` - the differential property,
  extending `crates/sutura-app/tests/differential.rs`.
- `a_distinct_count_across_two_sources_is_correct` - the case that cannot be re-aggregated, computed
  above, with the numbers compared against an independently computed truth.
- `each_leg_renders_with_bind_parameters_and_the_statement_is_snapshotted`.
- `two_models_on_one_source_become_one_leg`.
- `a_leg_that_was_pulled_rather_than_pushed_says_so`.

**Done when** rows equal the single-source corpus, each leg's statement is pinned, and the row cap and
memory bound are shown refusing on an oversized intermediate.

## 7. Conformance packs

**Goal.** One set of test bodies, bound to each adapter by a macro, so a new connector proves itself by
registering and declaring.

**Touches.** A new dev-only workspace crate for the packs and the macro;
`crates/sutura-cli/tests/` for the harness extraction that `federation.rs` deferred at two corpora and
is now the third.

**Adds.** Compile packs (a catalog and the question corpus: plan, rendered statement per dialect, bind
parameters, refusal variant - **no data system needed**) and execute packs (rows, and that they are
identical across adapters). A macro generating one named test per behaviour per adapter, so a filter can
select a tier and a failure names the behaviour. Declared capabilities select the packs, and a
capability declared unsupported that turns out to work FAILS.

**Tests.** The packs are the tests. Plus:
- `a_declared_unsupported_capability_that_works_is_a_failure`.
- `adding_an_adapter_touches_a_registration_and_no_pack_body` - asserted by the macro's expansion.

**Done when** the semantic compiler is conformance-tested across catalogs with no container anywhere,
and the compile half runs on every push.

## 8. The credential port

**Goal.** No signature exists that can run as the process.

**Touches.** `crates/sutura-domain/src/warehouse.rs` (the port), every adapter, the composition roots.

**Adds.** A credential broker port, `execute` and `dry_run` taking a credential, and the subject and
deadline **hoisted out of the legs into the set** - one subject for N legs, so disagreement is
unrepresentable rather than checked. Refusals for a subject with no credential and for a source that
cannot impersonate when the deployment requires it.

**Tests.**
- `an_execution_without_a_credential_does_not_compile` - a compile-fail doctest with a compiling twin.
- `two_legs_cannot_carry_two_subjects` - the type makes it unrepresentable; the test pins that.
- `a_subject_with_no_credential_is_refused_rather_than_downgraded`.

**Done when** the fallback is removed rather than forbidden, and the single-user path still works with
static credentials.

## 9. Postgres over OAuth

**Goal.** The first network source, and the first real impersonation.

**Blocked on one verification, to do FIRST:** whether the Rust client in use speaks SASL OAUTHBEARER.
A pure-Rust protocol implementation may not, which would make the driver choice part of this step
rather than downstream of it. Also confirm the `libpq` in the toolchain is built with the curl
dependency the method needs, and that a validator module exists for the deployment's provider.

**Touches.** A new adapter crate; `crates/sutura-config` for its source declaration.

**Adds.** A Postgres warehouse declaring `ImpersonationAtSource`, authenticating the connection as the
subject so the source maps it to a role. **Not** `SET ROLE` on a pooled connection: `RESET ALL` does
not clear the role, `SET LOCAL ROLE` outside a transaction fails open, and four advisories name
shared-pool-plus-role-switching.

**Tests.** The two-subject test that cannot exist today: two identities, a row-level policy at the
source, different rows, asserted. Compose tier by nature.

**Done when** two subjects get different rows through the same question, and the artifact question from
the ADRs is answered rather than deferred - which of the shipped binaries links a native driver.

## 10. The compose tier

**Goal.** Oracle, Postgres, Datahub and OpenMetadata brought up on demand, provisioned through the
sutura CLI, worktree-aware.

**Adds.** Deterministic ports derived from the worktree identity, a compose project name per worktree so
volumes cannot collide, discovery so no test hardcodes a port, readiness as a health gate rather than a
sleep, teardown scoped so it cannot kill a neighbour, and SKIPPED-and-exit-0 when docker is absent.

**Note it cannot be a nix check** - the sandbox has no network and no docker socket - so it is a CI job
and a `just` task that consume nix-built artifacts. The strongest version runs the OCI image that ships.

**Done when** two worktrees provision simultaneously without collision, and a missing service cannot
produce a silent pass.

## 11. Mutual TLS per source

**Goal.** Optional client certificates to a metadata source and to a data source.

**Adds.** Per-source client certificate, key and trust anchors; a partial declaration refused at load;
the trust store stated rather than inherited; and rotation reusing the polling-and-swap already built
for serving rather than a second path.

**Done when** three states per source are each tested - none, TLS with verification, mutual TLS - and
the middle one, the one that gets forgotten, has a test of its own.

---

## What is not in this plan

- **Arrow transport per source.** Owed as its own record first: `check-boundaries` forbids Arrow in the
  domain, `sutura-arrow` is listed as planned, and the pinned `duckdb` and `datafusion` disagree on the
  Arrow major.
- **A custom DataFusion planner or extension.** The most promising shape for keeping the pushdown unit
  as sutura's own plan, and unprototyped.
- **BigQuery and Oracle adapters.** Both wait on step 9 proving the shape, and Oracle additionally on a
  generator question: its dialect exists upstream as an empty feature, and the rendering it needs lives
  behind the transpile feature this workspace does not compile.
- **Untrusted-content marking in the result envelope.** Cheap before the first Arrow envelope, expensive
  after, so it lands with the envelope rather than after it.
- **The remaining metadata connectors.** OKF, Datahub, OpenMetadata, the RDBMS catalog, BPMN and RDF are
  all step-7 shaped once the packs exist: a registration, a declaration, and fixtures.
