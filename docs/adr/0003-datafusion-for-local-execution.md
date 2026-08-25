---
title: DataFusion for local execution
description: Why the local execution path adopts DataFusion and generates no SQL at all, why the execution port now carries a plan instead of a statement, and why federation is still later.
---

# DataFusion for local execution

Status: accepted. It adds an execution adapter and changes what the execution port carries. It
supersedes nothing: the dialect layer keeps the job of rendering SQL for a remote data system, and
federation is still later rather than now.

## Context

The local path is the one a person is on the first time they try this: a file on the same machine, no
server, no login. [The first-party models decision](0001-first-party-semantic-models.md) is why that
path exists at all, and the generator is what makes it work, producing the whole statement from a
model in the dialect of whatever will run it.

Building that generator against a local file produced one kind of bug over and over, and every one of
them was a *SQL-generation* bug rather than a bug about what a metric means:

- `DATE_TRUNC` over a date returned a `TIMESTAMP`, so the period column came back as a type the
  adapter refused.
- `GROUP BY` emitted `x AS y` when it was handed an aliased projection, which is not a grouping key.
- Aliases were emitted unquoted, because the dialect layer's force-quote flag does not cover them.
- Placeholders rendered as `?` for every dialect, including the one that needs `$1`.

Not one of those is about semantics. They are all about turning a plan into text correctly for a
target, and on this path the target is a process on the same machine as the caller. Which raises the
question this decision answers: why is there any text?

## Decision

**DataFusion is THE execution engine.** Not a second one beside `DuckDB`: the intended end state is one
cohesive engine, DataFusion for execution and `polyglot-sql` for rendering SQL when a query is pushed
down to a remote data system, taking concepts from the projects already surveyed in
[Architecture](../architecture.md#where-the-parts-come-from) - a semantic layer that compiles to a
plan, and a federation layer that decides where a subplan runs. Federation is explicitly later.

**`DuckDB`'s role changes accordingly, and it is worth being precise rather than polite about it.** It
is not a peer engine to be maintained in parallel; two engines to keep in step is a cost, not a
feature. What it is now is a **differential oracle**, and a load-bearing one: it is the only thing in
the repository that EXECUTES the SQL we render. The DataFusion path never produces a statement, so
without a real SQL engine in the loop the whole rendering half of the compiler would be checked only
by parsing it. One plan through both, rows compared, is what makes a wrong number have to be produced
twice - and it caught two real disagreements the first time it ran.

The argument that carries this is not that DataFusion is fast, or good. It is that **for local
execution DataFusion generates no SQL at all.** You build a logical plan and execute it over Arrow.
Every bug in the list above is a bug an adapter that never renders SQL *cannot have* - not one it is
less likely to have. That is a whole class of defect removed from the path that actually runs, which
is a different and better claim than a performance one.

### What made it possible: the port carries a plan

This is the part worth reading twice, because it is the actual change and the engine is the
consequence of it.

`Warehouse` used to take a rendered statement. That signature quietly asserted **that every data
system speaks SQL**, and one does not: an in-process engine executes a logical plan over Arrow and
never sees a string. So the port takes a `QueryPlan`, and *how* to execute it is the adapter's own
business - render a statement and send it, or build a plan of its own.

The plan therefore moved into `sutura-domain`, because a port speaks domain types, and a port naming
a compiler type would invert the direction
[the layout exists to keep](../architecture.md#hexagonal-by-construction). `QueryPlan` and
`Warehouse` there are the primary statement of this, and their module documentation says it at the
length it deserves.

Two smaller properties fall out, and they are what makes two adapters comparable at all:

- A plan still holds no SQL, and its serialized form is what a golden snapshot pins. What we decided
  shows up as a reviewable diff rather than as a different number.
- The result labels are defined once, on the plan. An adapter that builds an Arrow schema and an
  adapter that renders a projection cannot disagree about what the columns are called, because
  neither of them decides.

### What this costs

Stated plainly, because the weight is real and a reader deciding whether to keep this should see it.

**It is the heaviest thing in the repository.** `Cargo.lock` held 206 packages before this adapter,
and a bare `datafusion` measured at roughly 281, so one adapter adds about a third again as many
packages as everything else in the workspace resolved to put together. It brings arrow and tokio with
it.

**It lives in an adapter crate only.** The dependency-boundary check holds `sutura-domain` to serde,
thiserror and the proc-macro chain their derives need, over the whole transitive tree, and that does
not change: `datafusion`, `arrow` and `tokio` are three of the names it exists to fail on. So the
domain still compiles nothing heavy, and its test suite is still the inner loop.

**DataFusion's own SQL unparser has known dialect defects**, `DATE_TRUNC` argument order and
identifier quoting among them. That is not an argument against using it for local execution, where it
emits no SQL. It is precisely why rendering for a remote data system stays with the dialect layer
rather than moving to DataFusion. The reference implementation of this shape says the same thing from
the other side: wren-core is DataFusion-based, and a prior investigation found its dialect
correctness actually living downstream in Python rather than in its Rust core. Adopting DataFusion
therefore buys an execution engine and not a dialect layer, and planning as though it bought both
would be planning on somebody else's Python.

**There is real version skew between DataFusion and `datafusion-federation`.** One more reason
federation is later, on top of the reason that already governed it: federation is a second identity
to satisfy, and
[a plan that cannot run as one subject in both places](../architecture.md#data-systems-are-behind-a-second-port)
is refused rather than run partly as somebody else.

## What does not change

| Guarantee | Still held by |
| --- | --- |
| A plan resolves to exactly one data system | `PlanSources` asserted `len() == 1`. A second engine in the process is not federation and does not become it by being convenient |
| We never re-parse SQL we did not generate | Stronger here rather than weaker: an adapter that emits no SQL has none to re-parse, and the dialect layer's `transpile` feature is still not compiled, so a call to it does not build |
| No value from a question reaches the statement as text | For an adapter that renders, `GeneratedQuery` keeps the statement and the parameters in separate fields with no constructor that merges them. For an adapter that renders nothing there is no text for a value to reach at all: the plan carries typed parameters and each predicate names the one it binds by index |
| The domain acquires no framework dependency | The dependency-boundary check, over the whole transitive tree, unchanged. The plan moving into the domain moved a type, not a dependency: `QueryPlan` names no engine |
| No result cache | Nothing here adds one. Materializing into Arrow is a thing this class of engine is good at, and it is the half of the neighbouring project we decline - see below |
| Refusal is a result, not an error | Unchanged. An adapter's failure is its own typed error; a question that may not be asked is still refused before an adapter is reached |
| Every generated statement is valid in the data system it was generated for | Unchanged for the adapters that generate one: the goldens parse each statement in its target dialect, parse only, never re-emitting. *Gap: for an adapter that generates nothing there is no equivalent check yet. The plan defining its own result labels is what would make one possible - the same plan answered by both adapters, compared row for row - and nothing does it today* |
| Every query runs as the calling principal | Not held on this path, and this decision does not change that either way. A local file has no login, so there is nobody else to be, and `CredentialBroker` is still absent |

## Consequences

- **The local path stops being where dialect bugs are found, and that is a loss as well as a gain.**
  Executing against a local file was the cheapest way to exercise the generator end to end. With the
  local path no longer rendering SQL, the goldens carry that weight alone: they are what parse every
  statement in its target dialect, and they are now the only thing that does before a real remote
  data system is in the picture.
- **Two adapters can answer the same plan**, which is a comparison that was not previously
  expressible: the same question over the same file, once through a renderer and once through a plan
  builder, must produce the same rows. *Not built yet*, and it is the strongest test this change
  makes available.
- **The port is synchronous, so the adapter owns its runtime.** DataFusion's execution is async and
  tokio arrives with it; `Warehouse::execute` is a plain function, so the adapter blocks internally.
  That keeps tokio out of the domain and out of the service, at the price of an adapter that has to
  be careful about being called from inside somebody else's runtime.
- **Unlike DuckDB, it links no C library.** So it does not constrain the musl artifacts the way
  `libduckdb` does, and the default-off feature it sits behind is there for build time rather than
  for linkability.

## Alternatives considered

**Keep the hand-written generator and DuckDB only.** The least new weight by a wide margin, and the
option that needs no defence on size. Rejected because it leaves dialect correctness for local
execution as our burden, and that is exactly where the bugs were: four of them in one sitting, none
of them about what a metric means. An engine that needs no dialect removes the burden instead of
making us better at carrying it.

**Adopt Spice as well.** Rust, Apache-2.0, DataFusion-based, and its federation and connector layer
is the most exercised implementation of the stage we want next, which makes this genuinely
attractive. The acceleration half we cannot take, and not as a matter of configuration: a
materialized copy is read under whoever refreshed it, so under row-level security it is a cross-user
leak with a refresh schedule. Its front door is also SQL, where ours has no field for one. So it
stays a reference for the federation stage rather than becoming a dependency.
