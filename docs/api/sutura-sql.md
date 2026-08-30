<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-sql

The public API of `sutura-sql`, rendered from rustdoc JSON.

Rendering: a `QueryPlan` becomes one statement in one dialect.

Three modules and one type. `dialect` names the data systems we render for and owns the two
decisions the dialect layer does not make for us; `generate` turns a plan into a statement and
its bind parameters; `expression` compiles the one thing a catalog is allowed to author as SQL,
at load, for every dialect at once. `GeneratedQuery` is what `generate` returns, and it is at
the crate root because it is this crate's output rather than any one module's detail.

**Two entry points, one output type.** `generate` renders a whole answer from a `QueryPlan`;
`generate_leg` renders one leg of a federated question from a `LegPlan`. They share every
decision that could drift - the quoting, the placeholder style, the bucket, the joins, how a term
renders - and differ in the four ways `generate_leg`'s own documentation lists. Nothing in a
binary calls the second one yet: there is no splitter, so `AGENTS.md`'s *Built And Not Wired*
section is where its state is recorded.

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

**Nothing here translates between dialects, and exactly one thing here parses SQL.** The
statement is generated from a model, so there is no foreign SQL on the *query* path to parse.
Translation is banned separately: the `transpile` feature is not even compiled - see the feature
list in the workspace manifest for why not calling it was judged too weak.

The one exception is `expression`, and it is an exception with a stated shape. A catalog may
author a SQL fragment for a metric the closed measure vocabulary cannot express, and that
fragment is parsed - **at catalog-compile time, once, never on the query path** - checked against
a list of constructs this build refuses, qualified against the model's columns, and rendered for
every dialect. What reaches a statement afterwards is our own generator's output. `docs/adr/0004`
is the decision.

## `struct GeneratedQuery`

```rust
pub struct GeneratedQuery
```

A statement, its parameters, and the one data system it runs against.

**Parameters are a separate field and there is no constructor that merges them.** That is the
mechanism behind "no value from a question reaches the statement as text": to inline a value an
adapter would have to build the string itself, which is a diff rather than an oversight.

`source` rides along because a plan resolves to exactly one data system, and carrying it here is
what lets the composition root check that the adapter it is about to call is the one the plan
named.

**It is in this crate rather than in `sutura-domain`, and that is the same argument this crate
exists for.** It sat beside the `Warehouse` port while the port took a rendered statement, on the
reasoning that the port had to hand one to something. The port takes a `QueryPlan` now - an
adapter that executes over Arrow renders nothing and never sees one of these - and once the port
changed, nothing in the domain constructed or read one: the type was a concept the domain named
and did not use. Its producer is `generate` one module over, and every consumer - the SQL
adapters, and the CLI so `sutura compile` can print a statement - already depends on this crate.
So the move added an edge nowhere and made the domain smaller by exactly the part of it that was
not domain.

### Methods

```rust
pub const fn new(source: SourceName, sql: String, params: Vec<ParamValue>) -> Self
```

```rust
pub fn params(&self) -> &[ParamValue]
```

```rust
pub const fn source(&self) -> &SourceName
```

```rust
pub fn sql(&self) -> &str
```

### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

## `use None`

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

And two things the layer DOES decide that this module has to state anyway, because something
outside the renderer reads them. Both are declarations of what we expect the layer to do, and
both are MEASURED against it - the tests live in `mod@crate::generate`, which is the module allowed
to name the layer:

**Which character the quotes are.** We force quoting; the layer picks the character, and it is not
the same one everywhere. `BigQuery` uses a backtick, and in `GoogleSQL` a double quote is a STRING
LITERAL rather than an identifier quote - so the difference is not cosmetic in the direction that
matters: a statement quoted the wrong way is not a syntax error there, it is a statement about
different values. The golden suite's *no identifier reaches the statement unquoted* claim searches
for quoted spans, so it has to know which character to look for, and a hard-coded `"` silently
stopped asserting anything the moment a fourth dialect arrived.

**How the date bucket is spelled.** `DateTruncShape` carries the argument order and whether the
grain is a string literal or a bare keyword, and it exists because the parse check cannot catch
getting it wrong - see that type.

**How deep a qualifier a table may carry.** `Dialect::qualification` declares it, and the reason
it is not delegated is that the layer will happily RENDER `a.b.c` for a target with no third
position to put `a` in. See that accessor.

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
- `BigQuery`

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The name used on a command line and in a snapshot suffix.

```rust
pub const fn date_trunc_shape(self) -> DateTruncShape
```

How this data system spells truncating a date to a grain.

See `DateTruncShape` for why this is a declaration rather than something the parse check
would have caught.

```rust
pub const fn identifier_quote(self) -> IdentifierQuote
```

Which character this data system wraps an identifier in.

Declared here and measured in `mod@crate::generate` against what the layer actually emits, so
this cannot become a claim about a rendering nobody checked.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnknownDialect>
```

Parses a dialect name.

```rust
pub const fn placeholder_style(self) -> PlaceholderStyle
```

How this data system writes a bind parameter.

**`BigQuery` is `Question`, and the decision was taken against the client's request shape
rather than against the rendering.** Its job API takes either positional parameters, written
`?` in the statement with `parameterMode: POSITIONAL` and an ORDERED array carrying no names,
or named ones written `@name` with `parameterMode: NAMED`. A `crate::GeneratedQuery` carries
an ordered `Vec` of values and no names at all - the plan has none to give, because a
parameter's identity there IS its position - so positional is the shape that already matches
end to end. Choosing named would mean inventing a name per parameter in the generator, a third
`PlaceholderStyle`, and a map on `GeneratedQuery` for a driver to read: three new things, none
of which the domain has anything to put in them.

```rust
pub const fn qualification(self) -> Qualification
```

The deepest table path this data system resolves.

A declaration, exhaustively matched, so a fifth dialect cannot compile without answering -
the `DateTruncShape` and [`identifier_quote`](Dialect::identifier_quote) precedent, and for
the same reason: **the dialect layer renders `catalog.schema.name` for ANY target given three
parts.** Its `TableRef` is a name plus two `Option`s with no per-dialect arity check, so
without this declaration a `project.dataset.table` rendered for a target with no third
position produces a statement that either fails at the data system or, worse, resolves the
leading part as something else. `mod@crate::generate` refuses past what this returns.

The vocabulary is `sutura_domain::model::Qualification`, shared with the type that reports
how deep a *name* is - so the comparison is an ordering rather than a hand-written match.

**`BigQuery` is the reason the feature exists.** `project.dataset.table` is a first-class path
there, one credential reaches several projects, and a cross-project join is native and pushed
down. That is what makes cross-project **not** federation - see
`sutura_domain::model::qualified`'s header.

**Postgres is `Dataset`, and stops there because cross-DATABASE is not a thing it does.** Its
three-part form `database.schema.table` parses and is accepted only when the leading part is
the database already connected to, so rendering one would be a statement that works or fails
depending on a connection detail no catalog can see. A schema qualifier is the real capability
and is what a `schema.table` model gets.

**`ClickHouse` is `Dataset` for its `database.table`.** It has databases and no catalog above
them. Its arm is a rendering claim and not an execution one: nothing in this workspace
executes `ClickHouse`, which `AGENTS.md` already says of every `ClickHouse` golden.

**`DuckDB` is `TableOnly`, and that arm is the one worth reading twice** - `DuckDB` *does* have
schemas and attached catalogs, so this is narrower than what the engine can parse. It is
declared for what a `DuckDB` deployment HERE can resolve: `sutura-exec-duckdb` registers one
view per model in the default schema of the default catalog, and `sutura-exec-datafusion`
registers one file per model in its own table registry. A qualified name resolves to nothing in
either, so a refusal naming the path is the useful outcome and a rendered `a.b.c` that returns
`Catalog with name a does not exist` is not. Widening this arm is a change to what those
adapters ATTACH, not to what this renders.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`, `Serialize`

### `enum PlaceholderStyle`

```rust
pub enum PlaceholderStyle
```

How a bind parameter is written.

#### Variants

- `Question` - `?`, positional by order of appearance. `DuckDB`, `ClickHouse` and `BigQuery`.
- `Numbered` - `$1`, `$2`, numbered from one. Postgres.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum IdentifierQuote`

```rust
pub enum IdentifierQuote
```

Which character a dialect wraps an identifier in.

Two variants and no `Other(char)`, for this workspace's usual reason: a variant is a claim that we
render and pin a dialect using it, and a `char` field would let a caller invent one nothing was
tested against.

#### Variants

- `Double` - `"name"`. `DuckDB`, Postgres and `ClickHouse`.
- `Backtick` - `` `name` ``. `BigQuery`.

#### Methods

```rust
pub const fn character(self) -> char
```

The character itself, for building or searching for a quoted span.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum DateTruncShape`

```rust
pub enum DateTruncShape
```

How a dialect spells truncating a date to a grain.

**This type exists because the parse check cannot catch getting it wrong, and that was measured
rather than assumed.** Within one target, `polyglot_sql::parse` accepts `DATE_TRUNC('month', col)`
and `DATE_TRUNC(col, MONTH)` alike - a generic function call is a generic function call to a
parser, whatever the argument order means to the data system. So the golden suite's *every
generated statement parses under its target dialect* row is blind to this class by construction,
and the only things standing behind the bucket are this exhaustive match, the golden diff a
reviewer reads, and an execution against a real instance.

**What getting it wrong costs, stated precisely, because the two failures here are not the same
severity.** Sending `BigQuery` the grain-first shape produces a statement it **rejects**: the
first argument is the value to truncate, a string literal only coerces there if it is a canonical
date, and `'month'` is not one - while the column lands in the granularity slot, which takes a
keyword. So the defect is a deployment whose corpus is green and which fails on its first real
question, not one that returns a wrong number. The wrong-number risk on this dialect belongs to
`IdentifierQuote` instead, and that asymmetry is why the two are separate types.

A fifth dialect therefore cannot be added without stating its spelling, which is the one
mechanism available here.

Verified against the target's own function reference rather than inferred: the documented syntax
is `DATE_TRUNC(date_value, date_granularity)`, every granularity in the list is a bare keyword,
and one of them - `WEEK(<WEEKDAY>)` - is not expressible as a string at all.

#### Variants

- `GrainFirstAsLiteral` - `DATE_TRUNC('month', <date>)` - the grain first, as a single-quoted string literal.
- `DateFirstAsKeyword` - `DATE_TRUNC(<date>, MONTH)` - the date first, the grain a bare keyword.

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

## Module `expression`

Compile: an authored SQL fragment becomes one checked expression per target dialect.

`sutura_domain::expression` holds the fragment as text and knows nothing about SQL, because the
domain has no parser and `cargo xtask check-boundaries` keeps it that way. This module is the
other half: it parses what a catalog author wrote, refuses what it will not carry, qualifies the
columns against the model, and renders the result for **every** dialect this build renders for -
all of it at catalog-compile time, none of it on the query path.

# Parse and generate, never transpile

`Dialect::parse` in one dialect, `Generator::generate` in another. `Dialect::transpile` is not
called and its feature is not compiled, and that is a stronger reason than tidiness. Under
`TranspileOptions::default()` the unsupported level is `Warn`: an unsupported construct returns
`Ok(sql)` and pushes a diagnostic into `unsupported_messages`, which `Dialect::transpile` then
**discards**. The default failure mode of the convenient call is therefore silent wrong output.
Setting the level to `Raise` is not a usable net either - measured, it errors on every non-count
aggregate targeting `ClickHouse` while staying silent on all four of the real breakages
`Construct` refuses below.

Parse-and-generate also turns out to be *more* faithful for the aggregation subset: measured
byte-identical across all sixteen (read, write) pairs over `DuckDB`, Postgres, `ClickHouse` and
`BigQuery` for the conditional sum, the guarded ratio, `COUNT(DISTINCT k)`, `AVG`, `COALESCE`, a
bare `CASE`, `MIN`/`MAX` and `ARRAY_AGG(DISTINCT .. ORDER BY ..)`, with `CAST(.. AS DOUBLE)`
retargeting correctly.

# Why a `SELECT` wrapper and not the fragment API

The obvious way to parse a fragment - `Parser::new(dialect.tokenize(x))` then
`parse_expressions()` - **panics** on an empty token list, which is what `""`, whitespace-only
and comment-only input all produce, in every one of our dialects. Under `panic = "abort"` that is
a catalog file ending the process. So the fragment is parsed as `SELECT {fragment}` and the
projection is taken back out, with a guard on each way that can go wrong: one statement, one
projection, no `FROM`, no alias. `sutura_domain::expression::SqlFragment` refuses the empty
cases before they get this far, and the guards here hold for everything else.

**The authoring dialect is `DuckDB` and may not be `ClickHouse`.** Measured: `ClickHouse`'s
parser accepts `SUM(x))` and `x) FROM secret --`, silently dropping the tail. That is
injection-shaped input passing validation. `DuckDB` rejects both.

# What the compile guarantees, and what it does not

It guarantees the fragment is one expression, over columns the metric's own model declares -
**every one of them carrying that model's table**, asserted after the rewrite rather than assumed
from it - reaching no table and no query it was not given; that every function it calls is one of
the names in the allowlist `Construct::UnknownFunction` names; that it nests no deeper than the
checks can walk without the stack; that none of the constructs in `Construct` is present; and
that the rendering for each target is well-formed SQL that parses in that target's dialect.

It does **not** guarantee the target has the function. `MEDIAN(x)`, `COUNT_IF(x)` and
`PERCENTILE_CONT(..) WITHIN GROUP (..)` are emitted verbatim into Postgres and `ClickHouse` by
the dialect layer, and two of those three do not exist there. No per-dialect function catalogue
is compiled into this build, so nothing here can tell. That is precisely what the per-dialect
variants of `AuthoredSql` are for: the author names the dialect and takes the claim, and a
dialect with neither an exact fragment nor a `portable` one is refused rather than guessed at.

### `struct Rendering`

```rust
pub struct Rendering
```

One authored fragment, compiled for one dialect.

`authored_for` is the traceability half and is not cosmetic: with a `portable` fragment and a
`clickhouse` one in the same metric, "which one did this statement use" is otherwise a question
answered by re-deriving the resolution rule in your head.

#### Methods

```rust
pub const fn authored_for(&self) -> &DialectTag
```

The dialect word whose fragment was chosen: the target's own, or `portable`.

```rust
pub fn sql(&self) -> &str
```

The rendered expression, with every identifier already quoted.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `struct CompiledExpression`

```rust
pub struct CompiledExpression
```

An authored expression, compiled for every dialect this build renders for.

Complete by construction: `compile` returns one of these only when every entry in
`crate::dialect::ALL` resolved and rendered. So there is no per-query moment at which a target
turns out to have no expression - that failure has already happened, at load, naming the dialect.

#### Methods

```rust
pub fn for_dialect(&self, dialect: Dialect) -> Option<&Rendering>
```

The rendering for one target. Always `Some` for a dialect in `crate::dialect::ALL`.

```rust
pub const fn renderings(&self) -> &BTreeMap<Dialect, Rendering>
```

Every rendering, for a snapshot a reviewer reads.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`, `Serialize`

### `fn compile`

```rust
pub fn compile(authored: &sutura_domain::expression::AuthoredSql, table: &sutura_domain::model::TableName, columns: &std::collections::BTreeSet<sutura_domain::model::ColumnName>) -> Result<CompiledExpression, crate::expression::refusal::ExpressionError>
```

Compiles an authored expression for every dialect this build renders for.

`columns` is the metric's own model's declared column set and `table` its table. Both are
required rather than optional, which is the decision worth writing down: **an unknown column
fails the load.** Wren's cube path does not check them - its own documentation tells the agent to
expect a runtime error from the warehouse - while its model path does, through a schema-driven
AST rewrite. The model path is right. A metric whose fragment names a column that does not exist
is broken whether or not anybody asks about it, and the difference between finding out at load
and finding out at query time is the difference between a refusal an operator can fix and a stack
trace an agent shows a user.

### `fn embed`

```rust
pub fn embed(rendering: &Rendering) -> polyglot_sql::builder::Expr
```

The compiled expression, as something the statement builder will accept.

`Expression::Raw` is a true verbatim passthrough - the generator writes `raw.sql` and consults no
dialect flag - which is what makes it right here and wrong almost everywhere else: the text was
produced by `compile` for this exact target, with identifiers already quoted, so re-parsing it
would be re-parsing our own output for nothing.

Wrapped in a `Paren`. `Raw` reports `is_statement()`, carries no precedence and has no children,
so an unparenthesised one placed under an operator would bind by text rather than by structure.
The parentheses cost two characters and make the embedding position-independent.

### Module `refusal`

What a refusal says and what it carries: `refusal::Construct`, `refusal::Shape` and
`refusal::ExpressionError`.

Public, and a module rather than a `pub use` here, for two reasons that happen to agree.
`cargo xtask max-lines` caps a file under `crates/` at a thousand lines and cannot exempt
anything, and this file was at the cap; and the refusal vocabulary is the half of the compile a
reader consults rather than follows, so it reads better as its own page than as the first third
of this one. `crate::ExpressionError` and `crate::Construct` still name the same types, from the
crate root, which is the path everything outside this crate uses.
What a refusal from the compile says, and what it carries: the construct it found, the shape
guard it failed, and the error itself.

Its own module for two reasons. The plain one is size: `expression.rs` is a thousand-line file by
the gate that measures it, and the refusal vocabulary is the half of it a reader consults rather
than follows.

The one worth stating is that this is where the shape rule lives. **Every field here is the value
and never prose about it.** The dialect word is a `DialectTag`, which is what every construction
site already holds; the two refusals that name a *set* carry the set. The joins below are how a
message reads and not what it is - a caller wanting the sentence has `Display`, and a caller
wanting the list has the list, where before it had to split a message on `", "`.

#### `enum Construct`

```rust
pub enum Construct
```

A construct an authored fragment may not contain, and why.

Every variant is a refusal a compile can produce, and the reason is carried with it rather than
left in a design document: a refusal that names a construct without saying why sends an author to
read this file.

##### Variants

- `Query` - A `SELECT`, a subquery, a set operation.
- `TableReference` - A named table.
- `SchemaStatement` - A schema or data statement inside an expression.
- `Star` - `*`, either as a node or as `COUNT(*)`'s flag.
- `BindParameter` - `?` or `$1`.
- `Opaque` - A node the generator emits with no handling at all.
- `QualifiedColumn` - `t.column`.
- `QualifiedFunctionName` - A function name with a schema on it.
- `DateTimeFunction` - Anything that reads a date or a time.
- `UnknownFunction` - A called function whose name is not in the allowlist.
- `AggregateFilter` - `FILTER (WHERE ..)` on an aggregate.
- `Comment` - A comment inside the fragment.
- `RowConstructor` - A row constructor, which is also how `COUNT(DISTINCT a, b)` parses.
- `UnguardedDivision` - `/` whose divisor is not a `NULLIF`.
- `IntegerDivision` - `//`, or any integer division node.
- `IsTrue` - `x IS TRUE`, `x IS FALSE`, `x IS <expr>`.
- `NotAggregated` - A fragment that aggregates nothing.

##### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The name a refusal prints.

```rust
pub const fn why(self) -> &'static str
```

Why it is refused. One sentence, and it is the whole value of the refusal.

##### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

#### `enum Shape`

```rust
pub enum Shape
```

Which of the four shape guards a fragment failed.

Named individually because each one is a different mistake: two projections is a comma somebody
meant as an argument separator, a `FROM` is a whole query pasted into a measure, and an alias is
a habit from writing `SELECT` lists. "Not one expression" would send all three to read a grammar.

##### Variants

- `ManyStatements` - More than one statement: a `;` in the fragment.
- `NotASelect` - The wrapper did not come back as a `SELECT`. A set operation is the reachable case: `SUM(x) UNION SELECT 1` parses as a `Union`, not as a projection.
- `ManyExpressions` - Not exactly one projected expression.
- `CarriedFrom` - A `FROM` clause.
- `CarriedAlias` - An `AS name`.
- `CarriedClause` - Any other clause on the wrapper's `SELECT`, which taking the projection would DISCARD.

##### Methods

```rust
pub const fn as_str(self) -> &'static str
```

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

#### `enum ExpressionError`

```rust
pub enum ExpressionError
```

Why an authored expression could not be compiled.

**All of these are load failures.** A catalog that produces one does not serve; there is no
degraded mode in which the metric is skipped and the rest is answered, because a metric that is
present in a bundle and unanswerable is a metric an agent will ask about.

**Four of them cannot be produced by any fragment, and each says so on itself rather than here:**
`Self::Qualify`, `Self::Unrenderable`, `Self::Render` and `Self::RenderedDoesNotParse`
each need a defect in the dialect layer, and **not the same defect** - which is why the argument
is on the variant and not summarised here. `Self::Qualify` needs that layer's own transformer to
violate one of its own invariants; `Self::Unrenderable` and `Self::Render` are ruled out by
construction, because this module's caps sit under the layer's complexity guard and no dialect
configuration raises its unsupported level; and `Self::RenderedDoesNotParse` is **not** ruled
out by construction at all - it is the load-time net for a generator that emits text its own
parser rejects, which is the reason it is a check here rather than a test. They exist
because the calls they wrap return a `Result` and this crate may not `unwrap` one, and what is
pinned about them is the wiring - the fields, and that the cause survives `#[source]` - not a
refusal a catalog can provoke. `tests::the_four_refusals_only_a_dialect_layer_defect_can_produce`
is that test, and it is named for what it is so that nobody reads it as coverage of an input.

**Every field is the value, never prose about it**, and that is this enum's one shape rule. The
dialect word a refusal is about is a `DialectTag` and not a `String`, because that is what every
construction site already holds; the two refusals that name a *set* carry the set rather than a
sentence built from it. A caller that wants the sentence gets it from `Display`, and a caller that
wants the list has it - where before, recovering "which dialects was this authored for" meant
splitting a message on `", "`, which is a contract nothing checks and a format edit breaks.

##### Variants

- `UnknownDialect` - A dialect word that is not one this build renders for. Refused rather than ignored: a `postgresql:` beside a `portable:` would otherwise be a variant that is silently never chosen, and the author would never learn that Postgres got the portable fragment.
- `NoFragment` - No exact fragment and no `portable` one. The refusal wren's importer does not have.
- `Unparsable`
- `NotOneExpression`
- `Unrenderable` - The parse succeeded and the result could not be written back out, so it cannot be shown to be the projection and nothing else. Its own variant rather than a `Shape`, because a `Shape` carries no cause and this one has one worth keeping.
- `Refused`
- `UnknownColumn`
- `UnknownFunction` - A called function that is not one of the names a measure may call.
- `TooDeep` - A fragment nesting deeper than the checks can walk. See the guard in `super::parse`, in the parent module.
- `NotQualified` - A column the qualification rewrite did not reach. See `super::require_qualified`, in the parent module.
- `Qualify` - The qualification rewrite failed.
- `Render` - The target's generator refused the tree.
- `RenderedDoesNotParse` - The rendering came back as something its own target cannot parse. The same check the golden suite applies to every generated statement, applied here at load rather than in a test, because this is the one statement fragment whose text came from a file.

##### Implements

`Debug`, `Display`, `Error`

## Module `generate`

Generate: a plan becomes one statement in one dialect.

The module that turns a plan into SQL. An adapter that executes a plan without rendering it -
the in-process engine - never calls anything here.

It used to be the only module that names the dialect layer. `crate::expression` names it too
now, because compiling a catalog-authored fragment is parsing rather than rendering and the two
jobs share no code: this file builds an AST from a plan, that one takes an AST apart and refuses
most of it. A pre-1.0 API change upstream therefore touches two files in this crate, both of them
here rather than anywhere else.

Six things about how the dialect layer is used, every one of them measured rather than assumed,
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
four targets, so without the cast the type of the `period` column is whatever each dialect chose
and every adapter would need to know which.

**The time bucket's SPELLING is per dialect, and it is the one difference here the layer does not
absorb.** Three targets take `DATE_TRUNC('month', <date>)`; `BigQuery` takes
`DATE_TRUNC(<date>, MONTH)` - the arguments the other way round and the grain a bare keyword
rather than a string. `crate::dialect::DateTruncShape` holds the declaration and the reason it
has to be one: **within one target, the parse check cannot tell the two apart.** Both shapes
rendered for `BigQuery` parse as `BigQuery`, so the corpus would be green on the wrong one.
`the_parse_check_cannot_tell_the_two_bucket_shapes_apart` is that measurement.

**The limit on that claim, because the first version of it was too broad and a test caught it:**
the check does catch a statement rendered for the WRONG target, on the quoting rather than on the
bucket - double quotes are string delimiters in `GoogleSQL`, so a DuckDB-rendered statement fails to
parse as `BigQuery` at the first qualified column. `the_parse_check_does_catch_the_wrong_quote_character`
pins that, and the two tests together say precisely which half is covered by a mechanism and which
half rests on a declaration and a reviewed golden.

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
- `QualificationUnsupported` - A table path deeper than the target resolves.
- `NoPredicate` - A plan that carries no predicate at all.

#### Implements

`Debug`, `Display`, `Error`

### `fn generate`

```rust
pub fn generate(plan: &sutura_domain::plan::QueryPlan, dialect: crate::dialect::Dialect) -> Result<crate::GeneratedQuery, GenerateError>
```

Renders a plan as one statement, paired with its parameters.

### `fn generate_leg`

```rust
pub fn generate_leg(leg: &sutura_domain::plan::LegPlan, dialect: crate::dialect::Dialect) -> Result<crate::GeneratedQuery, GenerateError>
```

Renders one leg of a federated question as one statement, paired with its parameters.

**Four differences from `generate`, and each of them is why a second entry point exists rather
than a flag on the first.**

1. **It projects a LIST of term columns**, one per descending term, instead of one measure
   expression. That is the whole of 0009's Decision 2 at the rendering layer: a decomposed `Avg`
   travels as a sum beside a count and a ratio travels as an undivided numerator and denominator,
   so nothing here can emit a division. It never calls `measure_expression`, and it could not -
   there is no `PlanMeasure` in a `LegPlan` to hand it.
2. **The bucket and the joins are the fact leg's alone.** A dimension lookup reads a table with
   no time column, so it projects its keys and groups by them, which is a distinct key set.
3. **It emits no `LIMIT`.** A leg is not an answer:
   `sutura_domain::plan::MAX_ROWS` caps one answer's rows and
   `QueryPlan::row_limit` is how an adapter asks for one more than the cap, so a cap applied per
   leg would refuse a question no answer was too large for. What bounds a leg is the byte budget
   at the conversion boundary, which belongs with the code that converts.
4. **The `WHERE` clause is optional.** A `QueryPlan` always carries the two bounds of its range
   so `GenerateError::NoPredicate` is unreachable there; a lookup leg for a remote dimension
   that carries no filter has no predicate at all, and no clause is the correct rendering rather
   than an error.

Everything else is shared with `generate` on purpose - `column`, `aliased`, `aggregate`,
`term_expression`, `predicate`, `bucket_expression`, `joined` and `render` - so a
change to identifier quoting, to placeholder style or to how a term renders cannot apply to one
path and not the other.

**Nothing calls this from a binary.** There is no splitter, so no `LegPlan` is constructed
outside a test; what pins it is the golden family under `crates/sutura-app/tests/golden`, one
statement per shape per dialect, parse-checked in the dialect it was generated for.
