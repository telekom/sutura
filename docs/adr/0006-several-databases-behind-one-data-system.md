---
title: Several databases behind one data system, declined
description: Two ways to answer one question over several DuckDB databases, both declined and why - the wrong number a federation layer returned at these pins, and the per-subject impersonation that attaching several databases to one connection forecloses - with the measurements, the Arrow gap, the identity table, and a pointer to the record that decides what is built instead.
---

# Several databases behind one data system, declined

Status: accepted, and it decides two **declines**. Nothing here is built and nothing here asks to
be built. It supersedes nothing and amends nothing:
[DataFusion for local execution](0003-datafusion-for-local-execution.md) said federation is
explicitly later, and this record is what *later than what* turned out to mean once both cheap routes
were run rather than reasoned about.

The goal it was asked about is one question spanning several data sources, initially several DuckDB
databases. Two routes promised to deliver it cheaply. **Both are declined**, for different reasons
and with different evidence, and what is built instead is decided in ADR 0007,
*Federating across different data systems*: every data source is first class behind the `Warehouse`
port, reached by the semantic compiler rendering that source's dialect, with standard credentialed
access; one source is a whole-query pushdown, several sources are per-source legs that DataFusion
combines. **There is no DuckDB special case**, and that is the sentence this record exists to
support with numbers.

A record of two declines is worth as much as a record of a decision, and for the same reason: the
next person to reach for either route needs the evidence rather than the verdict. So the evidence is
kept whole below, including the parts that were the argument *for* a route before they became the
argument against it.

**Which claims were run, and which were reasoned.** Every measurement was produced by a throwaway
spike - crates with their own `[workspace]` table, outside this repository and not merged into it -
and re-run independently while this record was written. Where a claim was *not* executed it says so
in the sentence that makes it. That distinction is the whole credibility of this document, so it is
kept per claim rather than declared once.

**And a third category, because "run" is not "reproducible" and this record used to blur them.** The
spike is not in this repository and cannot be reached from it, so for a reader "run" means *executed
twice by the author* rather than *checkable from here*. A record whose credibility rests on the
run/reasoned distinction owes the reader the version of that distinction which holds for the reader:

- **Reproducible from this record alone.** Every version resolution, from `cargo generate-lockfile`
  over the requirements in the table below. Both compiler errors, quoted verbatim with the two
  versions in each graph named beside them. Every DuckDB transcript, because the statements are
  quoted and the library is the pinned `v1.5.5 (Variegata) d8cdaa33fd` the dev shell provides. The
  disallowed licence, because it is a registry field and `deny.toml` is in this repository.
- **Run, and NOT reproducible from this record.** The four timings below, and the transposed
  aggregate. For the second - the finding that outranks everything else here - the exact ingredients
  are listed where it is stated, so it can be rebuilt from nothing but this page. For the timings
  there is no such recipe, and the reading beside them is narrowed to what survives that.

## Context

The goal contained an ambiguity, and both declined routes lived inside it. **A data system here is a
connection and an identity, not a file.** Two DuckDB database files opened by one process, under one
set of file-system permissions, in one transaction, are one data system that stores its tables in two
places. Two data systems are two logins, two policy engines and two answers to *who is asking*, which
is the reason
[a plan resolves to exactly one of them](../architecture.md#the-engine-and-the-data-systems-behind-a-port).

Reading the goal the first way makes it cheap and makes it the wrong thing. Reading it the second way
makes it federation. Three routes were available:

1. **Below the port.** One adapter, one connection, several databases attached to it. Declined here.
2. **At the port.** A source is a source: one adapter and one credential each, legs rendered by our
   own generator, something above combining them. This is what ADR 0007 decides and this record does
   not duplicate.
3. **Above the port, by a federation layer.** A crate finds the largest subplan each source can run,
   pushes it there, and combines the results. Declined here.

## The first decline: a federation layer returned a wrong number

This goes first because it outranks every other argument in this document, and because it would not
have been found by reading manifests.

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

**What it takes to see it again, in full, so the finding does not rest on a spike nobody else can
open.** The recipe rather than the code, because the code is six declarations and a `main`:

| Ingredient | Value |
| --- | --- |
| `datafusion` | `54.1.0` - the major the federation crate requires, not the `55.0.0` this workspace pins |
| `datafusion-federation` | `0.5.5`, default features on, plus its SQL sub-crate |
| `duckdb` | `1.10505.0`, which brings `arrow 58.4.0` |
| The adapter | `SQLExecutor` implemented as the crate's own example implements it: execute the string, hand the driver's batches straight back, and **do not** reconcile the batch schema against the plan's by name |
| The table | one DuckDB file, one table, one integer group column and one exact-decimal measure column, so each casts into the other without raising |
| The question | one aggregate grouped by that integer column |

The two column types are the load-bearing part and they are not exotic: an integer group key beside
an exact decimal measure is the ordinary shape of every metric in this repository's own corpus. Make
the group key a string instead and the defect stops being a transposition and becomes a null measure
column, which is the second transcript below.

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
attached. That is the argument. What follows is why the route would have been expensive even without
it.

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
rendering for a remote data system stayed with the dialect layer. It is also the line ADR 0007 draws
between a combiner and a generator: DataFusion may combine, and never generates.

Observed in the same run, and worth recording because it is the opposite of what a federation layer
is assumed to buy: **it did not prune the projection below its own boundary.** The cross-source leg
shipped `SELECT "orders"."order_id", "orders"."rep_id", "orders"."day", "orders"."amount"` when only
two of those four columns were needed above it.

### Handing the driver's batches to the engine does not compile

The obvious shape for a source-per-provider route, and at these pins a type error. Built, not
reasoned:

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

**This constrains how a leg's rows come back, not whether the route is right.** ADR 0007's shape
sends each leg through `sutura-sql` and its own adapter, so what crosses the port is what the port
already carries; this gap is about putting the driver's own Arrow into the engine's session, which is
one implementation of that and not the only one.

### The numbers, on five million rows

Same question, same rows, all four legs agreeing on the answer. Warm runs.

**Read the build column before the numbers, because it decides which comparisons this table can
carry.** The first row is DuckDB's own release C++ executing inside the library; the other three are
first-party Rust compiled **without optimisation**. That is not a footnote, it is the reason the time
column is not a finding: a debug Rust leg against a release C++ leg measures the compiler profile at
least as much as it measures the route.

| Leg | Built as | Time | Peak resident |
| --- | --- | --- | --- |
| Several databases attached to one DuckDB connection | release C++, inside the library | 0.01s | 65 MiB |
| A provider per source, each pushing its own filters | debug Rust | 0.10s | 123 MiB |
| The federation layer, at DataFusion 54 | debug Rust | 0.09s | 119 MiB |
| A provider per source, pulling whole tables | debug Rust | 0.46s | 767 MiB |

There are two readings here and only one of them is a measurement.

**What this table does establish** is the comparison among the three Rust legs, which were built
identically and are therefore comparable to each other. **Federation buys nothing over a hand-written
pushdown**: the two middle rows are within noise of one another on both columns. And **pulling whole
tables is the one bad row** - six times the memory and five times the time of pushing the filters
down - bad in the way that matters, because it is the row that pulls rows into this process.

**What it does NOT establish is attaching against the rest.** Nothing here supports "an order of
magnitude faster", and an earlier version of this record said exactly that while ADR 0007 quoted it
back as a finding. **Withdrawn rather than qualified.** The honest claim is that **attaching buffers
the least** - 65 MiB against 119 to 123 MiB - and peak resident set is the column where the profile
mismatch does least damage, because what dominates it is the data a route holds rather than the
instructions it took getting there. Even that is an indication and not a measurement, since an
unoptimised build allocates more of its own, so it is stated as **a floor worth knowing** and never
as a ratio. Its time is not comparable to the other three at all, and no number derived from that
cell may be quoted anywhere.

**Rerunning the three in release is the cheap way to make the time column mean something, and nobody
has.** Until somebody does, a claim that attaching is faster is unsupported by this record - which is
the state a reader should be left in rather than a ratio to repeat.

**And a hand-written pushdown fails silently.** The same provider, against a column typed
`TIMESTAMP` instead of `DATE`, emitted `SELECT * FROM orders` for five million rows: 0.54s and 896
MiB, the right answer, no error, no warning. Its renderer returns "no SQL" for a literal type it does
not cover, and "no SQL" means the filter is simply not pushed. Reproduced. That is a hazard for
anything that renders per source, ADR 0007's track included, and it is why "the filter was not
pushed" has to be observable rather than a performance mystery.

### What attaching does, verified

Recorded in full, because a decline that did not establish the thing works is not a decline, it is a
guess. Two database files, each attached read-only, DuckDB 1.5.5:

- A join across them returns the right rows, and its plan shows both filters and both projections
  pushed into the two scans.
- A write through a read-only attachment is refused:
  `Invalid Input Error: Cannot execute statement of type "INSERT" on database "src_a" which is
  attached in read-only mode!`
- A prepared statement with bind parameters spans them.
- Databases can be attached with no file at all, in memory, which is what made it attractive as a
  fixture: `ATTACH ':memory:' AS mem_a` alongside `mem_b` gives two catalogs in one process with no
  path on disk.
- **An unqualified table name does not resolve.**
  `Catalog Error: Table with name regions does not exist! Did you mean "src_b.regions"?`

That last one matters, because a statement this repository generates says `FROM "orders"` and nothing
else: a table in a plan is one identifier and the generator force-quotes it. Two repairs exist and
they differ in the way that would have decided between them: `SET search_path` works, and with the
same table in two attached databases it answers from the first one **silently** - reproduced, two
databases each holding `orders`, and the aggregate is the first one's rows with no diagnostic
anywhere. One view per model in the session's own catalog also works, and a second `CREATE VIEW` of
the same name fails loudly: `Catalog Error: View with name "orders" already exists!`

And the finding that made the route look free: **the golden statement runs unchanged.** Taken verbatim
from `crates/sutura-cli/tests/snapshots/subscription-months-by-region-and-term__statement.snap`, its
`?` placeholders and its `LIMIT 10001` included, executed over views on two separate database files:
the right three rows. Not one byte of generated SQL differs from the single-database case.

So the route works, it is cheap, and it changes no generated SQL. It is still declined, and the next
two sections are why.

## The second decline: attaching forecloses the property that is the product

AGENTS.md's first paragraph says end-to-end impersonation is the point of this system. That has since
hardened from an aspiration into a requirement, and the requirement is **per source rather than per
dataset**:

> Every source declares how it establishes the identity a query runs as, as a closed two-variant
> `SourceIdentity`. `ImpersonationAtSource` - the query reaches the data as the asking subject - is the
> strong posture. `SharedServiceUser` - one identity for every caller - is the weak one, and it is
> permitted in **both** deployment modes: in single-user operation it is simply correct, because the
> one configured credential *is* that one user's, and in multi-user operation it is reachable only
> where an operator **acknowledged that specific source**, through a required per-source key that
> `Settings::refusals` checks at boot, with the operator's stated reason printed at startup beside
> the posture.

**This record used to state it as "permitted only in a single-user role that reads everything", and
that was wrong rather than merely narrow.** ADR 0008 and
[Pluggable by declaration](0011-pluggable-by-declaration.md) own the posture, and both permit an
acknowledged `SharedServiceUser` source in multi-user operation. The sentence is corrected here
rather than qualified, because two records disagreeing about what a deployment may do is worse than
either version of the rule. What survives from the old sentence is the part that was doing the work:
the weak posture must be arrived at **on purpose**, and it must be visible in the answer rather than
inferred from a deployment diagram.

**Nothing about data sensitivity is declared here.** This record previously argued the decline from
"the critical datasets are read under the asking user's own credentials in every case", which is a
per-dataset classification that [the plan](0009-the-plan-from-one-source-to-many.md) withdrew: what a
person may see lives in the data catalog and in that person's own permissions at the source, and a
classification authored here would be a second opinion about somebody else's authorization. So the
governing sentence is now an **operator obligation** and not a check - critical data goes on a source
declared `ImpersonationAtSource` - and what sutura mechanises is that `SharedServiceUser` is
unreachable by accident and unreachable in silence.

None of that changes the cost this section prices, and that is why the correction is safe to make
here: **a route where no leg can EVER run as the caller fails the requirement at every resolution**,
per dataset, per source or per deployment. ADR 0008 is the record of the requirement and this section
states only what the route in front of it costs.

Against that, the three routes are not equivalent, and the difference is structural rather than a
matter of effort:

| Route | Can a leg ever run as the subject who asked? |
| --- | --- |
| Attaching several databases to one connection | **No, structurally.** One process, one connection, one operating-system identity for every attached file. `ATTACH` takes no credential, and a networked scanner's credential would be a connection-global secret rather than a per-request one |
| A federation layer | **No.** `SQLExecutor::execute` receives a string, a schema and physical filters, and no session. Worse for this purpose: the layer's fusion key is the adapter's `compute_context`, so identity would have to be *inside* that string or two subjects' sub-plans could fuse into one - observed fusing in the spike when two sources shared a context |
| A source per adapter | **Yes.** A whole plan - never a fragment - crosses the port to one adapter that owns one connection, which is where a per-request credential belongs. ADR 0007 decides which plan shapes those are and the property that matters here holds for all of them: one plan, one source, one credential. Within the engine the same holds by a second route: `TableProvider::scan` receives `state: &dyn Session`, so per-request extensions are reachable. One hazard to design around rather than discover: `ExtensionOptions` requires an `entries` method that renders its values, which is exactly the shape `Secret` exists to prevent |

**So the cheapest route forecloses the headline property, and the weak posture is the only one it
could ever declare.** That was survivable while the goal was one person on a laptop, and it stays
correct for a single-user deployment - which is the mode `examples/single-player` is, and a first-class
one rather than a degraded one. It is not survivable as an architecture, and the reason is narrower
than the sentence this paragraph used to carry: not that some datasets are classified critical, since
nothing here classifies data, but that **a source served under one identity for every caller is a
posture an operator has to be able to choose per source** - and attaching removes the choice. Every
attached database inherits the one process identity, so a deployment that wanted one source
`ImpersonationAtSource` and one `SharedServiceUser` has no way to say so, and no way to record which
posture produced an answer. A connection with one operating-system identity cannot be told who is
asking, so it cannot be the strong posture for anything.

There is a second, sharper version of the same point, and it is the one that decides the case rather
than merely weakening it. **The cheapest next step from attaching two local databases is attaching a
Postgres**: one extension, no new Rust, the same code path. That step is the composing adapter with a
synthetic source name that this record refuses in advance below. Under it a catalog declares one
source, the plan names one source, the plan-stage set has one member and passes, and *a plan cannot
silently span two sources* reads green over a two-identity question. Attaching two local files is
harmless. Attaching a Postgres is the refused shape, and the distance between them is one line of
configuration.

For completeness, since it was the blocker while the route was still a candidate: **a DuckDB
connection is `Send` and not `Sync`** - it holds a `RefCell`, and a compiler error says so - while the
service requires its warehouse to be `Send + Sync + 'static`. A shared connection was therefore never
the served shape without a mutex that queues every concurrent question or a pool that re-attaches per
connection. It is a note rather than an argument now: the route is declined on identity, not on a
trait bound. The bound does not go away, though. Every connection-holding adapter meets it, so it is
inherited by the network adapters ADR 0007 plans rather than retired with this decline.

## The third decline: it is not a good test fixture either

Attaching two in-memory databases is a genuinely attractive fixture: no container, no daemon, no
temporary directory, and 0.01s. It is still the wrong one, and the reason is the same reason the
architecture declines it.

**A fast-tier test that attaches two databases to one connection exercises a shape this system does
not ship.** Two DuckDB connections - one per source, one adapter each - is precisely the production
shape of ADR 0007's several-source track, in the dialect that already has goldens. A test whose setup
differs structurally from production is the kind of coverage that reads green while proving something
else, and this repository has already paid for one of those: an engine that named itself after the
catalog's declared source made a source comparison vacuously true, and the test that now covers it
records that the call previously succeeded because the file was there and nothing else failed either.

Two things make the choice easier than a purity argument would:

**The production-shaped fixture needs no new adapter code, and the attached one does.**
`DuckDbWarehouse::open`, `::in_memory` and `::attach_csv` already exist, so two connections over two
in-memory databases with a CSV view each is a fixture written entirely in a test. Attaching needs a
new `attach_database` affordance, a view registration per model that uses `CREATE VIEW` rather than
the existing affordance's `CREATE OR REPLACE VIEW`, and a collision refusal behind it - new
production code, kept narrow, for a shape nothing ships.

**And neither is needed yet.** The two-source refusal is already provoked by fixtures built in Rust
with no data system at all, in `crates/sutura-app/tests/support/oracle.rs` and
`crates/sutura-http/src/testing.rs`, and that file mentions DuckDB zero times. A DuckDB-backed
multi-source fixture becomes useful the moment there is a combiner to test, which is ADR 0007's work,
and at that moment the right one is two connections. Building the attached one now would be work that
gets thrown away, which is the same conclusion from the other end.

## What is declined, precisely

**`datafusion-federation` is declined at these pins.** First because it returned a transposed
aggregate with no error, under a cast step documented to trust column order and an unparser that does
not preserve it. Then because it does not support the DataFusion this workspace runs, and the two ways
to make it fit are to hold the engine at a major a contrib crate chooses for us or to link two
engines, which does not compile. And then because it brings a second SQL parser and a disallowed
licence into the binary, executes a statement rendered by somebody else with the question's values
inlined in it, and did not even prune the projection below its boundary.

**Attaching several databases to one connection is declined as an architecture**, including for
DuckDB, including in single-user operation, and including as a test fixture. It forecloses per-subject
impersonation structurally; its cheapest continuation is a shape that makes a two-identity question
read green; and as a fixture it is more new production code than the production-shaped alternative for
less relevant coverage.

**What is built instead is not decided here.** ADR 0007, *Federating across different data systems*,
decides it: every data source first class behind the `Warehouse` port, the semantic compiler rendering
that source's dialect through `polyglot-sql` with bind parameters and forced quoting intact, standard
credentialed access per source, one source pushed down whole and several sources combined by
DataFusion above the legs. No DuckDB special case, and no exception for the local file that started
this.

**None of the six single-source assumptions is relaxed by this record.** It declines routes; it moves
no check. What each track of ADR 0007 forces, and in what order, is that record's business.

## Where the one-source assumption lives, and what would replace it

Kept because it is what any future federation work has to move, and because it was surveyed for this
record rather than for that one.

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
model's source into its set and then throws it away: the plan is stamped with the metric's own model's
source alone. So `answer`'s guard compares one name against the same name and passes, and the engine
resolves the joined table in a flat, unqualified namespace out of the one data directory. If the file
is there, the engine joins it and returns rows under the metric's certified name and the bundle's
digest, with the second source having decided nothing at all. Same failure class as the federation
defect above, arrived at from inside our own code.

**The startup refusal, the one that actually holds today, has no test.** Its wrong-name branch is
covered; its more-than-one branch is not. By this repository's own standard that is a mechanism nobody
has watched fail, and it is worth a test whichever route eventually lands.

And what replaces the rule when it does move: the plan stage's set stops holding source names and
starts holding the identity each source resolves to, refusing unless exactly one is in it. Two sources
that resolve to one subject are then allowed and two subjects are refused, which is the property that
was always meant. **That check cannot be written today, and the reason is a missing type rather than a
missing test:** `sutura_domain::identity` holds `Secret` and nothing else - no principal, no request
context, no credential broker - and nothing reaches `Warehouse::execute` that could say who is asking.

### The shape refused in advance

Named so it cannot arrive unnoticed, and cited by ADR 0007 as the hazard at the end of the attach
route: **an adapter that composes two others, implements the port itself, and reports a synthetic
source name.** It passes the startup check, it passes the plan stage, and it executes a two-identity
question. Nothing mechanical forbids it. What this record does is refuse it in advance and say why: it
produces precisely the state the one-source rule exists to prevent, while every check reads green.

What does hold, mechanically, is narrower and worth stating beside it. **A plan cannot represent two
sources:** the plan holds one source name, and a join and a column hold none, so a table in a plan
carries no source at all. Making a two-source plan representable means changing a domain type whose
serialized form is pinned by snapshots, so the first commit of any such attempt moves the 21
`plan@markdown` goldens.

**And a correction this record owes, because it claimed a stronger mechanism than exists.** An earlier
version of this paragraph said the definition digest is taken over that same serialized form, so such
a change "moves goldens and moves the digest". **The second half is false.** `DefinitionDigest::of`
takes the `Definitions` and the `Knowledge` and nothing else, so the digest covers what a catalog
authored and not what the compiler decided; `QueryPlan` is not in it, and no change to `QueryPlan`
moves a digest. The two are pinned by different snapshots on purpose - the plan by `plan@markdown`,
the digest by `catalog_digest@markdown` - and conflating them made a plan-shape change sound like a
provenance change. What is true and is the point that survives: the digest covers every model's
`source`, so a catalog that **moves a model to a second system** already produces a different digest,
which is a fact about the catalog rather than about the plan type.

## The tool surface: there is nothing to widen

A federated question is the same question. The caller names a metric; the metric names a model; the
model names a source; a dimension's relationship names a target model which names a source. **Every
source is derived from the pinned bundle, and the caller has no field that could name, add, remove or
hint at one.** So AGENTS.md's row for a new or widened tool input does not apply, now or when real
federation lands. Provenance does not change either: the digest already covers every model's source,
so a catalog that moved a model to a second system already produces a different digest.

One limit, stated so nobody over-claims in the other direction: the mechanism that row cites - dumped
tool schemas and a byte-compare - is not built. `schemars` is not a dependency, and AGENTS.md's own
canonical-sources table marks it planned. What is built and does hold is `deny_unknown_fields` on both
the domain query type and the wire body, with a test that provokes it.

## What does not change

| Guarantee | Still held by |
| --- | --- |
| A plan resolves to exactly one data system | The plan stage's source set, unchanged and untouched by this record |
| No value from a question reaches the statement as text | Unchanged, and it is the specific thing the declined federation route could not offer: `SQLExecutor::execute` takes a string and no parameter list, while a `GeneratedQuery` keeps statement and parameters in separate fields |
| The executed SQL is owned by `sutura-sql` | Unchanged, and it is the line ADR 0007 makes load-bearing: DataFusion may combine legs and never generates one |
| We never translate SQL, and the one thing we parse is parsed at load | Unchanged, and nothing here parses anything. Precise about the declined route: its unparser is a generator rather than a transpiler, so what it would have broken is the ownership of the executed statement, not the transpile ban |
| A result that hit the row cap is refused, not truncated | Unchanged. Both declined routes would have raised the question of how an intermediate result is bounded; neither is adopted, so neither does. Where that question is answered is [the plan](0009-the-plan-from-one-source-to-many.md), and the answer is a working set in bytes rather than a row count per leg |
| No result cache | Unchanged. Both declined routes buffer inside this process and the measurements above are how much. **Not a cache**, because nothing is keyed and nothing is reused across questions, and the honest way to say it is that reuse across questions is what would make it one, at which point AGENTS.md's rule applies and it is keyed on subject first or not at all |
| Adding a data system is a registration, not a test edit | Unchanged, and narrower than it reads: the test matrix's axis is *which adapter*, one warehouse per cell, one source name shared by all of them. A second SOURCE is a signature change on the harness, which is one of the costs ADR 0007 carries |
| The domain acquires no framework dependency | Unchanged. This record adds no dependency at all |
| Refusal is a result, not an error | Unchanged. Nothing here adds a refusal or a failure mode |
| Every query runs as the calling principal | Not held anywhere today, and this record's contribution is to say which routes could *ever* hold it. Attaching cannot, a federation layer cannot, a source per adapter can |

## Consequences

- **The roadmap's stated approach is declined and so is the cheap alternative.** What remains is the
  more expensive shape, and the value of this record is that the expense is now a known quantity
  rather than a suspicion: a wrong number from one route, a foreclosed product property from the
  other.
- **The measurements survive the decline and are the baseline for the route that is built - the memory
  column of them.** 65 MiB for attaching is the floor any per-source shape is measured against; 123
  MiB is what a rendered-per-source leg buffered on the same question; **767 MiB is what pulling whole
  tables costs, and it is the number to quote whenever pushdown per leg is described as an optimisation
  rather than a requirement.** The times are carried in the table for completeness and are not a
  baseline: three of the four legs are unoptimised builds and the fourth is not, so the only valid time
  comparison is among the three, where a rendered pushdown and the federation layer are within noise
  and pulling whole tables is five times either.
- **A silent-degradation hazard transfers to the route that is built.** A renderer that returns no SQL
  for an uncovered literal type does not fail; it stops pushing. Measured at 0.54s and 896 MiB with a
  correct answer and no diagnostic. Anything rendering per source needs that to be observable.
- **The Arrow major gap is a live constraint on one implementation of the built route.** Putting the
  driver's own batches into the engine's session does not compile at these pins, and the two bridges
  are a copy per buffer or raw FFI that `unsafe_code = "forbid"` bans. It does not constrain sending a
  whole rendered leg to an adapter, which is what the port already carries.
- **Two costs are inherited rather than retired.** Which artifact links a native driver - nixpkgs has
  no musl `libduckdb`, and the cross-built artifacts link the engine alone for that reason - and how a
  serving surface holds a connection that is `Send` and not `Sync`. Both belong to any
  connection-holding adapter, so ADR 0007 carries them; declining the attach route did not remove
  either.
- **The multi-source fixture is two connections, not two attachments**, and it needs no new adapter
  code. Until there is a combiner to exercise, the existing Rust-built two-source fixtures are
  sufficient and they touch no data system at all.
- **A multi-source corpus belongs in `examples/multi-player`, not in the single-player one.** Moving a
  model in the existing corpus to a second source was costed and is not viable as an incremental step:
  the definitions and digest snapshots move, every question joining the moved model flips from planned
  to refused, and the SQL golden count drops, which moves a sentence in AGENTS.md that a gate counts.
- **The engine stays behind the port for now.** AGENTS.md says it belongs above the port once
  federation lands, and that stays a design target whose terms ADR 0007 sets. What "above the port"
  means concretely: the port stops carrying a whole plan and a finished aggregate and starts carrying
  a fragment and intermediate rows, which gives up the property that push-down is complete today, and
  the part that is not pushed down becomes ours to filter. The measured 767 MiB row is what that looks
  like when it goes wrong.
- **A join that is not pushed buffers a side whole, by design.** The engine's hash join reported
  `mode=Partitioned` in the spike's plans, and the alternative it chooses elsewhere collects one side
  entirely. That is not a defect; it is precisely AGENTS.md's row about anything that stores or
  forwards rows, which has no mechanism and is a human review question. Flagged rather than resolved,
  and it belongs to whoever builds a combiner.

## What is explicitly not decided

- **What is built instead.** ADR 0007 decides the shape and the order; nothing in this record should be
  read as scheduling it.
- **The impersonation requirement itself** and its roles. ADR 0008 is that record, and ADR 0009 settles
  the part it got wrong: nothing here classifies data, because the catalog and the asking person's
  permissions at the source do. This one only says which routes can and cannot satisfy it.
- **Which upstream release closes the Arrow gap, and which closes the version skew. They are two
  different events and this record used to name a third that is neither.** An earlier version said the
  thing to watch was "whether DataFusion 56 moves to Arrow 59" - which the table above already
  contradicts: the pinned `datafusion 55.0.0` is **already** on `arrow 59.2.0`, and a DataFusion 56
  would move nothing here. Corrected, from `Cargo.lock` rather than from memory:
    - **The Arrow gap is the `duckdb` crate.** It is pinned at `1.10505.0`, it requires `arrow ^58`
      non-optionally, and that is where `arrow 58.4.0` in the lock comes from. So what removes the
      bridge problem is **the `duckdb` crate reaching Arrow 59** - or, symmetrically, this workspace
      holding the engine at 58, which is a downgrade of the thing that executes every query and is not
      a trade anybody has argued for. That gap constrains one implementation of the built route and
      nothing about whether the route is right.
    - **The version skew is `datafusion-federation`.** It requires `datafusion ^54` on the registry and
      on its repository main alike, while this workspace pins `55.0.0`. What removes that is the crate
      following DataFusion forward, and it is independent of the Arrow question.
  Nothing here predicts either, and **the wrong-number finding would still stand after both**: the
  transposed aggregate is a cast-by-position defect in the layer, not a version mismatch, and it is
  what the decline actually rests on.
- **Whether the column-order defect is reported upstream.** The repair is to reconcile by name, and the
  ingredients for rebuilding it are in the recipe above, so a report is a matter of somebody spending
  the afternoon rather than of anything being unknown. Stated as narrowly as this record can honestly
  put it: **at these pins, with an adapter written the way the crate's own example writes one, the
  layer returned a transposed aggregate.** Whether that becomes an issue, a pull request, or neither is
  a choice about how this project spends its time, and no claim here depends on the answer.
- **Whether the engine ever reads DuckDB storage directly.** At these pins it cannot, and the two
  bridges are above. The C data interface route additionally has nothing to call it with today, which
  makes it a driver question rather than a design one.
- **Whether to fork or vendor the federation crate.** Not attempted. It would mean owning an optimizer
  rule in the query path, and after the fork both the unparser's dialect defects and the column-order
  repair would be ours.
- **Whether per-model file locations are worth having for the engine.** It is one source with several
  paths rather than several sources, so it forecloses nothing and decides nothing here; it is a
  configuration question for whoever wants it.
- **Anything that stores or forwards rows.** AGENTS.md says nothing mechanical governs it and that it
  is a human review question rather than an agent's to certify.

## Alternatives considered

**Adopt the federation crate and hold DataFusion at the previous major.** The honest version of "just
use it", and the only one where a single engine links. Rejected first on the transposed aggregate,
which is a wrong number rather than a cost. Then on the rest, all measured: the engine that executes
every query would sit on the version a contrib crate happens to target; the graph grows by 47
packages; and the SQL parser and the disallowed licence arrive anyway, because feature unification does
not care that we asked for neither.

**Adopt it alongside the pinned DataFusion.** Resolves, then fails to compile, with two `SessionState`
types in one graph. The error text is above.

**Fork it onto the pinned DataFusion.** The only route that keeps both the current engine and the
crate's optimizer rule. Rejected: it puts a fork of a query-path optimizer under our maintenance, and
after the fork the column-order repair and the unparser's dialect defects are both ours.

**Use the contrib DuckDB table provider.** Attractive on one detail - it pins the same duckdb version
this workspace does - and rejected on the rest: the previous DataFusion major, the previous Arrow
major, and a normal dependency list that adds a third SQL builder, a second error library and a second
secret type to a workspace with considered opinions about all three.

**Attach several databases to one connection and serve it.** The lightest measured route, working, with
no change to any generated statement. Rejected on impersonation: one connection is one
operating-system identity and `ATTACH` takes no credential, so every attached database has the same
posture and it is the weak one - the deployment cannot declare a posture per source, cannot be told who
is asking, and cannot record which posture produced an answer. Its cheapest continuation is the
composing adapter with a synthetic source name, which is refused in advance above.

**Attach several databases only in the fast test tier.** Tempting, and rejected on the shape rather
than on the cost: the fixture would exercise something this system does not ship, while two
connections are the production shape, need no new adapter code, and cost almost nothing more. The
existing Rust-built fixtures already cover the two-source refusal in the meantime.

**`SET search_path` instead of one view per model.** The sub-decision inside the attach route, recorded
because it is the kind of thing that gets reached for again: with the same table in two attached
databases it answers from the first and says nothing, where a second `CREATE VIEW` fails loudly. A
silent choice of source under a certified metric name is the class of failure this repository refuses
at the cost of a whole invariant elsewhere.

**Export each database to Parquet and answer over the exports.** Works today with no code at all, and
worth naming because it is the baseline every option here has to beat. It fails on freshness and it
doubles the storage, and a copy is read under whoever made it - the same objection ADR 0003 raised
against an acceleration layer, arriving by hand. It also fails the impersonation requirement for the
same reason attaching does.

**Relax the plan-stage refusal and let the engine sort it out.** The change the goal's framing invites.
With both guards gone the plan carries only the metric's own model's source, the engine resolves the
joined table in a flat namespace out of one directory, and a question reaching across two nominal
sources is *answered* under the metric's certified name and the bundle's digest. Not a refusal, not an
error - a number. The same failure mode as the federation defect, arrived at from inside our own code,
which is why both are in this record.
