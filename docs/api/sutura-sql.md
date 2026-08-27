<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-sql

The public API of `sutura-sql`, rendered from rustdoc JSON.

Rendering: a `QueryPlan` becomes one statement in one dialect.

Two modules, and they are the two halves of what a SQL-speaking adapter needs: `dialect` names
the data systems we render for and owns the two decisions the dialect layer does not make for us,
`generate` turns a plan into a statement and its bind parameters.

# Why this is its own crate and not the compiler's last stage

It used to be `sutura_semantic::generate` and `sutura_semantic::dialect`, and rendering was
already off the compile path - `sutura_semantic::compile` stops at a plan, because the
`Warehouse` port's currency is a `QueryPlan` and an adapter that executes over Arrow renders
nothing. What did not follow was the dependency: rendering was a `pub` module of the core, so
`polyglot-sql` sat in the transitive closure of every consumer of `sutura-semantic` - including
the network binary, which links the engine, renders nothing, and can reach no code in here.

Sharing the quoting and the placeholder decisions between SQL adapters is a good reason for this
code to be shared. It is not a reason for it to sit in the core. A separate crate gives the same
sharing, keeps the generator out of the core's closure, and makes the direction a **gate** rather
than a comment: `cargo xtask check-boundaries` fails if `sutura-semantic` can reach either this
crate or `polyglot-sql`.

So the direction is one way only. This crate depends on `sutura-domain` - a plan comes in, a
`GeneratedQuery` goes out - and on the dialect layer. It does **not** depend on
`sutura-semantic`, and `sutura-semantic` does not depend on it. Neither needs the other: one
decides what to execute, the other writes it down for a data system that speaks SQL.

# Who calls it

An adapter that pushes a statement down: `sutura-exec-duckdb` renders for its own dialect and no
other. `sutura-cli` calls it too, because `sutura compile` exists to print the statement for a
dialect somebody named. The engine adapter calls nothing here, which is the whole point of the
port taking a plan.

**Nothing here parses SQL, and nothing here translates between dialects.** There is no foreign
SQL on this path to parse: the statement is generated from a model. Translation is banned
separately, in `clippy.toml`, and the `transpile` feature is not even compiled - see the feature
list in the workspace manifest for why not calling it was judged too weak.

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
