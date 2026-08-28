# The implementation plan

Operational, and expected to churn. The decisions it executes live in
[the ADRs](adr/0009-the-plan-from-one-source-to-many.md) and do not change because a step turned out
harder than it looked. If a step cannot be done as written, the ADR is the thing to argue with.

**Fifteen steps, seven of which can start at once.** Every step is one branch, one pull request, and
green before the next depends on it. `stax` manages the stack; the `git-ops/stacked-branches` skill has the
mechanics. Nothing in this plan is started.

## The stack

**Steps are named, not numbered, because the order moves and headings should not.** This table is the
order. The agent-facing surface is first because it is the API we expose: everything else is an
internal that a stable surface can grow behind.

| Order | Branch | Depends on | Can start now |
| --- | --- | --- | --- |
| 1 | `feat/agent-surface` | nothing unshipped | **yes** |
| 2 | `feat/federation-decomposability` | nothing | **yes** |
| 3 | `feat/principal-chain` | nothing | **yes** |
| 4 | `feat/query-bounds` | nothing | **yes** |
| 5 | `test/startup-source-refusals` | nothing | **yes** |
| 6 | `feat/source-registry` | 4 | after 4 |
| 7 | `feat/two-source-execution` | 2, 6 | after 6 |
| 8 | `feat/conformance-packs` | 7 for the execute half, nothing for the compile half | partly |
| 9 | `feat/credential-port` | 3, 6 | after 6 |
| 10 | `feat/postgres-oauth` | 9, and the driver question | after 9 |
| 11 | `feat/compose-tier` | 10 | after 10 |
| 12 | `feat/source-mtls` | 6, 10 | after 10 |
| 13 | `feat/demo-tasks` | 11, and one example to demo | after 11 |
| 14 | `build/supply-chain` | nothing - orthogonal | **yes** |
| 15 | `ci/prose-change-cost` | nothing - measure first | **yes** |

## The original thesis, and where each piece stands

Audited against the tree rather than remembered, because a thesis quietly losing a leg is how a
project becomes something else.

| Piece | State | Gap |
| --- | --- | --- |
| Wren's semantic model and compiler | **present** - markdown and YAML catalog, `Query` to `QueryPlan`, and the wren element set adopted | fan-out arithmetic declined on purpose; calculated fields only through an unwired hatch |
| Inspiration from Spice | **compared and declined as a dependency**, correctly - and nothing taken as SHAPE | the connector API, connection pooling and Arrow execution patterns are exactly what steps 5 to 9 need |
| Execution from DataFusion | **present**, and it stays the combiner under federation | none |
| Polyglot for rendering, transpilation if needed | **present for rendering**, three dialects compiled | the per-dialect rewrite layer Oracle needs sits behind a feature deliberately not compiled |
| Flexible sources and metadata systems | **decided, not built** | the eleven connectors and the declaration that carries them |
| Security | **the strongest part of the record** | the credential port is designed and unbuilt |
| The agent-facing surface | **MISSING FROM THIS PLAN** | there is no `sutura-mcp`, and until now no step for it |

### What wren gives, and what was declined

The model, the element set and the compile are here. What is NOT here is wren's fan-out arithmetic: a
dimension reached through a relationship whose declared cardinality may duplicate rows is **refused**
rather than computed safely. That is deliberate - a refusal beats a wrong number, and the cardinality
declaration is a trusted precondition nothing checks against the data - but the consequence is worth
stating plainly: **a question wren would answer, sutura declines.** Nobody should later file that as a
bug. The closed measure vocabulary is narrower than wren's calculated fields for the same reason, and
the authored-SQL hatch is the escape valve that exists and is not wired.

### Spice is compared, never mined

`docs/architecture.md` and ADR 0003 weigh Spice and decline adopting it as a dependency, which is the
right call and is recorded. What never happened is the other half - taking the SHAPES. Three of them
map directly onto steps in this plan, and reading them before designing costs less than after:

- **The connector API and the connection pool**, for steps 5 and 8. This matters more than it sounds: a
  DuckDB connection is `Send` and not `Sync` while the service requires `Sync`, and per-subject
  credentials make pooling a correctness question rather than a throughput one. Somebody has solved
  this shape already.
- **Arrow execution patterns**, for the owed Arrow record.
- **Its optimizer rules**, for step 6's combine and the pushdown decision.

What to keep declining: the acceleration and cache layer, the inference surface, and the runtime as a
runtime. Code and shapes, never a second control loop.

### Transpilation, if needed

**The position is not "never compile the feature". It is "never translate a third party's SQL."** One
feature flag conflates those two, and today's Oracle finding is the case that exposes it: the
per-dialect REWRITE layer - the thing that turns our own generated `DATE_TRUNC` into Oracle's `TRUNC` -
lives behind `transpile`, alongside the parser we decline. So Oracle and BigQuery correctness needs
either that layer or the same rewrites written here.

If the feature is compiled to reach it, three things must hold and one must change:

- The parser stays uncalled. Our generator's output is what is rewritten, not somebody's SQL.
- The goldens and the per-dialect parse-check still guard every rendered statement, which is what makes
  the rewrite auditable rather than trusted.
- The two documented hazards get handled explicitly rather than inherited: the default level returns
  `Ok` and **discards the diagnostic**, and the raising level errors on every non-count aggregate
  targeting ClickHouse while staying silent on the four real breakages. Neither default is acceptable
  as-is.
- And the invariant row changes, because its stated mechanism is "the feature is not compiled". A row
  whose mechanism moves gets rewritten, not reinterpreted.

## Operating inside a platform we do not own

This section is written **generically on purpose.** This repository is public, and the systems these
constraints come from are not ours to name. Each is written as the capability and its constraint, which
is the rule the root-of-trust file states and which usually improves the point anyway: a requirement
that survives being stripped of the product name is a requirement about architecture rather than about
a vendor.

Sutura will eventually run beside three shapes of thing: **a central API gateway fronted by an
enterprise identity front door**, **an agent platform that supplies a client and a review-and-deploy
plane**, and **third-party chat clients acting as callers**. None of them is in the data path. All of
them constrain the surface, and most of the constraints land on the step that is now first.

### What the agent surface must survive

- **Only TOOLS may be load-bearing.** A gateway of this kind surfaces tools and **ignores resources and
  prompts**. So a glossary, a catalog description or a "how to ask" hint may improve a cooperative
  client and must never be the thing that makes an answer correct. That matches what the prompt already
  is - advisory - but it becomes enforced by the transport rather than by our discipline, which is a
  stronger position and worth designing for deliberately.
- **Two authentication adapters behind one port.** Direct callers need the full resource-server dance:
  metadata discovery, a challenge, an audience-bound token we validate ourselves. Behind a gateway, all
  of that is dead code on that route while remaining mandatory on the other. Implement both behind one
  subject port **so the tool surface cannot tell the difference** - that is the most concrete
  justification the hexagon has been given, and it is a real cost rather than a free abstraction.
- **A forwarded user token is credential forwarding, not identity proof.** It is acceptable only if it
  is a signed token from a discoverable issuer that sutura verifies itself: issuer, audience,
  signature, expiry. It is unacceptable if the front door re-mints an opaque string only it can
  interpret. **This single question decides whether such a route can ever serve per-subject data**, and
  it is a question to ask rather than to assume.
- **Never accept a plain user-id header as identity.** Behind a service token, anything holding that
  token could obtain a credential for ANY user - a confused deputy, and with a per-user credential
  store it turns our own API into a mass-exfiltration surface. Audit entries naming individual people
  would then be fiction, which is worse than an honest shared-identity record because it will be
  trusted.
- **No auth-bearing tool parameter.** A credential in a tool argument is a credential in the model's
  context, and it makes the model a credential carrier. Authentication is transport-level, and the tool
  surface exposes no field for it - which the typed `Query` already guarantees by having no such field.
- **A timeout may change the surface's SHAPE, not just a setting.** A gateway of this kind enforces a
  request timeout in the tens of seconds, and streaming does not exempt a connection from it. If a
  governed turn can exceed it, the surface has to become submit-and-poll - a design change, decided
  before the tools are written rather than after. Measure a real turn first; and note that without push
  notifications the obvious asynchronous pattern is foreclosed.

### Two rules that outrank any of the above

- **Do not weaken an invariant to fit a platform.** If the only identity a route can offer is one
  sutura cannot verify, the honest outcome is that the route serves non-per-subject data only. That is a
  smaller product on that path, not a softer invariant.
- **Defence in depth is never a substitute.** A gateway token proving a request transited the gateway
  makes "assume nothing about the client" cheaper on that route, and gateway-level quota and audit
  complement ours. Neither replaces the check we make ourselves, because the platform is not ours to
  rely on.

## The agent-facing surface

**Goal.** The governed surface agents actually speak. `AGENTS.md` lists `sutura-mcp` among the planned
crates and nothing else in this plan mentioned it, which for a runtime whose thesis is *for AI agents*
was the largest omission in the record.

**Depends on nothing that is not already shipped.** The tool surface exists: a typed `Query` with no
field for SQL, a table, a predicate or row ids; refusals as results rather than errors; provenance in
the payload. MCP is a transport over that, not a redesign of it.

**Touches.** A new crate; `crates/sutura-http` for nothing, because the point is that both transports
serve the same tools.

**Adds.** A server exposing the tools the HTTP surface exposes, from **one schema source**, so the two
cannot disagree. The invariants for it are already written and already enforced by the domain types -
which is the reason this step is smaller than it looks.

**Tests.**
- `a_certified_question_is_answered_over_the_agent_surface`.
- `an_uncertified_question_is_refused_as_a_RESULT_rather_than_an_error`.
- `the_advertised_tools_differ_by_scope` - a tool a caller may not invoke is not advertised to it.
- `both_transports_describe_the_same_tools` - one source, or a test asserting the two descriptions are
  equal. This is the drift this step exists to prevent.

**Done when** an agent client can ask a certified question and be REFUSED an uncertified one over the
agent surface, with no client of ours in the loop. Note the demo step depends on this **or** on the
OpenAPI route; this is the better one, and the demo must not wait for it.

## Decomposability in the domain

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

## The principal chain

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

## The three bounds

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

## The startup refusals that already hold

**Goal.** Test what is already true. The more-than-one-source arm of `open_engine` has no test in
either binary, and it is the refusal that actually protects a deployment today.

**Touches.** `crates/sutura-serve/src/main.rs` and `crates/sutura-cli/src/commands.rs` test modules
only. No production code.

**Tests.** One per binary, each asserting the multi-source arm and asserting it is NOT the neighbouring
wrong-name arm, so it cannot pass on the wrong branch.

**Done when** both are red against a build with the branch removed. This is the cheapest step in the
plan and it closes a real gap.

## The source registry, the mode, and the boot check

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

## Two sources, one question, end to end

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

**Done when** rows equal the single-source corpus, each leg's statement is pinned, the row cap and
memory bound are shown refusing on an oversized intermediate, and **the two-source example flips from a
refusal to an answer** - the same corpus, so the diff is the behaviour change.

## Conformance packs

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

## The credential port

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

## Postgres over OAuth

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

**Done when** two subjects get different rows through the same question, **the multi-user example
demonstrates exactly that end to end**, and the artifact question from the ADRs is answered rather than
deferred - which of the shipped binaries links a native driver.

## The compose tier

**Goal.** Oracle, Postgres, Datahub and OpenMetadata brought up on demand, provisioned through the
sutura CLI, worktree-aware.

**Adds.** Deterministic ports derived from the worktree identity, a compose project name per worktree so
volumes cannot collide, discovery so no test hardcodes a port, readiness as a health gate rather than a
sleep, teardown scoped so it cannot kill a neighbour, and SKIPPED-and-exit-0 when docker is absent.

**Note it cannot be a nix check** - the sandbox has no network and no docker socket - so it is a CI job
and a `just` task that consume nix-built artifacts. The strongest version runs the OCI image that ships.

**Done when** two worktrees provision simultaneously without collision, and a missing service cannot
produce a silent pass.

## Mutual TLS per source

**Goal.** Optional client certificates to a metadata source and to a data source.

**Adds.** Per-source client certificate, key and trust anchors; a partial declaration refused at load;
the trust store stated rather than inherited; and rotation reusing the polling-and-swap already built
for serving rather than a second path.

**Done when** three states per source are each tested - none, TLS with verification, mutual TLS - and
the middle one, the one that gets forgotten, has a test of its own.

---

## `just demo`, and a chat interface for development

**Goal.** One task per deployment variant that brings up a fully working demo: sutura serving that
variant's example, and a chat interface pointed at it. `just demo single-user`,
`just demo federation`, `just demo multi-user`.

**This is a development and demonstration surface, and it must never become load-bearing.** A chat
client has no governance role: removing it removes no guarantee, and anything it appears to enforce
sutura enforces again. It is not in the release image, not in the default compose, and it is labelled
ungoverned where a reader could mistake it.

**What a chat interface needs from sutura, and this is the real decision.** Two routes exist and only
one is built:

- **The OpenAPI document, which ships today.** `crates/sutura-http/src/openapi.rs` generates it from
  the handlers, so it is always current and needs nothing committed. A chat UI that can call an
  OpenAPI tool server talks to sutura over it with no new code on our side. This is the cheap route.
- **An MCP server, which is planned and absent.** There is no `sutura-mcp` crate. When it exists it
  becomes the better route, because MCP is the surface agents actually speak - but the demo must not
  wait for it.

**And the part that cannot be waved away: a chat interface needs a MODEL.** Either a hosted provider,
which means a key and egress from the demo environment, or a local one, which is heavy. Say which the
demo assumes and make it configurable, because "bring up a demo" silently requiring an API key is the
kind of surprise that wastes an afternoon. Neither belongs in a default that runs in CI.

**Touches.** The justfile, a demo compose file separate from `compose.dev.yaml`, and the provisioning
from step 10 - the same worktree-aware ports and project names, since two people demoing at once is
the normal case.

**Which UI is deliberately not decided here.** A ready-made chat container is the fast path; something
lighter that speaks the OpenAPI surface directly is less to run. Pick it when the task is written,
against one criterion: **can it call an OpenAPI tool server without us maintaining a plugin?** If it
cannot, it is the wrong choice however good it looks, because a plugin we maintain is governance code
in a client we said has no governance role.

**Done when** `just demo federation` brings up a working scenario from a clean checkout, on host
docker, with the ports derived from the worktree; when a missing docker prints SKIPPED and exits 0
rather than failing; and when the README for each example says which `just demo` runs it.

## Supply chain: SBOM, provenance, signatures, licence report

**Orthogonal to everything above.** It touches the release path and no crate, so it can start at any
time and blocks nothing.

**What exists.** The licence half is largely done: `cargo-deny` gates licences against an exact
allowlist with `unused-allowed-license = "deny"`, so an allowed licence nothing uses is itself a
failure. `VENDOR.md` records what is vendored, from where, with what changes. A tag builds four
cross-compiled binaries and four images, all by nix.

**What does not exist**, confirmed by looking rather than assumed: no SBOM, no CycloneDX, no SLSA
provenance, no attestation, no signature. Nothing in `.github/`, `flake.nix`, `deny.toml` or the
justfile mentions any of them.

**And one claim to correct while here.** AGENTS.md says the licence obligation is kept from rotting by
"the `cargo-deny` licence gate plus a `NOTICE` check". There is no NOTICE check. `NOTICE` appears once
in `xtask`, in the docs-only path list that `classify` reads, which is not a check of anything. The
licence gate is real; the second half of that sentence is not.

**The thing to get right, because it decides whether an SBOM is worth having.** There are TWO sources
of truth and only one of them is `Cargo.lock`:

- `Cargo.lock` describes the crates. Complete for a Rust library, and **incomplete for a shipped
  artifact.**
- The nix closure describes what the binary and the image actually contain, including `libduckdb`, the
  vendored allocator and libc.

An SBOM generated only from `Cargo.lock` would omit the C libraries and **look complete while being
wrong**, which is worse than not having one: a consumer scans it, finds nothing, and concludes there is
nothing. So: a crate-level SBOM from the lockfile, an image-level SBOM from the closure, and each
saying which artifact it describes.

**The pieces, cheapest first:**

| Piece | Shape |
| --- | --- |
| Licence report | Generated from the dependency graph, attached to the release. The gate that decides its accuracy already exists |
| CycloneDX SBOM | One per artifact kind, from the two sources above |
| SLSA provenance | Attested for artifacts built in the workflow, keyless through the workflow identity, which is what makes the provenance mean anything |
| Signatures | The images signed, keyless, verifiable without a key we hold |

**Done when** each release artifact has an SBOM naming the right source of truth, a provenance
attestation, and a signature - **and when verification is exercised in CI rather than assumed.** An
attestation nobody verifies is a file. The smoke test is the deliverable, not the generation.

## The CI cost of a prose change

**Measured before touching anything**, on a real run, because the intuitive answer was wrong:

| Step in the docs workflow's verify job | Time |
| --- | --- |
| Structural gates, `nix build .#checks.x86_64-linux.hygiene` | **15m 45s** |
| Install the docs environment | 8s |
| Build the site, mkdocs `--strict` | **3s** |

**The site build is three seconds. It is not the cost.** The cost is a Rust dependency closure being
built so that `xtask` can read files, and the same check in `ci.yml` on the same commit took 11m 04s
WITH a cache. So this is not a docs problem and excluding pages from the docs workflow would not fix
it.

**What not to do, stated because it is the obvious move:** do not exclude `docs/adr/**` from the docs
workflow's paths. That workflow is *the only one a prose-only change starts* - `ci.yml` deliberately
skips prose - so excluding ADRs would run **no gates at all** on an ADR change. And mkdocs `--strict`
is what catches a dead cross-record link, which the 0006 and 0007 pair nearly shipped: one links the
other by filename, so landing them apart would fail the build. Three seconds is the wrong thing to
economise.

**In order, and stop as soon as it is fast enough:**

1. **Find out whether these numbers are cache misses.** Both runs measured were the first after a
   force-push. The same gates locally are seconds. Restructuring a workflow on a cold-cache
   measurement is how a fix gets built for a problem nobody had.
2. **Give the verify job the cache `ci.yml` already uses.** The docs workflow omits it deliberately and
   the reasoning is sound - the Actions cache is writable from a pull request and a *published* page
   must not be built from a store path a pull request could have placed there. But that argument is
   about publishing. The verify job runs on a pull request with a read-only token and publishes
   nothing, and `ci.yml` already accepts exactly this risk to gate merges. Cache verify, leave publish
   uncached, and say so where the current comment says the opposite.
3. **Only if it is still slow warm:** separate the gates that read prose from the ones that need a
   compiler, so a prose change runs `check-docs`, `check-guidance`, `text-hygiene`, `line-endings` and
   `max-lines` without a dependency closure. That needs a prebuilt `xtask`, which is its own piece of
   work and should not be started before step 1 says it is necessary.

**Done when** an ADR-only change is gated in about a minute rather than seventeen, with `check-docs`
and mkdocs `--strict` still running, and with the publish job's cache posture unchanged.

## The examples are the demo, one per deployment variant

**Every deployment variant gets a WORKING end-to-end example, and "working" means a test runs it.** Not
a README describing what would happen. The existing example is already exercised by
`crates/sutura-cli/tests/example.rs`, which asserts five exact measure sets, so the bar is set: an
example that drifts fails a test rather than misleading a reader.

| Variant | Example | What it demonstrates | Lands with |
| --- | --- | --- | --- |
| **Single user** | `examples/single-player` (exists) | One source, static credentials, the whole measure vocabulary | shipped |
| **Two data systems, refused** | the two-source corpus (written, unmerged) | That crossing two `SourceName`s is refused today, from a real on-disk catalog. A permanent governance boundary rather than a placeholder | before step 6 |
| **Federation** | the same corpus, answering | The same question that was refused now answers across two sources, so the diff shows exactly what changed in behaviour | step 6 |
| **Multi user** | a new corpus, compose-backed | Two subjects, the same question, DIFFERENT rows, enforced by the source | step 9 |

Three things this ordering buys, and the middle one is the reason to do it this way:

1. **The refusal corpus is the red half.** It exists before federation, asserts today's behaviour, and
   the day federation lands the same example flips from a refusal to an answer. That is red-before-green
   at the level of a demo rather than a unit test.
2. **The multi-user example cannot be faked.** No local file enforces a row-level policy, so this one is
   compose-backed by nature and lands with step 9, not before. A fixture that answered the same rows for
   both subjects and passed would be worse than no example at all - it would look like proof.
3. **The federation example is the same corpus, not a new one.** Reusing it is what makes the behaviour
   change legible; a fresh corpus would hide the change in unrelated diff.

**A note on naming.** The refusal corpus is currently `examples/federation`, whose README has to open by
saying it is not the federation example. Rename it to what it demonstrates - two data systems - and let
`federation` name the one that federates. Cheap now, one test and two READMEs.

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
