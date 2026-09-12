---
title: An authenticated metrics endpoint, and the label that cannot be forged
description: Prometheus over the existing listener with its own credential rather than the deployment token, because an API token holder can ask any question the catalog certifies and a scrape needs none of that. A hand-rolled registry in sutura-runtime, labels typed as &'static str so caller text is unrepresentable as one, the memory series absent rather than zero until an engine pool exists, and what is deliberately not exported.
---

# An authenticated metrics endpoint, and the label that cannot be forged

Status: **accepted as a design. Nothing here is built.**

Verified absent before deciding anything: no `prometheus`, `metrics-exporter-prometheus`,
`opentelemetry` or `sysinfo` in any manifest or in `Cargo.lock`; no `/metrics` route; and **no
`AtomicU64` or `AtomicUsize` anywhere in the workspace**, so the counters here are the first. The
absence is already deliberate in three places - `sutura-runtime`'s and `sutura-http`'s module docs and
`docs/serving.md` - and this record replaces that silence with a shape.

**Corrected on the last clause only, and the rest of that sentence still holds** - which is why it is
narrowed rather than struck. There is one `AtomicU64` in the workspace now: the correlation counter in
`crates/sutura-http/src/correlation.rs`, and an `AtomicUsize` in test fixtures. So the counters this
record specifies are the first **metric** counters, not the first atomics. The four candidate
dependencies are still absent from `Cargo.lock` and there is still no `/metrics` route, so the status
line above is accurate - this is the one record in the set whose *Nothing here is built* is still true.

## Decision 1: its own credential, never the deployment token

`docs/serving.md` states plainly that a holder of the API token *"can read the whole catalog and ask
any question the catalog certifies… There is no way to give one caller less than that."*

**So reusing that token for a scrape hands the monitoring system the ability to interrogate the
business.** A scrape needs to read counters. That is a privilege escalation into a monitoring
credential store, and monitoring credential stores are not where a data-access secret belongs.

`security.metrics_token` gates `/metrics` and nothing else, following `AccessToken`'s pattern exactly:
an RFC 6750 `b64token`, at least 32 characters, compared through the existing
`matches_in_constant_time` - which SHA-256s both sides before `subtle`'s constant-time equality, so
there is no length oracle. **One comparison implementation, reused rather than copied.**

Three startup refusals, each preventing a silent collapse of the separation:

| Refused at boot                                                                     | Why                                                                                                                                   |
| ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `metrics_token` equal to `access_token`                                             | Collapses the separation this record exists for, and nothing at runtime would show it                                                 |
| `/metrics` enabled with no `metrics_token`, in production or on a non-loopback bind | The existing `AccessTokenRequired` argument verbatim: the alternative is an unauthenticated way to read whatever the process can read |
| Registry initialisation failing                                                     | A `200` carrying half the series is worse than a process that did not start                                                           |

**The limiter sits OUTSIDE the gate**, as `sutura-http`'s router already requires and for the reason
recorded there: with the gate outermost a wrong-token attempt never cost a limiter cell, which made a
32-character shared secret guessable in an unlimited loop. The same bug is available on a new tier, so
the same ordering applies.

## Decision 2: one listener, and the trade-off stated

`/metrics` lives on the existing listener, outside the version prefix - the same argument `/health`
already makes, that a scrape config must survive a version bump without reconfiguring the monitoring
system.

**Not a second listener, and the cost of that choice is real:** a second `TcpListener` is a second
drain to keep in agreement, a second TLS decision and a second bind-address refusal. `docs/serving.md`
already warns that a second serving implementation means two drains.

**Where a second listener WOULD be right**, so this is a trade-off rather than an assumption: a
deployment that must expose the API on a routable address while keeping `/metrics` on loopback or a
cluster-internal interface, because the network is the only control it trusts. That wants
`observability.host`/`observability.port`, its own startup refusal, and the drain becoming one
`JoinSet` over both listeners so `stop()` still spends one budget. **The credential separation above is
what defends the surface; the network separation is defence in depth an operator may want.**

## Decision 3: hand-rolled, no new dependency

Roughly fourteen series. The Prometheus text exposition format is stable and line-oriented; every gauge
is an atomic load; the two histograms need a fixed bucket array and cumulative counters.

**The reason is specific to this case rather than general asceticism.** A facade crate arrives with a
global recorder, a macro layer, and a label API typed as `String` - which is precisely the cardinality
hole Decision 5 closes. Taking the dependency would mean adding the hazard and then guarding it.
`AGENTS.md`'s newtype section already argues the shape: write the boilerplate by hand first.

**Named alternative, if a histogram implementation is wanted rather than written:** `prometheus-client`,
whose `Family<Labels, Metric>` with derived label sets is the closest thing available to Decision 5's
mechanism. Its transitive tree has **not** been measured against this workspace, and `check-boundaries`
would not object because it guards `sutura-domain`'s tree only - so nothing mechanical would catch a
heavy tree here. Measure before adopting. Do **not** take `metrics` plus
`metrics-exporter-prometheus`: its exporter carries its own HTTP listener, which is Decision 2's second
drain arriving through the back door, and its label API is `String`-typed.

**Counter width is not a failure mode:** `u64` at ten questions a second is about fifty-eight billion
years. Say so rather than adding a wrap check.

## Decision 4: `sutura-runtime` owns the registry, behind a default-off feature

`sutura-domain` is excluded mechanically - `ALLOWED_IN_DOMAIN` is twenty-eight crates and a registry is
not among them, so adding one is an architecture decision by that gate's own doc comment.

`sutura-runtime` is right for the same reason it already owns `Admission`, and that module says it: the
resource it bounds is the process, and *"two independently sized semaphores would be two controls each
reporting a limit that the other can exceed."* A second registry has exactly that failure mode. It
already holds the subscriber, the panic hook, the shutdown flag and the admission semaphore - every
number worth exporting except the engine's.

**No port into the domain, and this is a deliberate refusal.** `sutura-domain`, `sutura-app`,
`sutura-semantic`, `sutura-sql`, `sutura-catalog-local` and both execution adapters contain **zero**
`tracing::` calls today, and every number below is readable at the transport or from `sutura-runtime`'s
own types. `AGENTS.md`'s rule is that a port arrives with its first implementor; there is none, so the
port is not written. If one is ever needed it is a sink taking a closed enum of unit-carrying variants
and holding no `String`.

## Decision 5: a label is `&'static str`, so caller text is unrepresentable as one

**The mechanism already exists and this record only has to use it.** `sutura_http::wire::refusal`'s
`refused` is a wildcard-free exhaustive match returning a `&'static str` code per refusal, and
`problem.rs` does the same for failures. Their own doc comment anticipates this use: *"grouping them is
what lets a monitor count attempts to ask outside the catalog as one number."*

So the label parameter's type is `&'static str`, and the only values ever passed are those two existing
accessors. **A caller's text is not `&'static str`, so it cannot be a label.** No fourth match, nothing
to keep in step.

**The trap this closes, which is not obvious.** `MetricName` accepts any identifier-shaped string up to
the length cap, and a caller may send a legal-but-unknown name and receive `metric_unknown`. So a
`metric` label would be **caller-controlled and combinatorially unbounded** - a memory leak in the
scraper and a live record of what people ask, mintable by anybody holding a token. Metric names look
bounded by the catalog and are bounded only on the answered path.

**Second layer, because the first bounds only what a CALLER can inject:** a test that exercises every
route and provokes every `RefusalReason`, renders the registry, and asserts the **exact series count
and the exact set of names**. This is the idiom `AGENTS.md` already records for the SQL goldens, where
`check-guidance` counts them and fails when the number drifts. A label added anywhere moves that number
and fails the test.

**State the limit.** Layer one bounds caller injection completely, by construction. It does not bound
the product of label dimensions - two closed five-valued enums on one family is twenty-five series, and
that is somebody's choice rather than a caller's. Layer two catches that. **There is no
`check-boundaries`-style gate for this and none is claimed:** that tool reads dependency direction,
`pub` fields and `Result<_, String>`, and nothing anywhere reads a label construction site.

## The series

| Metric                             | Type       | Labels                                                            | Why an operator needs it                                                                                                                                                                                                     |
| ---------------------------------- | ---------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sutura_questions_total`           | counter    | `code`                                                            | Rate, error rate, and the governance-versus-fault split. **A refusal is a correct outcome and must not page anybody**; `unavailable` and `internal` must. Nothing else separates them                                        |
| `sutura_question_duration_seconds` | histogram  | `outcome`                                                         | Whether the deployment is slow. Buckets chosen so the request timeout and the admission window both fall inside                                                                                                              |
| `sutura_execution_slots`           | gauge      | -                                                                 | The capacity denominator                                                                                                                                                                                                     |
| `sutura_execution_slots_in_use`    | gauge      | -                                                                 | **The most important load number here.** A slot is held until the WORK finishes, not until the caller is answered, so this is true in-flight work - which a request counter is not                                           |
| `sutura_admission_shed_total`      | counter    | -                                                                 | Separates "busy" from "shedding", which is what decides replicas versus bounds                                                                                                                                               |
| `sutura_admission_wait_seconds`    | histogram  | -                                                                 | Eight of eight slots with no wait is healthy; eight of eight with a four-second p99 is about to shed. Slots alone cannot tell you which                                                                                      |
| `sutura_rate_limited_total`        | counter    | `tier`                                                            | The abuse signal. `docs/serving.md` says deliberately that an unauthenticated caller can consume quota, and this is how an operator sees it                                                                                  |
| `sutura_rate_limit_buckets`        | gauge      | `tier`                                                            | The limiter's keyed store grows and is swept on an interval. This is its memory signal, and `LimiterHandle::tracked()` already exists and is read by nothing                                                                 |
| `sutura_unauthorized_total`        | counter    | -                                                                 | Credential problem or attack. **No address label** - that would be an access log                                                                                                                                             |
| `sutura_answer_rows`               | histogram  | -                                                                 | How close answers run to the row cap, which is a real capacity question. A histogram discloses a distribution, never a question                                                                                              |
| `sutura_catalog_metrics`           | gauge      | -                                                                 | The governed-coverage number [the raw SQL tool](0013-a-raw-sql-tool-off-by-default.md) depends on: *coverage is reported* is one of its three ramp mechanisms, and a deployment where it does not rise has learned something |
| `sutura_engine_worker_threads`     | gauge      | -                                                                 | `docs/serving.md` warns the default is `available_parallelism`, which under a CPU quota reports the host's cores and is usually too wide. Today that number exists only in a startup line that has scrolled away             |
| `sutura_build_info`                | gauge (=1) | `version`, `catalog_version`, `definition_version`, `environment` | Correlates "the numbers changed" with "the catalog changed"                                                                                                                                                                  |

**`definition_digest` is deliberately not a label.** Sixty-four hex characters that change on every
catalog edit, leaving a stale series per deploy. It is already in every answer's provenance and in the
log; `catalog_version` plus `definition_version` is what an operator can act on.

## What is deliberately not exported

- **Anything derived from a question's text** - metric name, dimensions, grain, filter values, range.
  Decision 5 is why.
- **Caller address in any form.** The one field that turns `/metrics` into an access log.
- **Source names.** Bounded by the catalog, so this exclusion is about **disclosure rather than
  cardinality**: it names the deployment's data systems.
- **Process RSS and CPU seconds.** Every container runtime reports both per-container with better
  fidelity than a self-report, and adding them needs a dependency or a platform read. **The useful
  memory number here is pool reservation against the ceiling; RSS is the orchestrator's job.**
- **Shutdown grace remaining.** `remaining_grace()` exists, but a scrape during the drain races process
  exit and the number is meaningful for at most fifteen seconds.

## The memory series, and why they are absent rather than zero

Three series - reserved bytes, the limit, and refusals - are specified and **must not ship yet**.

**Verified: there is no engine memory pool.** No `RuntimeEnv` is constructed anywhere in the workspace,
so DataFusion installs its unbounded pool. `SessionContext::new()` and `new_with_config` are the two
construction sites and neither sets one.

**Both of those absences are corrected further down this section, and the word *Verified* above is
why the pointer is here rather than only there:** a reader who stops at it does not go looking.

So a gauge reading `0` would be a lie an operator builds an alert on. **Register the series only when a
pool is configured: absent, not zero.**

What has to exist first is `feat/query-bounds`: a byte newtype for the ceiling, a `RuntimeEnvBuilder`
with the pool retained on the warehouse behind an accessor - the struct's fields are private today and
its hand-written `Debug` exposes only the source - and a new `RefusalReason`, because **resource
exhaustion is currently indistinguishable from a dead data system**: it arrives as
`DataFusionError::Execute` and leaves as `503 unavailable`, so a caller is told to retry against a
bound that will fire again. `ResourcesExhausted` appears nowhere in the workspace.

**Corrected: both verified absences above are now present, so the precondition this section waits on
is met and the series are still unshipped.** `crates/sutura-exec-datafusion/src/pool.rs` builds a
`RuntimeEnv` with a `GreedyMemoryPool` sized from the configured ceiling - that file's own module doc
states the same fact in the past tense, correctly. `ResourcesExhausted` exists as a `RefusalReason`
variant, is raised on the federated path and maps to `422`, so exhaustion is no longer
indistinguishable from a dead data system. **What this does NOT change is the decision**: the series
are still specified and still must not ship, and *absent, not zero* still holds, because the reason
was never the missing pool alone - it is the sentence two paragraphs down, which is unchanged and is
the one to read next to any gauge that does ship: the pool bounds the engine's own operators and
nothing else.

**Take the ceiling from the configured value, not from the pool.** `MemoryPool::memory_limit()` defaults
to unknown, so a pool that does not override it reports no ceiling and the ratio an operator wants is
unavailable.

**And state the limit next to whatever ships:** the pool bounds the engine's own operators and nothing
else. Not what a driver buffers, not `collect()` materialising every batch, not the row set built in the
conversion loop. **Pool-reserved is not process memory and must not be alerted on as if it were.**

## Availability: a scrape must not make the service work

- Render is O(series) and constant: atomic loads, one semaphore read, two limiter reads. **Nothing
  touches the catalog, the plan path, the engine or a data system.**
- **The metrics route's state type does not contain the `Surface`.** Stronger than a test: the handler
  cannot answer a question because it does not have the means, and a future change that wanted to would
  have to change the type.
- **Not behind the admission bound**, because an operator watching the drain needs it to keep answering
  while slots are full.
- Its own limiter tier, defaulting to about one scrape per second. Prometheus scrapes every fifteen to
  sixty seconds; faster is not a scrape.
- **The route's span at debug, and excluded from `sutura_questions_total`.** A scrape every fifteen
  seconds at info is noise that buries the lines that matter.
- **No background task.** A pull-based registry avoids the trap `sutura-http`'s middleware already
  records: the router is assembled before the runtime exists, so a `tokio::spawn` there panics at
  startup and one guarded by `Handle::try_current` silently does nothing. This is a second reason to
  hand-roll.

## What a test asserts

1. **The gate is separate.** No token → `401`; **the API token → `401`**; the metrics token → `200`. The
   middle assertion is the whole security argument in one line, and it is what fails if somebody reuses
   the existing gate.
2. **The series set is exact** - every route exercised, every `RefusalReason` provoked, the count and the
   names pinned. Fails when a label is added.
3. **No question text reaches the body.** Ask for a sentinel metric name, get `metric_unknown`, scrape,
   assert absence.
4. **A refusal counts as a refusal** - the refusal code moved, `internal` and `unavailable` did not.
   This is what keeps a governance decision from paging somebody.
5. **The scrape does no work** - slots-in-use is zero across a scrape, and structurally the route's
   state cannot reach the `Surface`.

Tests 1, 3 and 4 are red before the change by nature. Test 2 is the awkward one - the series do not
exist beforehand - so it uses `test-causality`'s stated-evidence path rather than skipping it silently.

## Consequences

- The first inbound credential that is not the deployment token, and the first configuration key whose
  wrong value is a silent privilege change rather than a startup failure. Hence three boot refusals.
- A default-off feature, following the `tls` precedent: `--all-features` in every gate entry point is
  what keeps it linted.
- **`AGENTS.md` gains no row from this record.** The label mechanism would qualify - *no caller-supplied
  text can become a metric label*, enforced by the `&'static str` label type plus the series-count test -
  but the table's own rule is that a row arrives with its mechanism, and neither exists yet. It goes in
  with the code.
