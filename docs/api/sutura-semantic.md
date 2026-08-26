<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-semantic

The public API of `sutura-semantic`, rendered from rustdoc JSON.

The semantic compiler: a modelled question becomes one plan for one data system.

Three stages, in three modules, and the split is the design rather than tidiness:

1. **Resolve** looks every name up in the pinned snapshot. Its only outputs are references into
   that bundle and refusals naming the argument that failed. It cannot reach a live catalog,
   because it is handed a `PinnedDefinitions` and nothing else.
2. **Plan** settles what nothing else may settle: which single data
   system the statement runs against, and which values become bind parameters. It holds no SQL,
   and its serialized form is what a golden snapshot pins.
3. **Generate** (`generate`) renders the plan for one dialect. It is the only module that
   produces SQL and the only one that names the dialect layer.

**`compile` runs the first two, and stops.** Stage 3 is not part of compiling, because the
`Warehouse` port takes a plan: an adapter that executes over Arrow renders nothing, and a
SQL-speaking adapter renders for the dialect it alone knows. Whoever wants SQL calls
`generate::generate` and names the dialect there.

`compile` returns a `Compiled` rather than a `Result` of a plan, because a refusal is an
answer: a caller must not be able to mistake "you may not ask that" for a transport failure and
retry until something works.

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

Rendering now lives where the dialect is actually known: `generate::generate`, called by the
adapter that speaks that dialect. Whoever wants SQL asks for it.

The error type is `BundleInconsistent` rather than an enum, because after the split that is the
only way this can fail. A refused question is not a failure and comes back as `Compiled`.

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

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

Generate: a plan becomes one statement in one dialect.

The only module that names the dialect layer, so a pre-1.0 API change upstream touches one file,
and the only one that produces SQL. An adapter that executes a plan without rendering it - the
in-process engine - never calls anything here.

Five things about how the dialect layer is used, every one of them measured rather than assumed,
and every one of them looking right until it was rendered:

**The fluent builder, not the AST structs.** `Expression::Select` has upwards of thirty fields
and no `Default`, so hand-building one is a list nobody can review. The builder panics on misuse
rather than returning an error, which is why every call here is on a shape that was proved to
work and nothing a catalog or a caller supplies changes which builder method runs.

**Identifiers are force-quoted, aliases included.** The generator quotes only what was quoted in
its source, is a reserved word, or the config forces. Ours were never in any source, so a column
called `order` would be emitted bare. `always_quote_identifiers` covers identifiers and does
**not** cover aliases, which come from a separate `Identifier` whose flag we set ourselves.

**`GROUP BY` gets the unaliased expressions.** Passing the aliased ones emits `GROUP BY x AS y`,
which no target accepts.

**The time bucket is cast to a date.** `DATE_TRUNC` over a date returns a TIMESTAMP in two of the
three targets, so without the cast the type of the `period` column is whatever each dialect chose
and every adapter would need to know which.

**Placeholders are ours.** The dialect layer renders every placeholder as `?` whatever the target,
and carries a per-dialect `parameter_token` it never reads - so Postgres would be sent `?` and
reject it. `crate::dialect::PlaceholderStyle` decides.

`transpile` is never called and is not compiled. See `clippy.toml` and the feature list in the
workspace manifest.

### `enum GenerateError`

```rust
pub enum GenerateError
```

Why a statement could not be rendered.

Not a refusal: a caller cannot cause one of these and there is nothing they could ask
differently. A plan that will not render is a bug here or upstream.

#### Variants

- `Render`
- `UnquotableAlias` - The builder produced something that is not an alias, so its identifier could not be quoted.
- `NoPredicate` - A plan that carries no predicate at all.

#### Implements

`Debug`, `Display`, `Error`

### `fn generate`

```rust
pub fn generate(plan: &sutura_domain::plan::QueryPlan, dialect: crate::dialect::Dialect) -> Result<sutura_domain::warehouse::GeneratedQuery, GenerateError>
```

Renders a plan as one statement, paired with its parameters.
