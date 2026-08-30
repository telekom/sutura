---
title: A table outside the connection's own dataset
description: How a metadata document names a table in another dataset or another project - why a qualified name is a composition of parsed identifiers rather than a string with dots in it, why cross-project is emphatically not federation and must not reach the splitter, how deep a path each dialect resolves, and the label collision a live BigQuery run found.
---

# A table outside the connection's own dataset

Status: **accepted.** Built, gated and pinned by goldens across four dialects, and a **qualified
read** is proved against a real project - a `project.dataset.table` path is resolved by the service
and a wrong dataset in one is refused. **A cross-PROJECT read is not proved**, because the acceptance
credential's IAM refuses `datasets.create` and there was no second project to read across; *What is
claimed, and what is not* at the end is the section to read before taking that green for more than it
is.

[0017](0017-what-a-bigquery-test-runs-against.md) and
[0018](0018-what-the-bigquery-wire-is-built-from.md) got a statement of ours accepted by a real
`BigQuery` dataset. This record is about the gap between *`BigQuery` works* and *`BigQuery` works
here*: the deployment this ships into is **multi-project**, and a metadata document could not name a
table outside the one dataset its connection defaults to.

## The report, and why the failure mode is a wrong number

Three things blocked it, all verified in the tree rather than inferred:

1. **A qualified name was unrepresentable.** `catalog::Model` held one `table: TableName`, and
   `TableName` parses through `parse_identifier`, which accepts `[A-Za-z_][A-Za-z0-9_]*` and rejects
   a dot as `IllegalCharacter`. There was no field for a path and no value that would have passed.
2. **The statement was emitted bare, for every dialect** - `builder::select(..).from(table)`.
3. **Resolution was a single connection-level default.** The `BigQuery` adapter holds one
   `default_dataset`, fixed when the warehouse opens and put into every `JobRequest`, so an
   unqualified name resolves there and nowhere else.

**The interesting half is what happens when the table exists in both places.** A table absent from
the default dataset is a clean `404` - honest. A table of the *same name* present in the default
dataset is read instead, and a plausible number comes back under a certified metric with its
definition digest unmoved. And same-name tables are the *normal* shape of a multi-project estate:
dev/prod splits, per-tenant datasets and staging copies all reuse table names deliberately. So this
sits in the same class as a wrong date bucket - valid SQL, nothing raised, wrong answer - and that is
what decides every choice below in favour of a refusal over a best effort.

## Decision 1 - a qualified name is a composition of parsed identifiers, never a string with dots

`sutura_domain::model::qualified` holds `QualifiedTable`, which is an `Option<TableQualifier>` and a
`TableName`; `TableQualifier` is an `Option<ProjectName>` and a **non-optional** `DatasetName`.

**Relaxing `TableName` to admit a `.` would have been one line, and it would have silently
invalidated the reasoning two invariants rest on.** *No identifier reaches the statement unquoted*
and *no value from a question reaches the statement as text* are both asserted over the golden corpus
by **stripping quoted spans** with a single toggle, and `golden/shared.rs` says in as many words why
that toggle is sound: an identifier is `[A-Za-z0-9_]` and nothing else, *"which is what makes that a
fact rather than an assumption - so neither a `"` nor a `'` can occur inside a name."* A dotted string
would have put a whole path where those assertions expect one name, and nothing would have failed.

So each part is parsed by the parser for its position and the generator quotes **per part**.
`QualifiedTable::parse` is a *splitter in front of the canonical constructor* for the convenience of
whoever writes a catalog document - it splits on `.`, which no part can contain, and hands each piece
onward. Nothing stores the text it was handed.

Three consequences worth stating because each was a decision:

- **A project without a dataset is unrepresentable rather than refused.** `project..table` is not a
  thing any data system names, and `TableQualifier` has no constructor taking a project alone and no
  optional dataset field. There is no check to get wrong.
- **`ProjectName` is the one name shape in this workspace that admits a hyphen, and it is not a
  loosening of the property above.** A `BigQuery` project id is lowercase letters, digits and hyphens
  - the credential this repository's acceptance leg runs under has one, which is how this was found
  rather than assumed - while a dataset id is letters, digits and underscore, exactly
  `parse_identifier`'s set. `model::Hyphens` is a flag on **one** parser rather than a second parser,
  and its documentation states the licence precisely: a hyphen is not `"`, `'` or `` ` ``, so the
  stripping argument is intact. A variant admitting a quote character, a dot or whitespace would
  invalidate it, and that is written where the flag is. A legacy domain-scoped project -
  `example.com:project` - is **refused**, because a `:` or a second `.` inside one part is the defect
  this module exists to prevent.
- **`Serialize` is hand-written to emit the dotted text.** `serde(try_from)` affects `Deserialize`
  alone, and a derived `Serialize` here would write a struct this type's own `Deserialize` refuses -
  the asymmetry that shipped in this workspace on `Date`. It matters because the definition digest is
  taken over the serialized form: a derived one would have covered a field layout appearing in no
  catalog file. An unqualified table therefore serializes exactly as a `TableName` did, which is why
  **no existing golden moved** and no existing digest moved.

The authoring surface is the same `table:` key carrying one part, two or three. There is no second
key for an author to get out of step with the first, and every document already on disk loads byte
for byte.

## Decision 2 - cross-project is NOT federation, and must not reach the splitter

**A source is a credential plus a billing project, not a project.** Two datasets, or two projects,
reached by one credential in one statement is **one** source, and `PlanSpansTwoSources` must not fire
on it.

This is the most expensive thing in this record to get wrong, and it was live rather than
hypothetical: row 10's splitter and combiner were being built in parallel. `BigQuery` joins across
projects **natively** - one credential, one job, one engine - and pushes the join down. Routing that
through a splitter and a client-side combiner would replace a pushed-down join with a slower one and
discard exactly the pushdown that makes the adapter worth having.

So the mechanism is a *shape* rather than a check: `sutura_semantic::plan` collects `SourceName` and
**nothing from any table path**, and the paragraph saying why now sits at that collection rather than
in a document. `PlanJoin` carries its own `QualifiedTable`, so a fact table in one dataset joined to a
dimension table in another is one `JOIN` in one statement. What `PlanSpansTwoSources` still refuses is
a second **credential**, which is the case it was always for.

## Decision 3 - how deep a path a data system resolves is declared, not guessed

`Dialect::qualification` returns `sutura_domain::model::Qualification` from an exhaustive match, on
the `DateTruncShape` and `identifier_quote` precedent - a fifth dialect cannot compile without
answering.

It has to be a declaration because **the dialect layer will render three parts for any target**: its
`TableRef` is a name plus two `Option`s with no per-dialect arity check. So without this, a
`project.dataset.table` rendered for a target with no third position produces a statement that fails
at the data system or, worse, resolves the leading part as something else.

| Dialect | Resolves | Why that arm |
| --- | --- | --- |
| `BigQuery` | `project.dataset.table` | A first-class path. One credential reaches several projects, and a cross-project join is native. This is the reason the feature exists |
| Postgres | `schema.table` | Cross-*database* is not a thing it does. Its three-part form is accepted only when the leading part is the database already connected to, so rendering one would work or fail on a connection detail no catalog can see |
| `ClickHouse` | `database.table` | It has databases and no catalog above them. A rendering claim, not an execution one - nothing here executes `ClickHouse` |
| `DuckDB` | the table alone | **The arm worth reading twice, because it is narrower than what DuckDB can parse.** DuckDB has schemas and attached catalogs. This is declared for what a DuckDB deployment *here* can resolve: `sutura-exec-duckdb` registers one view per model in the default schema and `sutura-exec-datafusion` registers one file per model in its own registry, so a qualified name resolves to nothing in either. Widening this arm is a change to what those adapters attach |

A path deeper than the target is `GenerateError::QualificationUnsupported`, naming both depths.
**Never a dropped qualifier** - dropping the part that does not fit is the wrong-number failure at
the top of this record, moved rather than fixed. It is a `GenerateError` and not a `RefusalReason`
because no question a caller could ask produces one: a table path comes from a catalog document,
which is upstream.

The engine and the two binaries that link it refuse the same thing independently:
`DataFusionWarehouse::scan` before it consults the registry, and `sutura-serve` and `sutura-cli`
**at boot**, so a deployment that could not serve its bundle does not start.

**No explicit table alias is emitted, and that is a declaration rather than an oversight.** A column
is qualified by the table's bare name, and `FROM a.b.c` gives the reference an implicit alias of `c`
in all four targets. Saying so explicitly with `AS "c"` would be better, and it is not reachable
through the fluent builder this repository uses deliberately: `left_join` takes a `&str` and the
aliasing constructor is private, so it would mean hand-building the AST. The implicit alias is what
holds, the live run is what measures it on the target where a misquoted identifier is not a syntax
error, and `sutura_sql::generate::table_path` carries this paragraph beside the code.

## Decision 4 - a label may not be spelled the same as a table the statement reads

`Definitions::assemble` already refused a dimension named after its metric or after the time bucket.
It now also refuses `LabelShadowsTable`: the metric's own name, `TIME_BUCKET_LABEL` and every
dimension name, against the metric model's table **and every joined model's table**.

**This one is a report, not a prediction.** The first live `BigQuery` submission returned `400
invalidQuery` - *"Cannot access field day on a value with type INT64"* - because a metric label
equalled the table name and `GoogleSQL` resolved the qualifier in `table.column` to the **select-list
alias** instead of to the table. Same root cause as the unqualified table itself: a physical name that
nothing checked against the labels beside it.

It is refused for **every** dialect rather than for the one that reported it, because a rule about
which of two things a qualifier binds to is precisely the kind of difference nobody should maintain
per target - and the alternative outcomes across four dialects are a wrong number, a rejected
statement and silence.

**It checks every label against every table, which is stricter than the collision that has to bite.**
A joined table is in the `FROM` only when a dimension reached through it is asked for, but any other
dimension can be asked for in the same question - so which pairs meet at query time is a function of
the question, and a load-time check that tried to predict it would sometimes let one through.
Refusing the cross product costs a catalog author one rename and cannot be wrong in the direction that
returns a number. The check reads the table's **bare** name, because that is what a qualifier binds
to: a metric named after the *dataset* collides with nothing.

**Amended, because the first version of this check compared for equality and a review reproduced the
gap.** GoogleSQL's lexical reference lists *aliases within a query*, *column names* and *field names*
as NOT case-sensitive - checked 2026-08-30 - while its **table** names are case-sensitive by default.
So a table named `Orders` is a distinct table, the qualifier `Orders` still resolves to a select-list
alias spelled `orders`, and an equality check let every such pair through to collide exactly as the
same-case one did.

The comparison now folds ASCII case, and it folds it for the two older checks too - a dimension named
after the time bucket, and a dimension named after its metric - because GoogleSQL documents a result
column's *name* as case-insensitive as well, so `Period` beside `period` is one column there. Neither
needed a new variant; what changed is the comparison.

**Which rule to compare under is a decision rather than an implementation detail, and it is made where
the vocabulary is.** `sutura_domain::model::IdentifierCase` is the vocabulary and
`sutura_sql::Dialect::identifier_case` is the per-dialect declaration, on the `Qualification` and
`DateTruncShape` precedent from Decision 3: the domain names what the difference is, a target answers
for itself, and a comparison decides. `BigQuery` is `InsensitiveAscii` from the reference above, and
`DuckDb` is too - **measured** on the pinned DuckDB 1.5.5, where a table created as a quoted `Orders`
is bound by a quoted `orders` qualifier and returns a result.

A bundle is dialect-agnostic, so both this check and Decision 5's compare under
`IdentifierCase::COARSEST` whatever a dialect declares. **That makes the declaration a self-check and
not a barrier, and saying which is the point:** a value declared `Sensitive` cannot make a bundle
unsafe, which is why `Postgres` and `ClickHouse` are declared from documented behaviour and not
measured - neither has a server in this repository to ask. What the declaration buys is
`every_dialect_is_at_most_as_case_folding_as_the_catalog_assumes`, so a variant folding *more* than
ASCII case fails a test instead of quietly making both comparisons too fine.

## Decision 5 - two tables whose paths end in one name are refused, not aliased around

Decision 1 says a column is qualified by the **last** part of a path, because `FROM a.b.orders` gives
the reference an implicit alias of `orders` in all four targets. What Decision 1 did not say is what
happens when two of the tables in one statement end their paths the same way, and a review reproduced
it by changing one fixture: a fact table at `analytics-prod.sales.orders` joined to a dimension table
at `reference-data.crm.orders` rendered an `ON` clause reading `orders.customer_id = orders.id` - one
table compared with itself - with every projected column qualified by an identifier naming two tables.
A real DuckDB 1.5.5 answers that statement `Binder Error: Ambiguous reference to table "orders"`; a
target that binds it to one side instead returns a number under a certified metric name. Same-name
tables across datasets are the estate shape this record exists for, so it is reachable rather than
exotic.

**Distinct explicit aliases are the fix that keeps the question answerable, and they are not reachable
through the builder this workspace renders with.** Measured against `polyglot-sql` 0.9.2:
`SelectBuilder::from_expr` takes an expression, so the `FROM` side could carry an `AS`, but
`left_join` and every other join method take a `&str` table name and `join_with_kind` is private - so
the joined side cannot be aliased without hand-building a select expression with upwards of thirty
fields, which `sutura_sql::generate`'s own header rules out. An alias on one side of a join and not
the other is not a fix.

So the decision is the other one, and it is made **unrepresentable rather than checked at render
time**: `sutura_domain::plan::StatementTables` is the only way to a `QueryPlan` - `QueryPlan::new`
takes it instead of a table plus a vector of joins - and its `parse` refuses two occurrences answering
to one identifier under `IdentifierCase::COARSEST`. There is therefore no ambiguous plan for any
dialect to render. `sutura_semantic::plan` turns that refusal into
`RefusalReason::PlanTablesShareAnIdentifier`, beside `PlanSpansTwoSources`, which is the other refusal
that stage produces about the shape of a statement rather than about anything a caller wrote.

**A QUERY-time refusal and not a load-time one, which is the opposite of Decision 4 and deliberately
so.** A colliding *label* costs its author a rename; a colliding *table* is a physical name nobody
here can change, and refusing the metric at load would make this record's own estate shape
unauthorable. So the metric stays authorable and only a question that actually puts both tables in one
statement is declined - a dimension on the fact table's own model is still answered, which
`golden/service.rs` asserts as the second half of the same test.

The comparison is over **positions** rather than over distinct paths, so the same table joined twice
through two relationships is refused too: two occurrences under one identifier is a duplicate alias
whether or not they name the same rows.

**One limit, stated because the shape is now in two places.** `LegPlan::Fact` carries a `joins` field
of its own and is built by struct literal, so this guard does not reach a leg. Nothing outside a test
constructs a leg - `AGENTS.md`'s *Built And Not Wired* says so - and the change that gives a leg a
producer is the change that should route it through `StatementTables`.

## What the goldens pin, and where

`crates/sutura-app/tests/golden/qualified.rs`, a third file on the dialect axis for the reason
`golden/legs.rs` is a second one: **the question corpus cannot produce these.** A question has no
field naming a table, so a path comes from a catalog document - and the shipped corpus under
`examples/single-player` is deliberately left unqualified, because the two data systems that
*execute* it register one file per model and would have nothing to resolve a project against.
Qualifying the corpus would have moved every existing golden and broken the executed axis in order to
demonstrate a rendering.

Three fixtures - a dataset qualifier, a project above a dataset, and that fact table joined to a
dimension table in **another project** - expanded over every registered dialect. Each cell handles
both outcomes, read off `Dialect::qualification` rather than listed, so a fifth dialect is covered
without an edit. Five statements carry the row cap, which moves `AGENTS.md`'s golden count from 88 to
93.

## What is claimed, and what is not

**Claimed, and measured against a real project on 2026-08-31.** The acceptance leg reads **the same
table three ways in one test** - unqualified through the job's `defaultDataset`, then `dataset.table`,
then `project.dataset.table` - and all three answer 42 for June and 99 for July. So a qualified path
both *renders* and *resolves*, and the implicit-alias declaration in Decision 3 holds on the target
where a misquoted identifier would not have been a syntax error.

**The negative control is what makes that a measurement rather than an inference**, and it is the
half worth insisting on: the same table name asked for in `sutura_no_such_dataset.<table>` is
**refused**. Without it, a service that ignored the qualifier and resolved the last part in its
default dataset would have produced three identical green legs while proving nothing - which is this
record's own opening failure, one layer further out.

**Not claimed, and each of these is a gap rather than a hedge:**

- **No second dataset was reached, and no second project.** Measured rather than assumed: the
  acceptance service account can see one dataset, and `datasets.insert` returns
  `403 accessDenied ... does not have bigquery.datasets.create permission`, so neither a second
  dataset nor a second project could be created to read across. **What that leaves unproved is
  narrower than it sounds and is worth stating exactly:** the three-part path is proved to *resolve* -
  the qualifier is read, and a wrong dataset in it is refused - and what is unproved is a read whose
  leading part is a project the credential does not already bill to. That is an IAM grant rather than
  a code change; the statement is identical in shape either way. It is the one thing to re-run when a
  second project exists.
- **A cross-project JOIN was not executed.** It is rendered, parse-checked, and pinned as one
  statement with one `JOIN` and one `SourceName` - which is the design claim of Decision 2 - but no
  local test can show a service performing it, and none pretends to.
- **Nothing about the corpus.** The live legs are hand-built statements, exactly as
  [0018](0018-what-the-bigquery-wire-is-built-from.md) says of its own.
- **Nothing about identity.** A service-account key is `SharedServiceUser`. Cross-project reads under
  a per-subject credential are [0008](0008-a-credential-per-leg-for-the-calling-subject.md)'s
  business, and this record touches none of it.
