---
title: The plan, from one source to many and one user to many
description: The order in which federation and impersonation get built, plus the five decisions that belong to no single record - a token exchange as the basis of impersonation, which aggregates survive descending, the two bounds that make a pull-up affordable and the third that counted the wrong thing, conformance over per-source tests, and why nothing on the path being built inlines a literal - so the bind-parameter row keeps its mechanism.
---

# The plan, from one source to many and one user to many

Status: **accepted as the plan of record, and partly built.** The branch table below is the authority
on which rows have landed - the ones struck through and marked `DONE` - and it is worth reading before
citing anything above it, because a row that shipped may have shipped in a shape this record's prose
still describes as proposed. `AuditSink` is the clearest case: it exists, with `TracingAuditSink` as
its first implementor, and the bullet further down says so in as many words.

**Corrected:** this status line read *"Nothing here is built."* while the same file carried a `DONE`
row and a bullet opening *`AuditSink` now exists*. A record contradicting itself on line 8 is the
worst version of this defect, because line 8 is what a reader trusts before reading anything else.

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
semantic-level operation because the plan already knows each model's `SourceName`. Each leg is a whole
mono-source leg plan - never a fragment, and **not a `QueryPlan`**: an earlier version of this
paragraph said it was, and
[federating across different data systems](0007-federating-across-different-data-systems.md) decides
the two shapes it actually needs, because `QueryPlan` requires a bucket, a measure and a measure label
and a dimension lookup has none of the three. Each is rendered by `sutura-sql` with bind parameters and
forced quoting, and executed through that source's own adapter under the asking subject's own
credential. DataFusion combines above: the join, the aggregation that could not descend, ordering,
limits.

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

**Two things about the token topology were NOT decided by either record, and they are load-bearing.**
An earlier version of this section claimed the caller's token is never forwarded upstream and that it
is audience-bound to sutura. Both cannot be true of BigQuery as 0008 describes it, where the caller's
token IS the subject token posted to the exchange and must carry the workforce provider's audience. So
either the front-door token is audience-bound to sutura and something exchanges it at our own
authorization server first - an exchange 0008 does not describe - or it carries the pool's audience and
"never forwarded" is wrong.

**ANSWERED, by [how a caller proves who it is](0014-how-a-caller-proves-who-it-is.md).** The first
horn: the inbound token is audience-bound to sutura and we validate that unconditionally, whatever the
client sends. And the exchange 0008 does not describe is real - so **the direct mode needs TWO
exchanges to BigQuery rather than one**, the first at the caller's own identity provider to retarget
the audience, the second the RFC 8693 exchange 0008 already works out. Behind a fronting component it
stays one, because that component's token already carries the pool provider's audience. That
asymmetry is itself an argument for supporting both inbound modes rather than only the direct one.

**What that record does NOT settle, so this paragraph is narrower rather than deleted:** whether an
enterprise identity provider can mint an *ID token* whose audience is a third party's client ID, which
is what the Security Token Service requires as its subject token. A delegation flow characteristically
returns an access token for the downstream resource instead. That is a verification which can
invalidate the BigQuery adapter rather than delay it, and it carries the same *blocked on one
verification, to do FIRST* marking the Postgres SASL question already has.

So `feat/credential-port` can now be written without guessing, and the two branches that named this as
a blocker are unblocked: that port's signature follows from one inbound audience and N outbound ones
from a single exchange decision, and `feat/agent-surface-scope` has a claim shape to filter on. The
record that answers it is the one this stack calls for; it exists rather than being owed.

Per source, and each of these is a capability claim to re-verify before it is depended on:

| Source | How a leg runs as the subject | Consequence |
| --- | --- | --- |
| PostgreSQL 18 | Native `oauth` method in `pg_hba.conf`, SASL OAUTHBEARER per RFC 7628, token validated by a pluggable validator module, then MAPPED to a database role | The connection *is* the subject. Per-subject connections, not role switching |
| BigQuery | Workforce identity federation plus an RFC 8693 exchange, yielding a token whose principal is the person | Service-account impersonation cannot produce a person, so it is not the path |
| Oracle | Proxy authentication, which switches identity on an existing physical connection and records `PROXY_USER` beside `SESSION_USER` | The only one that records the chain natively |
| DuckDB | It cannot. One process, one OS identity | Declares `SharedServiceUser`, which is honest and is recorded in the answer |

**And the bound that posture carries, stated where the claim is rather than in the branch that hits
it.** Per-subject connections mean one connection per concurrent subject against the server's
`max_connections`, and the connection pooler most deployments would reach for does not speak OAuth, so
nothing can front it. The sharper one is not capacity: a connection authenticated with a token
**outlives the token**, because the server checks it once, at authentication. So reuse has to be
bounded by the token's own expiry - closed at its `not_after`, never reused past it - or a per-subject
pool keeps a revoked subject's access until the connection happens to drop, which is exactly the
long-lived session this posture was chosen to avoid. 0008 records the capacity half as correct and not
scaling; the expiry half is a security property rather than a capacity one, and it is the reason this
row cannot be implemented as "a pool, keyed by subject".

**PostgreSQL 18 replaces the mechanism rather than mitigating it, and that is the news.** ADR 0008
records in detail why the obvious design was unusable: `RESET ALL` does not clear the role, `SET LOCAL
ROLE` outside a transaction fails open, nothing server-side can see a leaked role, and four
advisories about row-level-security plan caching name shared-pool-plus-role-switching as the
triggering pattern. Authenticating the connection as the subject removes that whole class rather than
guarding it.

Two things to verify before a Postgres adapter is committed to, because each can invalidate the
route: that a validator module exists for the deployment's identity provider, and **whether the
client in use speaks OAUTHBEARER at all** - a pure-Rust protocol implementation may not, which would
make the driver choice part of this decision rather than downstream of it.

**A third prerequisite stood here and is WITHDRAWN, because it was checked and it is false.** The
earlier version required that the `libpq` in the pinned toolchain be built with curl. Curl is needed
only for libpq's own built-in Device Authorization flow, which is an optional module behind
`--with-libcurl`; a client that already HOLDS a token supplies it through `PQsetAuthDataHook` with
`PQAUTHDATA_OAUTH_BEARER_TOKEN`, and the SASL `OAUTHBEARER` exchange itself needs no curl. A server
never runs the device flow, so the item would have blocked a step on a dependency this route does not
have. Checked against the
[libpq OAuth documentation](https://www.postgresql.org/docs/18/libpq-oauth.html) rather than reasoned
about, which is the difference between removing it and merely doubting it.

**The boot path executes too, and it does not execute as anybody who asked - which used to be a hole in
this record and is now answered elsewhere rather than restated here.** An anchor runs before any caller
exists, so the execution port has two ways in and only one of them has a subject.
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 1 decides it: anchors
run under a per-source, operator-declared verification identity through a **separate port method** that
cannot accept a request credential, and `answer` holds no value of that type - so the boot credential is
unreachable from the request path and the request credential is unreachable from the boot path. On a
`SharedServiceUser` source that identity IS the shared identity and nothing new is configured; on an
`ImpersonationAtSource` source the operator declares a static least-authority credential in that
source's own entry; and **a bundle with an anchor on a source that declares neither does not boot**,
naming the metric and the source. The consequence for this record is one sentence and it is the honest
one: under row-level security an anchor certifies the number the VERIFICATION identity sees, which is
not necessarily the number a caller sees, and nothing stronger is available without executing an anchor
per caller.

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
that is about to return an enormous number of narrow rows. Two things catch it instead and both bite
later: the byte budget at the conversion boundary described below, as the rows are converted rather than
when the statement is issued, and the working-set ceiling later still, when the combine reserves memory.
That is the trade, and it is the right one, because the alternative refused correct twelve-row answers at
a threshold that had nothing to do with the resource being protected.

**The working-set bound is the engine's memory pool, and its limits are part of the claim.** The pool
counts what its operators reserve and nothing else: not the row set a driver hands back, not a leg's
buffers before conversion. So the honest statement is that it bounds the COMBINE, and the two gaps
either side of it are named rather than implied. Its failure has to become a typed refusal rather than
an abort. **And the policy is DECIDED: fail immediately, never spill.** Two reasons, and the second is
the one that settles it. A refusal the caller sees beats a degraded answer it cannot, which is this
repository's stated posture on the query path. And spilling writes the ASKING SUBJECT'S ROWS to the
pod's local disk - a data-at-rest surface nothing in this design governs, on the one path whose whole
purpose is that a query executes as the person who asked. A bound that protects memory by creating an
ungoverned copy of the data has not protected anything. So: no spill directory, no disk sizing, and a
refusal that does not depend on disk state.

**Naming that gap is not closing it, so here is what closes it and where.** A leg big enough to exhaust
the process exhausts it while the driver is still materialising its `RowSet`, BEFORE the combiner
reserves anything - so the pool cannot refuse it, and a test that claimed otherwise would be asserting a
mechanism nobody built. The bound that reaches it is a **byte budget at the execution boundary**, applied
as rows are converted rather than after a whole `RowSet` exists, which puts it with the
`RowSet`-to-Arrow decision rather than with the configuration: `feat/leg-plan-types` defines the shape
that gets converted and `feat/two-source-execution` owns the boundary, so the budget lands there. Until
it does, the honest scope of the working-set ceiling is the combine alone. The consequence for the
branch that ships the ceiling is written here rather than left to be discovered: `feat/query-bounds` can
assert an operator reservation refused and CANNOT assert an oversized leg refused, because nothing in
that branch sees a leg.

**Which value governs a multi-source query, decided.** A global default with per-source overrides has an
obvious hole the moment one question reaches two sources whose overrides disagree, and leaving it to the
implementation is how a ceiling becomes whichever source was registered first. So:

- **The working-set ceiling is a QUERY-wide value and takes no per-source override.** There is one
  combiner and one working set, so a per-source ceiling would be a number with nothing to bound. The
  global value governs, and a source declaration that tries to set one is **refused at parse rather
  than ignored** - a setting that silently does nothing is worse than a missing one, because somebody
  will tune it and believe the result.
- **The deadline takes per-source overrides, and the value that governs a multi-source query is the
  MINIMUM over the sources the plan touches, itself bounded by the query-wide default.** Minimum rather
  than maximum, because the alternative lets one patient source extend the deadline of a question that
  also reaches an impatient one. A per-source override can therefore only ever make a query stricter,
  which is the same monotonic direction attenuation runs in further down this record.
- **The refusal names the source whose value governed**, or an operator tuning one number cannot tell
  which number bit.

**Against what, and measured by whom.** A ceiling above the container's memory limit is process death by
default under `panic = "abort"`, so the configured value is checked against the limit available at boot
and refuses to start when it exceeds it, or is derived from it. And the numbers themselves - **a
provisional 1 GB working set and a provisional three-minute deadline**, the ceiling global and the
deadline global with per-source overrides - are exactly that: provisional. `AGENTS.md`'s operating
contract already says verify rather than assert, and two numbers nobody measured are exactly what that
forbids - so they are marked as a starting point to be replaced by a measurement on the corpus, not
presented as findings. Whoever implements `feat/query-bounds` measures them; the record's job is to stop
the provisional numbers from hardening into decisions by being written in a record.

**One of the two is already contradicted by a route the surface has to survive**, and saying so here is
cheaper than discovering it when the first gateway-fronted deployment times out. A route whose front
door cuts a request at tens of seconds cannot carry a governed turn allowed 180 seconds, so on that
route one of the two numbers is decorative. The decision is that the DEADLINE yields: it is bounded by
the front door on any route that has one, rather than the front door being treated as an operational
detail a default may ignore. And where a governed turn can exceed that bound, the surface on that route
becomes submit-and-poll rather than the deadline becoming a number nothing enforces. The implementation
plan's *what the agent surface must survive* section owns which of those two happens and owes a measured
turn before choosing - the point here is that the two numbers can no longer sit in two records
disagreeing by an order of magnitude with nothing said about it.

**The legs run SEQUENTIALLY, so the wall clock bounds their SUM.**
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 4 decides this, and
it decides it here rather than in whoever writes the combiner because both of this record's bounds key
on the answer: the deadline applies to the sum of the legs and not to the longest one. The reasons are
that record's - `Warehouse::execute` is synchronous and blocking so parallel legs mean N blocking-pool
threads per question, a per-subject session is the expensive posture and parallelism multiplies it, and
running the legs into the combine one at a time is what lets the working-set ceiling refuse before the
next leg is even asked for. **And the limit, because the obvious reason to prefer sequential is the
wrong one:** sequential does NOT reduce the working-set peak, since a hash join needs every leg's build
side. It buys threads and sessions, not bytes, and claiming otherwise would be the overstated control
this section keeps deleting.

**One case sequential execution creates, and it is checked rather than noted.** A long first leg can
consume the credential's life before the last leg starts, so "the credential aged out between legs" is
reachable in a way it is not when every leg starts at once. The check is **before each leg, against the
one `not_after`** the credential set carries, and a leg that would start after it is not attempted -
which is a different case from an expiry mid-query, where there is nothing left to refuse. Parallel legs
are a later change with a measurement behind them: they turn this bound from a sum into a maximum and
multiply the session peak by the leg count, so both bounds move and it is a decision rather than a
tuning knob.

**Cancellation, DECIDED rather than left as two options.** `Warehouse::execute` is synchronous and
blocking, so a deadline that fires in the caller leaves the leg running inside the driver - each system
has its own interrupt, and none of them is reachable from a timeout wrapped around the call. **So the
deadline travels on the port**, beside the credential 0008 adds, and each adapter cancels for real with
its own mechanism. That is one more reason the credential and the deadline are hoisted into the leg set
in one diff rather than two.

**And the cost of deciding it that way, because it constrains what an earlier branch may claim.** Until
the port carries a deadline the bound is **"stop waiting"** and not "cancel". So `feat/query-bounds`
ships the configuration, the boot check and the refusal, and may NOT name a test for cancellation;
`feat/credential-port` is where the port gains the deadline and the first place a test may assert that
an execution stopped. A cancellation-named test in the earlier branch would assert a timeout and read
as proof of an interrupt, which is exactly the shape of an overstated control this file keeps deleting.

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
- **Compose:** Oracle, Postgres, Datahub, OpenMetadata, brought up on demand, ONE INSTANCE PER
  WORKTREE, provisioned through `xtask` rather than the shipped binary - docker orchestration in a
  release artifact is test scaffolding shipped to users, and `xtask` is never packaged. Concurrent
  worktrees must not collide on published ports, the compose project name, container and network
  names, or named volumes, and the isolation is **per-worktree naming plus ALLOCATED-and-discovered
  ports**: publish ephemerally, let docker and the operating system pick a free port, read back what
  was actually bound, and write it into a discovery file inside that worktree which the harness is the
  only way to read. No test holds a port constant, because the harness offers no constant to hold.
  Docker is a host dependency and is deliberately not pinned by nix.

**A port DERIVED from the worktree path was the earlier decision here, and it is WITHDRAWN.** Two
defects, and they are the same shape as the ones this file keeps finding rather than two unrelated
mistakes. Hashing a path into a port range is a total function from an unbounded set into a finite one,
so it cannot guarantee two worktrees get disjoint blocks; a corpus test over sample paths can only fail
to find a collision, which is not the same property. And the check meant to cover the residual -
refuse to provision if a port in the block is already bound - is check-then-bind, with the whole window
between the two open to anything else on the host. Both read as a guarantee and deliver a probability.
Letting the allocator allocate has neither defect, and it costs exactly one thing: a fixed port a
developer could memorise. The discovery file replaces it with a value that is correct rather than
remembered, and a `just` task that prints the endpoints costs a line.

**The compose (docker) tier cannot be a nix check.** A docker tier needs a network and a docker
socket, and a nix build sandbox has neither - which is why `just validate` builds without network.
A service needing neither, like the Unix-socket Postgres tier the adapter hosts inside its check,
CAN be one (`nix/postgres-tier.nix`). The docker tier is a CI job and a `just` task that CONSUME
nix-built artifacts. The strongest version of it runs the OCI image that actually ships.

**Amended for a nix-native service.** All the port machinery above - derived project names,
allocated-and-discovered ports, the withdrawn hash-to-port scheme, the race analysis - exists
because docker publishes TCP ports. A nix-native service (`nix/postgres-tier.nix`) is reached over
a unix socket, so it has no port: no allocator, no collision, no race, no per-worktree name. The
discovery file is written the same way (host = the socket directory), so a harness cannot tell the
two apart, and `just update` moves the pinned minor for the sandbox and a developer machine
together. Because a unix socket path caps around 100 bytes on macOS, the server lives in a short
per-worktree directory under `$TMPDIR`, keyed by a hash of the worktree so two worktrees cannot
clobber each other; only the endpoint file lives in the worktree, where the harness looks. The
`sutura-postgres-tier` script is the ONE provisioner: `checks.nextest` runs it in the sandbox over
`$NIX_BUILD_TOP`, and `just test` runs it in the dev shell - the same derivation, so the two cannot
drift. Postgres is NOT a compose service: no `dev-up` entry, and no `Provisioner` variant on the
`Services` table's rows (the discovery FILE still carries a per-file `provisioner` string, so a
harness can tell docker-written from nix-written endpoints). Docker orchestrates docker, and
Postgres is not docker.

**Absent docker is a SKIP locally and a FAILURE in CI, and those are not the same default.** An earlier
version of this record said the tier prints SKIPPED and exits 0, and said in the same breath that a
missing service cannot silently pass. Both cannot hold on one machine class. So: on a developer machine
a missing docker skips loudly, naming what was not run, because docker is a host dependency this
repository deliberately does not pin and a contributor without it must still be able to work. In CI the
same absence FAILS, because there the tier is the only thing standing behind a network adapter and a
green run that quietly tested nothing is the failure this whole tier exists to prevent. The mechanism
is one flag read from the environment, and the direction each way is written where the flag is read.

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

**The numbering lives in `docs/implementation-plan.md`, not here.** An earlier
version of this section numbered its own steps 0 to 8 while the plan numbered its branches differently
and 0007 numbered a third set 1 to 8, so "step 5" meant three things depending on which file was open -
two owners too many for one artefact, which is the thing this repository has a table about. So this
section keeps only what is a *decision*, names each step by the branch that carries it, and cites no
number at all:

| Branch | Done when |
| --- | --- |
| `feat/agent-surface` | One tool - ask a certified question - over the agent transport, its schema kept equal to the domain type by a test, and an uncertified question refused as a RESULT. **First, because it is the API we expose**, and thin because every other branch rebases on it |
| ~~`docs/inbound-identity`~~ | **DONE:** [how a caller proves who it is](0014-how-a-caller-proves-who-it-is.md). Two inbound modes with no default, the audience validated against our own resource identifier whatever the client sends, ceilings from scopes rather than from the question, and the two-exchange chain the direct mode needs. What it leaves open is named there: whether an enterprise identity provider can mint an ID token for a third party's audience, and where scopes are authored |
| `feat/agent-surface-scope` | The rest of the tool set, one schema source for both transports, and advertisement filtered by scope. Both properties need more than one tool to be testable. **Blocked on `docs/inbound-identity`**: a scope filter is authorization, and today's bearer gate authenticates the deployment rather than a caller, so there is no verified scope to filter on until that record exists |
| `feat/federation-decomposability` | A new aggregate cannot compile without stating how it federates; a ratio divided per leg is impossible rather than discouraged |
| `feat/principal-chain` | The chain is the key everywhere a subject is recorded, with both tail positions absent and no reader that assumes one position |
| `feat/query-bounds` | Each bound provokes its own refusal in a test; the working-set bound is shown biting rather than described |
| `test/startup-source-refusals` | Red against a build with the more-than-one-source arm of `open_engine` removed. **That arm has a test now, and it changed shape as well as gaining one**: the refusal is per-KIND rather than per-source-count, and `crates/sutura-serve/src/tests.rs` asserts on its own sentence - *one kind of data system at a time*. Two sources of one kind load and both open, which is what `docs/adr/0006`'s correction records |
| `feat/source-registry` | A duplicate alias, a missing file and a relative path each refused at parse, asserted on the variant. The shared-identity ACKNOWLEDGEMENT is a `NotFitToServe` variant from `sutura-config`'s own `Settings::refusals`, because that is the half configuration can see; the cross-check against whether the linked ADAPTER can carry a per-subject credential at all is a startup refusal in the composition root, because that is a property of the build rather than of the file. Each source's entry also carries the verification identity the anchor path runs under |
| `feat/leg-plan-types` | `LegPlan` exists as a closed TWO-variant domain type, rendered through one `sutura-sql` entry point with a golden family, and `Warehouse::execute` takes an `Executable` so every adapter's match over what it can be handed is exhaustive. [0007](0007-federating-across-different-data-systems.md) owns that shape and this row does not restate it. **An earlier version of this row said this is the branch that moves the definition digest, and that was false**: `DefinitionDigest::of` takes `&Definitions` and `&Knowledge` and nothing else, so no plan type is under the digest at all. The reason this is its own branch is that a port signature change touching every adapter and every fake should not land in the same diff as a combiner |
| `feat/two-source-execution` | Rows equal to the single-source corpus; each leg's statement snapshotted; a filtered remote dimension over an orphan key correct; a `CountDistinct` across sources EXACT, by transporting the distinct keys and counting above. Decision 2 decides the pull-up, so "or refused" is not an acceptable outcome here and an earlier version of this row that allowed it was the withdrawn decision surviving in a table |
| `feat/conformance-packs` | Adding a source touches a registry and a declaration, never a test function. The compile half needs no data system and lands early |
| `feat/credential-port` | No signature exists that can run as the process; what is hoisted out of the legs is the asker and the deadline, in one field each, so N legs cannot disagree; what differs per leg is a VARIANT rather than a field, including the third one that carries no credential material at all and says the leg ran under the deployment's own identity; the port carries the deadline so an adapter can cancel for real, which is the first place a test may assert that an execution stopped; and a second port method runs an anchor under a verification identity `answer` cannot construct. **ONE new refusal variant, not two** - a subject with no credential at a source. `SourceCannotImpersonate` is deleted rather than scheduled, because [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 6 walks every configuration that was supposed to reach it and each one is a boot refusal, an `Err` for a wiring defect, or the decided permitted behaviour. The broker port arrives with a REAL implementor rather than a fake - the static-credential broker single-user mode already needs. **Blocked on `docs/inbound-identity`** - Decision 1 says guessing the topology produces a port with the wrong signature, and this is the port |
| `feat/plan-spans-two-identities` | The plan-stage refusal is re-keyed from *a plan spanning two sources* to *a plan spanning two identities*, which is the property that was always meant. **The LAST assumption to move**, and it cannot move earlier: there is nothing to key on until a principal type exists, and moving it before then is the diff that produces a wrong number rather than a refusal. It also retires an AGENTS.md invariant row - deleted rather than demoted, per the rule at the head of that table - and makes the prompt guide for that refusal wrong, so the guide moves with it |
| `feat/compose-tier` | Two worktrees provision simultaneously without collision - one instance per worktree, ports ALLOCATED by docker and the operating system and read back into a discovery file, compose project name, container names and volumes per worktree, and no constant a test could hold; absent docker skips loudly on a developer machine and FAILS in CI. It comes BEFORE the first network adapter, because it stands up the source that adapter is tested against |
| `feat/postgres-adapter` | The whole existing corpus green against a containerised Postgres on a static credential, and the artifact question - which shipped binary links a native driver - answered in code. [Track 1](0007-federating-across-different-data-systems.md), and it de-risks the step after it |
| `feat/postgres-oauth` | Two subjects, different rows, in the compose tier |
| `feat/source-mtls` | Three states per source each tested - none, TLS with verification, mutual TLS - and the middle one, the one that gets forgotten, has a test of its own. [Transport security for a source](0010-transport-security-for-a-source.md) is the record, and it was missing from this table while its record was listed above as one of the seven this order carries out |
| `feat/raw-sql-tool` | The ungoverned tool [a raw SQL tool, off by default](0013-a-raw-sql-tool-off-by-default.md) decides: absent without its scope, a result type with nowhere to put a definition digest, read-only enforced by the source's own role rather than by inspecting the statement, and the ungoverned share reported as a number. Late in the order because every one of those needs something earlier - the scope gate, a source registry, and a credential port that can hand it a read-only role and a deadline |
| `feat/demo-tasks` | Every deployment variant has a working example, and one that drifts fails a test rather than misleading a reader |

**Two branches in `docs/implementation-plan.md`'s table are deliberately absent
from this one, and naming them is cheaper than leaving a reader to diff two tables.**
`build/supply-chain` and `ci/prose-change-cost` carry no decision from any record this order executes -
one is release plumbing and one is a CI measurement - so they are scheduled in the plan and have nothing
to be *done when* about here. Every other row of that table appears here.

**One branch that was in this table is no longer in it.** `ci/service-category-selection` - one CI job
per service category, selected from the diff - is DEFERRED rather than scheduled, and the reason is a
claim of its own that did not survive being checked: it was justified partly on `xtask`'s fail-open
behaviour covering a new adapter nobody registered, and `xtask/src/changes.rs` has one `Area` named
`rust` whose patterns include `crates/**`, so a new adapter crate MATCHES and the unmapped-path branch
is never reached. The property that sold the design cannot be exercised by the case it was sold for. It
comes back when there are at least two real service categories to select between and a measured runtime
that a selector would actually reduce - which is a measurement nobody can take before the tier exists.
`docs/implementation-plan.md` records what survives of the design for whoever
picks it up.

Two properties of that order are the decision rather than the schedule. **The agent surface is first**,
because it is the interface everything else is judged through and building it last means every earlier
step guessed at its shape. And **six of the first seven rows need no decision from anyone and touch no
adapter**, which is what makes them safe to start immediately. The seventh, `docs/inbound-identity`, is
the one that IS a decision, and it is early precisely because two later branches are blocked on it.
`feat/postgres-oauth` is where the identity story becomes real, and the first step that cannot be
verified without a live service.

## Found on a second pass, and not yet carried anywhere

These are gaps in the design as it stands, not open questions about the world. Two of them are cheap
now and impossible to retrofit, and they are marked.

- **The principal is a chain, and this is now DECIDED rather than open.** The target is stated:
  permissions for a specific agent, of a specific human, for a specific task. So the shape is human
  then agent then task, ordered, which is also the shape a token exchange maps onto rather than being
  translated into. Build it in `feat/principal-chain` while both tail positions are always absent:
  record the chain wherever a call is recorded, put the task on the request context, and make the chain
  the key a budget would use IF one existed. **There is no budget port**, and the bullet further down
  says so rather than this one implying otherwise - what this branch builds is the key, not the budget.
  **The reason it cannot wait:** a record that names only the subject can never later be told apart
  from one that meant "an agent acting for" them, and by the time anybody wants to tell them apart the
  records are already written.
- **Attenuation.** A task's credential is minted NARROWER than the subject holds - fewer metrics, a
  shorter lifetime, a smaller budget - and monotonically so, never broader. That is what makes an
  agent identity more than bookkeeping, and it is why a credential port takes the whole context rather
  than a bare subject: the broker is handed the chain, because the chain is what an attenuation
  decision is about. **What the broker HOLDS is not what a leg EXECUTES as, and an earlier version of
  this bullet ran the two together** - it said the chain is what `feat/credential-port` hoists out of
  the legs, which contradicted
  [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) and the plan, both of
  which hoist the ASKER. 0008 is right and the reason is in the source table above: no data system's
  identity model except Oracle's proxy authentication has anywhere to put "an agent acting for a
  human", so what a leg runs as is a subject, and `LegCredentials` names that field `asked_by` rather
  than `subject` precisely so it cannot be read as a claim about every leg. The chain lives on the
  caller and in the record written per call; the subject lives on the leg. **Where the attenuation
  policy lives is NOT decided, and this bullet says so rather
  than reading as a mechanism it does not have.** Two homes are possible and they differ in who is
  trusted. Scopes on the token the exchange returns: the authorization server decides, and sutura only
  requests and verifies, which keeps every authorization decision outside this system. Or deployment
  configuration read at boot: sutura decides, and then the claim that sutura makes no authorization
  decision needs restating to say it makes none about DATA and one about the agent. The catalog is not
  a candidate - a governance decision arriving through a content channel is the shape every other
  decision in this record refuses. Whichever is chosen, one property is not negotiable: an attenuation
  step can only narrow, and a step that could widen is a shape to make unrepresentable rather than a
  case to check.
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
- **A record per call, written before the outcome returns - and no retention obligation of our own.**
  Those are two claims, and an earlier version of this bullet collapsed them into one - "sutura keeps
  no audit archive... emit it, do not store it" - which put this record in contradiction with
  [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md). That record concludes
  that because the available exchange semantics express impersonation rather than delegation, the
  principal chain exists nowhere downstream, so recording it cannot be delegated to the data system.
  It is right about that, and this record is right that we inherit no retention duty. Both halves,
  stated separately, because that is what was wrong before:
    - **Sutura WRITES the record.** One per call, refusals included, carrying the whole chain, written
      before the outcome returns rather than after - a record written after the response is the record
      a crash loses, and the call worth having a record of is the one that went wrong. It is a
      **different channel from provenance**: provenance rides with the result and a client is free to
      drop it, and a record only the caller holds is not a record. So the deployment attaches a sink
      and sutura writes into it. AGENTS.md says a port trait arrives with its first implementor, so the
      sink port arrives with the one implementor that needs nothing from anybody - a structured writer
      over the tracing subscriber this repository already composes - and `feat/principal-chain` carries
      both, because that branch's entire justification is a record that can be told apart later. Today
      the only thing that records a call is one `tracing::info!` per outcome in
      `crates/sutura-http/src/routes/v1/query.rs`, whose own doc comment says there is no audit sink
      and nothing records a principal chain. That is the honest starting point, not a gap to describe.
    - **Sutura RETAINS nothing.** No archive, no rotation, no retention window, no query interface over
      past calls, and no obligation inherited from any of those. Under impersonation the sources audit
      each read under the asking subject, and a single-user deployment is a development and
      proof-of-concept shape. **The limit, next to the claim:** everything after the write belongs to
      the deployment - where the records go, how long they are kept, who may read them, and whether
      they satisfy an obligation the deployment has and sutura does not. A deployment that attaches no
      sink gets the tracing default and keeps whatever its log pipeline keeps, which may be nothing.
      That is a choice we make legible rather than one we make for anybody. **And the limit, in the
      direction that costs us:** an emitted record is worth what the sink behind it is worth, and
      sutura cannot vouch for a sink it does not retain. A deployment that attaches a sink which drops
      records, or attaches none, has no audit trail on this side and nothing here can tell it so -
      which is precisely why the sources' own logs, written under the asking subject, carry the part of
      the obligation that matters. **`AuditSink` now exists** - shipped by `feat/principal-chain` with
      `TracingAuditSink` as its first implementor - so the split this bullet argues for is built rather
      than proposed: sutura writes before the outcome returns and retains nothing.
  What a record is worth beyond attribution: refusals are the demand signal for which questions have no
  certified answer, and the ungoverned-share number
  [a raw SQL tool, off by default](0013-a-raw-sql-tool-off-by-default.md) reports is computed over the
  same stream. Both read what is written; neither needs an archive here, and neither can be computed if
  nothing is written at all - which is why "emit, do not store" was too weak a position to leave in a
  record three other records depend on.
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
- ~~**Whether Oracle can execute as the asking subject.**~~ **DEFERRED, and the posture is decided:**
  an Oracle source declares `SharedServiceUser` only.
  [A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) carries the whole finding -
  the capability is present in the database, in the C interface and in the bindings this workspace
  already depends on, and **no production-viable Rust crate exposes it**, Oracle's own official driver
  included. So this is a missing safe wrapper rather than a missing capability, the enforcement is the
  startup refusal that already exists for an adapter that cannot impersonate, and the consequence is an
  operator obligation: keep critical data off that source in multi-user mode, because sutura declares no
  data sensitivity and cannot see which dataset is critical.
- **Oracle's dialect.** `dialect-oracle` exists upstream as an empty feature, but the rendering it
  needs - a row limit that is not `LIMIT`, a date truncation that is not `DATE_TRUNC`, a parameter
  marker that is not `?` - lives behind the transpile feature this workspace does not compile. Oracle
  is a generator question before it is an adapter question.
- ~~Who declares a dataset critical~~. **Decided: nobody here.** Sensitivity lives in the data catalog
  and in the asking person's permissions at the source, which is what impersonation is for. Sutura
  declares a source MODE and records which mode produced an answer.
- ~~Audit~~. **Decided above, in *Found on a second pass*:** sutura writes one record per call
  including refusals, carrying the chain, into a sink the deployment attaches, and retains nothing. The
  port and its first implementor land in `feat/principal-chain`.
- **The budget.** Named in *Found on a second pass* above and in the identity record, with no port here
  and no branch in the plan. The chain is the key it would use, which is why `feat/principal-chain`
  builds the key and stops there.
