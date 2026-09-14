---
title: A raw SQL tool, off by default, and the ramp it exists to build
description: Why a deployment may enable a general SQL tool beside the certified one - because a database with DDL and comments and no semantic layer is where every adoption starts, and refusing everything until somebody commits to governance is how a governed product never gets adopted - the mechanisms that keep it from ever looking certified, which sources it can exist over at all, what it costs that the certified surface got for free, and why the record is accepted and deliberately unscheduled.
---

# A raw SQL tool, off by default, and the ramp it exists to build

Status: **accepted, scheduled LAST, amended 2026-09-13, and closed 2026-09-14** - the 2026-09-13
amendment settled the showcase's fork (which source), prerequisite 2's interim bound and
prerequisite 4's read-only story; **PR1 (`#666`) built the mechanism this record describes** -
`RawOutcome`, `Capability::RunSql`, the boot refusal, and the Postgres execution path below; and the
2026-09-14 "Built" section at the foot of this record closes step 5 (`#129`) - the demand-signal
mechanism, the two invariants rows PR1 deferred, and one mechanical gap this pass found and closed.
This record now amends the invariants file, over a mechanism unchanged since `#666` and reviewed
three times since.

**Corrected:** this said "deliberately UNSCHEDULED" and "no branch in the implementation stack" while
`docs/implementation-plan.md` carried `feat/raw-sql-tool` as a row, 0009's order table carried
the same branch with a done-when, and the identity half of the plan said it has a row now. Three records
against one is not a tie, and this was the stale one - exactly the cross-record contradiction class this
set claims to have eliminated. It is scheduled, it is last, and what follows is why last rather than why
never.

This record decides that a rule previously stated as absolute - *no SQL on the tool surface* - is a
rule about the CERTIFIED path rather than about the whole product. The reason is adoption rather than
convenience.

## Last on purpose, and here is what has to exist first

Scheduled last is not scheduled next, and the distance is four things that do not exist. Naming them
here is what stops the record from reading as a feature waiting for a sprint:

1. **A separate outcome type.** The load-bearing mechanism in this record is a result that has nowhere
   to put a definition digest. There is one outcome type today and it carries `Provenance` with no
   constructor that omits it, so the raw tool's result is a new type on the surface, with its own
   serialization and its own tests, before anything else here is reachable.
2. **An execution port that can cancel.** [The plan](0009-the-plan-from-one-source-to-many.md)'s third
   decision records that `Warehouse::execute` is synchronous and blocking, so a deadline that fires in
   the caller leaves the leg running inside the driver. That is a bound worth having on the certified
   path and a **hard prerequisite** here, for the reason the bounds section below gives.
3. **A scope gate on the tool surface, with tool advertisement filtered by it.** A caller without the
   scope must not see the tool listed, which means the surface has to know what a caller may call
   before it answers the question of what exists.
4. **A read-only story that is written down per mode**, because the honest version has two halves and
   only one of them is anything sutura enforces. That is the section below, and it was wrong in the
   first draft of this record.

And a fifth that is not a prerequisite but a fork in the road: **over the shipped engine this tool
cannot exist without reversing a decision two other records defend.** That is its own section.

## Why, and it is not "someone asked for it"

**A database with DDL and comments and no semantic layer is where every adoption starts.** Not an edge
case, the normal case. The semantic layer is the valuable thing and it is also organisational work -
somebody has to own a number and say what it means - and nobody funds that work before seeing the tool
answer a question.

So a product that refuses everything until a semantic layer exists asks for the commitment first and
delivers the value second. That order does not survive contact with a sceptical sponsor, and the failure
is not that they disagree about governance: it is that they never see it work.

**Three things this has to demonstrate, in order:**

1. **It works.** Real questions, real data, from what a database already documents about itself.
2. **Defining a metric is smooth.** The step from "that answer was useful" to "that answer is certified"
   has to be short, and the system should carry the candidate rather than leaving somebody to write it
   from scratch.
3. **Coverage grows, visibly.** More certified metrics means more questions answered the governed way,
   and that shift is a number somebody can see rather than a claim.

The third one is what stops this record from being a hole in the design. An ungoverned path with no
measurement becomes the comfortable default. An ungoverned path whose share is reported is a ramp. Who
does the reporting matters and is settled further down: sutura emits, the deployment's monitoring
computes, and a deployment that attached no sink gets the hole rather than the ramp.

## The mechanisms

**Off by default, per deployment.** Enabling it is a configuration decision that shows in a diff, and a
deployment that never enables it behaves exactly as before this record.

**A separate tool with its own scope, never a field on the certified one.** The typed question keeps
having no field for SQL, a table, a predicate or row ids. This is a second tool, and a caller without the
scope **does not see it advertised** - invisible rather than rejected, because a tool nobody may call
should not be in the list.

**Its result cannot claim certification, and that is a type rather than a rule.** The certified answer
carries provenance with no constructor that omits it. The raw tool returns a **different outcome type,
which has no field for a definition digest at all**, so labelling a raw result as certified is
unrepresentable rather than forbidden. That is the load-bearing mechanism in this record.

**Nothing decides read-only by INSPECTING the statement.** Doing so would mean parsing it, and a
parser that has to be right about every dialect's write forms is exactly the fragile thing this
codebase refuses elsewhere. Whatever bounds a write, it is not us reading the SQL.

**What actually bounds a write has two halves, and only one of them is ours.** The first draft of this
record said "the tool may only be enabled over a credential or role that cannot write", and that
sentence is false in the mode this tool mostly exists for. Under `ImpersonationAtSource` the credential
is the SUBJECT's, minted per request per
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) - sutura does not choose
it and has nothing to attenuate it with. So:

- **In single-user mode the static credential's grants bound it, and the OPERATOR chooses them.** That
  is a real control and it is not sutura's: it is a role at the source, written by whoever configured
  the deployment. What this record contributes is that it must be a deliberate choice rather than
  whatever credential was already lying in the settings file, and the startup line prints which
  credential the raw tool would run under.
- **Under impersonation the write boundary is the source's authorization of THAT SUBJECT, full stop.**
  A subject whose grants permit writes can write through this tool. That is the same answer the
  certified path gives - the source is the thing that knows what a person may do - and it is
  uncomfortable here only because the statement is arbitrary. Session-level read-only is not a repair:
  `default_transaction_read_only` and its equivalents are settable by the same session's SQL, so a
  statement that begins by unsetting it is a statement the session accepted.

**The one case sutura enforces anything is a declared capability, and it is narrow.** Where an adapter
can open the source read-only ITSELF, in a way the session cannot undo - DuckDB's read-only open flag,
a file engine that has no write path at all - that is a capability the adapter DECLARES, per
[pluggable by declaration](0011-pluggable-by-declaration.md), and a deployment may require it. Then the
refusal is at boot: the raw tool over a source that declares neither read-only opening nor an operator
acknowledgement of what the credential can do does not start. Note what that is worth and what it is
not: it covers exactly the adapters whose driver has such a flag, which today is the local ones, and it
is unavailable precisely in the impersonating network case where the concern is sharpest.

**Impersonation is required unless the deployment is single-user.** This is the sharpest constraint and
it follows from what the tool is: arbitrary SQL under a shared service account reads everything that
account can see, for anybody who can call the tool. So the raw tool is available where the source
executes as the asking subject - and in single-user deployments, which are development and
proof-of-concept shapes with one user and static credentials. Nowhere else.

**Every bound applies unchanged:** the result ceiling, the query timeout, the row cap. A raw statement is
not a reason to be unbounded; if anything it is the path most likely to need them.

**And one bound has to become real before this tool exists, rather than applying unchanged.** A
timeout wrapped around a blocking call stops the CALLER waiting and leaves the statement running inside
the driver, which is what [the plan](0009-the-plan-from-one-source-to-many.md)'s third decision says
plainly: either the deadline travels on the port and each adapter cancels for real, or the bound is
"stop waiting" and the failure mode ships with it. On the certified path that is a bad bound
over a plan the compiler wrote; here it is a bad bound over text a caller wrote, and a cross join is a
denial-of-service primitive that no result ceiling reached after the fact can stop - the ceiling is
counted on rows returned, and the join never returns. This is the only place caller text becomes a
statement, so it is the place where "stop waiting" and "cancel" stop being the same word. **The
deadline travelling on the port, with real per-adapter cancellation, is a prerequisite of this tool and
not a companion improvement.**

## Which sources this tool can exist over, because over the shipped engine it cannot

The served binary links `sutura-exec-datafusion`, which executes a plan over files and generates no
SQL. It has no SQL frontend, and that is a property of the closure rather than a convention about which
functions get called: `Cargo.toml` pins
`datafusion = { version = "55.0.0", default-features = false, features = ["parquet", "datetime_expressions"] }`,
and `grep -n 'sqlparser\|datafusion-sql' Cargo.lock` returns nothing - neither crate is in the lock at
all. [Several databases behind one data system](0006-several-databases-behind-one-data-system.md) is
where that was measured, and its strongest sentence is the one that applies here: **not calling a
parser is a convention, not having one is a property.**
[Federating across different data systems](0007-federating-across-different-data-systems.md) declines a
route for the same reason - *"`datafusion`'s `sql` feature stays off, so no second parser and no
unparser arrive"*.

So the sources split, and the split is not a detail of implementation order:

| Source                                                                                | Can the raw tool exist over it?                                                                                                                                                                                                                  |
| ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| A network data system reached under the asking subject - PostgreSQL, BigQuery, Oracle | **yes.** The statement is passed through unparsed, by us, to a system that has its own parser and its own authorization. Nothing in the closure changes                                                                                          |
| The shipped file engine                                                               | **not without accepting a second SQL parser.** A raw statement over it needs DataFusion's SQL frontend, which is the `sql` feature, which is `datafusion-sql` and `sqlparser` compiled into the shipped binary beside `polyglot-sql`             |
| A local DuckDB source                                                                 | yes in principle - it renders through `sutura-sql` and the driver takes a statement - but `sutura-exec-duckdb` is a dev-dependency and which artifact links a native driver is undecided in [the plan](0009-the-plan-from-one-source-to-many.md) |

**Enabling the second row is its own decision and not a consequence of this one.** It is the exact
trade 0006 and 0007 both declined, in a different context: a second parser in the shipped binary, a
licence surface to re-check, and the loss of the property that the only parser present is the one that
runs at load. This record does not make that decision, and a deployment reading it should not conclude
that the tool is available on the artifact it has.

## What this spends, and both halves have to be written down

**The first property genuinely does not hold, and pretending otherwise would be the defect.** "No
caller text reaches a statement" is false for this tool by construction - the statement *is* caller
text. Its safety therefore comes from the source's own authorization, which is why the impersonation
rule above is not optional.

On the certified path the guarantee is untouched and keeps its own mechanism: every value from a
question becomes a **bind parameter**, statement and parameters are separate fields with no merging
constructor, and a golden asserts no question literal appears in a statement. Behind that, a filter
value must also be a member of the catalog's declared allowlist - defence in depth, not the guarantee
itself. Neither is weakened by this record, because the raw tool is a different tool with a different
result type rather than a wider version of this one.

### The second property is what bounded prompt injection everywhere else, and this record is what spends it

Worth stating in the record set for the first time, because it is the thing being spent and it has been
implicit: **on the certified path a successful prompt injection cannot invent a query.** The typed
question carries no SQL, no table, no predicate and no row ids, so the most an injected instruction can
do is make the agent ask for a DIFFERENT certified metric, over a bounded range, within the caller's
own grants at the source. That is a wrong-answer problem. It is not a run-arbitrary-code problem, and
the difference is a property of the surface's shape rather than of anybody's care.

**The raw tool is precisely the capability that property depended on not existing.** An injected
instruction - in a result cell, in a catalog description, in a document an agent read three turns ago -
can now cause arbitrary SQL to execute under the asking subject's own credential. Every mechanism this
record has bounds **authorization**: the scope gate bounds WHO may call the tool, the subject's grants
bound WHAT it may read, the bounds bound how much and how long. **None of them bounds what the model
can be talked into sending.** So the honest statement is that an authorization boundary holds while an
intent boundary does not, and enabling this tool is a decision to accept that for the sake of the ramp.

Three things follow, and the third is where the record has to be careful not to overclaim a fix.

**The prompt channel is already handled, and it is the only one that is.**
[`docs/agent-prompt.md`](../agent-prompt.md) records the mechanism: every line of catalog prose is
emitted with `>` in front of it, so no line of catalog text can reach the generated document at column
zero, it cannot emit a heading or close a block, and a test provokes it with a description whose lines
are a heading, a fence and a bare instruction. The trust boundary is named in the text immediately
above the quoted block. That leaves the two channels this record adds traffic to, and neither has an
equivalent.

**The RESULT channel and the ERROR channel are unnamed anywhere in this set, and both are carriers.**
A result cell is text an agent reads and acts on, and with this tool the cell can be selected by
whoever wrote the statement. Worse and less obvious: **an echoed data-system error or refusal message
is part of the prompt.** Whoever controls the SQL controls part of the message a database returns about
it - a column alias, a table name, a constraint name reflected back verbatim - so a diagnostic passed
straight through is a channel from the statement's author into the agent's context. That is an argument
for the raw tool's failure text being *structured* rather than echoed, and for the same field-boundary
treatment the result envelope needs.

**Structure is the answer, and a filter is not.** [The plan](0009-the-plan-from-one-source-to-many.md)
already carries the decision this depends on - untrusted content marking in the result envelope, chosen
before an encoder ships, because a field boundary an encoder enforces cannot be forged by a cell value
and a delimiter line can. This record does not restate it or extend it; it records that the raw tool is
the strongest reason to have it, and that its own error text belongs inside the same boundary. And the
limit that record already states applies here without change: **a filter that quietly edits data
returns a wrong number**, so a detection hit belongs in a refusal rather than a silent scrub - which is
this repository's rule for the query path everywhere else and is not a special case here.

## The ramp, as mechanism rather than intention

**Every ungoverned answer is a WRITTEN demand signal, and sutura retains none of it.** The question
that was answered without a certified metric is the most valuable input the backlog has: it is
somebody's real question, in their words, with the SQL that answered it. Refusals already carry that
role on the certified path, and this extends it.

[The plan](0009-the-plan-from-one-source-to-many.md) settles the shape and this record does not reopen
it - and the shape is two claims rather than one, which is worth stating exactly, because an earlier
version of this paragraph quoted the wording that record has since retired. Sutura **writes** one
record per call, refusals included, carrying the whole principal chain, before the outcome returns,
into a sink the deployment attaches. Sutura **retains** nothing: no archive, no rotation, no retention
window, no query interface. So the demand signal and the coverage number below are computed over the
records `feat/principal-chain` writes to that sink, on the emitted stream rather than over an archive,
and nothing in this record builds a table.

**The limit, and it is the reason this is a ramp rather than a guarantee:** a deployment that attaches
no sink keeps whatever its log pipeline keeps, which may be nothing. So "the ungoverned path cannot
quietly become the default" is a property of a MONITORED deployment and not of sutura. That is a
weaker claim than the heading of this section wants, and it is the true one.

**The candidate definition is written by the AGENT, and validated by the catalog parse we already
have.** An earlier draft of this record had the tool emitting a draft definition "where the shape
allows it - a single aggregate over one table at one grain", which means recognising that shape in the
caller's SQL, which means parsing it - the exact thing this record refuses to do for read-only one
section up, for the right reason. Correcting it in the direction that costs us: **sutura parses no SQL
at any point on this path.** What happens instead is the division of labour the whole design already
assumes. The agent proposes a catalog document - prose and a closed measure vocabulary, which is
already what it writes - and sutura's existing document parse is what says whether it is a definition:
`deny_unknown_fields` at every depth, a `Measure` with no expression field, dimensions checked against
the model, and a refusal naming the key that was wrong. A human reviews and commits it. That is the
difference between "define your metrics" as advice and as a workflow, and it needs no new mechanism at
all - which is a better position than the draft's, not a weaker one.

**Coverage is a number the deployment's monitoring computes, not a series sutura keeps.** The share of
questions answered certified against ungoverned, over time, is exactly the quantity that matters, and
it is derivable from the per-call emission above by whatever consumes it. Saying it that way keeps this
record consistent with the no-retention position instead of quietly reintroducing a store under the
word "reported". Two consequences worth being plain about, because the second is a real weakness of the
design and not a detail: **a deployment that attaches no sink measures nothing**, so "the ungoverned
path cannot quietly become the default" is a property of a deployment that is monitored rather than a
property of sutura. And **rising is the whole point** - a deployment where the share does not move has
learned something worth knowing, and it can only learn it if somebody is looking.

**And the labelling is not decoration.** An ungoverned answer says so, in the payload, every time. A
reader who cannot tell which kind of answer they are holding has the worst of both designs.

## What this would change in the invariants, and why the row is not written yet

**Corrected 2026-09-14: the rows are written now** - see "Built (2026-09-14)" at the foot of this
record. This section is kept as the record of why they waited, not as the current state.

The row reading *"No SQL, table name, filter expression or row-id list on the tool surface"* is a
statement about the **certified** surface, and the mechanism behind it is untouched by this record:
`Query` declares no such field and `deny_unknown_fields` makes an attempt an error naming it. That
half is true today and stays true whatever happens here.

**The second half was not written into that table before the mechanism existed - and now that PR1
(`#666`) has built it, it is still not written here, on purpose.** The mechanism this row would cite

- `RawOutcome` with no field of type `Provenance`, `Capability::RunSql`, the `run_sql`-in-multi-user
  boot refusal - is real as of PR1's head, so the objection this section used to state (a mechanism that
  does not exist, a tool with no branch) no longer holds. What still argues for waiting is smaller: PR1
  is the FIRST commit the mechanism appears in, and this table's own history (`docs/adr/0022`'s two
  amendments) is where a guarantee written down too early - before a second reviewer's own read of the
  same code - turned out to describe a carrier that did not yet see every case. PR1's own body states
  this plainly rather than silently deferring it, and names the two rows the plan drafted so a reader
  can add them once the mechanism has had one more review's worth of scrutiny.

So: **the amendment lands with the implementation branch, not with this record** - and PR1 is that
branch. What had to exist first is the list at the top of this file - the separate outcome type, the
cancelling execution port (interim: the connect-time bound above), the scope gate and the advertisement
filter, the read-only story per mode - plus the tests that provoke each of them; PR1 builds all of it.
Until the invariants file is amended in a follow-up, this record is the decision and `AGENTS.md` says
only what holds.

**What is NOT weakened:** a certified answer is still produced by compiling a declared metric, not by a
model writing SQL. That is the point of the whole system and this record does not touch it. A deployment
with both paths has two clearly distinguished paths; it does not have a blurrier version of the certified
one.

## Consequences

- Two outcome types on the surface, and the difference between them is visible to a caller rather than
  documented. That is the intended cost.
- The raw tool cannot be tested by the conformance packs that assert certified behaviour, because it
  asserts different things. It needs its own small pack, and the list is narrower than the draft's:
  that it is absent from the advertised tools without the scope, that its result type has nowhere to
  put a digest, that the bounds bite, and - **only where an adapter declares it can open the source
  read-only** - that a write is refused. There is no pack asserting "a write is refused by the source"
  in general: that is the source's authorization of a subject, which the packs cannot fixture without
  two subjects whose grants differ, and the reason is the same one that keeps impersonation out of
  [the conformance packs](0012-conformance-packs-for-inputs-and-adapters.md).
- A deployment enabling it over a shared service account in multi-user mode must be refused at startup,
  for the same reason and by the same mechanism as a source declaring an impersonation it cannot perform.
- **Enabling it over the shipped file engine is a separate decision about a second SQL parser**, and
  that decision belongs in its own record rather than being inherited from this one.
- **This record's branch is `feat/raw-sql-tool`, row 20 of the plan, and it is scheduled LAST rather
  than unscheduled.** The prerequisites at the top of this file are what decide the position; three of
  the four are now spent, and the remaining one is what the row is blocked on. **This bullet used to
  say the record has no branch in the stack and that the status at the top is *unscheduled*. Both were
  false, and the correction at the top of this file had already said so for 300 lines** - a record with
  two answers to one question is worse than one answer, and this is the half that was left standing.
- The demo that motivated this becomes reproducible: catalog search plus a select, with the answers
  labelled for what they are, and a path from there to a metric that makes the next identical question
  certified.

## Amendment (2026-09-13): the showcase's source, the interim bound, and the read-only story

Written by the planning lane for `#129`, on `main` at `99109695`. Nothing in this amendment is built;
it answers the three open questions the base record left for the implementation branch, so the branch
starts from a decision rather than a discovery.

**The fork is resolved: the showcase points at Postgres.** `#124`/`#653` landed since the base record
was written - `sources.<alias>.kind: postgres` is declarable, `sutura-serve --features postgres` links
`sutura-exec-postgres`, and a channel is opened per its declared transport. That source executes with
`ImpersonationCapability::NoPlaceForASubject` - one connection under the deployment's declared identity,
no OAuth, no per-subject credential - so it is exactly the `SharedServiceUser` shape the base record's
table already answers **yes** for, over a network system with its own parser and its own authorization.
Nothing pins the raw tool to Postgres forever; a source that later declares
`ImpersonationCapability::PerSubjectCredential` is the stronger shape the base record already prefers
("available where the source executes as the asking subject"), and this amendment does not foreclose it.
**The shipped file engine remains out of scope, unchanged**: `datafusion`'s `sql` feature is still off,
neither `sqlparser` nor `datafusion-sql` is in `Cargo.lock`, and this amendment adds no dependency that
would put them there.

**No parser is needed, and this is the property rather than an implementation convenience.** The
statement is handed to `tokio-postgres` as literal, unparsed text; Postgres's own parser and its own
`GRANT`/`REVOKE` model are what authorize or refuse it. Sutura reads no keyword out of it - the base
record's own rule against inspecting a statement to decide read-only applies to every other purpose a
parser might be tempted for, too.

**Prerequisite 4, decided, and corrected once by root: the operator's role grant is still the PRIMARY
control, and sutura adds a second, real one - not the session-level control the base record already
rejected.** The first draft of this amendment read the base record's rejection of
`default_transaction_read_only`/`SET TRANSACTION READ ONLY` as closing every mechanism sutura could add,
and stopped at "nothing beyond a boot refusal." Root ratified the plan with one flip here: that reading
proved too much. What the base record rejects is a **setting** sent ahead of the caller's text on a
**session or connection the caller's next statement can still act on** - true of
`default_transaction_read_only` and of `SET TRANSACTION READ ONLY` alike, because either is undone by a
later statement in the same session. It is not true of a transaction sutura itself opens, uses for
exactly the caller's one statement, and closes by rolling back before the caller ever gets to send a
second one:

- **Every raw call is exactly one statement, sent through the extended query protocol**
  (`tokio_postgres::Client::query`/`query_raw`, never the simple query protocol `tokio_postgres::Client::batch_execute`
  or `simple_query` takes). Postgres refuses more than one command per `Parse`/`Bind`/`Execute` cycle on
  that protocol - a string carrying `SELECT 1; DROP TABLE t` does not smuggle a second statement in, and
  sutura reaches that refusal by which driver call it makes, not by counting semicolons in the text: a
  lexical count is exactly the parser the base record already refuses to write, and a quoted `;` inside a
  string literal would defeat one anyway.
- **Sutura wraps that one statement in its own `BEGIN ... READ ONLY` / `ROLLBACK`, unconditionally.**
  The transaction is opened immediately before the call's statement and rolled back immediately after,
  whether the statement answered, errored, or hit the timeout - never a `COMMIT`, so nothing the call did
  persists even where the role's grants would have allowed it to. Inside a transaction Postgres itself
  opened `READ ONLY`, an `INSERT`/`UPDATE`/`DELETE`/most DDL is refused by the **server** with
  `25006 read_only_sql_transaction`, independent of what the role could otherwise do - this is the one
  case the base record says sutura MAY enforce, and it exists here after all: not an adapter-declared
  read-only *open* flag (Postgres has none, unchanged from the first draft's finding), but an
  adapter-opened read-only *transaction*, scoped to the one call and never reused.
- **A caller's own `SET TRANSACTION READ WRITE`, sent as that one statement, flips nothing that survives
  the call.** It is not a write itself, so the read-only transaction does not refuse it outright - but it
  has no later statement in the same transaction to apply to before sutura's `ROLLBACK` runs, and no later
  call reuses that transaction, so the flip is discarded with everything else. This is exactly the
  property the base record's rejection was checking for and did not have on a shared session: **the
  caller's text cannot outlive the boundary sutura closes around it.**

So the write boundary for the Postgres showcase is now two things, not one: **the operator's role grant
is still primary and still an operator responsibility** - a role holding `INSERT`/`UPDATE`/`DELETE`/DDL
lets a *reader* of an authorized VOLATILE function reach those, see the limit below - **and the read-only
transaction sutura wraps around every call is a real, server-enforced second control**, catching the
ordinary case (a statement that is itself a write) even where an operator granted more than the tool
needs. Neither replaces the other: the transaction boundary answers only the shape "a write statement
inside the call sutura is making," the role grant is what a reader outside that one call - a later
certified leg, a different tool, a human with the same credential - is still bound by.

**The limit, named rather than left implicit, and it is not small.** A `READ ONLY` transaction is a
property of the SQL Postgres recognizes as a write - `INSERT`, `UPDATE`, `DELETE`, most DDL, sequence
advancement - and says nothing about a VOLATILE function's own side effects once the role may call it.
A function that writes a file, makes a network call, or uses `dblink`/`pg_notify` to reach outside the
transaction runs to completion inside a `READ ONLY` block, because Postgres's read-only check inspects
the statement's own effect on the database, not what a function it calls does beyond that. So the
transaction boundary bounds *SQL-visible* writes and nothing that happens through a function the role
is separately authorized to execute - naming that function in the raw statement is enough, and no parser
here would catch it even if one existed. **And the role grant it does not replace is not verified by
sutura either way**: nothing here reads back what the connecting role can do, so a deployment believing
its role has no write grant has sutura's word for neither half - the operator's own `GRANT`/`REVOKE`
statements are the only source of truth for what the role could do if the transaction boundary were not
there, and for what a VOLATILE function can still do with it in place.

Sutura's own further contribution is the boot refusal below, which stops the one shape the base record
calls out by name: a shared credential answering for every caller with no per-caller identity ever
established.

**The multi-user boot refusal, using the mechanism this deployment already has.** `sutura-config`
already declares deployment mode as a fact the operator states rather than derives -
`DeploymentIdentity::StaticCredentials` (`single-user`, with a written reason) or
`DeploymentIdentity::SubjectPerRequest` (`multi-user`) - and already refuses an unacknowledged
`shared-service-user` source under the multi-user mode (`Settings::identity_refusals`,
`NotFitToServe::SharedSourceNotAcknowledged`). The raw tool's refusal is the same shape, reusing the same
declared mode rather than inventing a second one: **a deployment that turns the raw-tool capability on
over a source whose adapter cannot exceed `SharedServiceUser` (Postgres, in this build) while
`security.identity` is `multi-user` does not start.** This is the "same reason, same mechanism" the base
record's consequences section already calls for; it is new code (`Settings::refusals` gains a case) and
not a restatement of the existing one, because the existing check is about acknowledgement and this one
is about whether the raw tool may be enabled over that posture at all - a source can be a fully
acknowledged shared source for the certified path and still be the wrong shape for arbitrary text under
it. **The limit, stated with the claim:** this is a boot-time refusal over a declared mode and a coded
adapter capability, not a runtime check that a caller's identity actually varies - `single-user` is
still a word an operator writes, and nothing here verifies that a `single-user` deployment truly has one
user.

**Prerequisite 2, the interim answer: ship on the bound Postgres already enforces, and name exactly what
it is not.** `telekom/sutura#160` PR1 (in flight, not yet on `main`) puts a `Deadline` on the port; engine
cancellation is PR2 and a Postgres `statement_timeout` *derived from that deadline* is PR4 - none of
which is discharged by PR1 landing alone. Waiting for PR4 is the literal reading of "the deadline
travelling on the port... is a prerequisite of this tool," but the merged Postgres adapter already sets
`SET statement_timeout = <SUTURA_DEV_STATEMENT_TIMEOUT_MS, default 15000>` once, at connect
(`PostgresWarehouse::connect_secured`), and the base record's own worry - a cross join that never
returns, which no row cap reached after the fact can stop - is already answered by that line: the
**server itself** aborts the statement, which is real per-adapter cancellation in the sense that
mattered to the base record (not "the caller stops waiting"). What it is NOT is the caller's own
request budget: the ceiling is one fixed, deployment-wide number, read once per connection, generous by
design ("a development tier, not a query budget") and not narrowed to whatever is left of
`server.request_timeout_seconds` for this particular call. **The decision: ship the raw tool's execution
on this existing ceiling rather than block on `#160` PR4**, because the property the base record asked
for - a runaway statement is killed by the source, not merely abandoned by the caller - already holds
for Postgres today, and the gap is precision of the number rather than absence of a bound. **The limit,
named rather than left implicit:** every statement on a raw-tool connection runs under one ceiling for
every caller and every question, so a caller with a five-second budget and a caller with the deployment's
full timeout are bounded identically; `#160` PR4 is still the fix for that, and this amendment does not
call the interim state "the deadline travelling on the port."

**Corrected:** this paragraph said "certified legs sharing the connection are unaffected, since a
fixture-only connection is not shared" - false for the served adapter PR1 ships. `PostgresWarehouse`
is ONE `tokio_postgres::Client` for both `run` (the certified path) and `run_raw`, and that client
pipelines: measured under concurrent load, two callers' `BEGIN` / statement / `ROLLBACK` triples
interleaved on the wire, so a raw caller's own refused write aborted a transaction a concurrent
certified `run` was inside, and the write itself ran outside any transaction and persisted. PR1's fix
is `PostgresWarehouse::execution_lock`, single-flighting every exchange on the shared client - so
`run` and `run_raw` no longer interleave - but the two paths still contend for the ONE connection: a
slow or many raw statements make a concurrent certified question wait, which this paragraph's own
"one ceiling for every caller" already names as a cost of sharing rather than as something narrowed
here. A raw-tool connection opened separately from the certified path's, distinct enough that the two
cannot contend, is left as future work rather than decided by PR1.

**What PR1 (`#666`) settled, where this amendment left it open:** the raw outcome type is
`sutura_domain::raw::RawOutcome`, carrying no field of type `Provenance` anywhere in its module - a
`compile_fail` doctest on the type is the mechanism, not a claim in this record. Its wire discriminant
shares no VALUE with a certified `ToolOutcome::Answer`'s, and neither serialized shape carries a
`provenance` or `definition_digest` key at any depth - **weaker than "share no serialized field
name"**, which this amendment used to claim and which is false: both bodies carry `columns` and `rows`
under the same two keys, because both carry rows. The audit record is a sibling constructor
(`CallRecord::of_raw`) with two new `RecordedOutcome` arms (`RawAnswered`, `RawRefused`), and it does
carry the caller's statement text, as an audit-only field the wire layer never reads. The capability id
is `run_sql`, the scope `sutura:sql.run` - the literal `crates/sutura-cli/tests/mcp.rs`'s own guard
test already asserted did not exist, corrected rather than deleted.

## Built (2026-09-14): the demand-signal mechanism, and what stayed a limit

This section closes the ramp section's own status: **the "written" half is built, and it was built
by `#666`, not by this step.** `CallRecord::of_raw` (`crates/sutura-domain/src/audit.rs`) and its
two `RecordedOutcome::{RawAnswered,RawRefused}` arms already carry the statement (opaque,
audit-only, the one accessor `CallRecord::statement()`), the row count, the outcome variant and the
full principal chain - the credential deadline rides on `CallRecord` itself either way, but
`TracingAuditSink`'s own rendering puts it on the log line only for `RawAnswered`, the same
asymmetry the certified `answered`/`refused` pair already has (neither `Refused` nor `RawRefused`
renders it) - and that sink already emits both raw arms as `raw_answered`/`raw_refused`
log events, distinct from the certified `answered`/`refused` pair, with the statement text logged
deliberately (its own doc: "this is a SINK, not a channel back to any caller"). `LocalService::run_sql`
(`crates/sutura-app/src/surface.rs`) writes the record before returning, exercised through the real
`Surface` rather than only the constructor.

**"Not stored" is an absence held by review, not by a gate, and that is stated here rather than left
implied.** No `absences` entry names it (`xtask/src/guidance/absences.rs`), because the sighting an
entry would need - a second `impl AuditSink for` in `crates/*/src` - is written by this workspace's
own tests today: every fixture sink in this tree (`crates/sutura-domain/src/audit.rs`'s `Recorded`,
`crates/sutura-app/src/surface.rs`'s test sinks) is `impl AuditSink for` something, under
`#[cfg(test)]`, in exactly the directories a naive sighting would scan - so the sighting would flag
the tests that prove the mechanism works, and refute itself the day it is written. The honest
statement is the limit itself: a deployment that links a second, RETAINING `AuditSink` in
non-test `src/` is a change nothing here catches, and "sutura retains nothing" is true of the one
implementor this workspace ships, not of every implementor a future PR could add.

**No new field, port, sink or counter was needed, and none was added.** A source name was declined -
`run_sql` "targets the sole registered data system" (`raw.rs`'s own doc), so a field naming it is a
constant. An elapsed-time field was declined - no call in this workspace, certified or raw, records
duration anywhere, and adding it only here would be a new, uneven mechanism rather than a gap in this
one. A stable digest was declined - this record's own position is that coverage is a number the
DEPLOYMENT's monitoring computes over the emitted stream, and a deployment wanting to count repeated
demand without keeping text can already hash the logged statement string itself; sutura computing that
hash is a step toward the "series sutura keeps" this record already refuses. A `run_sql`-specific
series on `/metrics` was declined for the same reason: that endpoint's own doc says it writes no audit
record and names nothing that names a question, and the distinguishing signal is the log event NAME, a
log pipeline's to read. The agent-authored-candidate-definition half of the ramp is unchanged from the
base record and needs no further mechanism.

**Two gaps were found in this record's own first pass, and both are closed now - `#703`'s review
found the second one.** `xtask check-boundaries`'s answer-path gate guarded only `answer`.
`sutura_app::run_sql` is a second door on the driving port, added by `#666` after the gate was
written, and nothing mechanical stopped a caller from reaching `sutura_app::run_sql` directly -
bypassing `LocalService::run_sql` and the audit write it makes - the same shape issue `#266`'s A1
named for `answer` itself. The gate (`xtask/src/boundaries/answer_path.rs`) classifies and guards
both doors: a needle per door (`ANSWER`, `RUN_SQL`), a liveness check against each door's own
defining file (`crates/sutura-app/src/raw.rs` for `run_sql`, since `lib.rs` only re-exports it), and
a fixture cell, `a_direct_call_to_run_sql_is_found_with_its_line`, constructed the same way every
other cell in this module is - directly against the classifier, not the real tree.

**That first pass still left `run_sql`'s module `pub` - `pub mod raw;` at `lib.rs` - which is a
SECOND, ungated spelling of the same door** (`sutura_app::raw::run_sql`), invisible to the
classifier by the same design that spares a call THROUGH the port
(`sutura_app::surface::Surface::run_sql`): both read as reaching something else first, then
`run_sql`. `#703`'s review proved it live - a bypass at that spelling in a caller crate compiled
clean and the gate printed `ok` over it - exactly the A1 shape this whole gate exists to close, not
among the limits the first pass stated. Closed by making the module private (`mod raw;`, keeping
`pub use raw::{..., run_sql}` so no caller loses access) and by a THIRD refusal: `pub mod raw`
present in `lib.rs` is now `Verdict::Fail` on its own, so the hole cannot be reopened silently. The
header's own limits list is corrected to state the property this holds: a door is guarded at the
crate root only, so the module it lives in may never be `pub`.

**The two invariants rows PR1 (`#666`) deferred are landed** in
`.agents/skills/sutura/invariants/SKILL.md`, citing the tests that already pass and have now had three
review rounds (`#666` rounds 1-2, `#686`): a raw answer cannot be rendered as certified, and `run_sql`
boots only single-user or over a source that cannot answer for more than one identity. Neither row's
mechanism changed to land it - only the wait for review scrutiny that PR1 itself asked for.

**What is left, named as pre-existing limits and not as this step's scope:** the per-request deadline
for the raw path, waiting on `#160` PR4; the raw tool sharing one lock-serialized connection with the
certified path rather than a dedicated one; the row cap's streaming stop bounding this process's own
heap rather than the server's work, pending a portal `max_rows` that needs `&mut Client`;
`RunSqlEnabledInMultiUserMode` refusing by declared mode alone rather than by the linked adapter's
actual shape (`#666`'s Finding 10); a dedicated read-only role for the development tier, documented
but not provisioned. None of these are the demand signal - they were named as limits before this step
and stay limits after it.
