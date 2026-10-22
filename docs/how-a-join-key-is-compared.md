# How a federated join key is compared

A federated answer joins a fact leg and up to three lookup legs, each on its own link column.
This page is the record of how that join is compared, what is refused before any row
is read, and what is **not** covered. It exists because the failure mode is a green
answer that proves less than it looks like: before the refusal existed, a type
mismatch between the two legs' link columns was indistinguishable from a key that
simply was not in the dimension table, and the answer came back wrong rather than
refused.

ADR 0007's *What is explicitly not decided* carried *"How a cross-source join key is
declared and compared"* as an open question
(`docs/adr/0007-federating-across-different-data-systems.md:1049-1052`). This page
closes the comparison half: the plan-time refusal from the catalog's declared types and
the runtime refusal from the legs' schemas. The declaration half exists as an optional
relationship field (see the declaration section below); what is still open, tracked in
`telekom/sutura#1482`, is that the splitter does not yet write it onto a plan and the
plan-time and load checks do not read it. ADR 0007's *Fifteenth amendment*
records the split.

## What is compared, and where

The combiner is `sutura-exec-datafusion`'s `DataFusionCombiner`, the only
implementor of `sutura_domain::plan::FederationCombiner` a published build links
(`sutura-domain`'s `RefusingCombiner` also implements it, behind `cfg(any(test, feature =
"fixtures"))`). The comparison happens
in three places over one vocabulary of kinds, `sutura_domain::plan::LinkKind`, and the
split is the whole of the design:

0. **Before either leg runs, the two key columns' declared types are classified to a
   kind and compared**, in `federated_plan`
   (`crates/sutura-semantic/src/plan/federated.rs`, `link_types_agree`). The
   classifier is `LinkKind::declared`
   (`crates/sutura-domain/src/plan/federated/link.rs`): it reads a column's
   `ColumnType` text, case-folded and before any parameter list, and answers a kind
   or nothing. For a type it does not know, or a column that declares no type, it
   answers nothing. The plan then does not refuse, and steps 1 and 2 below decide.
   Only two known and different kinds are refused. A key truncated to a grain is the
   exception on the fact side: the fact leg groups it as a date, so it reads as `Date`
   whatever its origin is declared as.

1. **Each leg's link column is classified to a kind from its Arrow schema**, before
   either leg's rows are read (`link_kind` in
   `crates/sutura-exec-datafusion/src/combine/schema.rs`).
   The kind is one of five - `ExactInteger`, `ExactDecimal`, `Text`, `Date`,
   `Boolean` - and anything else is refused (see below).

2. **The two legs' link kinds are compared to each other**, and a mismatch is
   refused (`agreeing_link` in
   `crates/sutura-exec-datafusion/src/combine/schema.rs`).
   With no declaration the check is `fact == lookup`: the two kinds must be the same
   variant, with no coercion, no widening and no fallback. `ExactInteger` against
   `Text` is refused; `ExactInteger` against `ExactDecimal` is refused; `Date`
   against `Text` is refused. With a declaration the pair must be one the declaration
   joins (see the declaration section).

The call site is the combiner's `arm_plan`:

```rust
agreeing_link(declared, fact.link_kind(&link)?, lookup.link_kind(&link)?)?;
```

where `declared` is the plan's `link_comparison` for the first lookup leg and `None`
for a later one. A second fact leg, when one is present for a cross-model ratio, is
checked against the first fact leg by kind equality, before any cast, so under a
declaration a second fact leg of another kind than the first is refused as
`LinkTypeMismatch`.

### Why kinds, not types

An Arrow column has one declared type, but the interior's own row builder may give
two legs different Arrow types for one logical column. A leg whose link values all
fit an `i64` comes back `Int64`; one whose column mixes a fitting value with exact
integral text comes back `Decimal128(38, 0)`. Both are `ExactInteger`, and
`DataFusion`'s comparison coercion joins them correctly, so requiring the two
schemas to be equal would refuse a legal pair. The kind is the coarsest unit at
which a mismatch means *no row can ever match*.

### Why from the schema, not the data

The check reads a TYPE and no value (the header of
`crates/sutura-exec-datafusion/src/combine/schema.rs`). The hand-written
combine that preceded this one decided a link column's kind from the first non-null
cell it happened to find, so two legs whose link columns could never match were
refused only when the data proved it - and two empty legs were answered as *no
rows* rather than refused. An Arrow column carries a declared type whether it has
rows or not, so the same mismatch is refused from the schema, before a batch is
read, for every input including an empty one
(`two_empty_legs_whose_link_kinds_disagree_are_refused` in
`crates/sutura-exec-datafusion/src/combine/tests/refusals.rs`).

## What is refused

### A link column whose Arrow type maps to no kind

`link_kind` returns `None` for any Arrow type outside the five kinds
(`link_kind` in `crates/sutura-exec-datafusion/src/combine/schema.rs`). The refusal is
split by what the type is:

- **A floating-point type** (`Float16`, `Float32`, `Float64`) is refused as
  `CombineError::FloatLinkKey` (`classify_unjoinable` in
  `crates/sutura-exec-datafusion/src/combine/schema.rs`), which reaches a caller as
  `FederatedAnswerRefusal::FloatLinkKey`
  (`FederatedAnswerRefusal::FloatLinkKey` in
  `crates/sutura-domain/src/plan/federated/failure.rs`).
  This is ADR 0007's float-key rule: formatting a float into an equality lets
  distinct values collide, so a floating-point link column is forbidden by the
  DATA, and a caller is told.

- **Any other unmapped type** is refused as `CombineError::LinkTypeNotMapped`
  (`classify_unjoinable` in
  `crates/sutura-exec-datafusion/src/combine/schema.rs`), which stays
  this workspace's own wiring defect and reaches a caller as nothing - it is not
  a `FederatedAnswerRefusal`. A question naming such a column would have failed
  at the presentation edge whatever the combine did.

### Two key columns declared as different kinds

`federated_plan` refuses the question as `RefusalReason::FederationLinkTypeMismatch`
(HTTP `409`, code `federation_link_type_mismatch`) when the crossing relationship's
two key columns both declare a type, both types classify, and the two kinds differ:
`BIGINT` against `VARCHAR`, `DATE` against `BIGINT`, `NUMERIC(38,9)` against an
integer. `INT64`, `bigint` and `integer` are one kind, a `NUMERIC` written with scale
`0` is an integer, and a decimal is told apart only when its scale is written. A
float, a timestamp, a nested type and a bare `NUMERIC` classify as nothing and defer.

The refusal names the relationship and carries each declared type **quoted and
escaped** (`sutura_domain::plan::QuotedColumnType`), never raw: a source's own
dictionary supplied the text. It is a claim about the catalog and not about the data,
so a catalog that mis-declares a key refuses a join that would have matched.

### Two legs whose link kinds disagree

`agreeing_link` refuses a mismatch as `CombineError::LinkTypeMismatch`
(`agreeing_link` in `crates/sutura-exec-datafusion/src/combine/schema.rs`),
carrying both sides' kind as a prose word - *an exact integer*, *text*, *a date* -
via `LinkKind::word` (`crates/sutura-domain/src/plan/federated/link.rs`). The
combiner maps that to `FederatedAnswerRefusal::LinkTypeMismatch`
(`answer_not_well_formed` in `crates/sutura-exec-datafusion/src/combine.rs`,
`FederatedAnswerRefusal::LinkTypeMismatch` in
`crates/sutura-domain/src/plan/federated/failure.rs`), and it reaches a
caller inside `RefusalReason::FederatedAnswerNotWellFormed`.

The refusal is **deterministic**: the same plan against the same legs refuses
again. A caller told to retry retries forever, which is the opposite of a
data-system failure, and `FederatedAnswerRefusal` says so in its own doc
(`FederatedAnswerRefusal` in `crates/sutura-domain/src/plan/federated/failure.rs`).

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

### What the plan-time check does not decide

It decides only from declared types, and only for the crossing relationship's key:
a type it cannot classify, a column with no declared type, and the link of a
cross-model ratio's second fact leg are decided by the runtime check after both legs
have executed. `Column::data_type` is otherwise descriptive text - a quote of what
the source called the column - and `LinkKind::declared` reads it only to refuse.

### A relationship may declare how its keys compare

A relationship can declare how its two keys compare, with the field `comparison`
(markdown: `compare_as:`, one of `exact_integer`, `exact_decimal`, `text`, `date`,
`boolean`; it names the kind BOTH keys compare AS). Exactly two casts are admitted:
text read as an exact integer, and an exact integer widened to an exact decimal. A
cast leg needs an anchor leg of the declared kind, so two text legs under a declared
`exact_integer` are refused. Text to decimal, date and boolean are not admitted: text
to decimal rounds a longer fraction, text to date applies an offset and drops a time,
a decimal to an integer drops its fraction. The combiner reads the declaration off the
plan (`FederatedPlan::link_comparison`): a pair the declaration does not join is
refused as `LinkKindsNotJoined`, a value that does not cast is refused as
`LinkValueNotCast`, carrying kinds only and never the value; no value becomes NULL.

Limits: the splitter does not yet write the declaration onto the plan, and the
plan-time check and the load check do not read it yet, so no served question carries
one; the declaration applies to the first lookup leg and to a second fact leg, not to
a later lookup leg; DataHub reads none; a text value past i64 is refused; the cast leg
is a second copy of one column held outside the memory pool. The second fact leg must
be of the first fact leg's kind, else it is `LinkTypeMismatch` even where the
declared cast would join it; and Arrow's integer cast also accepts a leading sign and
surrounding ASCII spaces, so ` 1`, `1 ` and `+1` are the key 1 under a declared
`exact_integer`. The cells are in
`crates/sutura-exec-datafusion/src/combine/tests.rs` and `combine/tests/refusals.rs`
(a later lookup leg: `combine/tests/fan_in.rs`).

### Case and whitespace on a text key

The combiner compares what it was handed. Two text keys that differ only in case
or trailing whitespace are different keys, and `IdentifierCase` decides
identifiers rather than values. No normalisation is applied, and none is planned:
the rule is that a join key is compared exactly, and a text key that differs in
case is a mismatch in the data rather than a defect in the comparison.

Two cells in `crates/sutura-exec-datafusion/src/combine/tests.rs` hold it:
`two_text_link_keys_that_differ_only_in_case_do_not_join` (`C1` against `c1`, and `c1`
against `C1`) and
`two_text_link_keys_that_differ_only_in_trailing_whitespace_do_not_join` (`c1 ` against
`c1`, and `c1` against `c1 `). Each pair is asked twice. A left join keeps the fact row
with a null remote side. An inner join drops it. Both orders are held, so a fold or a
trim on one side only fails too. Leading whitespace, a tab and a newline are not held. Each cell has a committed mutation under
`devco/claim-mutations/` that folds or trims the key before the join, and each fails
under it.

The rule holds for an undeclared pair; a text key under a declared `exact_integer` is
read by the cast, so leading zeros are dropped (`01001` and `1001` are one key). What
else the cast accepts, such as surrounding spaces, is Arrow's and is not pinned here.

Limits. The combiner already behaved this way, so the cells pin it and do not change
it. They run the combiner over in-memory legs: what a source returns for a key is not
read. Under the mutation each cell fails on its left-join assert first, so the
inner-join assert is not killed on its own.

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
(`crates/sutura-cli/src/serve/kind.rs:60`) is a closed enum - one variant per
adapter this build linked - implementing `Warehouse`. When `open_engine`
(`crates/sutura-cli/src/serve.rs:865`) sees more than one kind group non-empty,
it routes to `kind::open_mixed`, which opens each group and merges them into
one `Warehouses<AnyWarehouse>`. `answer_federated<W, B, C>`
(`crates/sutura-app/src/federated.rs:115`) is generic in `W: Warehouse`, and
`AnyWarehouse` implements `Warehouse`, so a fact leg on `files` and a lookup
leg on `bigquery` reach the same combiner this page describes. ADR 0007's
Second amendment
(`docs/adr/0007-federating-across-different-data-systems.md:1176-1185`)
records this: "rather than refusing at the kind mismatch as `one_kind` used
to." `one_kind` is a *concept name* used in a comment
(`crates/sutura-cli/src/catalog.rs:83`)
to describe the single-kind fast path, not a function - there is no `fn
one_kind` anywhere in the codebase. So the type-mismatch this page describes
can fire across two different warehouse kinds, not only within one.

What IS genuinely one-kind is the *catalog* kind (`CatalogKind::Markdown` vs
`CatalogKind::Datahub`): a deployment naming both is refused at boot by
`catalogs_of_more_than_one_kind_in_one_deployment_are_refused`
(`crates/sutura-cli/src/serve/tests.rs:776`). That is a catalog-shape
refusal, not a warehouse-kind refusal, and has nothing to do with join-key
comparison.
