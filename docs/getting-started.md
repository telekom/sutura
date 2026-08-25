---
title: Getting started
description: Point the binary at a catalogue and a file, and ask one question.
---

# Getting started

There is a catalogue, a data file and a question in the repository already - the fixtures the golden
suite runs on - so the fastest way to see what this does is to point the binary at those.

```bash
cargo run -p sutura-cli --features exec-duckdb -- \
  catalog crates/sutura-app/tests/fixtures/catalog
```

`--features exec-duckdb` is not optional for anything that reads data. The adapter is default-off,
because two of the four shipped artifacts are musl and there is no musl `libduckdb` to link them
against; a build without it still compiles a question to SQL, and says so if you ask it to run one.

## What a catalogue says

```text
version local-working-tree
digest  c05bf3c529039924fb4abce95d1a2c4b7908308aec557dce2694f71283905e4e

average_order
  measure    avg(amount_cents)
  filters    none
  grains     month
  dimensions none
  anchor     none
average_order_value
  measure    sum(amount_cents) / count_distinct(order_id), zero-safe
  filters    none
  grains     day, month
  dimensions none
  anchor     none
revenue
  measure    sum(amount_cents)
  filters    none
  grains     day, month
  dimensions channel, region, segment
  anchor     470023 over [2026-06-01, 2026-07-01)
web_revenue
  measure    sum(amount_cents)
  filters    channel = "web"
  grains     day, month
  dimensions none
  anchor     none
```

The digest is over the canonical form of the parsed definitions, so reformatting a document does not
move it and changing what a metric means does. It travels with every answer.

`describe` prints one metric in full, including the prose from the body of its document - which is
what the markdown half of the format is for.

```bash
cargo run -p sutura-cli --features exec-duckdb -- \
  describe crates/sutura-app/tests/fixtures/catalog revenue
```

## Asking

A question is a small file. There is no field in it for SQL, a table, a predicate or a list of row
ids, so an uncertified question is not something you can write down:

```yaml
metric: revenue
grain: month
range:
  start: 2026-06-01
  end: 2026-07-01
dimensions: [region]
```

`compile` turns it into a statement and stops. No data system is involved, which makes it the
command to reach for when the question is what we would have run:

```bash
cargo run -p sutura-cli -- \
  compile crates/sutura-app/tests/fixtures/catalog \
          crates/sutura-app/tests/fixtures/questions/revenue-by-region.yaml
```

```sql
SELECT "customers"."region_code" AS "region",
       CAST(DATE_TRUNC('month', "orders"."order_date") AS DATE) AS "period",
       SUM("orders"."amount_cents") AS "revenue"
FROM "orders" JOIN "customers" ON "orders"."customer_id" = "customers"."id"
WHERE "orders"."order_date" >= ? AND "orders"."order_date" < ?
GROUP BY "customers"."region_code", CAST(DATE_TRUNC('month', "orders"."order_date") AS DATE)
ORDER BY "customers"."region_code", CAST(DATE_TRUNC('month', "orders"."order_date") AS DATE)
LIMIT 10000
```

(printed on one line; wrapped here to read). Both dates are bind parameters, and the plan behind the
statement is printed after it. Pass a dialect as a third argument - `duckdb`, `postgres` or
`clickhouse` - to see the same plan rendered for another data system: `ClickHouse` gets `dateTrunc`
and `sum`, Postgres gets `$1` and `$2` instead of `?`.

`query` answers it. It checks every declared anchor first, and will not serve a bundle whose anchors
did not all match:

```bash
cargo run -p sutura-cli --features exec-duckdb -- \
  query crates/sutura-app/tests/fixtures/catalog \
        crates/sutura-app/tests/fixtures/questions/revenue-by-region.yaml \
        crates/sutura-app/tests/fixtures/data
```

```text
-- definitions local-working-tree c05bf3c529039924fb4abce95d1a2c4b7908308aec557dce2694f71283905e4e
region  period      revenue
north   2026-06-01  225072
south   2026-06-01  244950
west    2026-06-01  1
```

## Being refused

A refusal is a result, not an error, and the exit status says so. Ask for a region the catalogue does
not declare a value for:

```bash
cargo run -p sutura-cli --features exec-duckdb -- \
  query crates/sutura-app/tests/fixtures/catalog \
        crates/sutura-app/tests/fixtures/questions/refused-value-not-allowed.yaml \
        crates/sutura-app/tests/fixtures/data
```

```text
refused: DimensionValueNotAllowed { metric: MetricName("revenue"), dimension: DimensionName("region") }
```

Note what the refusal does not say: the value you asked for. A rejected value is not echoed into a
message that reaches a log, a terminal and an agent's context, because that is how a rejected value
becomes somebody else's input.

The other fixtures under `questions/` named `refused-*` reach the rest of the refusals, one per
reason.

## Your own catalogue

A catalogue is a directory of markdown documents. Each one declares what it is, so a file in the
wrong place is an error rather than a definition that was quietly never loaded.

A model names a table and its columns:

```markdown
---
kind: model
name: orders
source: local
table: orders
columns: [order_id, order_date, customer_id, channel, amount_cents]
---
One row per order, as booked. Money in minor units, so a total is exact.
```

A metric names a model, what it measures, the grains it answers at and the dimensions it may be
broken down by:

```markdown
---
kind: metric
name: revenue
model: orders
measure:
  simple: { aggregate: sum, column: amount_cents }
time_column: order_date
grains: [day, month]
dimensions:
  - name: channel
    column: channel
    values: [web, store]
anchor:
  range:
    start: 2026-06-01
    end: 2026-07-01
  value: 470023
---
Total booked order value, in minor units.
```

`measure` names its shape, and there are three. `simple` is one aggregate over one column, above.
The other two exist because most certified metrics are not that:

```yaml
# A ratio: one aggregate divided by another, over possibly different columns.
measure:
  ratio:
    numerator:   { aggregate: sum,            column: amount_cents }
    denominator: { aggregate: count_distinct, column: order_id }
    zero_safe: true
```

```yaml
# How many rows have a boolean column true. Its own shape because COUNT(col) counts
# the false ones too, which is a wrong number that raises no error.
measure:
  count_if: { column: churned_in_month }
```

A metric may also carry `required_filters`, which are part of what it *means* rather than something
a caller chooses:

```yaml
required_filters:
  - equals: { column: channel, value: web }
```

Four things about all that are worth knowing before you write one:

- **A measure is a shape from a closed vocabulary, not an expression.** There is no field for
  `sum(price * quantity)`, and [the closed vocabulary for
  measures](adr/0002-a-closed-vocabulary-for-measures.md) argues why: a string field is an escape
  hatch, and an escape hatch on the query path is the thing being defended against. What the
  vocabulary cannot say belongs in a statement rendered upstream. `zero_safe` is required rather than
  defaulted, because "a rate over an empty period is null" and "is an error" are both defensible and
  a definition should say which.
- **A `required_filter` is definitional, and a caller can neither see it nor turn it off.**
  `web_revenue` *means* the web number; a statement that left the predicate out would return total
  revenue under a certified name. That is a wrong answer arrived at by omission rather than by
  tampering, which is the more likely failure and the harder one to notice. Note the consequence for
  modelling: a metric with a required filter on `channel` should not also declare `channel` as a
  dimension, or grouping by it would be a way to ask the metric for the figure it excludes.
- **`values` is what makes a dimension filterable.** Without it the dimension can be grouped by and
  not filtered on, because a filter needs an allowlist - the alternative is comparing against
  whatever the caller sent.
- **An `anchor` is a number this metric produced when somebody certified it.** It is re-executed
  before the bundle is served, so a definition that has stopped meaning what it claimed fails
  readiness instead of answering. Declare one for any metric whose value you would act on.

For the data, `query` expects one CSV per model, named after the model's table, in the directory you
pass it. Nothing is written: the database is built in memory from the CSVs on every run, so it cannot
drift from them.
