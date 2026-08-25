# Single player

A complete catalog over a small synthetic telco warehouse, and the shortest path from a
clone to an answered question.

```bash
cargo run -p sutura-cli --features exec-duckdb -- \
  query examples/single-player/catalog \
        examples/single-player/questions/recurring-revenue-by-month.yaml \
        examples/single-player/data
```

```
-- definitions local-working-tree 937f8070916afd0fed6338cb17ba6a72da0bc53dd0daba520c0d54f4059544fe
period  recurring_revenue
2026-01-01      237320
2026-02-01      232822
2026-03-01      216700
2026-04-01      206160
2026-05-01      202994
2026-06-01      197122
```

Four things happened before that table appeared. The catalog was read and hashed, so the
digest on the first line names exactly the definitions the number came from. Every metric
that declares a certified number re-executed and reproduced it. The question was resolved
against those definitions and compiled to one statement, with every value bound as a
parameter. Only then did anything run. If the second step had failed, there would be no
table: a bundle whose anchors do not hold is not fit to serve, and saying so is the whole
point.

`--features exec-duckdb` is needed because the data-system adapter is default-off. Without
it, `compile` still renders the statement and `query` explains that it cannot run one.

## What is here

```
catalog/models/*.md            what tables exist and which columns may be read
catalog/relationships/*.md     which joins are allowed, and at what cardinality
catalog/metrics/*.md           what each certified number means
data/*.csv                     one file per model, named after the table
questions/*.yaml               the corpus, including the ones that are refused
```

A catalog document is YAML frontmatter and a prose body, and the prose is part of the
format rather than a comment. It travels with the definition and comes back out of
`sutura describe`, so it is where a definition says what it means and why it is drawn that
way. Read `catalog/metrics/subscriptions_churned.md` first; it is the one that explains a
metric this vocabulary cannot express.

Three more commands, in the order a reader usually wants them:

```bash
E=examples/single-player
cargo run -p sutura-cli --features exec-duckdb -- catalog  $E/catalog
cargo run -p sutura-cli --features exec-duckdb -- describe $E/catalog recurring_revenue
cargo run -p sutura-cli --features exec-duckdb -- compile  $E/catalog $E/questions/recurring-revenue-by-region-and-family.yaml
```

`compile` needs no data at all. It prints the statement, the parameters and the plan, which
is the useful thing to look at when the question is what sutura decided rather than what
the answer was.

## The measure vocabulary, in one catalog

A measure is one of three shapes, and a metric may carry filters that are part of its
definition. There is no field anywhere that takes a SQL expression. All four are here:

| Metric | Shape | Why it is written that way |
| --- | --- | --- |
| `voice_minutes` | `simple` | One aggregate over one column. Most metrics look like this |
| `recurring_revenue` | `simple` + `required_filters` | The active-only predicate is part of the name, and a caller can neither see it nor remove it |
| `subscriptions_churned` | `count_if` | A count of a boolean column would count the `false` rows too |
| `revenue_per_customer` | `ratio`, `zero_safe` | A sum over a distinct count of customers. Not the mean of a column, and computing it as one is a different number |

The pair worth reading together is `active_subscriptions` and `subscription_base`. They are
the same aggregate over the same rows in the same month; one carries `status = active` as a
definitional filter and the other does not. For June 2026 their certified numbers are 58
and 61, and `subscriptions_churned` reports 3 for the same month. One predicate, three
numbers that add up, and no caller can reach any of them.

## Anchors

Four metrics declare an `anchor`: a range and the number the metric produced over it when
it was certified.

```yaml
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 197122
```

Each one is re-executed before the catalog can answer anything. Change a number in
`data/`, or widen a required filter, and the run stops with a mismatch naming the metric
instead of quietly returning a different figure under the certified name.

The three metrics without anchors are the ones whose result is a float. A division and a
sum of decimals are not exact in binary, so an anchor written as text would pin how a
language prints a binary expansion rather than pinning a number. Money is held in minor
units throughout for the same reason: `mrr_cents` is an integer, so a total is exact and
the comparison is too.

## Refusals

Five questions in the corpus are named `refused-*` because that is what they are for. A
refusal is a result rather than an error, decided before anything runs, and it names what
was wrong:

```bash
cargo run -p sutura-cli --features exec-duckdb -- \
  query examples/single-player/catalog \
        examples/single-player/questions/refused-value-not-allowed.yaml \
        examples/single-player/data
```

```
refused: DimensionValueNotAllowed { metric: MetricName("recurring_revenue"), dimension: DimensionName("region") }
```

The other four ask for a grain the metric does not declare, a metric nobody has defined, a
filter on a dimension that is group-by only, and a dimension on a metric that has none.
None of them reach the data system.

## The data

It is synthetic, all of it. A seeded pseudo-random generator produced it, the customer
numbers look like `C0001` because a generator wrote them, and no row corresponds to a real
person, contract or account. It is shaped like telco data so the metrics are recognisable,
and none of the figures mean anything outside this directory.

It is also deliberately small. Every file is a few hundred lines and under the repository
limit of 1000, which is a property worth keeping rather than an accident: a quickstart is
worth more when a reader can open the CSV and check the arithmetic by hand. The daily usage
extract is therefore a two-week window, 2026-06-01 to 2026-06-14, rather than the full
quarter, and the two metrics over it are anchored to nothing for the float reason above.
The monthly snapshot covers all six months of the first half of 2026, because a revenue
series with one point is not a series.

## As a test

The same directory is an integration test, and there is no second copy of it:

```bash
cargo test -p sutura-cli --features exec-duckdb --test example
```

It loads the catalog, pins the digest, re-runs every anchor, runs the whole corpus and
snapshots the generated SQL and the rows that came back. That is what stops the commands
above from rotting: an edit that changes what this example does shows up as a snapshot diff
to review rather than as a README that used to be true.
