---
title: The plan, from one source to many and one user to many
description: The order in which federation and impersonation get built, plus the five decisions that belong to no single record - a token exchange as the basis of impersonation, which aggregates survive descending, the two bounds that make a pull-up affordable and the third that counted the wrong thing, conformance over per-source tests, and why nothing on the path being built inlines a literal - so the bind-parameter row keeps its mechanism.
---

# The plan, from one source to many and one user to many

Status: **accepted as the plan of record. Nothing here is built.**

Seven records decide the pieces.
[Several databases behind one data system](0006-several-databases-behind-one-data-system.md)
declines a federation crate and declines attaching several databases below the port.
[Federating across different data systems](0007-federating-across-different-data-systems.md) decides
the shape that replaces them.
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) decides identity.
[Transport security for a source](0010-transport-security-for-a-source.md) decides mutual TLS.
[Pluggable by declaration](0011-pluggable-by-declaration.md) decides how an adapter says what it
provides and which mode it is in.
[Conformance packs](0012-conformance-packs-for-inputs-and-adapters.md) decides how every adapter is
held to the same behaviour.
[A raw SQL tool, off by default](0013-a-raw-sql-tool-off-by-default.md) decides the ungoverned path
that exists to build a ramp to the governed one.

This record is the ORDER those happen in, and it carries five decisions that belong to none of them
alone. It adds no code and no dependency.

## The architecture, in one page

**One source: no federation at all.** The semantic compiler produces one statement in that system's
dialect and the whole query is pushed down. This is what ships today for DuckDB, and it is the cheap
high-value track for Postgres, BigQuery and Oracle.

**Several sources: per-source legs, combined here.** The plan splits BY SOURCE, which is a
semantic-level operation because the plan already knows each model's `SourceName`. Each leg is a
mono-source `QueryPlan`, rendered by `sutura-sql` with bind parameters and forced quoting, executed
through that source's own adapter under the asking subject's own credential. DataFusion combines
above: the join, the aggregation that could not descend, ordering, limits.

**Every source uses the semantic compiler and standard credentialed access.** There is no shape in
which one connection stands in for several sources, and DuckDB is not an exception. Generation per
dialect, never transpilation: `polyglot-sql` occupies the position `sqlglot` occupies elsewhere.

**Same-source fusion needs a relationship, not just a shared `SourceName`.** An earlier version of
this said grouping by source was enough. It is not: two lookup models on one remote source with no
declared relationship between them would fuse into a cross product. The rule is **same source AND
connected by a declared relationship in this plan**, which is analysis over the join graph rather than
a `BTreeMap` - still far cheaper than recovering source membership from a physical plan, and the test
that pins it must be named for the relationship rather than for the source.

## Decision 1: a token exchange is the basis of impersonation, not an option beside it

**A source reached under one shared identity cannot answer per subject, and the answer must say so.**
Impersonation is a must-have for the sources configured for it, and the mechanism is an **RFC 8693
token exchange**, which is what 0008 calls it throughout: the caller's identity arrives at sutura, and
sutura obtains a credential the SOURCE accepts for that same subject. There is no service-account
fallback, because 0008 removes the signature that could have one - `execute` and `dry_run` take a
credential.

**Two things about the token topology are NOT decided by either record, and they are load-bearing.**
An earlier version of this section claimed the caller's token is never forwarded upstream and that it
is audience-bound to sutura. Both cannot be true of BigQuery as 0008 describes it, where the caller's
token IS the subject token posted to the exchange and must carry the workforce provider's audience. So
either the front-door token is audience-bound to sutura and something exchanges it at our own
authorization server first - an exchange 0008 does not describe - or it carries the pool's audience and
"never forwarded" is wrong. **Who performs the exchange, and what audience the inbound token carries,
is the question to answer before the credential port is built.** Written here as open rather than
resolved in prose, because guessing it produces a port with the wrong signature.

Per source, and each of these is a capability claim to re-verify before it is depended on:

| Source | How a leg runs as the subject | Consequence |
| --- | --- | --- |
| PostgreSQL 18 | Native `oauth` method in `pg_hba.conf`, SASL OAUTHBEARER per RFC 7628, token validated by a pluggable validator module, then MAPPED to a database role | The connection *is* the subject. Per-subject connections, not role switching |
| BigQuery | Workforce identity federation plus an RFC 8693 exchange, yielding a token whose principal is the person | Service-account impersonation cannot produce a person, so it is not the path |
| Oracle | Proxy authentication, which switches identity on an existing physical connection and records `PROXY_USER` beside `SESSION_USER` | The only one that records the chain natively |
| DuckDB | It cannot. One process, one OS identity | Declares `SharedServiceUser`, which is honest and is recorded in the answer |

**PostgreSQL 18 replaces the mechanism rather than mitigating it, and that is the news.** ADR 0008
records in detail why the obvious design was unusable: `RESET ALL` does not clear the role, `SET LOCAL
ROLE` outside a transaction fails open, nothing server-side can see a leaked role, and four
advisories about row-level-security plan caching name shared-pool-plus-role-switching as the
triggering pattern. Authenticating the connection as the subject removes that whole class rather than
guarding it.

Three things to verify before a Postgres adapter is committed to, because each can invalidate the
route: that the `libpq` in the pinned toolchain is built with the curl dependency the method needs,
that a validator module exists for the deployment's identity provider, and **whether the Rust client
in use speaks OAUTHBEARER at all** - a pure-Rust protocol implementation may not, which would make
the driver choice part of this decision rather than downstream of it.

**Single-user is a deployment mode, not a smaller multi-user.** Credentials are static configuration,
one user, one host, not multi-tenant. It is a development and proof-of-concept shape, and a
non-impersonating source is fine for everything there.

**Sutura classifies no data, and that is the point of impersonation.** What a person may see lives in
the data catalog and in that person's own permissions at the source. There is no sensitivity flag
here, nothing derived upward through joins, and no refusal because a dataset was labelled - a second
opinion about someone else's authorization is the failure this design avoids. What sutura declares is
the source's MODE, per [Pluggable by declaration](0011-pluggable-by-declaration.md), and what it
records is which mode produced an answer. The startup refusal is therefore narrower and more useful:
a source declaring an impersonation the deployment cannot perform refuses at boot, because the
execution port takes a credential and there is no fallback to fall back to.

## Decision 2: which aggregates descend, and what happens when they cannot

**Push what can be pushed. Where an aggregate cannot be computed per leg and re-aggregated, retrieve
finer-grained rows and compute above, and pay the processing cost.** A non-decomposable measure is not
a refusal.

This is a correctness problem before it is a performance one, and it is not a corner case: `Avg` and
`CountDistinct` do not survive re-aggregation, and **six of the eleven shipped metrics use one of
them**, five of those blocked by `count_distinct` alone.

The mechanism is an exhaustive match over the closed measure vocabulary, so a new aggregate **cannot
compile** without stating how it federates:

- pushable as written: `Sum`, `Count`, `Min`, `Max`
- pushable decomposed: `Avg` travels as a sum and a count, divided once above
- not pushable: `CountDistinct`, which needs the distinct keys themselves

**A ratio is decomposed, never divided per leg.** Numerator and denominator travel as separate pushed
aggregates and the division happens once, above. The reason is specific: a `NULLIF(denominator, 0)`
guard applied per leg silently DROPS a subgroup instead of nulling it, which is a wrong number with
no error.

**The join kind in the combine is derived from where the filters went.** INNER for a remote dimension
that carries a filter, LEFT for one that does not. This is the second correctness finding in
[federating across different data systems](0007-federating-across-different-data-systems.md), and it is
the one that produces a wrong number rather than a refusal: a filter pushed into a lookup leg plus a
left join keeps every unmatched key and adds a null bucket to the answer, so the filter widens the
result instead of narrowing it. Single-source rendering puts that filter in a `WHERE` above the left
join, which is an inner join in effect, and reproducing it is not optional - it is what *rows
identical, any source* means. Both kinds are needed: an unfiltered dimension must stay LEFT or an
orphan fact key is silently dropped.

**Whether a leg was pushed or pulled must be observable** - a log line, a metric, or a test asserting
the pushed statement. A hand-written renderer that silently stopped pushing and fell back to reading
a whole table was measured at seven times the memory with a correct answer and no diagnostic. Silence
is the failure mode here, not error.

## Decision 3: two bounds, and the third counted the wrong thing

"Worst case we pay the processing cost" is only a cost if it is bounded. Unbounded is not a cost, it is
an outage. But the earlier version of this section called a result-size ceiling a memory bound, and it
is not one: a join or an aggregate can exhaust the combiner long before a result exists. Naming each bound for
what it actually counts is also what retired the third one: a count of rows per leg protects nothing that
is scarce.

| Bound | What it counts | Failure mode without it |
| --- | --- | --- |
| ~~Rows, per leg~~ | ~~Rows a leg returns before the combine sees them~~ | **RETIRED** - see below. A leg is grouped by the remote join key, so its row count is KEY CARDINALITY: unrelated to the answer's size and unrelated to the memory the combine needs |
| **Working set, per query** | The combiner's operator reservations: hash-join build side, aggregate state, sort | Shipped profiles compile `panic = "abort"`, so an allocation failure is PROCESS DEATH for every caller |
| **Wall clock, per query** | Time from admission to last row | A query runs on behind an abandoned response |

**The per-leg row bound is RETIRED, and the working-set bound is what replaces it.** This was the one
open question in this record, and it is now decided: **a leg is bounded in BYTES, not in rows.** The
reason the row bound was wrong is not that it was inconvenient - it counted the wrong thing. A leg
grouped by a remote join key returns key cardinality, which has no relationship to how much memory the
combine needs or to how large the answer is: `revenue by region` over fifty thousand customers is fifty
thousand rows of two small columns, which is a few megabytes, and its answer is twelve rows. A row count
cannot tell that from fifty thousand rows of wide text. The working set can, because it counts what is
actually scarce.

So, precisely:

- **A federated leg's statement carries no answer-shaped row cap.** `row_limit()` is `max_rows + 1` over
  an ANSWER, and a leg is not an answer. The mono-source path is unchanged - **no existing golden
  moves**, and AGENTS.md's counted claim about 63 goldens reading `LIMIT 10001` stands as written,
  because a federated leg is a new plan shape with its own goldens rather than an edit to those.
- **The answer keeps its row cap, unchanged.** A result at the cap is still distinguishable from one cut
  off by it, and `answer()` still returns `ResultTooLarge`. That bound is about what a caller receives
  and is not what this retires.
- **The working-set ceiling is what refuses a runaway leg**, and it refuses on the quantity that would
  actually have exhausted the process. 0007's paragraph pricing the per-leg cap - the twelve-row answer
  refused at 10,001 - is superseded by this, and that paragraph says so rather than being deleted, since
  the arithmetic in it is why the bound moved.

**What this gives up, stated rather than glossed:** there is no longer a cheap early refusal on a leg
that is about to return an enormous number of narrow rows. The working-set bound catches it, but later -
when the combine reserves memory - rather than at the moment the rows arrive. That is the trade, and it
is the right one, because the alternative refused correct twelve-row answers at a threshold that had
nothing to do with the resource being protected.
**The working-set bound is the engine's memory pool, and its limits are part of the claim.** The pool
counts what its operators reserve and nothing else: not the row set a driver hands back, not a leg's
buffers before conversion. So the honest statement is that it bounds the COMBINE, and the two gaps
either side of it are named rather than implied. Its failure has to become a typed refusal rather than
an abort, and whether the policy is spill-then-fail or fail-immediately is a decision, not a default to
inherit.

**Against what, and measured by whom.** A ceiling above the container's memory limit is process death by
default under `panic = "abort"`, so the configured value is checked against the limit available at boot
and refuses to start when it exceeds it, or is derived from it. And the numbers themselves - **a
provisional 1 GB working set and a provisional three-minute deadline**, global with per-source overrides
- are exactly that: provisional. `AGENTS.md`'s operating contract already says verify rather
than assert, and two numbers nobody measured are exactly what that forbids - so they are marked as a
starting point to be replaced by a measurement on the corpus, not presented as findings. Whoever
implements `feat/query-bounds` measures them; the record's job is to stop the provisional numbers from
hardening into decisions by being written in a record.

**Cancellation is not free, and the port cannot do it today.** `Warehouse::execute` is synchronous and
blocking, so a deadline that fires in the caller leaves the leg running inside the driver - each system
has its own interrupt, and none of them is reachable from a timeout wrapped around the call. So either
**the deadline travels on the port**, beside the credential 0008 adds, and each adapter cancels for
real - or this bound is "stop waiting" and the table's own failure mode is what ships. The first is the
intent; naming the second is what stops a test being written that claims cancellation and asserts a
timeout.

Each bound produces a **typed refusal**: refused, never truncated, never degraded. A partial answer
under a certified metric name and a definition digest is worse than an error, because it looks like an
answer.
## Decision 4: one conformance suite, not a suite per source

**Every metadata provider and every data source conforms to the same tests and functions.** A new
connector proves itself by registering and declaring, never by editing a test. What must be identical
and what may differ is the whole design:

- **Rows: identical, in a stated canonical form.** Same question, same catalog, any source, same
  answer - but "identical" needs defining or the execute packs go red on the first network source for
  reasons that are not defects: floating-point sums differ by summation order, decimal scale and
  rounding differ per system, tie order and NULL placement differ, and date truncation differs across
  date and timestamp types and time zones. `differential.rs` has met none of these because it compares
  two engines under one type mapping. So the packs state: the canonical form rows are compared in, the
  tolerance for approximate types and none for exact ones, and the ordering imposed before comparing.
- **Refusals: identical.** Same variant, whatever the source.
- **Rendered SQL: differs**, so it lives in snapshots keyed by source and dialect. No assertion in a
  shared test function may hard-code dialect syntax, or the suite has quietly become per-source.
- **Exceptions: only a DECLARED capability**, skipped loudly with the declaration named.

The declaration pattern already exists and should be reused rather than invented: `Capability::every()`
walks a provider's declared knowledge capabilities through exhaustive matches, and content for an
undeclared capability fails the load. Generalised to data sources it carries dialect features,
impersonation, Arrow-native access and per-aggregate pushdown - and the same declaration then answers
Decision 1's startup refusal, because a source declaring an impersonation the deployment cannot
perform is a configuration with no fallback. One mechanism, two requirements.

Two tiers, and the boundary between them is structural rather than a preference:

- **Fast and hermetic:** in-memory DuckDB, **several connections rather than several attachments**,
  because two connections are the shape that ships and cost nothing more in a test.
- **Compose:** Oracle, Postgres, Datahub, OpenMetadata, brought up on demand, ONE INSTANCE PER WORKTREE, provisioned through
  `xtask` rather than the shipped binary - docker orchestration in a release artifact is test
  scaffolding shipped to users, and `xtask` is never packaged - and **worktree-aware** so concurrent worktrees never collide on ports, project names or
  volumes. Docker is a host dependency and is deliberately not pinned by nix.

**The compose tier cannot be a nix check.** A nix check builds in a sandbox with no network and no
docker socket, which is why `just validate` builds without network. It is a CI job and a `just` task
that CONSUME nix-built artifacts. The strongest version of it runs the OCI image that actually ships.

The test AGENTS.md says cannot exist yet - two subjects, different rows - is a compose-tier test by
nature, because no local file enforces a row-level policy. A fixture that answered the same rows for
both subjects and passed would be worse than no test.

## Decision 5: nothing on the path we are building inlines a literal, and the row stands

The earlier version of this section permitted inlining and then restated AGENTS.md's row to match. That
was a decision taken for a consumer that does not exist, and it weakened a row with a live mechanism to
license a path nobody is building. **Reversed.**

**What the architecture above actually does.** The pushdown unit is sutura's own `QueryPlan`, rendered
per leg through `sutura-sql`. That renderer produces a `GeneratedQuery` whose statement and parameters
are separate fields with no merging constructor, and a golden asserts no question literal appears in a
statement. So on the federated path there is nothing to inline, and the row keeps its mechanism:

> No value from a question reaches the statement as text.

**Unchanged. Not restated, not narrowed.** Where inlining would arrive is a renderer we do not own -
a federation layer's own unparser emitting SQL from a logical plan, which has no access to our
parameter list. This record's position is that adopting such a renderer for a source is the change that
must carry the amendment, in the same diff as the code, with the golden it breaks in front of the
reviewer. Deciding it in advance means the row is already soft when that diff arrives.

**What survives from the earlier reasoning, because it is true and useful either way.** The value space
reaching a statement at all is closed and small, and that was verified in the code rather than assumed:

- A dimension is filterable ONLY if the catalog declares `allowed_values`. Without one it can be
  grouped by and not filtered, "because the alternative is comparing against a value the caller
  supplied, and the pinned bundle is the only thing entitled to say which values exist".
- A caller's filter value is refused unless the dimension permits it, in `sutura-semantic`'s resolve
  step, as `DimensionValueNotAllowed`.
- Allowlist entries are `DimensionValue` newtypes, one line, control characters refused, bounded in
  number.

A caller therefore SELECTS from a closed, load-validated set and never supplies. That is defence in
depth behind the binding, not a replacement for it.

**And the escaping corpus is worth building now regardless**, because the residual risk it covers is a
catalog author rather than a caller, and AGENTS.md already treats catalog documents as untrusted input.
A declared value may contain a quote or a backslash, and it reaches an identifier position or a
parameter depending on the construct. So: a corpus snapshotted AND executed with rows compared - a
quote, a doubled quote, a backslash, a trailing backslash, a comment introducer, a block opener, a
semicolon with a second statement, a bare keyword, placeholder syntax, a zero-width joiner, a
right-to-left override. Values the newtypes already refuse belong in it too, named so nobody mistakes
defence in depth for a live hole. If an inlining renderer is ever adopted, this corpus is the evidence
that review will ask for, and having it already green is the difference between a decision and a hope.
## The order

Stacked, smallest first, each step green before the next. `stax` manages the stack; the
`git-ops/stacked-branches` skill has the mechanics.

**The numbering lives in [the implementation plan](../implementation-plan.md), not here.** An earlier
version of this section numbered its own steps 0 to 8 while the plan numbered fifteen branches
differently, which is two owners for one artefact - the thing this repository has a table about. So this
section keeps only what is a *decision* and names each step by the branch that carries it:

| Branch | Done when |
| --- | --- |
| `feat/agent-surface` | One tool - ask a certified question - over the agent transport, its schema derived from the domain type, and an uncertified question refused as a RESULT. **First, because it is the API we expose**, and thin because every other branch rebases on it |
| `feat/agent-surface-scope` | The rest of the tool set, one schema source for both transports, and advertisement filtered by scope. Both properties need more than one tool to be testable |
| `feat/federation-decomposability` | A new aggregate cannot compile without stating how it federates; a ratio divided per leg is impossible rather than discouraged |
| `feat/principal-chain` | The chain is the key everywhere a subject is recorded, with both tail positions absent and no reader that assumes one position |
| `feat/query-bounds` | Each bound provokes its own refusal in a test; the working-set bound is shown biting rather than described |
| `test/startup-source-refusals` | Red against a build with the more-than-one-source arm of `open_engine` removed. That arm has no test in either binary today |
| `feat/source-registry` | A duplicate alias, a missing file and a relative path each refused at parse, asserted on the variant; the posture and its acknowledgement checked at boot |
| `feat/two-source-execution` | Rows equal to the single-source corpus; each leg's statement snapshotted; a filtered remote dimension over an orphan key correct; a `CountDistinct` across sources correct or refused |
| `feat/conformance-packs` | Adding a source touches a registry and a declaration, never a test function. The compile half needs no data system and lands early |
| `feat/credential-port` | No signature exists that can run as the process; a declared impersonation the deployment cannot perform refuses at boot |
| `feat/compose-tier` | Two worktrees provision simultaneously without collision - one instance per worktree, ports DERIVED from the worktree path rather than allocated, compose project name and volumes per worktree, endpoints read from a discovery file so no test holds a constant; absent docker prints SKIPPED and exits 0. It comes BEFORE the first network adapter, because it stands up the source that adapter is tested against |
| `ci/service-category-selection` | A PR touching one adapter runs one service job; one touching the semantic core or the shared harness runs all of them; `main` runs all unconditionally; and the run prints which rule decided. Extends `xtask`'s existing `Area` table, so "core changed" is a `consumers` edge and an unmapped path still FAILS OPEN to running everything |
| `feat/postgres-adapter` | The whole existing corpus green against a containerised Postgres on a static credential, and the artifact question - which shipped binary links a native driver - answered in code. [Track 1](0007-federating-across-different-data-systems.md), and it de-risks the step after it |
| `feat/postgres-oauth` | Two subjects, different rows, in the compose tier |
| `feat/demo-tasks` | Every deployment variant has a working example, and one that drifts fails a test rather than misleading a reader |

Two properties of that order are the decision rather than the schedule. **The agent surface is first**,
because it is the interface everything else is judged through and building it last means every earlier
step guessed at its shape. And **the first six rows need no decision from anyone and touch no adapter**,
which is what makes them safe to start before the open questions below are closed.
`feat/postgres-oauth` is where the identity story becomes real, and the first step that cannot be
verified without a live service.
## Found on a second pass, and not yet carried anywhere

These are gaps in the design as it stands, not open questions about the world. Two of them are cheap
now and impossible to retrofit, and they are marked.

- **The principal is a chain, and this is now DECIDED rather than open.** The target is stated:
  permissions for a specific agent, of a specific human, for a specific task. So the shape is human
  then agent then task, ordered, which is also the shape a token exchange maps onto rather than being
  translated into. Build it in `feat/principal-chain` while both tail positions are always absent: key the budget on
  the chain, record the chain wherever a call is recorded, and put the task on the request context.
  **The reason it cannot wait:** a stored row that says only the subject can never later be told apart
  from one that meant "an agent acting for" them.
- **Attenuation.** A task's credential is minted NARROWER than the subject holds - fewer metrics, a
  shorter lifetime, a smaller budget - and monotonically so, never broader. That is what makes an
  agent identity more than bookkeeping, and it is why a credential port takes the whole context rather
  than a subject.
- **A classification a source EXPOSES is carried through, never assigned here.** The earlier version of
  this bullet had sutura computing the maximum sensitivity of a federated result and declaring it on
  provenance, which contradicts the section above: sutura classifies no data. The smaller true claim:
  if a source labels what it returns, that label travels with the answer unaltered. What sutura does
  not do is invent one, compute one, or refuse a QUESTION on one. There is exactly one place a carried
  label may legitimately be read rather than only forwarded, and it is a startup check rather than a
  query-time one:
  [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 5c names it as the
  shape that would restore dataset granularity to the shared-identity acknowledgement, and marks it
  unbuilt because no metadata adapter exists to expose a label yet. The re-linkage question - that joining two
  permitted reads can identify someone neither read identified - is real and is NOT ours to answer:
  the sources authorized each read, and whoever accepts a federated deployment accepts that.
- **A record per call including refusals - but no retention obligation of our own.** Under
  impersonation the sources do their own auditing, each under the asking subject, and single-user
  deployments are development and proof-of-concept shapes. So sutura keeps no audit archive and
  inherits no retention duty. What a record is still worth: refusals are the demand signal for which
  questions have no certified answer, and that is product data rather than a log. Emit it, do not
  store it as a governance artifact.
- **A budget checked at PLAN time and shared across replicas.** `dry_run` already exists on the
  warehouse port, so a cost ceiling can refuse BEFORE spending rather than aborting mid-query, which
  is strictly better. Per-pod counters are not a budget.
- **Connection material lives apart from governance metadata.** Which anchors, which host, which
  credential is a different lifecycle from what a metric means, and putting both in one settings tree
  is how a catalog edit becomes a connectivity change.
- **Untrusted content marking in the result envelope, decided BEFORE the encoder ships.** Catalog
  descriptions and result cells are text an agent follows, so structure has to carry the boundary: a
  field boundary an encoder enforces cannot be forged by a cell value, and a delimiter line can.
  **Cheap now, expensive later:** retrofitting a field boundary once an envelope exists is the
  expensive half, and a filter that quietly edits data returns a wrong number, so a detection hit
  belongs in a refusal rather than a silent scrub.
- **Advertised tools filtered by scope**, so a tool the caller may not invoke is invisible rather than
  rejected on call.
- ~~Data-side identifiability~~. **Deferred deliberately for the start.** So a changed number means
  the data or the semantics moved, and telling which is a question answered by hand until something
  identifies the data side. Recorded so nobody mistakes the silence for an oversight.
- **Determinism, now stated:** the same semantics over the same data must give the same result. A
  different number is therefore evidence that the data moved or the semantics did, and never evidence
  that the system is nondeterministic. Two consequences: nothing in the plan or the render may depend
  on iteration order, wall-clock time or a set that is not ordered; and telling the two causes apart
  needs the data side to be identifiable, which is the one part still missing.
- **Degradation per dependency is the last thing unwritten.** The pinned bundle makes a metadata
  outage survivable, which is an unclaimed benefit of pinning, and anything that cannot be reached on
  the authorization path must fail closed. Nobody has written the rest down.

## What is not decided

- **Which artifact links a native driver.** `sutura-exec-duckdb` is a dev-dependency today, and
  nixpkgs has no musl `libduckdb`. Shipping any data source changes the cross-build matrix. A
  glibc-only CLI path unblocks the early steps; the served surface needs an answer.
- **Arrow in, per source, and where an Arrow-typed boundary lives.** `check-boundaries` forbids Arrow
  in the domain, AGENTS.md lists `sutura-arrow` as planned, and the pinned `duckdb` and `datafusion`
  disagree on the Arrow major. This wants its own record.
- **Whether a custom DataFusion planner or extension carries the semantic extras.** It keeps the
  pushdown unit as sutura's own plan, so bind parameters and the dialect goldens stay on this side of
  the boundary. It is the most promising shape and it is unprototyped.
- **Oracle's dialect.** `dialect-oracle` exists upstream as an empty feature, but the rendering it
  needs - a row limit that is not `LIMIT`, a date truncation that is not `DATE_TRUNC`, a parameter
  marker that is not `?` - lives behind the transpile feature this workspace does not compile. Oracle
  is a generator question before it is an adapter question.
- ~~Who declares a dataset critical~~. **Decided: nobody here.** Sensitivity lives in the data catalog
  and in the asking person's permissions at the source, which is what impersonation is for. Sutura
  declares a source MODE and records which mode produced an answer.
- **Audit and budget.** One record per call including refusals, and a budget shared across replicas,
  are named in the identity record and have no port here yet.
