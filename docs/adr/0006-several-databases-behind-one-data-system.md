---
title: Several databases behind one data system
description: Why one question over several DuckDB databases is delivered below the execution port rather than by a federation layer above it, the wrong answer that federation returned at these pins, what attaching several databases forecloses for per-subject impersonation, where the one-source assumption actually lives, and what is deliberately left undecided.
---

# Several databases behind one data system

Status: accepted, and **nothing here is built.** It decides the shape of a capability and declines a
dependency; it adds no code. It supersedes nothing and amends nothing:
[DataFusion for local execution](0003-datafusion-for-local-execution.md) said federation is
explicitly later, and this record answers *later than what*, at the versions this workspace pins and
against the code it actually has.

The goal it answers is one question spanning several data sources, initially several DuckDB
databases, in single-player mode. The goal names a federation layer as the route. **The route it
gets is not that one**, and the first reason is not the dependency: at these pins the federation
layer returned a wrong number.

**Which claims were run, and which were reasoned.** Every measurement below was produced by a
throwaway spike - three crates, each with its own `[workspace]` table, on a branch that is not for
merge - and then re-run independently while this record was written. Where a claim was *not*
executed it says so in the sentence that makes it. That distinction is the whole credibility of this
document, so it is kept per claim rather than declared once.

## Context

The goal contains an ambiguity worth removing first, because the whole cost difference lives in it.
**A data system here is a connection and an identity, not a file.** Two DuckDB database files opened
by one process, under one set of file-system permissions, in one transaction, are one data system
that stores its tables in two places. Two data systems are two logins, two policy engines and two
answers to *who is asking*, which is the reason
[a plan resolves to exactly one of them](../architecture.md#the-engine-and-the-data-systems-behind-a-port).

So "several DuckDB databases" is the first reading and not the second, and there are three ways to
serve it:

1. **Below the port.** One adapter, one connection, several databases attached to it. From the
   plan's point of view there is one data system, and nothing above the adapter changes.
2. **At the port, without a federation layer.** A table provider per source in one engine session,
   each pushing its own filters down. This is a near relative of what the engine already does for the
   files of one source.
3. **Above the port.** A federation layer finds the largest subplan each source can run, pushes it
   there, and combines the results. This is the only one of the three that is federation, and the
   only one that needs a second identity.

## The disqualifying finding: federation returned a wrong number

This goes first because it outranks every other argument in this document, and because it is the one
that would not have been found by reading manifests.

At DataFusion 54, with a federated DuckDB adapter written exactly as the crate's own example writes
one, a **single-source** aggregate over one database came back with two columns transposed. Run, not
reasoned - the statement the layer generated and the rows it returned, verbatim:

```text
[sales.duckdb] SELECT sum("orders"."amount") AS "total", "orders"."rep_id" FROM "orders"
               WHERE (...) GROUP BY "orders"."rep_id" ORDER BY "orders"."rep_id" ASC NULLS LAST

+--------+-------+          what the same question answers directly:
| rep_id | total |          rep_id 10 -> 150.00
+--------+-------+          rep_id 11 -> 275.00
| 150    | 10.00 |
| 275    | 11.00 |
```

The unparser emits the aggregate column before the group column; the plan's schema has the group
column first; and the layer's cast step casts **by position**. Its own doc comment says so - *"It
assumes the record batch columns are correctly ordered."* Decimal and Int32 cast into each other
cleanly, so nothing raised an error. Reconciling by name inside the adapter repairs it, verified in
the same spike by a switch that does exactly that; **nothing in the trait requires an adapter to do
it**, and the crate's own example does not.

With both tables in one database file the whole query fuses into one pushed statement, and then the
shift is total:

```text
+--------+--------+-------+
| region | orders | total |
+--------+--------+-------+
| 4      | 425    |       |
| 1      | 75     |       |
```

The group column now holds counts, the count column holds sums, and the measure is empty, because a
region string cast to a decimal is null.

For a system whose whole product is an answer certified under a definition digest and carried with
provenance, **a wrong number is worse than a refusal**, and this one arrives with the digest
attached. That is the argument. Everything below is why the route would have been expensive even
without it.

## What was measured

The pins are `Cargo.toml` and `Cargo.lock`; the resolutions are `cargo generate-lockfile` on
throwaway crates; the DuckDB transcripts are the `duckdb` the dev shell provides, which is the
version the adapter links.

| Fact | Value |
| --- | --- |
| DataFusion, pinned | `55.0.0`, `default-features = false`, features `parquet` and `datetime_expressions` |
| DataFusion, latest published | `55.0.0` |
| Arrow, in the lock | `59.2.0` under DataFusion **and** `58.4.0` under duckdb. Two majors, already |
| duckdb crate, pinned | `1.10505.0`, which requires `arrow ^58`, not optional |
| DuckDB library | `v1.5.5 (Variegata) d8cdaa33fd` |
| `sqlparser` in the lock | absent. The dialect layer is `polyglot-sql 0.9.2` |
| `datafusion-federation`, latest published | `0.5.5`, requiring `datafusion ^54` |
| `datafusion-federation`, repository main | the same requirement, `54` |
| `datafusion-table-providers`, latest published | `0.13.1`, requiring `datafusion ^54.0` and `arrow ^58.0` |
| Its DuckDB provider | requires `duckdb =1.10505.0` - our exact pin - plus `sea-query`, `snafu`, `secrecy`, `r2d2` |

### What the resolver does with it, and what the compiler then says

| Probe | Packages | What arrives |
| --- | --- | --- |
| The pinned DataFusion and its two features, alone | 239 | no `sqlparser`, no `datafusion-sql`, no `bzip2` |
| The same, plus `datafusion-federation = "0.5"` | 334 | `datafusion 54.1.0` **and** `55.0.0`; `arrow 58.4.0` **and** `59.2.0`; `datafusion-sql 54.1.0`; `sqlparser 0.62.0`; `bzip2 0.6.1` |
| DataFusion held at `54.1.0`, federation with `default-features = false` | 286 | one DataFusion, and still `datafusion-sql 54.1.0`, `sqlparser 0.62.0`, `bzip2 0.6.1` |

**Two DataFusion majors resolve rather than conflict, and then do not compile.** Cargo is content:
they are different semantic majors, so it links both. The compiler is not, and the message is the
finding - built, not reasoned:

```text
error[E0308]: mismatched types
   | let _ctx = SessionContext::new_with_state(state);
   |            expected `SessionState`, found a different `SessionState`
note: there are multiple different versions of crate `datafusion` in the dependency graph
   | datafusion-55.0.0/src/execution/session_state.rs  this is the expected type
   | datafusion-54.1.0/src/execution/session_state.rs  this is the found type
```

**The SQL parser comes back, and the third row is why holding DataFusion back does not avoid it.**
Federation depends on `datafusion` with default features, and features unify across a build graph,
so the `sql` feature is on whatever this workspace asks for. `Cargo.toml` turns it off deliberately,
and its comment is the strongest sentence in that file: not calling a parser is a convention, not
having one is a property. Adopting federation compiles `datafusion-sql` and a second SQL parser -
`sqlparser 0.62.0`, beside `polyglot-sql` - into the shipped binary.

**A licence the allowlist does not carry comes back with it.** The default features include
`compression`, which brings `bzip2 0.6.1` and through it `libbz2-rs-sys`, whose registry licence
field is `bzip2-1.0.6`. `deny.toml` allows eight licences and that is not one of them, and
`unused-allowed-license = "deny"` means the list is exactly the graph rather than a wish list. Not
run: `cargo deny` over the probe. What was checked is the licence string and the allowlist.

### What the pushed-down half executes, and what it does not prune

`SQLExecutor`'s executing method takes one string:

```rust
fn execute(
    &self,
    query: &str,
    schema: SchemaRef,
    filters: &[Arc<dyn PhysicalExpr>],
) -> Result<SendableRecordBatchStream>;
```

There is no parameter list, and DataFusion's own unparser produces the string. So the statement that
executes is rendered by DataFusion rather than by `sutura-sql` and no golden covers it, and every
value from the question is inside that string as text, because there is nowhere else for it to be.
The second of those is a row in the invariants table and it is not a row that bends. The first is the
worry ADR 0003 recorded from the other side: that unparser has known dialect defects, which is why
rendering for a remote data system stayed with the dialect layer.

Observed in the same run, and worth recording because it is the opposite of what a federation layer
is assumed to buy: **it did not prune the projection below its own boundary.** The cross-source leg
shipped `SELECT "orders"."order_id", "orders"."rep_id", "orders"."day", "orders"."amount"` when only
two of those four columns were needed above it.

### The two-provider route does not compile either

Handing the driver's Arrow batches straight to the engine is the obvious shape, and at these pins it
is a type error. Built, not reasoned:

```text
error[E0308]: mismatched types
   | ctx.register_batch("t", first)?;
   |     expected `datafusion::arrow::array::RecordBatch`, found `arrow::array::RecordBatch`
note: there are multiple different versions of crate `arrow_array` in the dependency graph
   | arrow-array-59.2.0/src/record_batch.rs  this is the expected type
   | arrow-array-58.4.0/src/record_batch.rs  this is the found type
```

Two bridges exist and both cost something. Arrow IPC is safe and copies every buffer on the way out
and again on the way in. The Arrow C data interface is version-stable across majors by design, and
**at these pins there is nothing to call it with**: the pinned duckdb crate has no
`ArrowArrayStream` surface at all - checked, zero occurrences in its source - only a `stream_arrow`
returning batches of the older Arrow. Obtaining a C stream therefore means raw FFI through
`libduckdb-sys`, and `unsafe_code = "forbid"` means first-party code cannot write it. Precise about
the Arrow side: its reader's `try_new` is safe and only `from_raw` is `unsafe`, so the ban bites on
getting the stream out of the driver rather than on Arrow's own API.

### The numbers, on five million rows

Same question, same rows, all four legs agreeing on the answer. Warm runs; the three Rust legs are
**debug builds**, so read the memory as the finding and the times as an ordering.

| Leg | Time | Peak resident |
| --- | --- | --- |
| Several databases attached to one DuckDB connection | 0.01s | 65 MiB |
| A provider per source, each pushing its own filters | 0.10s | 123 MiB |
| The federation layer, at DataFusion 54 | 0.09s | 119 MiB |
| A provider per source, pulling whole tables | 0.46s | 767 MiB |

Only the last row is bad, and it is bad in the way that matters: it is the one that pulls rows into
this process. Federation buys nothing over a hand-written pushdown here, and attaching is an order
of magnitude cheaper than both.

**And a hand-written pushdown fails silently.** The same provider, against a column typed
`TIMESTAMP` instead of `DATE`, emitted `SELECT * FROM orders` for five million rows: 0.54s and 896
MiB, the right answer, no error, no warning. Its renderer returns "no SQL" for a literal type it
does not cover, and "no SQL" means the filter is simply not pushed. Reproduced.

### What DuckDB does today, verified

Two database files, each attached read-only, DuckDB 1.5.5:

- A join across them returns the right rows, and its plan shows both filters and both projections
  pushed into the two scans.
- A write through a read-only attachment is refused:
  `Invalid Input Error: Cannot execute statement of type "INSERT" on database "src_a" which is
  attached in read-only mode!`
- A prepared statement with bind parameters spans them.
- **An unqualified table name does not resolve.**
  `Catalog Error: Table with name regions does not exist! Did you mean "src_b.regions"?`

That last one matters, because a statement this repository generates says `FROM "orders"` and
nothing else: a table in a plan is one identifier and the generator force-quotes it. There are two
ways to make an unqualified name resolve, and **they differ in the way that decides which one is
allowed here.**

`SET search_path` works, and with the same table in two attached databases it answers from the first
one silently. Reproduced: two databases each holding `orders`, `search_path` naming both, and the
aggregate is the first database's rows with no diagnostic anywhere. A number under a certified metric
name, from a source nobody chose.

One view per model in the session's own catalog also works, and a second `CREATE VIEW` of the same
name fails: `Catalog Error: View with name "orders" already exists!` A collision stops the startup
instead of picking one.

And then the part that decides it. **The golden statement runs unchanged.** Taken verbatim from
`crates/sutura-cli/tests/snapshots/subscription-months-by-region-and-term__statement.snap`, its `?`
placeholders and its `LIMIT 10001` included, executed over views on two separate database files: the
right three rows. Not one byte of generated SQL differs from the single-database case, so no
statement snapshot is regenerated.

## Identity: what each route forecloses

AGENTS.md's first paragraph says end-to-end impersonation is the point of this product. None of the
three routes delivers it today, and they differ in whether they *could*. This is the sharpest
distinction in this record, and it is not in the goal's framing at all.

| Route | Can a leg ever run as the subject who asked? |
| --- | --- |
| Attaching several databases to one connection | **No, structurally.** One process, one connection, one operating-system identity for every attached file. `ATTACH` takes no credential, and a networked scanner's credential would be a connection-global secret rather than a per-request one |
| The federation layer | **No.** `SQLExecutor::execute` receives a string, a schema and physical filters, and no session. Worse for this purpose: the layer's fusion key is the adapter's `compute_context`, so identity would have to be *inside* that string or two subjects' sub-plans could fuse into one - observed fusing in the spike when two sources shared a context |
| A provider per source | **Yes, reachably.** `TableProvider::scan` receives `state: &dyn Session`, and a session exposes typed per-request extensions, so two providers can be two credentials. One hazard to design around rather than discover: `ExtensionOptions` requires an `entries` method that renders its values, which is exactly the shape `Secret` exists to prevent |

So the cheapest route is also the one that **forecloses** the product's headline property for that
leg. In single-player mode the foreclosure costs nothing, because there is one subject: one person,
one process, one set of file permissions, and a local database file has no login, so there is nobody
else to be. It costs something later, and this record says so rather than leaving it to be
discovered: **code written against one attached connection cannot grow into per-subject execution.**

## Where the one-source assumption actually lives

A survey of the tree found **six** independent single-source assumptions, and the refusal everybody
names is neither the first nor the most important. In the order a question meets them: the startup
refusal in both composition roots, which rejects a catalog naming more than one source; the
compile-time engine-source constant the declared source must equal; the service holding exactly one
warehouse; the plan stage's source set, which refuses `PlanSpansTwoSources`; the plan having one
source field while a join carries only a table name; and `answer` refusing when the plan's source is
not the warehouse's.

**In either shipped binary the plan-stage refusal is unreachable.** The startup refusal fires first,
before a socket is opened, so a deployment cannot load a two-source catalog at all. Deleting the
plan-stage check would have no observable effect on a deployment - worth knowing precisely so that
nobody reads it as permission.

**Removing it would produce a wrong answer rather than an error.** The plan stage collects the joined
model's source into its set and then throws it away: the plan is stamped with the metric's own
model's source alone. So `answer`'s guard compares one name against the same name and passes, and the
engine resolves the joined table in a flat, unqualified namespace out of the one data directory. If
the file is there, the engine joins it and returns rows under the metric's certified name and the
bundle's digest, with the second source having decided nothing at all. This repository already fixed
the same shape from the other side: an engine that named itself after the catalog's declared source
made the source comparison vacuously true, and the test that now covers it records that the call
previously returned an engine because the file was there and nothing else failed either.

**The startup refusal, the one that actually holds today, has no test.** Its wrong-name branch is
covered; its more-than-one branch is not. By this repository's own standard that is a mechanism
nobody has watched fail.

## Decision

**`datafusion-federation` is declined at these pins.** First because it returned a transposed
aggregate with no error, under a cast step that is documented to trust column order and an unparser
that does not preserve it. Then because it does not support the DataFusion this workspace runs, and
the two ways to make it fit are to hold the engine at a major a contrib crate chooses for us or to
link two engines, which does not compile. And then because it brings a second SQL parser and a
disallowed licence into the binary, executes a statement rendered by somebody else with the
question's values inlined in it, and did not even prune the projection below its boundary.

**Reach across several DuckDB databases is delivered below the port, as one data system.** One
adapter, one connection, each database attached read-only, one view per model in the session's own
catalog. The catalog declares one source; the plan names one source; the statement is the one
`sutura-sql` already renders. It is an order of magnitude cheaper than either alternative and it
changes no generated SQL.

**A provider per source is named as the shape federation grows into, and it is not built now.** It is
the only one of the three routes where a leg could ever run as the subject who asked, because a
provider's scan receives the request's session. Today it does not compile at these pins, for the
Arrow reason above. Recording it here is what stops the cheap route from being mistaken for the
direction.

**None of the six assumptions is relaxed.** They were obstacles to the route the goal named, not to
the goal. The plan-stage refusal is kept exactly as it is, and no plan spans two sources, because
several databases behind one connection are one source.

**This capability is named for what it is.** It is not federation, and it is not impersonation. This
record does not let it be called either later.

### The mechanisms, and where each one lives

| What must hold | What makes it hold |
| --- | --- |
| A model's table resolves to exactly one database | One view per model, created at startup with `CREATE VIEW` and **not** `CREATE OR REPLACE VIEW`. The existing CSV affordance on that adapter uses `OR REPLACE`, so copying it would silently rebind a colliding name - the same failure as `search_path`, arrived at from the other direction |
| Every model the served bundle names has a table behind it | The shape the HTTP composition root already uses for the engine: collect what registration actually succeeded, then refuse anything the served bundle needs and the set does not have |
| The attach affordance cannot become a general SQL hatch | It takes an alias and a path and nothing else, the way the CSV affordance takes a table and a path. That adapter's own doc comment states the rule: a narrow typed affordance rather than a run-this-SQL method, which is what would make every check upstream optional |
| The catalog cannot widen what is opened | The list of database files is configuration, read by the composition root. A catalog document names a table, never a file, so a catalog edit cannot reach a database the deployment did not attach |
| Nothing is written | Every attachment is read-only, and DuckDB enforces it, reproduced above |
| There is one execution leg | The adapter holds one connection field. A second connection is a second field or a collection, in a file the line cap keeps readable. **Structural, not gated** |
| The generated SQL does not change | The statement snapshots, unchanged. A route needing qualified table names would have moved every one of them, which is the loud version of this same check |
| The startup refusal it relies on is watched | It gets the test it never had, for the more-than-one branch, before anything is built on top of it |

### What replaces the one-source rule on the day it does change

Stated now, because the day it changes is the day somebody will want to write the smallest possible
diff, and the smallest possible diff is the one that produces the wrong number described above.

The plan stage collects sources into a set and refuses unless exactly one is in it. Real federation
does not delete that; **it re-keys it.** The set stops holding source names and starts holding the
identity each source resolves to, and the refusal - a plan spanning two identities - fires unless
exactly one is in it. Two sources that resolve to one subject are then allowed and two subjects are
refused, which is the property that was always meant.

**That check cannot be written today, and the reason is a missing type rather than a missing test.**
`sutura_domain::identity` holds `Secret` and nothing else: no principal, no request context, no
credential broker. Nothing reaches `Warehouse::execute` that could say who is asking.

And the ordering matters more than the wording. The plan-stage refusal is the **last** thing to move,
not the first, because everything that would make a multi-source plan mean anything is below it: a
source that selects an adapter rather than being compared for equality; qualification in the engine's
table namespace, which is bare and single-level today; per-source configuration, where there is one
data directory and one source-name constant; a service that holds more than one warehouse; and the
two startup refusals, retired one at a time and each with a test on the way out. Only then the plan
stage. Moving it first is exactly the change that compiles, passes, and answers wrongly.

One further guard becomes load-bearing at that point: the load-time refusal of a dimension reached
through a relationship whose declared cardinality may duplicate rows. Today a cross-source
relationship passes that check and trips the plan-stage one. Make cross-source joins legal and the
duplication guard is the only thing between a cross-source join and a wrong sum, resting on a
declaration nothing compares against the data - and the reconciliation test behind it needs a fixture
whose unmatched key sits on the far side of the boundary.

### What stops this from becoming multi-player federation quietly

**A plan cannot represent two sources.** The plan holds one source name, and a join and a column hold
none: a table in a plan carries no source at all. Making a two-source plan representable means
changing a domain type whose serialized form is pinned by snapshots, and the definition digest is
taken over that same serialized form, so the first commit of any such attempt moves goldens and moves
the digest. That is louder than any check, and it is why this decision spends nothing on the plan
type.

**Both composition roots refuse a multi-source catalog at startup**, and also refuse a single source
whose name is not the one the linked adapter answers for. Under this decision that check is
untouched, and the multi-database adapter answers for one name like every other adapter.

**And the shape that would walk around all of it, named so it cannot arrive unnoticed:** an adapter
that composes two others, implements the port itself, and reports a synthetic source name. It passes
the startup check, it passes the plan stage, and it executes a two-identity question. Nothing
mechanical forbids it. What this record does is refuse it in advance and say why: it produces
precisely the state the one-source rule exists to prevent, while every check reads green.

## The security position

Written the way the `SourceUnavailable` doc comment is written, because overstating this would be
worse than not saying it.

**No security property currently held is at stake here, because none is held.** There is no
per-caller identity in this workspace: the configuration module says so, the function that answers
*does anything here establish who the caller is* is a constant `false` printed on every boot, the
bearer token authenticates the deployment, and no subject reaches the query path. The one-source rule
is a *placeholder* that keeps that gap visible, and a correctness control - see the wrong answer
above - rather than an identity control. `examples/multi-player` exists as an empty directory whose
README says the same thing and names the missing port.

What single-player mode makes true is narrower than it sounds, and it is exactly what this decision
leans on: **there is one execution leg, so "every leg runs as the same subject" holds by construction
rather than by comparison.**

What it does not deliver is in the identity table above, and it is a foreclosure rather than a gap:
attaching cannot express two credentials, and the day two are needed is the day this stops being one
data system. The least-authority half is the read-only attachment, verified to be enforced by DuckDB
rather than by our intention.

## The tool surface: there is nothing to widen

A federated question is the same question. The caller names a metric; the metric names a model; the
model names a source; a dimension's relationship names a target model which names a source. **Every
source is derived from the pinned bundle, and the caller has no field that could name, add, remove or
hint at one.** So AGENTS.md's row for a new or widened tool input does not apply, now or when real
federation lands. Provenance does not change either: the digest already covers every model's source,
so a catalog that moved a model to a second system already produces a different digest.

One limit, stated so nobody over-claims in the other direction: the mechanism that row cites - dumped
tool schemas and a byte-compare - is not built. `schemars` is not a dependency, and AGENTS.md's own
canonical-sources table marks it planned. What is built and does hold is `deny_unknown_fields` on
both the domain query type and the wire body, with a test that provokes it.

## What does not change

| Guarantee | Still held by |
| --- | --- |
| A plan resolves to exactly one data system | The plan stage's source set, unchanged. Several databases behind one connection are one data system, which is what makes this decision cheap rather than what makes it a loophole |
| No value from a question reaches the statement as text | Unchanged, and it is the specific thing the declined route could not offer: the statement and its parameters stay separate fields, and a prepared statement with parameters spans attached databases, verified |
| The executed SQL is owned by `sutura-sql` | Unchanged. There is no second generator here, and the statement snapshots are the evidence: the golden ran across two databases byte for byte as it stands |
| We never translate SQL, and the one thing we parse is parsed at load | Unchanged, and nothing here parses anything. Precise about the declined route too: its unparser is a generator rather than a transpiler, so what it would have broken is the ownership of the executed statement, not the transpile ban |
| A result that hit the row cap is refused, not truncated | Unchanged. One statement, one `LIMIT`, one result to measure |
| No result cache | Unchanged, and nothing here buffers anything: DuckDB executes one statement over its own storage and returns the finished aggregate. Both declined routes buffer inside this process, and the measurements above are how much. **Not a cache**, because nothing is keyed and nothing is reused across questions, and the honest way to say it is that reuse across questions is what would make it one, at which point AGENTS.md's rule applies and it is keyed on subject first or not at all |
| Adding a data system is a registration, not a test edit | Unchanged, and narrower than it reads: the test matrix's axis is *which adapter*, one warehouse per cell, one source name shared by all of them. A second SOURCE is not a registry line, it is a signature change on the harness - another reason this decision adds no source |
| The domain acquires no framework dependency | Unchanged. This decision adds no dependency at all |
| Refusal is a result, not an error | Unchanged. Everything new here refuses at startup, which is a process that does not come up rather than a question that gets a status |
| Every query runs as the calling principal | Not held on this path, and now recorded as *foreclosed* on it rather than merely absent. See the identity table |

## Consequences

- **The stated approach is declined and the goal is still delivered.** One question over several
  DuckDB databases is buildable now, in the adapter, with no new dependency and no assumption
  relaxed, at a tenth of the cost of either alternative.
- **The DuckDB adapter would have to ship, and today it does not.** It is a development dependency of
  `sutura-app`'s tests, and that is not incidental: nixpkgs has no musl `libduckdb`, and the
  cross-built artifacts link the engine alone for exactly that reason. So this capability arrives
  either behind a feature the musl artifacts do not enable, or in a glibc artifact only, or by
  changing what the release builds link. **This record does not decide which**, and it is the same
  shape of gap [ADR 0004](0004-a-named-escape-hatch-for-authored-sql.md) ended on.
- **On the served surface it needs more than an attach affordance, and this is a cost this record
  found rather than inherited.** A DuckDB connection is `Send` and **not** `Sync` - it holds a
  `RefCell`, and a compiler error says so - while the service requires its warehouse to be
  `Send + Sync + 'static`. Today's adapter is used by tests that call the answering function
  directly, so nothing has needed that bound of it. Serving it therefore means one of: a mutex, which
  makes every concurrent question queue behind one connection and is the ceiling the engine's
  worker-thread rationale was written about; a connection pool, where every pooled connection has to
  re-attach and re-register before it can answer; or the command-line path only. **Not decided here.**
- **DuckDB's own attach is not exposed today.** That adapter has open, in-memory and a CSV view
  affordance, and nothing else. The new affordance is small, and the row above about keeping it narrow
  is the whole of its design.
- **A hand-written pushdown renderer is a silent-degradation surface**, measured: one uncovered
  literal type turned a filtered scan into a full one, with the right answer and no diagnostic. If
  the provider-per-source route is ever built, "the filter was not pushed" has to be observable -
  a refusal, a log line, or a test that asserts the pushed statement - rather than a performance
  mystery.
- **The engine's half of the same capability needs no ADR and almost no code.** Where the data is
  files rather than DuckDB storage, the engine already holds a table per model in one session and
  joins them; the only thing tying them together is that the composition root derives every path from
  one directory. Per-model locations are a configuration change with a startup refusal. What it does
  not survive is a name collision: the engine's table namespace is bare and single-level, so two
  sources whose tables share a name would collide with no source name available to disambiguate them.
- **A multi-source fixture belongs in `examples/multi-player`, not in the single-player corpus.**
  Moving a model in the existing corpus to a second source was costed and is not viable as an
  incremental step: the definitions and digest snapshots move, every question joining the moved model
  flips from planned to refused, and the SQL golden count drops - which moves a sentence in AGENTS.md
  that a gate counts. The single-player corpus is arranged to avoid exactly this, and its own fixture
  says so.
- **The engine stays behind the port.** AGENTS.md says it belongs above the port once federation
  lands, and that stays true and stays a design target. This record narrows it: it moves when a plan
  may span two identities, and *several databases under one identity is not that day.* What "above the
  port" means concretely: the port stops carrying a whole plan and a finished aggregate and starts
  carrying a fragment and intermediate rows, which gives up the property that push-down is complete
  today, and the part that is not pushed down becomes ours to filter - the second policy
  implementation this design declines to keep. The measured 767 MiB row is what that looks like when
  it goes wrong.
- **A join that is not pushed buffers a side whole, by design.** The engine's hash join reported
  `mode=Partitioned` in the spike's plans, and the alternative it chooses elsewhere collects one side
  entirely. That is not a defect; it is precisely AGENTS.md's row about anything that stores or
  forwards rows, which has no mechanism and is a human review question. Flagged rather than resolved.
- **Version skew is a measured cost rather than a note.** ADR 0003 said there is real skew and used it
  as a small reason federation is later. It is bigger than it looked: the crate tracks the previous
  major, so adopting it hands the version of the thing that executes every query to a contrib crate's
  release schedule.

## What is explicitly not decided

- **Which artifact ships DuckDB**, and whether the multi-database adapter is feature-gated, glibc
  only, or a change to the release builds. The blocker is the missing musl library, not a preference.
- **How the served surface gets a connection that is `Sync`** - mutex, pool, or not at all. See the
  consequence above; each answer has a different cost and none of them is a detail.
- **Whether DataFusion 56 moves to Arrow 59, and whether the federation crate follows.** That single
  event would remove both the bridge problem and the skew at once, and it is the thing to watch before
  reopening either declined route. Nothing here predicts it.
- **Whether the column-order defect is reported upstream.** It reproduces in about twenty lines and
  the repair is to reconcile by name; whether that becomes an issue, a pull request, or neither is a
  choice about how this project spends its time, not a technical question.
- **Whether the engine ever reads DuckDB storage directly.** At these pins it cannot, and the two
  bridges are above. The C data interface route additionally has nothing to call it with today, which
  makes it a driver question rather than a design one.
- **Whether to fork or vendor the federation crate onto the pinned DataFusion.** Not attempted. It
  would mean owning an optimizer rule in the query path, and both the unparser and the column-order
  repair would be ours afterwards.
- **Whether the port becomes per-source.** That is the "above the port" move and it belongs to the
  identity work, not to this one.
- **What a per-leg credential is.** There is no principal type and no credential broker, and this
  record adds neither. The identity table says which route could carry one.
- **Anything that stores or forwards rows.** AGENTS.md says nothing mechanical governs it and that it
  is a human review question rather than an agent's to certify. Nothing here proposes to, and the
  hash-join note above is flagged for the same reason.

## Alternatives considered

**Adopt the federation crate and hold DataFusion at the previous major.** The honest version of "just
use it", and the only one where a single engine links. Rejected first on the transposed aggregate,
which is a wrong number rather than a cost. Then on the rest, all measured: the engine that executes
every query would sit on the version a contrib crate happens to target; the graph grows by 47
packages; and the SQL parser and the disallowed licence arrive anyway, because feature unification
does not care that we asked for neither.

**Adopt it alongside the pinned DataFusion.** Resolves, then fails to compile, with two
`SessionState` types in one graph. The error text is above.

**Fork it onto the pinned DataFusion.** The only route that keeps both the current engine and the
crate's optimizer rule. Rejected for now rather than on principle: it puts a fork of a query-path
optimizer under our maintenance, and after the fork the column-order repair and the unparser's
dialect defects are both ours.

**Use the contrib DuckDB table provider.** Attractive on one detail - it pins the same duckdb version
this workspace does - and rejected on the rest: the previous DataFusion major, the previous Arrow
major, and a normal dependency list that adds a third SQL builder, a second error library and a second
secret type to a workspace with considered opinions about all three.

**Write a first-party provider per source.** No federation crate, no second SQL generator, and the
only route where a leg could run as the subject who asked. Blocked at these pins by the Arrow major
gap, and carrying the silent-degradation hazard measured above. **Named as the direction rather than
rejected**, which is the difference between this and every other entry here.

**Several directories for the engine, over Parquet and CSV.** The cheapest thing in this document and
the sibling of the decision rather than a competitor: it delivers the same goal where the data is
files. Adopted in that scope, and it is a configuration change rather than an architecture one.

**Export each database to Parquet and answer over the exports.** Works today with no code at all, and
worth naming because it is the baseline every option above has to beat. It fails on freshness and it
doubles the storage, and a copy is read under whoever made it - the same objection ADR 0003 raised
against an acceleration layer, arriving by hand.

**`SET search_path` instead of one view per model.** Fewer moving parts, one statement at startup, and
rejected on the reproduction above: with the same table in two attached databases it answers from the
first and says nothing.

**Relax the plan-stage refusal and let the engine sort it out.** The change the goal's framing
invites. With both guards gone the plan carries only the metric's own model's source, the engine
resolves the joined table in a flat namespace out of one directory, and a question reaching across two
nominal sources is *answered* under the metric's certified name and the bundle's digest. Not a
refusal, not an error - a number. The same failure mode as the federation defect above, arrived at
from inside our own code, which is why both are in this record.
