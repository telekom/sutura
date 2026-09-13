---
title: Where a deadline lives
description: One absolute deadline per answer, opened by the transport and carried on the execution port, so every adapter can stop the data system rather than the caller's wait; running out of time as a typed refusal and not a retryable failure; and a federated answer sharing one instant across its legs with no division. Per adapter, what the data system enforces and what sutura only observes.
---

# Where a deadline lives

Status: **accepted; the first slice landed the record, the port signature and the refusal. Postgres
now stops the certified path on it - `SET LOCAL statement_timeout`, PR4 of `telekom/sutura#160` - and
its own raw SQL tool path is stopped by the connect-time ceiling, since `Warehouse::execute_raw`
carries no per-request deadline; the engine and BigQuery slices that make their own adapters honour
it are still to follow.** Four accepted
records lean on this being decided - [0007](0007-federating-across-different-data-systems.md),
[0008](0008-a-credential-per-leg-for-the-calling-subject.md) part 4,
[0009](0009-the-plan-from-one-source-to-many.md)'s third decision and
[0013](0013-a-raw-sql-tool-off-by-default.md)'s second prerequisite - and 0009 already says the
deadline travels on the port. What none of them decided is the shape, the outcome and the split of
one budget across legs. This record decides those three and nothing else.

## What is true today, measured

- `Warehouse::execute` and `dry_run` take a plan and a `&Presented` and no time budget. Nothing below
  a transport knows when to stop.
- `server.request_timeout_seconds` bounds what the **caller waits for**. On HTTP an outer layer
  answers `408`; on the agent surface one deadline wraps the admission wait and the answer. On both,
  the blocking task that holds the port call cannot be aborted, so a question keeps running and keeps
  its execution slot until the data system answers - the `invariants` rows for the bound say so.
- `RefusalReason` has no variant for *this question ran out of time*. The only way time surfaces is
  the transport's own `timeout` failure, written as *the question may still be running*.
- One adapter bounds its work, alone. `sutura-exec-bigquery` opens a `CallDeadline` per port call,
  spends it across the token exchange and the job, sends what is left as `timeoutMs` and
  `jobTimeoutMs` - the second is what cancels at the service, and stops billing - and refuses with
  `DeadlineSpent` when nothing is left. It cannot share one budget across the two port calls one
  answer makes, so `QueryDeadline::within_request_timeout` divides the request timeout by a
  `CALLS_PER_ANSWER` constant that describes `sutura_app::answer`'s call pattern from another crate,
  and its own note says nothing checks that number.
- The engine adapter runs each question under `block_on` on a runtime built **without a timer**, and
  nothing in it reads a clock. The Postgres adapter sets one `statement_timeout` at connect, from a
  development tuning value, not from any request.

So: a long question is bounded on BigQuery by that adapter's own arithmetic, unbounded on the
engine a release ships, and unaccounted for across the legs of a federated answer.

## Decision 1: the port carries one absolute deadline, and each adapter stops the data system with it

```rust
// sutura_domain::warehouse - the port's own vocabulary, beside `Executable` and `Presented`.

/// The instant one answer's execution has to be finished by. Opened ONCE per request, by the
/// transport, at the instant the request arrived; shared by the pre-flight, every leg and the
/// re-check between them. What is left is a comparison against an instant the caller supplies -
/// the domain reads no clock, exactly as `Expiry::passed_by` reads none.
#[derive(Debug, Clone, Copy)]
pub struct Deadline { opened: Instant, budget: Budget }

impl Deadline {
    pub fn opened_at(opened: Instant, budget: Budget) -> Self;
    /// What is left at `now`, or `None` when it is spent. Never a zero duration: zero means
    /// *no timeout* to every client underneath, and handing it on would turn a spent budget into
    /// an unbounded wait.
    pub fn remaining_at(self, now: Instant) -> Option<Duration>;
    pub const fn budget(self) -> Budget;
}

/// How long one answer may execute. Parses: non-zero. The only production constructor is
/// `sutura_config::RequestTimeout::budget`, which is what bounds it by the request timeout.
pub struct Budget(Duration);
impl Budget { pub const fn parse(budget: Duration) -> Result<Self, NoBudget>; }

pub trait Warehouse {
    fn dry_run(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline)
        -> Result<PreFlight, Self::Error> { Ok(PreFlight::NotAsked) }
    fn execute(&self, executable: Executable<'_>, presented: &Presented, deadline: Deadline)
        -> Result<RowSet, Self::Error>;
    /// Was this failure the deadline, fired at the data system or found spent before the
    /// statement was sent? The fourth predicate beside `working_set_exhausted`,
    /// `result_did_not_fit` and `source_refused`, for their reason: `Self::Error` is the
    /// adapter's own type, and a predicate cannot mint a refusal.
    fn deadline_exceeded(&self, _error: &Self::Error) -> bool { false }
}
```

**A parameter, by value, on both request-time methods.** By value because it is two `Copy` words and
carries no secret; on `dry_run` as well as `execute` because against a networked data system the
pre-flight is a round trip that spends the budget, and a pre-flight with a budget of its own is how
one answer came to make two calls with two budgets. `verify_anchor`, `preflight` and `declared_key`
are the boot path - no request, no request timeout - and are unchanged: BigQuery bounds them with its
configured job bounds, the engine does not bound them, and this record does not claim otherwise.

**An instant, not a duration.** A duration has to be re-derived at every call site from a start
nobody carries, and the result is arithmetic - `CALLS_PER_ANSWER` is what that arithmetic looks like
when it has to live in another crate. An instant makes *what is left* a read. Monotonic
(`std::time::Instant`) and not a wall clock, for the reason `CallDeadline` already gives: a stepped
clock is a job abandoned early or one that outlives its caller. It is not recorded in the audit
record; the refusal carries the configured budget, which is the number an operator can act on.

**Opened by the transport at arrival, before admission.** The admission wait is inside the caller's
bound on both transports, so a deadline opened after `admit` would exceed what the caller was
promised by however long the wait took - the same defect the agent surface had for one release with
the reply deadline, found by reading both paths. On HTTP the layer that already holds the bound
opens it and hands it down as a request extension; on the agent surface the function that already
wraps both waits opens it; the command-line tool opens one from the same settings key. `Surface::answer`
takes it, so a transport cannot forget it - the same shape as the working-set ceiling, which reaches
`answer` as a parameter and not as a global.

**The budget is the request timeout minus a fixed reply margin, in one constructor.**
`RequestTimeout::budget()` is `request_timeout - REPLY_MARGIN` with the margin one second, and
`RequestTimeout::parse` refuses a timeout that does not exceed the margin - so the smallest accepted
request timeout becomes two seconds, and a one-second value is a startup refusal naming the margin.
The margin exists because the transport's own give-up stays where it is, as the backstop for an
adapter that does not honour the deadline, and two bounds at the same instant are a race the backstop
wins: the caller would see `408` for a question that was in fact stopped. Fixed rather than
proportional because what it covers - the cancellation reaching back through the driver, the audit
write, the response - does not scale with the timeout. Held by a test that adds the two back together
and by the parse refusal; the shipped default of thirty seconds becomes a budget of twenty-nine.

**Each adapter stops the data system with what is left, its own way.** The port makes the budget
unignorable - an adapter that does not read it has a parameter it does not read, visible in review -
and does not make the stop uniform, because it cannot: the interrupts are the data systems' own.

| Adapter                                 | Mechanism                                                                                                                                                                                                                                                                                                                                                                                                                | Enforced BY the data system                                                                                                                                         | Only observed by sutura                                                                                                                                                                                                                                                                                                                      |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sutura-exec-datafusion` (shipped)      | `tokio::time::timeout(remaining, rows)` inside `block_on`, on a runtime that now enables its timer; dropping the future drops the stream, and the engine's own spawned partition tasks abort on drop (`datafusion-common-runtime`'s `SpawnedTask`). The physical optimizer's default `EnsureCooperative` rule wraps every non-cooperative leaf so a stream yields per batch, and a yield is where a dropped future lands | The engine is this process, so *the data system* is the engine's operators: work stops at the next yield point, memory reservations are released with the operators | Planning (analyzer, optimizer) is synchronous inside the future and is not interrupted; one started blocking file read (one range of one file, `object_store`'s `maybe_spawn_blocking`) runs to its end; nothing after it is polled                                                                                                          |
| `sutura-exec-bigquery` (not shipped)    | `CallDeadline` opened FROM the port's deadline rather than from the configured job bounds; `timeoutMs` and `jobTimeoutMs` are what is left; `DeadlineSpent` before the send when nothing is                                                                                                                                                                                                                              | `jobTimeoutMs`: the service cancels the job and stops billing. This is the strongest enforcement in the table                                                       | The socket allowance (`CONNECT_MARGIN`) above what is left; the cancelled job's reply crossing the network inside the reply margin - when it does not, the caller sees the transport's `408` and the audit record carries the refusal                                                                                                        |
| `sutura-exec-postgres` (dev-dependency) | Certified `dry_run`/`execute`: `SET LOCAL statement_timeout = <remaining ms>`, scoped to a transaction each call opens and always rolls back, clamped to the connect-time ceiling (never widened past it); `SQLSTATE 57014 query_canceled` is the predicate. The raw SQL tool's own path (`execute_raw`) carries no per-request deadline - the port takes none there - so its `SET LOCAL` is always that ceiling         | The server aborts the statement itself                                                                                                                              | Two extra round trips per certified call (`BEGIN`+`SET LOCAL`, then `ROLLBACK`), themselves bounded by nothing but the socket; `57014` is also what an operator's manual `pg_cancel_backend` produces, and the predicate cannot tell the two apart; the raw tool is stopped by the ceiling, never by what is left of the asker's own request |
| `sutura-exec-duckdb` (dev-dependency)   | Nothing in the first slices. The driver exposes an interrupt handle; honouring the deadline needs a watchdog thread per call                                                                                                                                                                                                                                                                                             | -                                                                                                                                                                   | Everything. It is a test venue, and its column stays empty until somebody pays for the thread                                                                                                                                                                                                                                                |

**What is refused BEFORE the data system is asked**, in `sutura_app`, against the one instant: a
budget already spent when `dry_run` would be called, when `execute` would be called, or when the next
leg would start. This is 0008 part 4's *a leg that would start after `not_after` is not attempted*,
applied to the deadline, and it is where a slow admission wait or a slow first leg is caught. The
adapters check again on their own side - BigQuery does today - because an adapter may not assume who
called it.

**Not taken: per-adapter deadlines with no port change.** It is what exists and it is defensible: a
deadline the data system enforces is the only kind that actually cancels, and BigQuery's is one.
What it cannot do is share a budget across the calls one answer makes, so every adapter would carry
its own copy of `CALLS_PER_ANSWER` - a constant about `sutura_app` living in N crates, checked by
nothing in any of them - and an adapter that carried no deadline at all would be indistinguishable
from one that did. The port carrying it is what makes the omission visible.

**Not taken: an asynchronous port.** It would let the transport's own timeout drop the future and
cancel for free where the adapter's work is a future. It is a rewrite of every adapter and of the
composition roots that `block_on` a synchronous interior, for one property this record gets with a
parameter, and it would make the engine's `block_on` on its own runtime a nested-runtime problem.
0008 part 3 decided the interior names no framework; nothing here needs it to.

**Not taken: a cancellation token instead of a deadline.** A token would also carry a peer's
`notifications/cancelled`, which the agent surface delivers and nothing reads. It is the more
general shape and the more expensive one: every adapter needs a thread or a future watching it, where
a deadline is a number each data system already takes. The peer-cancellation half is a later change
that can arrive as a second reason the same predicate answers `true`; nothing here forecloses it.

## Decision 2: running out of time is a refusal, and not a `503`

`RefusalReason::DeadlineExceeded { budget_seconds: u64 }`, code `deadline_exceeded`, `422` on HTTP,
the refusal channel on the agent surface. Mapped in `sutura_app::answer` and in `execute_leg` from
the predicate, after `working_set_exhausted` and `result_did_not_fit` and before `source_refused`,
and produced directly there for a budget found spent before a call.

**Both ways, once.** For a failure: time is load-dependent in a way memory is not - the same
question under less load may finish, so *repeating this without modification will fail the same
way*, which is what `422` promises, is likely here rather than certain; a deadline is also what a
data system in trouble looks like from outside, and a refusal invites a caller to narrow a question
that was fine. For a refusal: the deployment decided the bound and the data system enforced it, so
something WAS judged - which is the line the agent surface draws for its own `timeout` failure, and
it falls on the other side here; a `503` invites an automatic retry that spends the whole budget at
the data system again, and on the one adapter where the budget is money it bills again; a `408`
already means *the transport gave up* and reusing it would make the two indistinguishable; and an
`Err` writes no audit record, so a question the deployment stopped at a configured bound would leave
no outcome anywhere. The retry loop and the missing record decide it, and they are the same two
reasons `ResourcesExhausted` and `ResultTooLarge` won this argument. The load-dependence is stated in
the sentence rather than denied: the guide tells an agent to narrow, and says a narrower question is
what changes the outcome.

**What the variant carries: the configured budget, in seconds.** A configured number, the same for
every caller, safe in a log, and the number an operator tunes - `ResourcesExhausted` carries its
ceiling for the same reason. Not how long the question would have taken, which nobody knows, and not
which leg spent it, which would tell a caller how the deployment's sources compare.

**The transport's own `timeout` stays, and now means one thing.** Before this record it was the only
statement about time. After it, `408`/the failure channel means *the caller's bound passed and this
deployment cannot say whether the question stopped* - which is true exactly when an adapter did not
honour the deadline inside the reply margin. The refusal means *it stopped, here is the budget*.

**Not taken: a failure with a new status.** A `504` would be honest about the class and is a 5xx,
so every client that branches on the class retries it. A `408` is taken.

## Decision 3: a federated answer shares one instant, and there is no division

Legs run one after another (0008 part 4), so the wall clock bounds their sum. The whole of the
answer's arithmetic is therefore: the same `Deadline` is handed to every leg, and before each leg
`sutura_app::federated::execute_leg` asks what is left. Leg 2 sees what leg 1 left, without anybody
computing a share. The refusal for a budget leg 1 spent names the same variant; which leg spent it is
in the audit record's leg entries and not in the caller's sentence.

**The combine is outside it.** `FederatedPlan::combine` is a pure function over rows already in
memory, bounded by the working-set ceiling and not by time. The last thing the deadline bounds is the
last leg; a combine that outruns the reply margin surfaces as the transport's `408`. Stated rather
than closed, because the combine has no `.await` to land a cancellation on.

**Not taken: dividing the budget per leg.** A share per leg is a number with an assumption in it -
that the legs are alike - and the wrong one refuses a question whose fast leg would have left its
slow leg plenty. It is `CALLS_PER_ANSWER` again, one level up.

**Not taken: the minimum over per-source overrides.** 0009 planned a global deadline with per-source
overrides, minimum over the sources a plan touches. No override exists in the settings tree today and
nothing needs one; adding the key ahead of a source that needs it is a bound nobody measured. When one
arrives it shortens the budget the transport opens and changes nothing on the port.

## What holds it, and what does not

| Claim                                                       | Held by                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Does not reach                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| No request-time execution runs without a deadline           | The two port methods take one and neither has a default body that could omit it; a `compile_fail` doctest for the call without one, with the compiling twin beside it                                                                                                                                                                                                                                                                                                                                                                                                                                      | The boot path: `verify_anchor`, `preflight`, `declared_key`                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Opened once per request, by the transport, before admission | Held by READING `router.rs`'s layer order (HTTP: `enforce_timeout` is layered outside the versioned subtree's rate-limit and token layers) and `server::answer` (MCP: `Deadline::opened_at` precedes `admitted(..)` inside the same `timeout`) - not by a test on either surface                                                                                                                                                                                                                                                                                                                           | No cell crosses the transport-to-app boundary; a transport that opened the deadline AFTER admission, or re-derived it inside the app, would not be caught                                                                                                                                                                                                                                                                                                                                                                                 |
| The budget fits inside the caller's bound                   | `RequestTimeout::parse` refuses a timeout at or below the margin; one test that `budget + REPLY_MARGIN == timeout`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | `Budget::parse` is `pub`, so a test or a third transport can open a larger one; what holds production is that one constructor and review                                                                                                                                                                                                                                                                                                                                                                                                  |
| The engine stops at the deadline                            | Not yet - the port carries the deadline and every adapter, the engine included, accepts and ignores it in this slice. The engine slice adds a test over a streaming source that yields until a stop instant well past the budget: the call returns inside the budget plus a tolerance, the predicate answers `true`, the source's drop flag is set                                                                                                                                                                                                                                                         | Everything, until that slice lands: a non-yielding operator, planning, and one started blocking read stay unreached even after it does                                                                                                                                                                                                                                                                                                                                                                                                    |
| Postgres stops the certified path at the deadline           | `SET LOCAL statement_timeout` sent before `dry_run`'s prepare and `execute`'s statement, inside a transaction this adapter opens and always rolls back; `57014 query_canceled` is `deadline_exceeded`'s predicate. A gate-reachable test on the Postgres tier (`crates/sutura-exec-postgres/tests/deadline.rs`): a statement cross-joined with `pg_sleep` under a small budget returns `DeadlineExceeded` well inside it, and a second cell reads `pg_settings.setting` for `statement_timeout` back through the same certified path, proving the per-statement value rather than the connect-time ceiling | The raw SQL tool's own statement: `Warehouse::execute_raw` carries no per-request deadline, so its `SET LOCAL` is always the connect-time ceiling, never what is left of the caller's own budget - `telekom/sutura#129`'s cancellation prerequisite is discharged for the raw tool only to that ceiling, not to the request's. `57014` is also what a manual `pg_cancel_backend` produces, indistinguishable to the predicate. The round trip that opens the transaction and sets the timeout is itself bounded by nothing but the socket |
| Running out of time is a refusal                            | Three exhaustive matches, `check-refusal-coverage`'s provocation, one app-level test through a fake whose error satisfies the predicate                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | The transport race for an adapter slower than the margin                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| A spent budget is not executed                              | An app-level test with a fake that records calls and a deadline opened in the past                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Nothing - but it is the weaker half; the data system stopping is the claim that matters                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

## Limits, stated once more where a reader will look for them

- **Leg 2 is untouched.** A deadline is a bound on how long a source works for the deployment's
  identity or the asker's; it says nothing about which identity executed. Nothing published executes
  as the asker.
- **Cancellation is at the data system's granularity, not instantaneous.** The table above says what
  each one is. None of the three is *a thread is killed*.
- **The reply margin is a fixed second.** It is measured against the engine and not against a
  network. On BigQuery the reply of a cancelled job may take longer, and the caller then sees `408`
  while the audit record says the job was stopped.
- **The DuckDB adapter observes nothing.** Its row is empty on purpose.
- **The raw SQL tool is stopped by a ceiling, not a request's own budget.** `Warehouse::execute_raw`
  carries no `Deadline` - a signature change the raw tool's own callers do not have yet - so a caller
  statement is bounded by whatever `SUTURA_DEV_STATEMENT_TIMEOUT_MS` this deployment configured at
  connect, the same number for every raw call regardless of how much of that caller's own request
  timeout was left.
- **A cost budget is a different bound.** Time and money are not interchangeable and the BigQuery
  bytes-billed ceiling stays where it is.
