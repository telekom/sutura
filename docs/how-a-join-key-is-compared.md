# How a federated join key is compared

A federated answer joins two legs - a fact leg and a lookup leg - on a link column.
This page is the record of how that join is compared, what is refused before any row
is read, and what is **not** covered. It exists because the failure mode is a green
answer that proves less than it looks like: before the refusal existed, a type
mismatch between the two legs' link columns was indistinguishable from a key that
simply was not in the dimension table, and the answer came back wrong rather than
refused.

ADR 0007's *What is explicitly not decided* carried *"How a cross-source join key is
declared and compared"* as an open question
(`docs/adr/0007-federating-across-different-data-systems.md:1040-1042`). This page
closes the comparison half: the runtime refusal. The declaration half - a catalog
field saying two columns are comparable before either leg runs - is still open, and
the last section says so.

## What is compared, and where

The combiner is `sutura-exec-datafusion`'s `DataFusionCombiner`, the sole
implementor of `sutura_domain::plan::FederationCombiner`. The comparison happens
in two places, and the split is the whole of the design:

1. **Each leg's link column is classified to a kind from its Arrow schema**, before
   either leg's rows are read
   (`crates/sutura-exec-datafusion/src/combine/schema.rs:162-180`, `link_kind`).
   The kind is one of five - `ExactInteger`, `ExactDecimal`, `Text`, `Date`,
   `Boolean` - and anything else is refused (see below).

2. **The two legs' link kinds are compared to each other**, and a mismatch is
   refused
   (`crates/sutura-exec-datafusion/src/combine/schema.rs:150-158`, `agreeing_link`).
   The check is `fact == lookup`: the two kinds must be the same variant. There is
   no coercion, no widening, and no fallback. `ExactInteger` against `Text` is
   refused; `ExactInteger` against `ExactDecimal` is refused; `Date` against `Text`
   is refused.

The call site is `crates/sutura-exec-datafusion/src/combine.rs:346`:

```rust
agreeing_link(fact.link_kind(&link)?, lookup.link_kind(&link)?)?;
```

A second fact leg, when one is present for a cross-model ratio, is checked the same
way against the first fact leg at `combine.rs:360`.

### Why kinds, not types

An Arrow column has one declared type, but the interior's own row builder may give
two legs different Arrow types for one logical column. A leg whose link values all
fit an `i64` comes back `Int64`; one whose column mixes a fitting value with exact
integral text comes back `Decimal128(38, 0)`. Both are `ExactInteger`, and
`DataFusion`'s comparison coercion joins them correctly, so requiring the two
schemas to be equal would refuse a legal pair. The kind is the coarsest unit at
which a mismatch means *no row can ever match*.

### Why from the schema, not the data

The check reads a TYPE and no value
(`crates/sutura-exec-datafusion/src/combine/schema.rs:1-8`). The hand-written
combine that preceded this one decided a link column's kind from the first non-null
cell it happened to find, so two legs whose link columns could never match were
refused only when the data proved it - and two empty legs were answered as *no
rows* rather than refused. An Arrow column carries a declared type whether it has
rows or not, so the same mismatch is refused from the schema, before a batch is
read, for every input including an empty one
(`crates/sutura-exec-datafusion/src/combine/tests/refusals.rs:46-69`,
`two_empty_legs_whose_link_kinds_disagree_are_refused`).

## What is refused

### A link column whose Arrow type maps to no kind

`link_kind` returns `None` for any Arrow type outside the five kinds
(`crates/sutura-exec-datafusion/src/combine/schema.rs:162-180`). The refusal is
split by what the type is:

- **A floating-point type** (`Float16`, `Float32`, `Float64`) is refused as
  `CombineError::FloatLinkKey`
  (`crates/sutura-exec-datafusion/src/combine/schema.rs:193-196`,
  `classify_unjoinable`), which reaches a caller as
  `FederatedAnswerRefusal::FloatLinkKey`
  (`crates/sutura-domain/src/plan/federated/failure.rs:56-58`).
  This is ADR 0007's float-key rule: formatting a float into an equality lets
  distinct values collide, so a floating-point link column is forbidden by the
  DATA, and a caller is told.

- **Any other unmapped type** is refused as `CombineError::LinkTypeNotMapped`
  (`crates/sutura-exec-datafusion/src/combine/schema.rs:197-200`), which stays
  this workspace's own wiring defect and reaches a caller as nothing - it is not
  a `FederatedAnswerRefusal`. A question naming such a column would have failed
  at the presentation edge whatever the combine did.

### Two legs whose link kinds disagree

`agreeing_link` refuses a mismatch as `CombineError::LinkTypeMismatch`
(`crates/sutura-exec-datafusion/src/combine/schema.rs:154-157`), carrying both
sides' kind as a prose word - *an exact integer*, *text*, *a date* - via
`LinkKind::word` (`schema.rs:58-66`). The combiner maps that to
`FederatedAnswerRefusal::LinkTypeMismatch`
(`crates/sutura-exec-datafusion/src/combine.rs:445`,
`crates/sutura-domain/src/plan/federated/failure.rs:62-65`), and it reaches a
caller inside `RefusalReason::FederatedAnswerNotWellFormed`.

The refusal is **deterministic**: the same plan against the same legs refuses
again. A caller told to retry retries forever, which is the opposite of a
data-system failure, and `FederatedAnswerRefusal` says so in its own doc
(`crates/sutura-domain/src/plan/federated/failure.rs:22-25`).

### What the refusal replaced

Before `LinkTypeMismatch` existed, a type mismatch was a silent non-match. Under
an inner join the row was dropped, so the answer came back with fewer rows and no
error. Under a left join every fact row survived with a null remote side, so the
row count was right but the measure landed in a null bucket that should not exist.
Both are a wrong answer that looks like a right one, which is why the refusal is
taken at the combine rather than left to the presentation edge. The test that pins
both flavours is
`crates/sutura-app/src/federated/tests/leg_refusal_test.rs:268-323`, in two cells -
one per join kind - because a test asserting only a row count passes on the left
half.

## What is NOT covered

### No plan-time or catalog-time check

The runtime check fires after both legs have executed. Nothing at the catalog or
plan stage compares the two declared key columns: `Column::data_type` is
descriptive text - a quote of what the source called the column, and nothing
branches on it (`crates/sutura-domain/src/catalog.rs:114-119`). So both legs run
before the refusal, which is the cost of deciding from the schema rather than from
a declaration. A plan-time refusal in `federated_plan`, which holds the
relationship and both models, is the smallest design that would move the check
earlier - and it needs a classifier from `ColumnType` text to a kind, which does
not exist today.

### No comparability declaration on a relationship

`Relationship` carries a name, two models, a join type, and join keys - and no
field saying the two sides are comparable
(`crates/sutura-domain/src/catalog.rs:363-370`). A catalog author has no way to
declare that an `INTEGER` here and a `NUMBER(38)` there are intended to join, so
the runtime check is the only thing that holds the pair. The declaration half of
ADR 0007's open question - option (b) in the issue - is still open, and this page
does not close it.

### Case and whitespace on a text key

The combiner compares what it was handed. Two text keys that differ only in case
or trailing whitespace are different keys, and `IdentifierCase` decides
identifiers rather than values. No normalisation is applied, and none is planned:
the rule is that a join key is compared exactly, and a text key that differs in
case is a mismatch in the data rather than a defect in the comparison.

### Null link keys

A null link key does not join: the combiner excludes nulls before it counts and
before it joins, so a null-keyed fact row cannot match a null-linked lookup row, and
the join kind decides a null fact key the way it decides an unmatched one - null
under left, dropped under inner. This is decided and asserted, not incidental, and
it is not what this page is about: it is stated here only to name what is **not**
covered by the type-mismatch refusal.

### Mixed warehouse kinds are served, not refused

A federated answer whose two legs sit on two different warehouse kinds is
**reachable and served**. `sutura-cli`'s `kind::AnyWarehouse`
(`crates/sutura-cli/src/serve/kind.rs:56`) is a closed enum - one variant per
adapter this build linked - implementing `Warehouse`. When `open_engine`
(`crates/sutura-cli/src/serve.rs:800`) sees more than one kind group non-empty,
it routes to `kind::open_mixed`, which opens each group and merges them into
one `Warehouses<AnyWarehouse>`. `answer_federated<W, B, C>`
(`crates/sutura-app/src/federated.rs:114`) is generic in `W: Warehouse`, and
`AnyWarehouse` implements `Warehouse`, so a fact leg on `files` and a lookup
leg on `bigquery` reach the same combiner this page describes. ADR 0007's
Second amendment
(`docs/adr/0007-federating-across-different-data-systems.md:1169-1178`)
records this: "rather than refusing at the kind mismatch as `one_kind` used
to." `one_kind` is a *concept name* used in a comment
(`crates/sutura-cli/src/catalog.rs:73`)
to describe the single-kind fast path, not a function - there is no `fn
one_kind` anywhere in the codebase. So the type-mismatch this page describes
can fire across two different warehouse kinds, not only within one.

What IS genuinely one-kind is the *catalog* kind (`CatalogKind::Markdown` vs
`CatalogKind::Datahub`): a deployment naming both is refused at boot by
`catalogs_of_more_than_one_kind_in_one_deployment_are_refused`
(`crates/sutura-cli/src/serve/tests.rs:680`). That is a catalog-shape
refusal, not a warehouse-kind refusal, and has nothing to do with join-key
comparison.
