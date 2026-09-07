<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-semantic

The public API of `sutura-semantic`, rendered from rustdoc JSON.

The semantic compiler: a modelled question becomes one plan for one data system.

Two stages, in two modules, and the split is the design rather than tidiness:

1. **Resolve** looks every name up in the pinned snapshot. Its only outputs are references into
   that bundle and refusals naming the argument that failed. It cannot reach a live catalog,
   because it is handed a `PinnedDefinitions` and nothing else.
2. **Plan** settles what nothing else may settle: which single data
   system the statement runs against, and which values become bind parameters. It holds no SQL,
   and its serialized form is what a golden snapshot pins.

**`compile` runs both, and stops. There is no third stage in this crate.** The `Warehouse`
port takes a plan: an adapter that executes over Arrow renders nothing, and a SQL-speaking
adapter renders for the dialect it alone knows. Whoever wants SQL calls `sutura_sql::generate`
and names the dialect there.

**`sutura-sql` is a separate crate, and this crate does not depend on it.** Rendering used to be
a `pub` module here - `generate` and `dialect` - which put `polyglot-sql` in the transitive
closure of every consumer of the core, the network binary included: it links the engine, renders
nothing, and could reach no line of that code. Moving the two modules out drops the generator
from this crate's tree, and `cargo xtask check-boundaries` fails if either edge comes back, so
the direction is a gate rather than a sentence in this comment.

`compile` returns a `Compiled` rather than a `Result` of a plan, because a refusal is an
answer: a caller must not be able to mistake "you may not ask that" for a transport failure and
retry until something works.

**Nothing here emits SQL, parses SQL, or names a dialect.** After the split that is a fact about
the dependency list rather than a discipline: there is no SQL generator in this crate's tree to
call.

## `enum Compiled`

```rust
pub enum Compiled
```

What compiling a question produced.

A refusal is a variant here rather than an `Err`, which is the same choice
`sutura_domain::query::ToolOutcome` makes and for the same reason.

### Variants

- `Planned` - The question resolved to one data system, and this is what we decided to execute.
- `Federated` - The question resolved to two data systems, split into a fact leg and a lookup leg.
- `Refused` - The question was refused, and this is why.

### Methods

```rust
pub const fn plan(&self) -> Option<&DomainPlan>
```

The plan, if the question resolved to one data system.

```rust
pub const fn refusal(&self) -> Option<&RefusalReason>
```

The refusal, if there was one.

### Implements

`Debug`

## `enum CompileFailure`

```rust
pub enum CompileFailure
```

Why compiling failed, which is never why a question was refused.

**Two arms rather than one bundle error, and `telekom/sutura#338` is the report.** A question the
deployment declines comes back as `Compiled::Refused`; what reaches this type is our own side
being wrong. Those are the two ways that can happen: the pinned bundle names something it does not
hold, and the splitter built a two-source plan that
`FederatedPlan::new` then rejected. The second used to
be flattened into `RefusalReason::FederationNotExecutable`, the refusal every two-source
question already gets from a shipped binary - so a wiring defect and a governance answer arrived
as one value, and a caller could not tell which it had.

**The limit, next to the claim:** nothing provokes `NotAssembled`
today. Every `FederatedPlanError` variant is structurally unreachable from the splitter as it
stands - `crate::plan::PlanError` enumerates why, one variant at a time - so what this arm buys is
that a future edit which makes one reachable surfaces as a failure rather than as a refusal a
caller would retry.

### Variants

- `Bundle` - The pinned bundle names a model or a relationship it does not hold.
- `NotAssembled` - A two-source plan this workspace compiled and could not then assemble.

### Implements

`Debug`, `Display`, `Error`

## `fn compile`

```rust
pub fn compile(query: &sutura_domain::query::Query, pinned: &sutura_domain::pinned::PinnedDefinitions) -> Result<Compiled, CompileFailure>
```

Resolves and plans. It does not render.

**The dialect used to be an argument here, and that was a parameter that could lie.** Compiling
rendered a statement alongside the plan, and the one caller that answers questions threw the
statement away - because the port takes a plan, and a SQL-speaking adapter renders its own. So the
dialect decided nothing, while a caller could hand this `Postgres` and a `DuckDB` warehouse and
nothing anywhere would notice the disagreement.

Rendering now lives where the dialect is actually known: `sutura_sql::generate`, in its own
crate, called by the adapter that speaks that dialect. Whoever wants SQL asks for it, and
linking this crate no longer links a SQL generator.

The error type is `CompileFailure` and a refused question is not one of its arms: a refusal is
an answer and comes back as `Compiled`.

**A broken bundle is no longer the only way this can fail**, which is the change
`telekom/sutura#338` asked for and the one thing about it a test can hold:

```compile_fail
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::Query;
use sutura_semantic::{BundleInconsistent, compile};

fn _only_a_broken_bundle(query: &Query, pinned: &PinnedDefinitions) -> Option<BundleInconsistent> {
    compile(query, pinned).err()
}
```

And the twin, so a rename cannot make that block pass vacuously:

```
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::query::Query;
use sutura_semantic::{CompileFailure, compile};

fn _either_way(query: &Query, pinned: &PinnedDefinitions) -> Option<CompileFailure> {
    compile(query, pinned).err()
}
```

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`
