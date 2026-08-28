---
title: The plan, from one source to many and one user to many
description: The order in which federation and impersonation get built, plus the five decisions that belong to no single record - three-legged OAuth as the basis of impersonation, which aggregates survive descending, the three bounds that make a pull-up affordable, conformance over per-source tests, and why inlining a literal is not a security compromise here.
---

# The plan, from one source to many and one user to many

Status: **accepted as the plan of record. Nothing here is built.**

Three records decide the pieces.
[Several databases behind one data system](0006-several-databases-behind-one-data-system.md)
declines a federation crate and declines attaching several databases below the port.
[Federating across different data systems](0007-federating-across-different-data-systems.md) decides
the shape that replaces them.
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) decides identity.
[Transport security for a source](0010-transport-security-for-a-source.md) decides mutual TLS.
[Pluggable by declaration](0011-pluggable-by-declaration.md) decides how an adapter says what it
provides and which mode it is in.

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

**Same-source fusion is grouping, not analysis.** Two models on one source become one leg because
they share a `SourceName`. Recovering that from a physical plan is hard; reading it off the semantic
plan is a `BTreeMap`.

## Decision 1: three-legged OAuth is the basis of impersonation, not an option beside it

**A source reached under one shared identity cannot answer per subject, and the answer must say so.** Impersonation is a must-have for the sources configured for it, and the mechanism is
three-legged OAuth: leg 1 authenticates the caller to sutura with a token audience-bound to sutura,
and leg 2 reaches the source AS that subject. The client's token is never forwarded upstream, and
there is no service-account fallback, because ADR 0008 removes the signature that could have one:
`execute` and `dry_run` take a credential.

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

**Whether a leg was pushed or pulled must be observable** - a log line, a metric, or a test asserting
the pushed statement. A hand-written renderer that silently stopped pushing and fell back to reading
a whole table was measured at seven times the memory with a correct answer and no diagnostic. Silence
is the failure mode here, not error.

## Decision 3: three bounds, each of them a refusal

"Worst case we pay the processing cost" is only a cost if it is bounded. Unbounded is not a cost, it
is an outage, so the pull-up path ships with three bounds together:

| Bound | Why it is not covered by what exists | Failure mode without it |
| --- | --- | --- |
| Rows, per leg | `row_limit()` is `max_rows + 1` and protects the ANSWER, not an intermediate | A leg pulls orders of magnitude more rows than the answer holds |
| Memory, per query | Nothing bounds a working set today | Shipped profiles compile `panic = "abort"`, so an allocation failure is PROCESS DEATH for every caller |
| Runtime, per query | Admission shedding and a transport timeout bound waiting and responding, not executing | A query runs on behind an abandoned response |

**The numbers, and where they live.** A result-size ceiling of **1 GB per query** and a query timeout
of **three minutes**, both configurable, set GLOBALLY as defaults with **per-source overrides** - a
warehouse and a local file do not deserve the same patience. Each is parsed as a newtype in
`sutura-config`'s limits and produces a **typed refusal**.
Refused, never truncated, never degraded: a partial answer under a certified metric name and a
definition digest is worse than an error, because it looks like an answer.

State what each does not bound. A memory ceiling on the combiner does not bound what a driver buffers
before handing rows over, nor what a source spends on its own side. An overstated control is itself a
defect.

## Decision 4: one conformance suite, not a suite per source

**Every metadata provider and every data source conforms to the same tests and functions.** A new
connector proves itself by registering and declaring, never by editing a test. What must be identical
and what may differ is the whole design:

- **Rows: identical.** Same question, same catalog, any source, same answer. That is the conformance
  property, and `differential.rs` is the seed - it already runs one plan two ways and compares rows.
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
- **Compose:** Oracle, Postgres, Datahub, OpenMetadata, brought up on demand, provisioned through the
  sutura CLI, **worktree-aware** so concurrent worktrees never collide on ports, project names or
  volumes. Docker is a host dependency and is deliberately not pinned by nix.

**The compose tier cannot be a nix check.** A nix check builds in a sandbox with no network and no
docker socket, which is why `just validate` builds without network. It is a CI job and a `just` task
that CONSUME nix-built artifacts. The strongest version of it runs the OCI image that actually ships.

The test AGENTS.md says cannot exist yet - two subjects, different rows - is a compose-tier test by
nature, because no local file enforces a row-level policy. A fixture that answered the same rows for
both subjects and passed would be worse than no test.

## Decision 5: a literal may be inlined, and that is not a security compromise here

Where a rendering path inlines a value rather than binding it, that is permitted. The reason it costs
nothing is structural, and it was verified in the code rather than assumed:

- A dimension is filterable ONLY if the catalog declares `allowed_values`. Without one it can be
  grouped by and not filtered, "because the alternative is comparing against a value the caller
  supplied, and the pinned bundle is the only thing entitled to say which values exist".
- A caller's filter value is refused unless the dimension permits it, in `sutura-semantic`'s resolve
  step, as `DimensionValueNotAllowed`.
- Allowlist entries are `DimensionValue` newtypes, one line, control characters refused, and bounded
  in number.

**So no caller-supplied text can ever reach a statement.** The complete inlinable value space is:
catalog-declared dimension values, ISO dates from a bounded `TimeRange`, and an integer row limit. A
caller SELECTS from a closed, load-validated set; it never supplies.

**AGENTS.md's row must be restated rather than softened**, because its stated mechanism - every value
becomes a bind parameter - no longer covers every path, and a row that loses its mechanism gets
deleted rather than demoted. The replacement claim is narrower and true: *no CALLER text reaches a
statement, because a filter value must be a member of the catalog's declared allowlist.*

**The residual risk is a catalog author, not a caller**, and AGENTS.md already treats catalog
documents as untrusted input. A declared value may itself contain a quote or a backslash. So escaping
of catalog-declared values is verified continuously, by a corpus that is snapshotted AND executed with
rows compared: a quote, a doubled quote, a backslash, a trailing backslash, a comment introducer, a
block opener, a semicolon with a second statement, a bare keyword, placeholder syntax, a zero-width
joiner, a right-to-left override. Values the newtypes already refuse belong in that corpus too, named
so nobody mistakes defence in depth for a live hole.

## The order

Stacked, smallest first, each step green before the next. `stax` manages the stack; the
`git-ops/stacked-branches` skill has the mechanics.

| # | Step | Done when |
| --- | --- | --- |
| 0 | **Decomposability in the domain.** The exhaustive match, the ratio rule, no plumbing. | A new aggregate cannot compile without stating how it federates; a ratio divided per leg is impossible rather than discouraged |
| 1 | **The three bounds**, as configuration and typed refusals. | Each bound provokes its own refusal in a test; the memory bound is shown biting rather than described |
| 2 | **Test the startup refusals that already hold.** The more-than-one-source arm of `open_engine` has no test in either binary. | Red against a build with the branch removed |
| 3 | **Per-source configuration**, keyed, parsed as newtypes. | A duplicate alias, a missing file and a relative path each refused at parse, asserted on the variant |
| 4 | **Two sources, one question, DuckDB both sides.** The split, the per-leg render, the combine. | Rows equal to the single-source corpus; each leg's statement snapshotted; a `CountDistinct` across sources correct |
| 5 | **The conformance harness.** Extract the shared test functions; declared capabilities per source. | Adding a source touches a registry and a declaration, never a test function |
| 6 | **The credential port.** `execute` taking a credential, the subject hoisted out of the legs. | No signature exists that can run as the process; a declared impersonation the deployment cannot perform refuses at boot |
| 7 | **One network source, Postgres first**, on OAuth per Decision 1. | Two subjects, different rows, in the compose tier |
| 8 | **The compose tier and worktree-aware provisioning.** | Two worktrees provision simultaneously without collision; absent docker prints SKIPPED and exits 0 |

Steps 0 to 2 need no decision from anyone and touch no adapter. Step 7 is where the identity story
becomes real, and it is the first step that cannot be verified without a live service.


## Found on a second pass, and not yet carried anywhere

These are gaps in the design as it stands, not open questions about the world. Two of them are cheap
now and impossible to retrofit, and they are marked.

- **The principal is a chain, and this is now DECIDED rather than open.** The target is stated:
  permissions for a specific agent, of a specific human, for a specific task. So the shape is human
  then agent then task, ordered, which is also the shape a token exchange maps onto rather than being
  translated into. Build it in step 0 while both tail positions are always absent: key the budget on
  the chain, record the chain wherever a call is recorded, and put the task on the request context.
  **The reason it cannot wait:** a stored row that says only the subject can never later be told apart
  from one that meant "an agent acting for" them.
- **Attenuation.** A task's credential is minted NARROWER than the subject holds - fewer metrics, a
  shorter lifetime, a smaller budget - and monotonically so, never broader. That is what makes an
  agent identity more than bookkeeping, and it is why a credential port takes the whole context rather
  than a subject.
- **A federated result inherits the MAXIMUM sensitivity of its inputs**, and re-linkage is assessed
  separately from access, because joining two permitted reads can identify someone neither read
  identified. Sensitivity belongs on provenance as a declared property so the rule has somewhere to
  live before it is needed.
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
