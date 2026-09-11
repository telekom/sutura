---
title: A raw SQL tool, off by default, and the ramp it exists to build
description: Why a deployment may enable a general SQL tool beside the certified one - because a database with DDL and comments and no semantic layer is where every adoption starts, and refusing everything until somebody commits to governance is how a governed product never gets adopted - the mechanisms that keep it from ever looking certified, which sources it can exist over at all, what it costs that the certified surface got for free, and why the record is accepted and deliberately unscheduled.
---

# A raw SQL tool, off by default, and the ramp it exists to build

Status: **accepted, and scheduled LAST.** Nothing here is built, and this record does not amend an
invariant yet - see *What this would change in the invariants* below, which says why the row cannot be
written before the mechanism exists.

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

| Source | Can the raw tool exist over it? |
| --- | --- |
| A network data system reached under the asking subject - PostgreSQL, BigQuery, Oracle | **yes.** The statement is passed through unparsed, by us, to a system that has its own parser and its own authorization. Nothing in the closure changes |
| The shipped file engine | **not without accepting a second SQL parser.** A raw statement over it needs DataFusion's SQL frontend, which is the `sql` feature, which is `datafusion-sql` and `sqlparser` compiled into the shipped binary beside `polyglot-sql` |
| A local DuckDB source | yes in principle - it renders through `sutura-sql` and the driver takes a statement - but `sutura-exec-duckdb` is a dev-dependency and which artifact links a native driver is undecided in [the plan](0009-the-plan-from-one-source-to-many.md) |

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
emitted with `> ` in front of it, so no line of catalog text can reach the generated document at column
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

The row reading *"No SQL, table name, filter expression or row-id list on the tool surface"* is a
statement about the **certified** surface, and the mechanism behind it is untouched by this record:
`Query` declares no such field and `deny_unknown_fields` makes an attempt an error naming it. That
half is true today and stays true whatever happens here.

**The second half must not be written into that table before it exists.** The row this record would
add - a raw tool exists only as a separately-scoped, off-by-default capability whose result type
cannot carry a definition digest, available only where the source executes as the asking subject or
the deployment is single-user - cites as its mechanism an outcome type that does not exist, in a tool
that has no branch in any implementation stack. The table's own rule decides the case, and it decides
it against us: *a row that loses its mechanism gets deleted, not demoted to advice*, and a row that
never had one is the same case. A reader who finds a raw-tool row in the enforced table and then finds
no raw tool has learned that the table can be aspirational, which costs more than the row was worth.

So: **the amendment lands with the implementation branch, not with this record.** What has to exist
first is the list at the top of this file - the separate outcome type, the cancelling execution port,
the scope gate and the advertisement filter, the read-only story per mode - plus the tests that provoke
each of them. Until then this record is the decision and `AGENTS.md` says only what holds.

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
