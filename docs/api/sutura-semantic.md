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

- `Planned` - The question resolved, and this is what we decided to execute.
- `Refused` - The question was refused, and this is why.

### Methods

```rust
pub const fn plan(&self) -> Option<&DomainPlan>
```

The plan, if the question resolved.

```rust
pub const fn refusal(&self) -> Option<&RefusalReason>
```

The refusal, if there was one.

### Implements

`Debug`

## `fn compile`

```rust
pub fn compile(query: &sutura_domain::query::Query, pinned: &sutura_domain::pinned::PinnedDefinitions) -> Result<Compiled, BundleInconsistent>
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

The error type is `BundleInconsistent` rather than an enum, because after the split that is the
only way this can fail. A refused question is not a failure and comes back as `Compiled`.

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`
