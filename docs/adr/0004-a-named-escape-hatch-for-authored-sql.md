---
title: A named escape hatch for authored SQL
description: Why a metric may carry a SQL expression somebody wrote, why it is a separate named shape rather than a field on the measure, why the fragment is parsed and generated rather than transpiled, and which four constructs are refused at load and why each one is on the list.
---

# A named escape hatch for authored SQL

Status: accepted. It amends
[the closed vocabulary decision](0002-a-closed-vocabulary-for-measures.md), whose *Alternatives
considered* rejected exactly this, and it does not supersede it: the closed vocabulary stays closed,
and this record is about what sits **beside** it and how it is kept visible.

## Context

Two things pushed against the closed vocabulary from opposite directions, and neither is a metric
that a wider `Measure` would fix.

**Some metrics need constructs a closed vocabulary cannot hold.** A window function, a percentile, an
expression over two columns - `SUM(price * quantity)` - a `sumIf` over one dialect's own aggregate.
Adding a `Term` per construct is the mistake
[0002](0002-a-closed-vocabulary-for-measures.md#two-levels-not-three-siblings) already recorded once
in a smaller form: at some point the vocabulary stops being a vocabulary and becomes a badly-typed
expression language with a bespoke parser.

**Some providers already carry SQL per metric.** A wren cube holds
`SUM(CASE WHEN status = 'active' THEN mrr_eur END)` in the file. There is nothing to map that onto,
so a provider whose catalog is written that way arrives as "unsupported" for its entire metric set -
not as a degraded import, as no import. And this is the point where "not all providers have all
capabilities" stops being an abstract statement: a wren-style directory has authored SQL because a
person wrote the file, and a metadata service that stores no executable SQL per metric, or an RDF
vocabulary that never will, has none and is *complete* rather than degraded.

0002's rejection of free-text SQL gave two reasons. The first - "a parse failure becomes a runtime
refusal instead of a review comment" - is answerable by parsing at load, which 0002 itself concedes
is better. The second - "a catalog that loads on one version and fails to parse on the next fails
readiness for every metric in it" - is real and is not answered. It is accepted as a cost here, and
it is bounded: it applies only to metrics that use the hatch, and the failure is a loud load failure
naming the metric, the dialect and the position, which is the failure mode this repository prefers.

## Decision

**A separate, explicitly-named shape beside the measure - never a field on it.**

`Computation` has two variants and a metric says which in a word:

| Variant | On disk | Who produces it |
| --- | --- | --- |
| `Computation::Measure` | `measure:` | Every provider. The ordinary case, and the default path |
| `Computation::AuthoredSql` | `authored_sql:` | Only a provider whose catalog carries SQL per metric |

The shape matters as much as the capability. There is no `expression:` key on a measure, no
`Option<String>` beside one, and no arrangement in which "this metric is free-text SQL" is invisible
in a diff or absent from an operator's listing. `Computation::kind()` returns the word, so "which
metrics use the hatch" is one accessor over the pinned definitions rather than a grep over files.
Writing both keys is refused rather than resolved by precedence, for the reason a two-termed term is:
a document that writes both means one of them, and choosing would certify a number nobody asked for.

`AuthoredSql` is a **map from a dialect word to a fragment**, with `portable` reserved for "every
target". Resolution is exact dialect, then `portable`, then **refuse**. That third step is a
deliberate departure from wren's own OSI importer, which falls back to the first non-empty variant:
that hands a Postgres query a Snowflake expression because it happened to be listed first, which is a
number computed by a definition nobody chose, under a certified name. A dialect word that is not one
this build renders for is also a load failure, not a variant silently never chosen - otherwise a
`postgresql:` beside a `portable:` means Postgres quietly gets the portable text and nobody learns
that the variant written for it was never read.

The domain holds the fragment as **text** and no more. It checks that a fragment is present, bounded
and free of control characters, and it owns none of the SQL judgement, because it has no parser and
`cargo xtask check-boundaries` keeps it that way.

### Parse and generate, at load, per dialect

`sutura_sql::expression::compile` does the work, at catalog-compile time, for **every** dialect the
build renders for at once. What it stores is the rendered string per target; what reaches a statement
at query time is that string, verbatim, and nothing is parsed on the query path.

`Dialect::parse` then `Generator::generate`, never `Dialect::transpile`. The reason is stronger than
"we banned translation":

- `TranspileOptions::default()` sets `unsupported_level: Warn`. An unsupported construct returns
  `Ok(sql)` and pushes a diagnostic into `unsupported_messages`, which `Dialect::transpile` then
  **discards**. The default failure mode of the convenient call is silent wrong output.
- Setting the level to `Raise` is not a usable net either. Measured: it errors on every non-count
  aggregate targeting `ClickHouse` - `SUM`, `AVG`, `MIN`, `MAX` all `Err` - while staying silent on
  all four of the breakages below.
- Parse-and-generate is also *more faithful* for the aggregation subset, and needs no feature this
  build does not already compile. Measured byte-identical across all sixteen (read, write) pairs over
  `DuckDB`, Postgres, `ClickHouse` and `BigQuery` for the conditional sum, the guarded ratio,
  `COUNT(DISTINCT k)`, `AVG`, `COALESCE`, a bare `CASE`, `MIN`/`MAX` and
  `ARRAY_AGG(DISTINCT .. ORDER BY ..)`, with `CAST(.. AS DOUBLE)` retargeting per dialect.

The fragment is parsed as `SELECT {fragment}` rather than through the dialect layer's fragment API,
and that is not a stylistic preference. `Parser::new(dialect.tokenize(x))` followed by
`parse_expressions()` **panics** on an empty token list, which is what `""`, whitespace-only and
comment-only input all tokenize to, in all four dialects. Under `panic = "abort"` a blank line in a
catalog file would end the process. `clippy.toml` now bans both methods, and the ban was confirmed to
resolve by writing the call and watching clippy reject it - an unresolvable path in
`disallowed-methods` is silently ignored, so an unverified entry would read as enforcement and do
nothing.

**The authoring dialect is `DuckDB` and may not be `ClickHouse`.** Measured: ClickHouse's parser
accepts `SUM(x))` and `x) FROM secret --`, silently dropping the tail. That is injection-shaped input
passing validation. DuckDB rejects both, and rejects `SUM(x) garbage garbage`, `SUM(x), COUNT(y)`,
`SUM(x) AS foo`, `SUM(x) FROM t` and `1; DROP TABLE t` through the four shape guards.

### Column references are checked, and qualified

An unknown column **fails the load**. Wren has both behaviours: its cube path does not check column
references and its own documentation tells the agent to expect a runtime error from the warehouse,
while its model path does check them, through a schema-driven AST rewrite. The model path is right. A
metric whose fragment names a column that does not exist is broken whether or not anybody asks about
it, and the difference between finding out at load and finding out at query time is the difference
between a refusal an operator can fix and a stack trace an agent shows a user.

A qualifier written by hand is refused, and the compile qualifies every column itself with the
metric's model table - the same rule the plan already follows, for the same reason: an unqualified
column in a statement that later grows a join binds to whichever table happens to have it, and that
is a wrong number rather than an error.

### The denylist

Four constructs are refused at catalog-validation time. **Nothing upstream errors on any of them**,
which is the whole reason the list exists.

| Refused | Why |
| --- | --- |
| `FILTER (WHERE ..)` | `aggregate_filter_supported` is set `false` by six dialects and is **never read anywhere in the dialect layer**. FILTER is emitted unconditionally for every target, including the six that cannot run it |
| `COUNT(DISTINCT a, b)` | Gated by `multi_arg_distinct`. `DuckDB` to `DuckDB` rewrites it into `COUNT(DISTINCT CASE WHEN a IS NULL THEN NULL WHEN b IS NULL THEN NULL ELSE (a, b) END)` while `ClickHouse` leaves it alone, so identity is not identity and the same fragment counts different things per target |
| Any date or time function | Truncation is the generator's, from the question's `Grain`, so no metric needs one - and `generator.rs`'s `generate_date_trunc` special-cases only TSQL/Fabric and ClickHouse, so a fourth dialect's argument order is a live defect waiting for that dialect to be compiled. It is a `generate`-level defect, so avoiding `transpile` does not fix it |
| A bare `/` between aggregates | `DuckDB` to Postgres inserts `CAST(.. AS DOUBLE PRECISION)`, `DuckDB` to `BigQuery` does not, and Postgres to anything is left alone. **The transpiler changes the NUMBER depending on which dialect is declared as the source.** An explicit `NULLIF` suppresses the rewrite, which is why the divisor must be one - and it makes the zero-denominator behaviour explicit, which is what `ZeroDenominator` records for the closed vocabulary |

Plus `x IS TRUE`, already recorded as not portable in
[the architecture notes](../architecture.md), and a set of structural refusals that are about *reach*
rather than portability: a subquery, a table reference, a star, a bind placeholder, a schema
statement, a node the generator emits with no handling at all, and a fragment that aggregates
nothing.

### Two holes the four shape guards do not close

Both were found by testing rather than by reasoning, and both are recorded because the shape guards
read like a boundary and are not one.

**A scalar subquery in the projection.** `SUM(x) + (SELECT secret FROM secret_table)` is one
statement, one expression, no `FROM` and no alias - it passes all four guards - and it reads a table
the plan never granted. Closed by refusing any query node and any table reference, using the dialect
layer's own `is_query` and `is_ddl` classifiers as well as our lists.

**A clause that taking the projection would discard.** `SELECT 1 WHERE true` is legal in the
authoring dialect with no `FROM` at all, so `SUM(x) WHERE secret = 1` parses as one statement with one
projection and no `FROM`, and taking `expressions[0]` **throws the `WHERE` away**. The metric would
then be certified as `SUM(x)`, silently, over a predicate its author wrote and nobody removed on
purpose. Confirmed for `WHERE`, `GROUP BY`, `HAVING`, `QUALIFY`, `ORDER BY`, `LIMIT`, `WINDOW` and a
leading `DISTINCT`: each parses, each is dropped. Closed by rendering the whole wrapper statement and
the projection alone and requiring them to differ by exactly `SELECT ` - a check on the *rendering*
rather than on a list of `Select` fields, so it holds for a clause nobody thought of.

A comment is refused too, and it is the mildest of the three: `SUM(x) -- note` is re-emitted as
`SUM(x) /* note */` **into** the statement, putting catalog text between our own generated tokens.
The generator does escape a closing `*/` into `* /` - measured, so it is not an injection - but a
measure has no reason to carry prose that the document around it can hold instead.

## Consequences

- A catalog that uses the hatch cannot be loaded by a build that renders no SQL. `sutura_sql` is
  where the compile lives, and the network binary does not link it. That is not a gap to paper over:
  a build that cannot validate authored SQL must refuse a catalog that carries it, rather than serve
  the metric unvalidated. The composition root is what must call the compile, and a `Computation`
  that has not been through it is unvalidated by construction.
- The engine adapter cannot execute an authored expression at all. It builds a logical plan over
  Arrow and its `sql` feature is deliberately not compiled. So `Computation::measure()` returning
  `None` has to be a refusal there, naming the metric - never a skipped metric and never a
  substituted measure.
- **Portability is the author's claim, not ours.** The compile proves the fragment is one expression
  over declared columns, that it holds none of the refused constructs, and that each rendering parses
  in its own target's dialect. It does **not** prove the target *has* the function: `MEDIAN(x)` and
  `COUNT_IF(x)` are emitted verbatim into Postgres, where neither exists, and `PERCENTILE_CONT`
  likewise into ClickHouse. No per-dialect function catalogue is compiled into this build. Per-dialect
  variants are how an author discharges that claim precisely, and a dialect with no variant and no
  `portable` fragment is refused rather than guessed at.
- A residual fail-open remains on the date denylist: a date function spelled with a name not on the
  list, and with no typed node, renders verbatim. It is bounded by the fact that nothing needs one,
  and it is stated rather than hidden.
- Definition digests do not move. `Computation` is externally tagged so that flattened into a metric
  document a closed-vocabulary metric serializes as the `measure:` key it already had.
- The definition digest now covers a string somebody wrote. A catalog edit still cannot change what
  executes - the digest moves and travels with the answer - but a reviewer reading a diff is now
  sometimes reading SQL, which is a review burden the closed vocabulary did not impose.

## Alternatives considered

**`expression:` as a field on `Measure`, as the reference modelling languages do.** One key, no new
type, and drop-in compatible with the documents people already have. Rejected because it makes the
hatch invisible: a reviewer scanning a measure cannot tell which metrics are governed by a closed
vocabulary and which are text, and `AGENTS.md`'s claim that the vocabulary holds no SQL expression
would become false with nothing to replace it. The claim is worth keeping true of the closed part,
and that requires the two to be different shapes.

**One expression string with no dialect on it, as wren's cube path has.** Simplest, and it is what
the files being imported actually contain. Rejected because "it does not translate" then has no
honest answer: either the fragment is silently wrong on some target, or the metric is unusable there
and nothing says which. A map with a `portable` word and per-dialect overrides says it, and a provider
that already stores per-dialect SQL maps onto it without loss.

**An allowlist of AST node kinds instead of a denylist.** Fail-closed, and it would have caught the
subquery hole by construction. Rejected on the count: the scalar and aggregate space is some six
hundred node kinds, and enumerating the ones that are fine would recreate the closed vocabulary this
hatch exists to widen. The reach half is enumerable and small - there are only so many ways to name a
relation - so reach is a denylist backed by the dialect layer's own `is_query` and `is_ddl`
classifiers, and portability is a denylist of four measured defects. The residual risk of an
unlisted-but-harmful node kind arriving in a future version of the dialect layer is accepted and
stated.

**Refuse the whole class and keep pushing these metrics to the pinned-statement path.** No parser
anywhere on the definition path. Rejected for the reason 0002 rejected it once already: that path
needs an upstream renderer, which is the precondition the first-party path exists because it is
absent - and it now also means refusing an entire class of metadata provider rather than one metric.
