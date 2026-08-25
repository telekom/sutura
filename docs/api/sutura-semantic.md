<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-semantic

The public API of `sutura-semantic`, rendered from rustdoc JSON.

The semantic compiler: a modelled question becomes one statement for one data system.

Three stages, in three modules, and the split is the design rather than tidiness:

1. **Resolve** looks every name up in the pinned snapshot. Its only outputs are references into
   that bundle and refusals naming the argument that failed. It cannot reach a live catalog,
   because it is handed a `PinnedDefinitions` and nothing else.
2. **Plan** (`plan::Plan`) settles the two things nothing else may settle: which single data
   system the statement runs against, and which values become bind parameters. It holds no SQL,
   and its serialized form is what a golden snapshot pins.
3. **Generate** (`generate`) renders the plan for one dialect. It is the only module that
   produces SQL and the only one that names the dialect layer.

`compile` runs all three. It returns a `Compiled` rather than a `Result` of a statement,
because a refusal is an answer: a caller must not be able to mistake "you may not ask that" for
a transport failure and retry until something works.

**Nothing here parses SQL, and nothing here translates between dialects.** There is no foreign
SQL on this path to parse: the statement is generated from a model, so the rule holds by
construction. Translation is banned separately, in `clippy.toml`, and the `transpile` feature is
not even compiled - see the workspace manifest for why not calling it was judged too weak.

## `enum Compiled`

```rust
pub enum Compiled
```

What compiling a question produced.

A refusal is a variant here rather than an `Err`, which is the same choice
`sutura_domain::query::ToolOutcome` makes and for the same reason.

### Variants

- `Statement` - The question resolved, and here is the statement and the plan behind it.
- `Refused` - The question was refused, and this is why.

### Methods

```rust
pub const fn plan(&self) -> Option<&Plan>
```

The plan, if the question resolved.

```rust
pub const fn query(&self) -> Option<&GeneratedQuery>
```

The generated statement, if the question resolved.

```rust
pub const fn refusal(&self) -> Option<&RefusalReason>
```

The refusal, if there was one.

### Implements

`Debug`

## `enum CompileError`

```rust
pub enum CompileError
```

Why compilation failed, as opposed to being refused.

Neither variant is something a caller did. A broken bundle is an operator's problem and a
generator failure is ours, so neither is offered to the caller as a refusal they might retry
differently.

### Variants

- `Bundle`
- `Generate`

### Implements

`Debug`, `Display`, `Error`

## `fn compile`

```rust
pub fn compile(query: &sutura_domain::query::Query, pinned: &sutura_domain::pinned::PinnedDefinitions, dialect: Dialect) -> Result<Compiled, CompileError>
```

Resolves, plans and generates.

## `use None`

## `use None`

## `use None`

## `use None`

## Module `dialect`

Which data system a statement is rendered for, and the two things we do not delegate.

The dialect layer we build on knows more about dialects than we do: it renders `DATE_TRUNC` as
`dateTrunc` and `SUM` as `sum` for `ClickHouse`, which is exactly the kind of difference nobody
should be maintaining by hand. Two things it does not decide for us, both measured rather than
assumed:

**Placeholder syntax.** A placeholder renders as `?` for every dialect, including the one that
needs `$1`. The crate carries a per-dialect `parameter_token` field and never reads it. So the
style is chosen here, per dialect, and a target that needs numbering gets numbering.

**Identifier quoting.** The generator quotes an identifier only when it was quoted in the source,
is a reserved word, or the config says always. Our identifiers were never in any source, so
without forcing it a column called `order` would be emitted bare. We force it, and we force it
for aliases too, which the config's own flag does not cover.

### `enum Dialect`

```rust
pub enum Dialect
```

The data systems a statement can be rendered for.

A closed set rather than a passthrough of the dialect layer's thirty-three, because each entry
here is a claim that we generate correct SQL for it and have a golden that says so. Adding one is
a feature flag, a match arm and a snapshot.

#### Variants

- `DuckDb`
- `Postgres`
- `ClickHouse`

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The name used on a command line and in a snapshot suffix.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownDialect>
```

Parses a dialect name.

```rust
pub const fn placeholder_style(self) -> PlaceholderStyle
```

How this data system writes a bind parameter.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum PlaceholderStyle`

```rust
pub enum PlaceholderStyle
```

How a bind parameter is written.

#### Variants

- `Question` - `?`, positional by order of appearance. `DuckDB` and `ClickHouse`.
- `Numbered` - `$1`, `$2`, numbered from one. Postgres.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct UnknownDialect`

```rust
pub struct UnknownDialect
```

Why a dialect name was not recognised.

#### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `constant ALL`

Every dialect, for iterating a golden suite over all of them.

A `const` rather than a derive, so a new variant that is not added here fails the exhaustiveness
test below rather than being silently untested.

## Module `generate`

Generate: the plan becomes one statement in one dialect.

This is the only module that names the dialect layer, so a pre-1.0 API change upstream touches
one file. It is also the only module that produces SQL.

Four things about how the dialect layer is used, each measured rather than assumed:

**The fluent builder, not the AST structs.** The `Expression` enum's `Select` has upwards of
thirty fields and no `Default`, so hand-building one is a list nobody can review. The builder
panics on misuse rather than returning an error, which is why every call here is on a shape that
was proved to work; nothing a catalog or a caller supplies changes which builder method runs.

**Identifiers are force-quoted.** The generator quotes an identifier only if it was quoted in the
source, is a reserved word, or the config says always. Ours were never in any source, so a column
called `order` would be emitted bare. `always_quote_identifiers` fixes that for identifiers and
does **not** cover aliases, which the generator writes from a separate `Identifier` whose `quoted`
flag we set ourselves.

**`GROUP BY` gets the unaliased expressions.** Passing the aliased ones emits `GROUP BY x AS y`,
which is not valid in any of the three targets. It looked right until it was rendered.

**Placeholders are ours.** The dialect layer renders every placeholder as `?` whatever the target,
and carries a per-dialect `parameter_token` it never reads, so Postgres would get `?` and reject
it. `crate::dialect::PlaceholderStyle` is what decides.

**`transpile` is never called, and is not even compiled.** See `clippy.toml` and the feature list
in the workspace manifest.

### `enum GenerateError`

```rust
pub enum GenerateError
```

Why a statement could not be rendered.

Not a refusal: a caller cannot cause one of these, and there is nothing they could ask
differently. A plan that cannot be rendered is a bug here or upstream.

#### Variants

- `Render`
- `UnquotableAlias` - The builder produced something that is not an alias, so the alias identifier could not be quoted. Reported rather than ignored: silently emitting an unquoted alias is how a metric named `order` becomes a syntax error at the data system.

#### Implements

`Debug`, `Display`, `Error`

### `fn generate`

```rust
pub fn generate(plan: &crate::plan::Plan, dialect: crate::dialect::Dialect) -> Result<sutura_domain::warehouse::GeneratedQuery, GenerateError>
```

Renders a plan as one statement, and pairs it with its parameters.

## Module `plan`

Plan: the resolved question becomes something owned, and two things are settled here and
nowhere else.

**The plan names exactly one data system.** A question whose join would reach a second one is
refused before anything runs, because a second data system is a second identity to satisfy, and a
plan that runs partly as somebody else is the failure this design exists to prevent.

**Every value from the question becomes a bind parameter.** They are collected here, in the order
the generated statement will refer to them, so no caller-supplied value reaches the generator as
text. The generator has no access to the question at all.

A plan holds no SQL. Its public contract is its serialized form: it is the artifact a golden
snapshot pins, so a change to what we plan shows up as a reviewable diff rather than as a
different number.

### `struct Plan`

```rust
pub struct Plan
```

One statement's worth of decisions, and no SQL.

#### Methods

```rust
pub const fn metric(&self) -> &MetricName
```

The metric this plan answers about. Public because a caller that gets a plan back wants to
know what it is a plan for without deserializing it.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `use None`

The label the truncated time column is projected under.

Re-exported from the domain rather than defined here, because it is part of the result schema and
`Definitions::assemble` has to refuse a dimension by this name for the same reason: two columns
with one label is a result a caller cannot read.

### `constant MAX_ROWS`

The most rows any generated statement may return.

A hard cap rather than a budget, for now. It exists because a bounded range and a bounded set of
group-by keys still permit a large result, and the cost of that lands on a shared data system.
When there is a real budget this becomes its floor.
