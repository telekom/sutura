---
title: Where a budget lives
description: Why the budget key is the subject from PrincipalChain::attribution() and not the whole chain, why the first counter is per-replica and says so rather than adding a shared store, why a spent budget is a 429 and not the 422 ResourcesExhausted already uses, and what PreFlight has to carry before any of this can be checked.
---

# Where a budget lives

Status: **accepted for the estimate; the counter and the refusal are follow-on branches, undecided
in code and decided here.** `maximumBytesBilled` bounds one BigQuery job. Nothing bounds a subject,
an agent or a deployment across many jobs, and #139 is the report that measured this against
`main`: `PreFlight` carries `NotAsked` and `Accepted` and no number, `RefusalReason`'s variants
include none for spend, and every `Budget`-named value in the tree (`FEDERATED_BUDGET`, the MCP test
durations) bounds bytes or time already answered, not money not yet spent.

**Amended: the counter and the refusal this record decided are now built.** `sutura_app::spend::SpendLedger`
is the mechanism *Where the counter lives* decides, keyed and windowed exactly as stated there, and
`RefusalReason::BudgetExhausted` at `429` is the refusal *What a spent budget produces* decides. Every
limit this record states - per-replica rather than deployment-wide, the collapsed key with no
`security.inbound`, the agent-spends-the-subject's-own-budget cost, the federated under-count - is
unchanged by the build and still holds. The *"Nothing here enforces anything"* bullet below is what
this amendment closes.

## Context

The pieces this decision assembles already exist and none of them talk to each other, stated here at
the time of this record (`main` before #662 - `PreFlight` gains the field this section says it lacks
in the PR this ADR ships beside):

- **`Warehouse::dry_run`** returns `PreFlight`, asked before `execute` spends anything.
  `PreFlight::Accepted` carries no field - `crates/sutura-domain/src/warehouse.rs` - so an adapter
  that priced its own dry run has nowhere to put the number.
- **BigQuery's dry run is free and slotless** and its `QueryResponse` already carries
  `totalBytesProcessed` as a top-level field, beside `kind`, `jobReference` and `cacheHit` -
  `document.rs`'s own header names it among the fields nothing
  here reads. `wire.rs`'s `validate_job` discards it today, in words:
  *"a dry run returns `totalBytesProcessed`, and this discards it... `PreFlight::Accepted` also
  carries no field for an estimate, so there is nowhere to put it."*
- **`PrincipalChain`** is the key a budget would use, and says so in its own doc comment:
  *"what a budget would be keyed on if a budget existed... `Eq` and `Hash` are derived so that it
  can be one when something does."* `attribution()` returns a two-variant `Attribution` -
  `BareSubject` or `ActingFor { subject, actors }` - so a subject asking directly is already
  distinguishable, in the type, from an agent asking for them.
- **`FEDERATED_BUDGET`** in `crates/sutura-app/src/federated.rs` is a *working-set byte ceiling* for
  one combine, unrelated to money and already spent by the time it is checked.

Three questions, and each has a real alternative:

## The key: the subject from `attribution()`, not the whole chain

**Decision: the map key is the `Subject`** - specifically, the subject `PrincipalChain::attribution()`
returns, whether the case is `BareSubject` or `ActingFor`. The actor chain, when `ActingFor` names
one, is not part of the key; it does not disappear, it moves - see below - to the refusal and the
audit record, so a spend is billed to the subject but a reader can still see which actor incurred it.

**Alternative declined, reversing this lane's own first draft: the whole `PrincipalChain` (subject,
actors when present, task when present) as one opaque key via its derived `Eq`/`Hash`.** The draft
this replaces picked the chain and named its own cost honestly - *"a subject who can mint new agent
identities gets a fresh, unspent budget per identity"* - and then kept the pick anyway, on the
reasoning that a budget keyed this finely governs an identity that asks a lot, not one that
multiplies. On review that reasoning had it backwards: a key fine enough to tell an agent apart from
the subject it acts for is exactly a key an agent can walk around by asking under a new chain, and
#139 opens against a shape that multiplies - *a thousand jobs*, not one relentless asker from one
fixed identity. A chain key gives every minted identity a fresh, unspent allowance; the multiplying
threat is the one this record has to close, and a key that includes the actors cannot close it.

**The cost of the pick, stated where the claim is:** collapsing `ActingFor` into its `subject` pools
every agent acting for a person into that person's own allowance. An agent that retries - the exact
threat #139 opens with, read the other way - then spends the human's own budget, so their own direct
next question can be refused by their tool's mistake. This is a real cost and it is the same one the
first draft declined the subject key over; it is accepted here because it is bounded and diagnosable
where the chain key's cost was neither: the subject's own tooling caused it and the subject's own
tooling can be fixed, the counter still refuses at the same ceiling either way, and - because the
actor chain travels with the refusal and the audit record instead of disappearing - *whose* retry
spent the budget is answerable after the fact even though the counter did not key on it in advance.
A subject who cannot see or govern their own agents is exposed to this cost; a subject is never
exposed to another subject's agents, because the key is still per-subject. **That per-subject
isolation is itself conditional on `security.inbound` being configured:** on a deployment that
declares none, `PrincipalChain::attribution()`'s subject is `Subject::TheDeploymentItself` for every
caller (`crates/sutura-http/src/principal.rs`), so the per-subject key collapses to one shared key and
the budget this record designs becomes, on that shape, a deployment-wide budget in per-subject
clothing - one caller's runaway refuses everyone.

**Where the actor chain goes instead.** Not into the key, and not discarded: `PR2`'s refusal shape
and whatever audit record already carries `PrincipalChain` (`crates/sutura-domain/src/audit.rs`
already accepts one) name the acting chain beside the subject the spend was billed to, so a spent
budget's record answers both *whose allowance* (the key) and *who asked* (the chain) without
conflating them into one bucket.

**Alternative declined: key on the source or the deployment.** A single deployment-wide counter
answers a different question - *can this deployment as a whole afford this* - which is real and is
listed under *Consequences* as a control this decision does not replace. It cannot answer *whose*
spend to refuse, which is the question `attribution()` exists to make answerable and the reason
#139 cites it by name.

## Where the counter lives: per-replica for the first version, stated as such

**Decision: the first counter is in-process, one per replica, keyed by the subject from
`attribution()`, windowed - it resets on a fixed period as well as on restart, never only on
restart.** No new dependency, no new failure mode, and it ships with the estimate rather than behind
a store decision. Windowed rather than a lifetime cap is decided here, not deferred to
`feat/budget-refusal`, because *What a spent budget produces* below argues a `429` from the
self-healing property a window gives and a lifetime cap does not have: a cap that only clears on
restart makes the refusal permanent until a redeploy, which is #139's own complaint about a `429`
inviting a retry that will also fail - so the counter shape and the refusal's status are one decision,
made together here rather than the status assuming a shape nothing had picked yet.

**Alternative declined, for now: a shared store across replicas.** This is the strictly better
control - #139's own premise is that per-pod counters are not a budget - and it is declined here
only because it is a second decision hiding inside this one: which store, and **what a store that is
down does to the questions that do not need it**. `docs/implementation-plan.md`'s own rule for the
identity path is *"anything that cannot be reached on the authorization path must fail closed"* -
but a budget is not identity, and a store failure that refuses every question because the counter is
unreachable turns an availability incident in a side dependency into an outage of the certified path,
which is worse than the ungoverned spend it replaces. Whether an unreachable store should fail open
(spend ungoverned until it returns) or fail closed (refuse until it returns) is exactly the decision
#139 named as *not a detail*, and it deserves its own record once a store is chosen rather than an
answer picked to unblock this one.

**Alternative declined: the data system's own quota, sutura only reporting.** BigQuery has
project-level quotas that a deployment can set independently of anything sutura does, and that is a
real, already-available, cross-replica control **for BigQuery**. It is not a general answer: DuckDB
and Postgres meter nothing sutura can attach a quota to, so a decision that only BigQuery can enforce
is not a decision about *a budget*, it is a decision about one adapter's own service. It belongs
beside this record as an operator note for BigQuery deployments, not instead of the counter.

**The cost of the pick, stated where the claim is:** a deployment with N replicas gets N times the
configured ceiling before every replica has independently refused, and a counter resets on every
restart or rolling deploy, in addition to its own window. This is not the budget #139 asks for; it is
the smallest piece of it that adds nothing to reason about failing, and the settings key it reads
MUST say so in its own
description rather than name itself `budget` unqualified - `per_replica_spend_ceiling`, not
`spend_budget`, so a deployment that reads its own configuration cannot mistake the limit for the
goal.

## What a spent budget produces

**Decision: a `RefusalReason` variant, and a `429`, not the `422` `ResourcesExhausted` already
uses.** Every existing 4xx row in `docs/adr/0005-a-refusal-carries-a-status.md`'s table is
permanent in the sense that matters to a client: the same question refused now is the same question
refused on the next identical attempt, because nothing about the refusal changes with time -
`ResourcesExhausted`'s ceiling does not move, `SourceRefused`'s grant does not appear, narrowing is
always the only way out. A spent budget is the one shape here that is **not** permanent: the same
question, asked again after the window this record's key tracks rolls over, is a different answer,
because the thing that changed is nothing about the question - it is only that time passed. That is
what a `429` states and a `422` or `403` do not, and it is why #139's own worry - *"a `429` invites a
retry that will also fail"* - does not apply to an immediate retry, which this decision expects to
fail exactly once more, but does apply to *no* retry, which is wrong for a refusal that resolves on
its own. A `Retry-After` naming the window's reset, where the counter's shape makes one knowable, is
the detail that turns *"try again"* from a guess into a fact the response carries.

**Alternative declined: `403`, matching `CredentialUnavailable` and `SourceRefused`.** Declined
because those two are permanent - a grant does not reappear on a timer - and reusing their status for
a refusal that self-heals would teach a caller that `403` from this service sometimes means *come
back later*, which spends the meaning `docs/adr/0005` built `403` to have.

**Alternative declined: reuse `ResourcesExhausted`'s `422` with a new payload.** Declined for the
same reason as declining `403`: `422`'s row says *narrowing helps and repeating does not*, and a
budget refusal is not that - narrowing does not help (the question already fits the metric and the
range), and repeating **does**, once the window resets. A shared variant with two different retry
semantics behind one status is the kind of finding `docs/adr/0005` was written to stop.

**Not decided here: the variant's exact fields.** The counter is windowed, decided above, so a
`Retry-After` naming the window's reset is possible; the field carrying it, and whatever else the
variant names, follow from `feat/budget-refusal`'s own implementation. The refusal's status and its
category - self-healing, not narrowable - are decided; its exact payload is not.

**This decision amends `docs/adr/0005-a-refusal-carries-a-status.md` before the code exists to amend
it with.** That record's Context section states *"the two \[statuses retried by convention, `429`
and `408`\] and no refusal maps to either"* (0005, Context), and
`crates/sutura-http/src/wire/refusal.rs`'s own header comment repeats it; both become false the day
`feat/budget-refusal` lands a `RefusalReason` at `429`. Recorded here, ahead of that PR, in the same
spirit 0005 already amends itself in place for a landed row (0008's `CredentialUnavailable`) - `0005`
gets its own amendment paragraph and status-table row, and `refusal.rs`'s header comment its own
edit, in the commit that lands the variant, not this one.

**`429` is already spent on this surface, by a different key.** `Failure::RateLimited`
(`crates/sutura-http/src/problem.rs`) is `429` today, keyed on the caller's address by the rate
limiter - "you personally asked too often" about a connection, not a subject. A budget refusal keyed
on `attribution()`'s subject would share the status and differ in `code`, the same shape `problem.rs`
already uses for `Unavailable`/`IdentityUnavailable`/`AtCapacity` sharing `503`: two keys, address and
subject, behind one status, told apart by the reader that matters - the client branching on `code`.
Stated here so the shared status is a decision rather than a collision noticed later.

## What "spend" means per data system

**BigQuery** already has a service-enforced, per-job money bound (`maximumBytesBilled`) and a free
estimate (`totalBytesProcessed`) this record's estimate step reads. What sutura adds is
the thing BigQuery's own bound cannot see: a ceiling that sums across many jobs by the same asking
identity, which is exactly the shape #139 opens with - *a thousand jobs that each scan just under
the per-job bound.*

**The in-process engine (DataFusion) and DuckDB bill nobody.** There is no money bound to add for
either. "Spend" for them is bytes scanned, and neither adapter's `dry_run` produces that number
today - DuckDB answers `PreFlight::Accepted` because `prepare` really resolves the statement and
reads no plan statistics that would estimate bytes touched; DataFusion answers the port's own
`NotAsked` default and does not override it at all, because checking its plan means running most of
the answer twice - see its own `dry_run` doc comment. Either way an estimate for these two is `None`
honestly, the same as it is for Postgres, and the budget this record's counter enforces is checked
against **whatever legs did estimate**, with an unestimated leg contributing nothing to the sum
rather than refusing the plan for lacking a number nobody promised. Whether an
engine-side estimate is worth building - from table statistics, or from `EXPLAIN` - is not decided
here and is not blocking: the smallest honest slice ships with BigQuery's number and everyone else's
honest absence.

**Postgres** has `EXPLAIN`, which estimates rows and a planner cost unit, not bytes, and no money
attaches to either. Folding a Postgres cost estimate into a byte-denominated budget would need a
conversion this record does not attempt; Postgres's `dry_run` stays `prepare`-only and its
contribution to a byte budget stays `None`, same as the engine.

## `PreFlight` carries an estimate

**Decision: `PreFlight::Accepted` gains one field, `estimated_bytes: Option<EstimatedBytes>`, where
`EstimatedBytes` is a one-field newtype over `u64`** (not a bare integer, so a byte count cannot be
confused with any of the other `u64`s a plan carries) rather than a `u64`, and not a required field,
because most adapters have nothing to put there and `NotAsked` already exists to keep *did not ask*
distinct from *asked and accepted* - `Some`/`None` on `Accepted` keeps *asked, accepted, priced* apart
from *asked, accepted, could not price*, the same discipline one level in. Zero is a legitimate
estimate (a cached result, a trivial `SELECT`), so the newtype validates nothing beyond existing -
unlike `BytesBilledCeiling`, which forbids zero because zero would refuse every question, an estimate
of zero is simply the truth for some questions.

**Alternative declined: a required `estimated_bytes: u64` with `0` standing for "unknown."** Declined
because it is exactly the ambiguity `PreFlight::NotAsked` was written to remove one level up: `0`
from an adapter that could not price and `0` from a genuinely free question become the same value,
and a caller cannot tell a real number from an absent one - the defect this whole record's key
citation names.

**Blast radius, named rather than discovered in review:** every existing construction and match of
`PreFlight::Accepted` - `sutura-app`, `sutura-conformance`, `sutura-exec-bigquery`'s own tests and
acceptance suite, `sutura-conformance/tests/bound.rs` - becomes `Accepted { estimated_bytes: None }`
except the one call site with a real number to put there
(`crates/sutura-exec-bigquery/src/lib.rs`'s `dry_run`, which already discards
`totalBytesProcessed` and now keeps it). This is mechanical and it is one commit, per
`AGENTS.md`'s own rule for a repeated change.

## Consequences and limits

- **What this record ships nothing new for.** No port method, no `RefusalReason` variant, no
  counter, no store. `PreFlight` carrying a number is the only type change this record authorizes
  directly; the key, the counter's home and the refusal's status are decided so the two follow-on
  branches build the same shape rather than three different guesses.
- **The per-replica counter is not the budget #139 asked for**, and its own settings key must say so.
  It stops one replica's runaway, not a deployment's. A shared store is the strictly better control
  and is deliberately not decided here - see *Where the counter lives* above for why forcing that
  choice now would answer the wrong question first.
- **An agent acting for a subject spends that subject's own budget, not a budget of its own.** Stated
  once above, held here as the standing limit: a misbehaving or retrying agent can exhaust the
  allowance the subject needed for their own next direct question. The actor chain travels with the
  refusal and the audit record so the spend is diagnosable after the fact, but the counter itself
  cannot tell the subject's own asking apart from an agent's on the way to the ceiling.
- **On a deployment with no `security.inbound`, the per-subject key is a deployment-wide key.**
  Every caller's `attribution()` names `Subject::TheDeploymentItself`, so the isolation *The key*
  section claims between subjects holds only once leg 1 is configured; on the shape that ships
  before it, this budget is one shared allowance and any caller's runaway refuses every other.
- **The estimate is a plan-time number and not a promise.** `PreFlight::Accepted` already carries this
  limit for acceptance - *"the data system's opinion at pre-flight time and not a guarantee about
  execute"* - and an estimate inherits it: what a job is actually billed for can differ from
  `totalBytesProcessed`, which is why #139's own verification section calls that gap "the one number
  here that cannot be assumed" and routes the live check to `just bigquery-acceptance` rather than a
  fake.
- **A federated answer's estimate is the sum of every leg's `Some`, ignoring every leg's `None`.**
  This under-counts whenever a `None` leg would have scanned real bytes, which today is every leg
  that is not BigQuery. Stated here so the follow-on branch cannot silently treat a partial sum as a
  complete one.
- **Nothing here enforces anything.** *(Amended - see the top of this record: the counter and the
  refusal are built.)* A number that nothing refuses against is a metric, not a budget.
  `feat/preflight-estimate-carries-a-number` shipped the number; the counter and the refusal shipped
  the shape this bullet named.
