# Identity, services and the operational work

The second half of [the implementation plan](implementation-plan.md), and one document with it: the
stack table lives there and stays the only owner of a step number, the ordering decisions are stated
there, and this page carries the branch sections for rows 12 to 21 - plus the work that is deferred
rather than scheduled, and what is out of scope entirely.

Split from that page because the two together crossed the 1000-line limit `cargo xtask max-lines`
enforces. The seam is the stack's own phase boundary: everything on the other page needs no live
service and no identity decision, and everything here needs one or both.

## The credential port

**Goal.** No signature exists that can run as the process.

**WAS blocked on `docs/inbound-identity`, and that block is now lifted** - the record landed as
[how a caller proves who it is](adr/0014-how-a-caller-proves-who-it-is.md), so this step can be written
without guessing. The reason it was a hard block rather than a preference is worth keeping, because it is
why the record came first:
[The plan](adr/0009-the-plan-from-one-source-to-many.md)'s Decision 1 says in bold that who performs
the RFC 8693 exchange and what audience the inbound token carries is the question to answer before this
port is built, because guessing it produces a port with the wrong signature - and a port signature is
the most expensive thing in this stack to change afterwards, since every adapter and both composition
roots implement it.

**Touches.** `crates/sutura-domain/src/warehouse.rs` (the port), every adapter, the composition roots.

**Adds.** A credential broker port minting in ONE call for every source the plan reads, `execute` and
`dry_run` taking a credential, and **the asker and the deadline hoisted out of the legs into the set** -
one field each for N legs, so disagreement is unrepresentable rather than checked.
[A credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) names the field
`asked_by` rather than `subject` on purpose: it is the identity the question arrived under, and it is
NOT a claim that every leg executed as it. What each leg executed as is a **variant**, and there are
three of them, because the third posture is not the absence of the other two - one carries the asker's
own bearer material, one carries a principal the data system switches to on a connection the deployment
authenticated, and the third **carries no credential material at all** and says out loud that the leg
ran under the deployment's own identity for that source. That third variant is what makes "no signature
can run as the process" true rather than hollow: the engine that ships cannot impersonate anybody, so
under a two-variant shape it would receive a value it ignores and the fallback would be back as a
variant nobody looked at.

**Also on the port, and it is what closes the boot-path hole**: `dry_run` returns a pre-flight outcome
with a default that says *not asked*, rather than a claim an adapter did not make; and a **second
method runs an anchor under a verification identity** - a distinct type `answer` holds no value of and
cannot construct, accepted by nothing else, with the settings layer as its only constructor. The boot
credential is therefore unreachable from the request path and the request credential unreachable from
the boot path, and the anchor result comes back in its own wrapper so a boot result cannot be handed
back as a result set without a named conversion somebody wrote.

**And the deadline is checked before EACH leg**, because the legs run sequentially and a long first leg
can consume the credential's life before the last one starts. That case does not exist under parallel
execution and it is created deliberately here, so it is checked rather than noted.

**The broker port arrives with a real implementor, not a fake.** `AGENTS.md` says a port trait arrives
with its first implementor, and `examples/multi-player/README.md` says the same thing about
`CredentialBroker` specifically - "a trait with no implementor is a guess at a signature". The
implementor here is the **static-credential broker single-user mode already needs**: credentials as
configuration, one user, one host, which is a shipping deployment mode rather than test scaffolding.
So no rule bends and no fake stands in for one.

**ONE new refusal variant, not two, and the second is DELETED rather than descoped.** An earlier
version added a refusal "for a source that cannot impersonate when the deployment requires it".
[A credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) part 6 walks every
configuration that was supposed to reach it, and each one turns out to be a boot refusal, an `Err` for
a wiring defect between the broker and the configuration, the no-credential refusal, or the decided
permitted behaviour - a shared source in a multi-user deployment answers and records the posture it ran
under. So `SourceCannotImpersonate` is deleted, and this step adds exactly one variant: a subject with
no credential at a source. A variant no configuration can provoke is one the refusal enum refuses to
carry, and keeping it "for later" is the invented debt this round has already corrected twice.

**A wiring mismatch is an `Err`, not a refusal, and the direction matters.** An adapter for a source
configured to impersonate that receives the shared-identity variant returns an `Err`; so does an
adapter for a shared source that receives subject material. Nothing about the question was wrong, so it
surfaces as a service failure rather than as something a client library can retry into an answer - and
the second direction is the more interesting one, because an adapter that quietly ACCEPTED subject
material it cannot use would report a leg as impersonated that ran shared.

**Tests.**
- `an_execution_without_a_credential_does_not_compile` - a compile-fail doctest with a compiling twin.
- `a_second_leg_cannot_carry_a_second_asker` - one field, one constructor, no `insert`, so the test
  pins a property of the type rather than a check somebody could move.
- `answer_cannot_pass_a_verification_identity` - a compile-fail doctest with a compiling twin. This is
  the one place the boot-path separation is mechanised, and it is the whole reason the anchor path got
  its own method rather than a second argument.
- `a_shared_leg_carries_no_credential_material` - the third variant, asserted on the shape, because a
  placeholder secret is exactly what this variant exists to prevent.
- `an_adapter_that_receives_the_wrong_posture_returns_an_err_rather_than_a_refusal` - both directions,
  since the accepting-what-it-cannot-use direction is the silent one.
- `a_subject_with_no_credential_is_refused_rather_than_downgraded` - the one new refusal variant.
- `a_leg_that_would_start_after_the_deadline_is_not_attempted` - the sequential-execution case, checked
  before each leg against the one expiry the set carries.
- `a_query_over_its_deadline_is_cancelled_in_the_adapter` - the first place this can be asserted rather
  than named, because the deadline now reaches the adapter. `feat/query-bounds` deliberately does not
  claim it.
- `the_static_credential_broker_serves_the_single_user_path` - the implementor, exercised, so the port
  is not shaped by one caller.

**Done when** the fallback is removed rather than forbidden, the single-user path still works with
static credentials, a deadline that fires stops an execution rather than stopping a wait, and the boot
path cannot reach a request credential or the reverse.

## The plan-stage refusal, re-keyed to identity

**Goal.** `PlanSpansTwoSources` becomes *a plan spanning two identities*, which is the property the
check was always reaching for. Today the plan stage collects sources into a `BTreeSet` and refuses
unless exactly one is in it; once federation ships, two sources in one plan is the normal case and the
thing that must still be refused is a plan whose legs would not all evaluate as the same asker.

**It is the LAST assumption to move, and that ordering is the decision rather than the schedule.**
[0007](adr/0007-federating-across-different-data-systems.md)'s survey of the six single-source
assumptions puts this one last for a reason that is not politeness: there is nothing to key on until a
principal type exists, so re-keying it before `feat/principal-chain` and `feat/credential-port` means
re-keying it to a value that is not there. And the failure direction is the expensive one - a
re-key that lands early does not refuse where it should, which is a wrong answer rather than an outage.
`feat/two-source-execution` is also a prerequisite, because until two sources can be answered at all
the old refusal is still the correct behaviour and removing it would strand every federated question.

**Touches.** `crates/sutura-semantic` for the plan-stage check, `crates/sutura-domain` for the refusal
variant's fields, `crates/sutura-app/src/prompt.rs` for the guide, and the goldens and prompt snapshots
that carry the current refusal.

**Adds.** The re-keyed check, and three things that MOVE with it rather than after it:

- **The prompt guide for that refusal becomes wrong and is rewritten in the same diff.**
  `PLAN_SPANS_TWO_SOURCES` in `crates/sutura-app/src/prompt.rs` tells an agent that crossing two data
  systems is not answerable. Once it is, that guide is advice that sends an agent away from a question
  this deployment would answer - worse than no guide, because the agent has no way to tell.
- **One AGENTS.md invariant row is DELETED, not demoted.** *A plan cannot silently span two sources* is
  a row whose mechanism this branch replaces, and the rule at the head of that table is that a row
  losing its mechanism gets deleted. The replacement row is the identity-keyed one, with its own
  mechanism named, and it is a different claim rather than a reworded one.
- **Two prose statements elsewhere say the one-source rule is currently enforced**, in
  [DataFusion for local execution](adr/0003-datafusion-for-local-execution.md) and in `docs/concepts.md`,
  and they move in the same change. A record that describes a retired check as live is the defect this
  whole review round was about.

**Tests.**
- `a_plan_over_two_sources_under_one_asker_is_answered` - the behaviour that changes, red before.
- `a_plan_whose_legs_would_run_as_two_identities_is_refused` - the property that was always meant,
  which needs a fixture where two sources resolve to two different askers.
- `the_refusal_names_the_identities_rather_than_the_sources` - asserted on the typed fields, because
  the variant is the contract and a message that still says "sources" would be the old check wearing a
  new name.
- `the_prompt_guide_for_that_refusal_no_longer_says_two_systems_cannot_be_crossed` - the guide and the
  refusal move together or an agent is misled by one of them.

**Done when** two sources under one asker answer, two identities in one plan refuse, the refusal names
what it now keys on, and no record or guide left in the tree still describes the retired rule as live.

## The compose tier

**Goal.** Oracle, Postgres, Datahub and OpenMetadata brought up on demand, **one independent instance
per worktree**, provisioned through `xtask`.

**It comes BEFORE the first network adapter, and that is a change from an earlier version of this
plan** which had it depend on Postgres-over-OAuth. That was inverted: this tier exists to stand up the
source an adapter is tested against, so an adapter merged first has nothing real to run against and its
tests degrade into a fake asserting our own rendering back to us. It depends on nothing in this
repository and can start immediately.

**Provisioning lives in `xtask`, not in the shipped binary.** An earlier version put it in the sutura
CLI. Docker orchestration in a release artifact is test scaffolding shipped to users, and the repo
already has the place for it: `xtask` is the repo-inspection and gate tool, it is never packaged, and
`classify` / `check-changed` already own the "what does this change require" decision this tier keys
off. The CLI keeps no docker knowledge. Commands are cited as `just` tasks over `xtask` subcommands, the
way every other gate is, because `check-guidance` fails a citation of a task that does not exist.

### Per-worktree instances, and why a port cannot be a constant

**The requirement, stated as the thing that must never happen:** two worktrees of this repository -
an agent's and a human's, or two agents' - run the service tier at the same time and neither notices
the other. Not "usually works": a port collision here does not fail cleanly. Docker binds the first
claimant and the second gets a connection refused that looks like a broken adapter, or worse, connects
to the *neighbour's* container and passes against the wrong fixture. **A test that silently talked to
another worktree's database is the failure this section exists to make impossible.**

Four things collide, and every one of them needs a per-worktree value - which is why "just pick a
different port" is not the fix:

| What collides | Per-worktree value | What it prevents |
| --- | --- | --- |
| Published ports | **Whatever docker and the operating system allocate**, published ephemerally, read back after the container is up | A bind failure, or a connection to the neighbour's service |
| Compose project name | A name derived from the worktree path, so it is stable and readable | Shared networks and, critically, **shared named volumes** - a stale Postgres data directory from another branch is a fixture nobody can debug |
| Container and network names | Derived from the project name, never literal | `docker compose down` in one worktree stopping the other's containers |
| The provisioned state a test reads | A discovery file written **inside that worktree**, gitignored | A test hardcoding a port, which is the way this whole mechanism gets bypassed one PR at a time |

**Allocation and discovery, NOT derivation - and that reverses what an earlier version of this section
decided.** It said the base port was a pure function of the worktree path, that two worktrees therefore
got different blocks without a registry, and that `xtask` would refuse to provision if a port in the
block was already bound. Two defects, and the second is worse than the first:

- **A hash into a port range cannot guarantee disjoint blocks.** It is a total function from an
  unbounded set of paths into a finite set of blocks, so collisions exist by construction. The test that
  was going to prove the property - a corpus of sample paths - can only fail to find one, which is not
  the same claim. And one of the corpus cases asserted the opposite of what was wanted: on a
  case-insensitive filesystem, macOS by default and where this branch was validated, two paths
  differing only in case are the **same directory**, so demanding disjoint blocks for them would make
  the block depend on how somebody typed the path.
- **Check-then-bind is a race.** "Refuse if any port in the block is already bound" leaves the whole
  window between the check and docker's bind open to anything else on the host, including the
  neighbouring worktree doing the same check at the same time. It reads as a guarantee and delivers a
  probability, which is the failure mode this repository has a rule about.

So the ports are **allocated by the thing that owns them**: publish ephemerally, let docker and the
operating system pick, and read back what was actually bound. That has neither defect and no race,
because there is no window - the allocation and the bind are one operation. What it costs is exactly
one property: a fixed port a developer could memorise between runs. The discovery file replaces it with
a value that is correct rather than remembered, and a `just` task that prints the current endpoints
costs a line.

**Naming stays derived, because naming has no allocator.** The compose project name, and the container,
network and volume names under it, come from the worktree path - stable, readable, and collision-proof
in the only way that matters: two different worktrees are two different paths, and a name collision
fails loudly at `docker compose up` rather than silently connecting to a neighbour. A hash collision in
a NAME is a startup error; a hash collision in a PORT is a test that passes against the wrong fixture.
That asymmetry is the whole reason one of the two moves to allocation and the other does not.

**Discovery, so no test hardcodes anything.** Provisioning writes the endpoints it actually bound into
a file the test harness reads, in that worktree. A test that reads a constant port is a test that works
alone and fails in parallel, and it passes review easily, so the mechanism has to be the *only* way to
learn an endpoint - if the harness offers no constant, none gets written. With allocated ports this
stops being belt-and-braces and becomes load-bearing: there is no constant to read even if somebody
wanted one.

**CI is a worktree too, and allocation covers it more simply than derivation did.** A CI runner has one
checkout and no sibling worktrees, but several jobs per commit can land on the same runner class, and
an ephemeral port is correct there for the same reason it is correct locally - nothing has to reason
about what else is running.

**Adds.** Ephemeral publishing and read-back, the per-worktree compose project name, the discovery
file, readiness as a health gate rather than a sleep, teardown scoped so it cannot kill a neighbour,
and the docker-absent behaviour below.

**Absent docker SKIPS locally and FAILS in CI, and an earlier version of this section had those
confused.** It said the tier prints SKIPPED and exits 0, and said in the same breath that a missing
service cannot produce a silent pass. Both cannot hold on one machine class, so:

- **On a developer machine, a missing docker skips loudly** and names what did not run. Docker is a host
  dependency this repository deliberately does not pin with nix, and a contributor without it has to be
  able to work on everything else.
- **In CI, the same absence FAILS.** There the tier is the only thing standing behind a network adapter,
  and a green run that quietly tested nothing is the exact failure this whole tier exists to prevent.

The mechanism is one flag read from the environment, and the direction it points each way is documented
where the flag is read - which is what `AGENTS.md` asks for: neither direction is the default, and what
a wrong answer costs decides it per mechanism.

**Note it cannot be a nix check** - the sandbox has no network and no docker socket - so it is a CI job
and a `just` task that consume nix-built artifacts. The strongest version runs the OCI image that ships.

**Tests.** These are testable without docker, and the ones that matter are:

- `the_project_name_is_derived_from_the_worktree_path_and_is_stable` - the same worktree gets the same
  compose project on every run, which is what makes teardown scoped.
- `two_worktree_paths_produce_different_project_names`, and `a_case_only_path_difference_is_one_worktree`
  where the filesystem folds case - the path is canonicalised before it is used, which also resolves
  symlinks, so the same directory reached two ways is one worktree rather than two.
- `an_endpoint_is_read_from_the_discovery_file_and_never_from_a_constant`, and
  `the_harness_exposes_no_way_to_read_a_constant_endpoint` - the discovery file is the only path, which
  is what stops the mechanism eroding one pull request at a time.
- `teardown_names_only_this_worktrees_project` - the neighbour-killing case, asserted on the arguments
  rather than by running docker.
- `absent_docker_skips_locally_and_fails_in_ci` - both directions of the one flag, because a
  fail-open/fail-closed decision with only one side tested is half a decision.

**Deleted rather than kept beside the new mechanism:** the derivation tests -
`two_worktree_paths_derive_disjoint_port_blocks`, `the_same_path_derives_the_same_block_every_time` and
`a_bound_port_in_the_block_refuses_to_provision`. They test a mechanism that no longer exists, and a
test kept for a withdrawn design is how the design comes back.

**Done when** two worktrees provision simultaneously without collision - demonstrated by running them,
not asserted - no test can name a port, and a missing service cannot produce a silent pass in CI.

## Selective service CI, and why it is DEFERRED rather than scheduled

**This was a branch in the stack table and is no longer one.** The idea: a pull request that touches the
Datahub adapter stands up Datahub and nothing else, a push to `main` stands up everything. It is a
reasonable idea and it is not ready to be work, for two reasons that are worth writing down rather than
quietly dropping.

**The claim that justified it does not hold, and this is the important half.** The design was sold
partly on inheriting `xtask`'s fail-open property: *a path matching no area sets `run_all`, so a new
adapter nobody added to the table runs everything rather than silently running nothing.* Checked against
`xtask/src/changes.rs` rather than assumed: there is one `Area` named `rust`, and its patterns are
`crates/**`, `examples/**`, `xtask/**`, `Cargo.toml`, `Cargo.lock`, `clippy.toml`,
`rust-toolchain.toml`, `.cargo/**`. **A new adapter crate lands under `crates/`, so it MATCHES an area
and the unmapped-path branch is never reached.** The property that made the design safe cannot be
exercised by the case it was described as protecting, and no test could demonstrate it, because there
is no diff that reaches it. That is the same defect shape as a `disallowed-methods` entry whose path
does not resolve: it reads as enforcement and does nothing.

**And the cost it saves has never been measured, because the thing it selects over does not exist.**
There is no service tier today, so nobody knows whether the whole tier is two minutes or sixteen, and
whether one category is a tenth of that or a third. A selector is only worth its own failure modes if
the run it shortens is long; deciding that in advance is the same mistake the section below makes about
a workflow restructured on a cold-cache measurement.

**What brings it back, precisely:** at least two real service categories exist, the whole tier's runtime
has been measured on CI hardware, and that measurement says a selector would actually reduce it. Not
before.

**And the repair the fail-open claim needs, written down so nobody re-derives the broken argument.**
The claim was that an adapter nobody added to the table runs everything, because an unmapped path sets
`run_all`. It cannot be reached: the `rust` area's patterns include `crates/**`, so a new adapter crate
matches and the unmapped-path branch never fires. Making that claim true needs one of two changes to
`xtask/src/changes.rs`, and both are real work rather than a rename: **an `Area` per adapter**, so an
unregistered adapter genuinely matches nothing; or **a `rust` area whose patterns do not swallow a new
adapter crate**, which means enumerating the crates that exist and accepting that adding any crate now
edits the table. Until one of those lands, the fail-open property is a property of paths outside
`crates/`, `examples/` and `xtask/` only - and saying so is what stops the next reader recovering the
same wrong argument from the same true-sounding sentence about `run_all`.

**What survives for whoever picks it up**, because the analysis was not wasted:

- **A service category is an `Area`, not a second selector.** `xtask/src/changes.rs` already holds one
  `Area { name, patterns, consumers }` per area, maps a diff onto them, and writes `GITHUB_OUTPUT` so a
  workflow can gate a step. "Core changed, so run everything" is already expressible in the right
  shape - `consumers` is documented as *a dependency edge, not a category*, which is exactly what that
  rule is - so the semantic core's areas list every service category as a consumer and the edge does
  the fan-out. No negation pattern, no second filter step with inverted quantifier semantics.
- **`crates/**` has to be split before a per-adapter rule can exist at all.** That is the concrete
  first task, and it is what makes the fail-open story true rather than decorative: with one area per
  adapter crate and no catch-all pattern over `crates/`, a new adapter genuinely is an unmapped path.
  Doing that split is a change to a gate every hook and every CI job reads, which is why it is not a
  side effect of a docker task.
- **The matrix comes from the registry, not from the workflow.** The list of categories a workflow fans
  out over is emitted by `xtask` and read from the same `tests/adapters` registry the conformance packs
  register into, consumed as JSON. Never typed into a workflow file. The reason is a failure this repo
  guards elsewhere: two owners for one artefact. A category list in YAML plus a registry in Rust means
  a new adapter can be registered, conformance-tested locally, and absent from CI's matrix - and
  nothing fails, because a matrix that omits an entry is not an error.
- **The selection has to be visible in the run.** Which categories were selected and which rule
  selected them - the area that matched, the consumer edge that pulled it in, or `run_all` and the path
  that triggered it. A selective CI that does not say what it skipped is a CI whose green is
  uninterpretable, which is the same reasoning as *whether a leg was pushed or pulled must be
  observable*: silence is the failure mode.
- **It is unit-tested Rust rather than YAML**, because a selection rule fails by *not* running
  something, which is the class of bug nothing else catches.

## A Postgres adapter, on a static credential

**Goal.** [Track 1](adr/0007-federating-across-different-data-systems.md): one source, queried
directly, over a real network protocol. No federation, no OAuth, no impersonation - a warehouse
declaring `SharedServiceUser`, single-user mode, exactly the posture `examples/single-player` already
ships. **`SharedServiceUser` and `ImpersonationAtSource` are the only two spellings**, per
[pluggable by declaration](adr/0011-pluggable-by-declaration.md); an earlier version of this section
wrote `Shared`, and a mode with two names in one document is how it acquires two meanings.

**Why it is its own step, ahead of the OAuth one.** Three things it settles that nothing else can:

- **Which shipped artifact links a native driver.** Open in two ADRs. `sutura-exec-duckdb` is a
  dev-dependency because nixpkgs has no musl `libduckdb`, and a Postgres driver has the same question
  with a different answer available: a pure-Rust client links nothing, which may make the cross-build
  matrix a non-issue for this source and *not* for the next one. Answering it against real code beats
  answering it in prose.
- **The rendered SQL meets a real Postgres.** 21 statement goldens and 21 parameter goldens exist and
  every statement is parse-checked, and parse-checked is
  [explicitly narrower](adr/0007-federating-across-different-data-systems.md) than accepted: the
  dialect layer's parser is not gated per dialect for every construct. Today only DuckDB vouches for
  acceptance, through `differential.rs`. This is the second data system to do so, and the first over a
  wire protocol.
- **It de-risks the step after it.** If the client in use turns out not to speak SASL OAUTHBEARER, that
  blocks `feat/postgres-oauth` alone rather than the entire network story, and the adapter it would have
  blocked is already merged and useful.

**Touches.** A new adapter crate; `crates/sutura-config` for its source declaration; the
`tests/adapters` registry, which is the one-entry registration AGENTS.md's invariant promises.

**Adds.** A `Warehouse` over Postgres: render through `sutura-sql` in the compiled `dialect-postgresql`,
bind parameters as parameters, forced quoting intact, `LIMIT 10001` unchanged. Nothing about the plan or
the generator moves.

**Tests.**
- The whole existing golden and refusal corpus, registered for this adapter and green - which is the
  claim that adding a data system is a registration rather than a test edit, tested for the first time
  against a system that is not DuckDB.
- `the_rendered_statement_is_accepted_by_a_real_postgres`, over the corpus, in the compose tier.
- The row-cap leg: a result at the cap distinguishable from one cut off by it, asserted here too,
  because `row_limit()` being `max_rows + 1` is a property of the generator and this is a new executor
  reading it.

**Done when** the corpus is green against a containerised Postgres, the artifact question has an answer
in code rather than in a record, and the answers are identical to DuckDB's for every case the
conformance packs cover.

## Postgres over OAuth

**Goal.** The first network source, and the first real impersonation.

**Builds on the adapter above rather than introducing one.** The driver, the source declaration and the
corpus registration are already merged and green on a static credential, so this step changes exactly
one thing: how the connection is authenticated.

**The verification this step was blocked on is DONE, and the answer is the unwelcome one.** It asked
whether the client the adapter chose speaks SASL `OAUTHBEARER`. Neither Rust client does: `tokio-postgres`
and `sqlx` each declare `SCRAM-SHA-256` and `SCRAM-SHA-256-PLUS` and nothing else, and neither repository
carries an issue or a pull request about OAuth - checked on 2026-08-29, and `sqlx` has moved to
`transact-rs/sqlx`, so a search of the old path finds nothing for the wrong reason. So the option this
paragraph used to leave open - "a second client for this source" - has one occupant, `libpq`, and
supplying a token you already hold to `libpq` is reachable **only from C**: there is no connection
parameter for it, and `PQsetAuthDataHook` with a `PGoauthBearerRequest` is the whole interface. The step
therefore owes **first-party protocol code**, and that is scoped rather than open-ended - see *Adds*.

**And the server side owes a module, which `feat/compose-tier` established the route for.** Core
Postgres ships no validator; upstream's stub is `src/test/modules/oauth_validator/validator.c` and
reaches no installed artifact, because `src/Makefile`'s `SUBDIRS` never names `test/modules` and the
official image deletes its source and its toolchain. The route that needs no compiler is a prebuilt one
on the registry the tier already reaches: `percona/percona-distribution-postgresql:18` installs
`percona-pg_oidc_validator18`. `compose.services.yaml`'s postgres block carries the whole finding, the
module's maturity and the reason the fixture belongs under the `identity` profile; this step wires it and
brings it up, which is what proves it.

**A third prerequisite was listed here and is withdrawn, because it was checked and it is false.** The
earlier version required that the `libpq` in the toolchain be built with curl. Curl is needed only for
libpq's own built-in Device Authorization flow, an optional module behind `--with-libcurl`; a client
that already holds a token supplies it through `PQsetAuthDataHook` with
`PQAUTHDATA_OAUTH_BEARER_TOKEN`, and the SASL `OAUTHBEARER` exchange itself needs no curl. A server
never runs the device flow. Checked against the
[libpq OAuth documentation](https://www.postgresql.org/docs/18/libpq-oauth.html), which is what makes
this a removal rather than a doubt - and blocking a step on a prerequisite that does not exist costs
whoever picks it up a day.

**Touches.** A new adapter crate; `crates/sutura-config` for its source declaration.

**Adds.** A Postgres warehouse declaring `ImpersonationAtSource`, authenticating the connection as the
subject so the source maps it to a role. **Not** `SET ROLE` on a pooled connection: `RESET ALL` does
not clear the role, `SET LOCAL ROLE` outside a transaction fails open, and four advisories name
shared-pool-plus-role-switching.

**Adds, second: a token-first `OAUTHBEARER` mechanism, and the scope is the smallest honest one.** The
initial client response carrying the bearer token, and one challenge/response round for the failure
case - which is deliberately the same scope the `node-postgres` work limits itself to. **Token-first**
means the adapter supplies a token and the mechanism performs the SASL exchange and nothing else: no
discovery, no device flow, no identity-provider conversation, because part 4's `LegCredentials` is
already where the token comes from. `pgx` has a shipped implementation to read against - the OAuth
support merged into `jackc/pgx` on 2026-03-01 - so this is a port of a known-good exchange rather than a
protocol design. It belongs in the adapter crate, not in the driver: a fork of `tokio-postgres` is a
maintenance liability this step does not need if the driver exposes enough of the authentication
handshake, and **whether it does is the one thing to check before writing any of it.**

**Adds, third: a boot refusal for a source that declares impersonation against a server that cannot do
it - and its scope is smaller than it sounds, because the failure is already closed.** Today such a
deployment discovers the problem at its first question; the deployment already opens a connection at
boot, so the check has somewhere to live. What it must NOT be sold as is plugging a hole:
[a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) records the
determinability findings, and two of them bound this check hard. An `oauth` line in `pg_hba.conf` with an
empty `oauth_validator_libraries` is refused at HBA parse, so the postmaster does not start at all - that
state is not reachable on a running server. And `oauth_validator_libraries` is `GUC_SUPERUSER_ONLY`, so
the boot role usually cannot read it. So the check is three-valued and the middle value is the honest one:

- **Refuse** when the server does not have the mechanism at all - `server_version_num < 180000`. Free,
  needs no privilege, and it is the misconfiguration that actually happens: a source declared
  `ImpersonationAtSource` pointed at a Postgres 17.
- **Refuse** when `current_setting('oauth_validator_libraries', true)` returns an empty string, which is
  readable only where the boot role holds `pg_read_all_settings` or is superuser.
- **Record UNDETERMINED, in the startup log, naming the source** when that read raises `42501`. It must
  not collapse into a pass: an operator who wants the stronger check grants the boot role
  `pg_read_all_settings`, and the record says so with the cost - that role exposes every setting,
  file paths included, so it is a deliberate grant rather than a default.

**Tests, for that check specifically, and they are unit tests against a fake rather than a container:**
`a_source_declaring_impersonation_against_a_server_without_the_mechanism_does_not_boot`, its green twin
with an 18 server, and `an_unreadable_validator_setting_is_recorded_as_undetermined_rather_than_passing` -
the third is the one that stops the check being written as a two-valued one that passes on refusal to
answer, which is how it would fail to a false green in every deployment that did not grant the role.

**Tests.** The two-subject test that cannot exist today, and three assertions rather than one, because
"different rows" alone would pass against a fixture that differed for the wrong reason. Compose tier by
nature.

- **Two identities, a row-level policy at the source, different rows**, asserted against what each
  identity is entitled to rather than merely against each other.
- **The session reports the borrowed subject in-session.** Postgres exposes the authenticated identity,
  so the test asserts the connection believes it is the asker - which is what distinguishes
  authenticating the connection as the subject from switching a role on somebody else's connection.
- **A connection is not reused past the token's expiry.** A session authenticated with a token outlives
  the token, because the server checks it once at authentication, so a question after the expiry has to
  open a new connection under a freshly minted credential - observable as a changed backend identifier.
  This is the assertion that keeps a per-subject pool from becoming the long-lived session
  [a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) rejects.

**And what this test must NOT assert:** anything about clearing a role on a shared connection. That
mechanism is rejected rather than merely unused - `RESET ALL` does not clear the role and `SET LOCAL
ROLE` outside a transaction fails open - so a test asserting the cleanup worked would be pinning a
design this plan does not build.

**Done when** two subjects get different rows through the same question, **the multi-user example
demonstrates exactly that end to end**, and the artifact question from the ADRs is answered rather than
deferred - which of the shipped binaries links a native driver.

## Mutual TLS per source

**Goal.** Optional client certificates to a metadata source and to a data source.

**Adds.** Per-source client certificate, key and trust anchors; a partial declaration refused at load;
the trust store stated rather than inherited; and rotation reusing the polling-and-swap already built
for serving rather than a second path.

**Done when** three states per source are each tested - none, TLS with verification, mutual TLS - and
the middle one, the one that gets forgotten, has a test of its own.

## The raw SQL tool, off by default

**Goal.** The ungoverned path
[a raw SQL tool, off by default](adr/0013-a-raw-sql-tool-off-by-default.md) decides, with every
mechanism that keeps it from ever looking certified. **It has a row in the stack table now**, because an
earlier version of this plan executed the record without scheduling it: 0009 lists 0013 among the
records this plan carries out and the table had no branch for it, which is how a record becomes a
decision nobody implements.

**Late in the order, and every prerequisite is real rather than defensive.** Four things it needs, and
none of them exists today:

- **A separate scope, and advertisement filtered by it**, so a caller without the scope does not see the
  tool at all. That is `feat/agent-surface-scope`, and it in turn is blocked on `docs/inbound-identity`,
  because filtering on a scope nobody verified is not a control.
- **A different outcome type**, whose shape has nowhere to put a definition digest - which is what makes
  "certified" unrepresentable for this path rather than merely absent. That is new code in this branch.
- **Read-only enforced by the source**, as a role that cannot write, never by inspecting the statement:
  deciding read-only-ness by parsing is the thing this repository does not do, and a parser reachable
  from caller text is worse than the tool it would be guarding. So it needs a credential that can be a
  read-only role, which is `feat/credential-port`, and a source to attach it to, which is
  `feat/source-registry`.
- **An execution path that can be cancelled**, because an arbitrary statement is exactly the case where
  a deadline that only stops waiting leaves work running. That is the deadline on the port, also
  `feat/credential-port`.

**Adds.** The tool, its scope, its outcome type, the read-only role requirement as a startup refusal
rather than a runtime check, and the coverage number: the share of questions answered certified against
ungoverned. That number reads the record stream `feat/principal-chain` writes - it is not a second
store, and it cannot be computed at all if nothing is written, which is one of the three things that
made the audit-sink question worth settling before this branch exists.

**Tests.** Its own small pack, because it asserts different things from the certified corpus:
- `the_raw_tool_is_absent_without_its_scope` - invisible rather than rejected on call.
- `a_raw_result_cannot_carry_a_definition_digest` - a compile-fail doctest with a compiling twin, since
  the point is that the shape cannot hold one.
- `a_write_through_the_raw_tool_is_refused_by_the_source_and_not_by_a_parser` - asserted against a real
  read-only role in the compose tier, because a fake proving our own guard back to us is the failure
  mode here.
- `the_ungoverned_share_is_reported` - over the records, per deployment.

**Done when** the tool is off by default, invisible without its scope, incapable of returning something
that looks certified, read-only because the source says so, and its share of traffic is a number
somebody can look at.

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

**Touches.** The justfile, a demo compose file separate from `compose.dev.yaml`, and the `xtask`
provisioning from the compose tier - the same worktree-aware ports and project names, since two people
demoing at once is the normal case.

**Which UI is deliberately not decided here.** A ready-made chat container is the fast path; something
lighter that speaks the OpenAPI surface directly is less to run. Pick it when the task is written,
against one criterion: **can it call an OpenAPI tool server without us maintaining a plugin?** If it
cannot, it is the wrong choice however good it looks, because a plugin we maintain is governance code
in a client we said has no governance role.

**Done when** `just demo federation` brings up a working scenario from a clean checkout, on host
docker, with its endpoints read from the compose tier's discovery file rather than from a constant;
when a missing docker skips loudly on a developer machine - a demo is not a gate, so this one skips
everywhere rather than failing in CI, and it does not run in CI at all; and when the README for each
example says which `just demo` runs it.

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

1. ~~**Exclude ADR pages from the workflows.**~~ **HALF DONE, and the other half is PAUSED pending the
   cheap check that makes it safe.** This item was two changes in one commit, and they are not equally
   sound, so they are recorded separately rather than left marked done together.

   **The path-filter half stands, and it fixed a real defect.** `mkdocs.yml` was in `docs.yml`'s filter
   AND matched `ci.yml`'s leading `**`, so an ADR-only pull request that added a nav entry started BOTH
   workflows - the "exactly one workflow starts" claim in `docs.yml`'s header was false and had never
   been checked. `ci.yml` now excludes `docs/**` and `mkdocs.yml` by name, so a prose-only change no
   longer starts the 120-minute chain. Neither path can affect what cargo produces.

   **The gate-skip half is PAUSED, and one path exclusion that rode along with it is reverted
   outright.** The two have different fates and lumping them together is what made the original commit
   hard to review. `docs.yml`'s
   `verify` job was made to skip the 15m45s `hygiene` build when every changed path was markdown under
   `docs/` or `mkdocs.yml`, keeping only the 3-second `mkdocs --strict` build. The reasoning was that
   `--strict` covers what a page edit actually risks - a dead link, a bad anchor, a page in no nav
   entry, a nav entry with no file, a missing asset - and that the deferred check, `check-guidance`,
   would still run on the `main` push. **That last part is the defect: a check that runs after the
   merge is not a pull-request gate.** A `just` task citation that does not exist would merge green,
   then fail the publish, and the branch that broke it is already in `main` - which is exactly the
   failure mode `ci.yml`'s own filter comment warns about for its own two path lists ("a filter that
   let a path start a run on `main` but not on the pull request that introduced it would report the
   merge green and then fail immediately after it"). The exclusion of
   `.github/workflows/docs.yml` from `ci.yml` was part of the same commit and reverted with it, on the
   narrower ground that a workflow definition is a change to what CI does and should not be excluded
   from the workflow that gates CI on the argument that cargo cannot see it.

   **The synthesis, which is the decided end state rather than a compromise.** The objection above is
   narrow enough to answer without paying the whole 15m45s again: keep the skip, and give the pull
   request a **cheap text-only equivalent of the citation check** - grep the changed markdown for
   `just <task>` and check each name against `just --summary`. That is seconds of shell, needs no Rust
   closure, and covers the one gate whose absence made post-merge gating unacceptable. So the step is
   unconditional only until that check exists, and building it is part of this branch rather than a
   later idea. **The framing to avoid, because this section had it:** the skip was not "rejected in
   review" - it was asked for deliberately, and the review predates it. What review found was one
   defect in it, and the fix for a defect that specific is the check above, not the cost.

   **What that means for the rest of this section:** the 15m45s is still owed a measurement, and that
   is what point 2 is for - it decides whether points 3 and 4 are needed at all once the text-only
   check has removed the reason the gate had to stay. Nothing about the closure being the cost has
   changed.
2. **Find out whether these numbers are cache misses.** Both runs measured were the first after a
   force-push. The same gates locally are seconds. Restructuring a workflow on a cold-cache measurement
   is how a fix gets built for a problem nobody had - and it is the reason the gate-skip above was the
   wrong thing to do FIRST, independently of whether post-merge gating is acceptable. This measurement
   is the point of the branch.
3. **Give the verify job the cache `ci.yml` already uses.** The docs workflow omits it deliberately and
   the reasoning is sound - the Actions cache is writable from a pull request and a *published* page
   must not be built from a store path a pull request could have placed there. But that argument is
   about publishing. The verify job runs on a pull request with a read-only token and publishes
   nothing, and `ci.yml` already accepts exactly this risk to gate merges. Cache verify, leave publish
   uncached, and say so where the current comment says the opposite. **This is the first thing that
   could make the gate cheap without making it late.**
4. **Only if it is still slow warm:** separate the gates that read prose from the ones that need a
   compiler, so a prose change runs `check-docs`, `check-guidance`, `text-hygiene`, `line-endings` and
   `max-lines` without a dependency closure. That needs a prebuilt `xtask`, which is its own piece of
   work and should not be started before the measurement in point 2 above says it is necessary. This is
   the other way to make the gate cheap rather than late, and between them these two are what the
   reverted skip was trying to shortcut.

**Done when** an ADR-only change is gated in about a minute rather than seventeen **on the pull
request**, with `check-docs`, `check-guidance` and mkdocs `--strict` all still running there, and with
the publish job's cache posture unchanged. "Gated on the merge instead" does not satisfy this.

## The examples are the demo, one per deployment variant

**Every deployment variant gets a WORKING end-to-end example, and "working" means a test runs it.** Not
a README describing what would happen. The existing example is already exercised by
`crates/sutura-cli/tests/example.rs`, which asserts five exact measure sets, so the bar is set: an
example that drifts fails a test rather than misleading a reader.

| Variant | Example | What it demonstrates | Lands with |
| --- | --- | --- | --- |
| **Single user** | `examples/single-player` (exists) | One source, static credentials, the whole measure vocabulary | shipped |
| **Two data systems, refused** | the two-source corpus (written, unmerged) | That crossing two `SourceName`s is refused today, from a real on-disk catalog. A permanent governance boundary rather than a placeholder | before `feat/two-source-execution` |
| **Federation** | the same corpus, answering | The same question that was refused now answers across two sources, so the diff shows exactly what changed in behaviour | `feat/two-source-execution` |
| **Multi user** | a new corpus, compose-backed | Two subjects, the same question, DIFFERENT rows, enforced by the source | `feat/postgres-oauth` |

Three things this ordering buys, and the middle one is the reason to do it this way:

1. **The refusal corpus is the red half.** It exists before federation, asserts today's behaviour, and
   the day federation lands the same example flips from a refusal to an answer. That is red-before-green
   at the level of a demo rather than a unit test.
2. **The multi-user example cannot be faked.** No local file enforces a row-level policy, so this one is
   compose-backed by nature and lands with `feat/postgres-oauth`, not before. A fixture that answered the same rows for
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
- **Oracle impersonation, DEFERRED with the posture decided.** An Oracle source declares `SharedServiceUser` only.
  The capability exists in the database and no production-viable Rust crate exposes it, Oracle's own
  official driver included, so it is a missing safe wrapper rather than a missing capability -
  [a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) has the full finding,
  the preference order for closing it, and the one option ruled out rather than deferred. Native driver
  is the committed transport.
- **BigQuery and Oracle adapters.** Both wait on `feat/postgres-oauth` proving the shape, and Oracle additionally on a
  generator question: its dialect exists upstream as an empty feature, and the rendering it needs lives
  behind the transpile feature this workspace does not compile.
- **Untrusted-content marking in the result envelope.** Cheap before the first Arrow envelope, expensive
  after, so it lands with the envelope rather than after it.
- **The remaining metadata connectors.** OKF, Datahub, OpenMetadata, the RDBMS catalog, BPMN and RDF are
  all `feat/source-registry`-shaped once the packs exist: a registration, a declaration, and fixtures.
  Branch name rather than a step number, because the numbers in the stack table have moved twice.
- **Selective service CI.** Deferred rather than scheduled, with the reason and what survives of the
  design in its own section above: the fail-open property that justified it cannot be exercised by a
  new adapter, because `crates/**` already matches an area, and the cost it would save has never been
  measured because the tier it selects over does not exist.
- **A budget.** A cost ceiling checked at plan time and shared across replicas is named in
  [a credential per leg](adr/0008-a-credential-per-leg-for-the-calling-subject.md) and in
  [the plan](adr/0009-the-plan-from-one-source-to-many.md)'s *what is not decided*, and it has no
  branch here. `feat/principal-chain` builds the key a budget would use and stops there; where a
  counter shared across replicas lives is undecided, and per-pod counters are not a budget.
