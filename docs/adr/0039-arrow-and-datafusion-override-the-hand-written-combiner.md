---
title: Arrow and DataFusion override the hand-written combiner
description: Owner instruction overriding ADR 0007's second amendment and the unmerged ADR 0037. Records the five requirements the federation path is now held to - Arrow native, DataFusion native, DataFusion so federation arrives later rather than being redeveloped, always impersonation-capable, ADBC - and decides the first step of it: datafusion's compression feature is on deliberately, deny.toml carries the one licence it adds, and a compressed CSV or NDJSON source is a stated capability with the extension parsed rather than inferred. States what compressed Parquet does and does not owe to that feature, why the datafusion-federation dependency cannot land ahead of its caller, and the three measurements that bound the remaining steps - the arrow 58/59 split, the ADBC driver manager's own major, and the size of the port change.
---

# Arrow and DataFusion override the hand-written combiner

Status: **accepted by owner instruction**, overriding two records. Steps 1, 2, 3 and 5 are BUILT;
step 4 is blocked ahead of an upstream manifest change. Each step says which parts, beside each
measurement.

What it overrides:

- [Federating across different data systems](0007-federating-across-different-data-systems.md)'s
  *second amendment* - *the combiner is NOT DataFusion* - and its *Where the `RowSet`-to-Arrow
  boundary lives* decision, which put the port's currency at `RowSet` for the first federated
  milestone. Both are corrected in place in that record rather than struck.
- ADR 0037, *`DataFusion` is the combiner and the federation boundary is Arrow*, which refused Arrow
  as domain vocabulary on a cost measurement. **That record is not in this branch's tree** - it lives
  unmerged on `feat/datafusion-combiner-0007` together with the `ComputeContext` it justifies - so
  it is overridden here by name and amended where it lands, because amending a file that is not
  present would be fiction.

## The five requirements

The instruction is one direction with five things that must all end up true, and they are written
here because no single step below satisfies more than two of them:

|   | Requirement                                                                             | Where it stands                                                                                                                                                                                                            |
| - | --------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| a | **Arrow native**                                                                        | **Built.** Step 2, both halves: the interior names the array types and `Warehouse::execute`'s currency is record batches.                                                                                                  |
| b | **DataFusion native**                                                                   | **Built.** Step 3: the combine is one `DataFusion` logical plan behind a driven port.                                                                                                                                      |
| c | **DataFusion so federation arrives later** rather than being redeveloped for multi-node | Groundwork: a DataFusion-native local combine plus a provider seam. `datafusion-federation` is not a dependency - step 4 adopts it and is blocked upstream - so there is no provider pushdown and no multi-node execution. |
| d | **Always impersonation-capable**                                                        | Unchanged by this record and constrained by it: see *What this does not buy*.                                                                                                                                              |
| e | **ADBC**                                                                                | Built. `sutura-exec-bigquery`'s `adbc` transport is the only BigQuery mode.                                                                                                                                                |

**No hand row handling is the standing rule this comes from**: `DataFusion`, Arrow, Arrow Flight or
ADBC, and nothing that walks a result one cell at a time. That rule is what makes the blast radius
of step 2 worth paying rather than a refactor for its own sake.

## Step 1, decided and built: compression is a capability, not unification

`datafusion`'s `compression` feature is **on**, and `deny.toml` allows `bzip2-1.0.6`.

**Why this was not already true**, because the reason was a real control rather than an oversight:
the feature pulls `bzip2` -> `libbz2-rs-sys`, whose registry licence field is `bzip2-1.0.6`, and
`deny.toml` sets `unused-allowed-license = "deny"`. So the licence could not be allowed ahead of a
feature that needed it, and the feature could not be turned on while the licence was refused.
`Cargo.toml`'s own comment recorded that pair and ADR 0006 measured it while declining a federation
crate. Breaking the cycle is a decision about what this runtime reads, and the decision is that a
compressed CSV or NDJSON source is a capability worth one permissive licence.

**What the feature buys, precisely, because the two halves are bought by different things:**

- **Compressed Parquet owes it nothing.** Parquet records its codec per column chunk inside the file
  and the `parquet` feature's own codecs read it, so a Snappy, GZIP or ZSTD Parquet file has always
  read. `compression` adds no Parquet capability at all, and the attach path refuses an OUTER codec
  around a Parquet file by name - `orders.parquet.gz` is a wrapper around something already
  compressed, and reading it would mean unwrapping a file nobody meant to write.
- **Compressed CSV and NDJSON is the whole of what it buys.** Those two are plain text, so an outer
  codec is the only compression they have.

**The codec is parsed from the path, not inferred.** `crates/sutura-exec-datafusion/src/attach.rs`'s
`Codec::of_path` answers a suffix this build reads, or refuses a suffix that certainly names a codec
and is not one - `.gzip`, `.lz4`, `.zip`. The failure it replaces is silent rather than loud: handing
a compressed file to the engine as text does not error, it infers a schema from the codec's header
bytes and resolves the table to columns nobody declared. An extension that names no codec at all
(`orders.txt`) stays text, because a CSV may legitimately be called that.

**One list, read two ways.** `sutura_exec_datafusion::candidates` enumerates every file name a
model's table may arrive under - Parquet, then CSV and NDJSON each plain and once per codec - and
`attach_file` dispatches on the same table. Both of `sutura-cli`'s file-source searches read that
function instead of spelling their own preference order, which is what stops a deployment offering a
candidate the engine then refuses, or missing one it reads.

**What is proven and what is not.** `gz` and `bz2` are written by a dev-dependency writer and read
back end to end, `bz2` specifically because it is the licence entry: an allowed licence whose codec
nothing reads is an entry justified by nothing. `xz` and `zst` are **parse-only** - the suffix maps
to the right codec and no cell decodes one - and no gate would notice if this build could not in fact
decode them. Adding a writer for either is the fix if that matters.

## Step 2, decided, and built as far as the interior: Arrow as domain vocabulary

**The decision:** the domain names the Arrow array types, and `Warehouse::execute`'s currency
becomes Arrow record batches. That reverses 0007's *the port's currency stays `RowSet`* and 0037's
refusal of Arrow as domain vocabulary.

**Built: the first half, which is the architecture decision.**
`sutura_domain::warehouse::arrow` holds `Accumulating` - a stream checked against its announced
schema by NAME and type, under a row ceiling, before a value is read - and `ResultBatches::to_rows`,
the one Arrow-to-`Value` decode in this workspace, with a type pass ahead of it. The engine reads
its own collected batches through it and has no `cell` of its own any more.

**Built: `BigQuery` stops hand-decoding, which is what the four open review threads on
`telekom/sutura#929` were all asking for.** The transport port's `run` returns `ResultBatches`, so
the ADBC driver's typed Arrow arrays reach the interior unchanged. What that DELETED, rather than
moved:

| Gone                                                     | What it was                                                                                                                                                                                                                                    |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `adbc/decode.rs`                                         | cast every column to `Utf8`, then a text cell per value                                                                                                                                                                                        |
| `rowset.rs`                                              | a schema pass over six `FieldType`s, then a `parse::<i64>()` per cell                                                                                                                                                                          |
| `transport.rs`'s `Cell`, `Field`, `FieldType`, `JobRows` | a text-cell result vocabulary, shaped by the deleted HTTP wire transport where every value arrived as a JSON string whatever its declared type was                                                                                             |
| seven `BigQueryError` variants                           | an unmapped type, an `INT64` that did not parse, a `FLOAT64` that did not parse, a `BOOL` that was neither spelling, a non-finite double, a date that did not parse, a ragged row - one `Unreadable` wrapper over the interior's own cause now |
| `BigQueryError::Incomplete`                              | a delivered count against the endpoint's `totalRows`; see below                                                                                                                                                                                |
| the `arrow-cast` dependency                              | what the cast-to-text needed, and `cargo check` named it once nothing used it                                                                                                                                                                  |

**Why `Incomplete` is gone rather than relaxed.** It refused a page whose row count was below the
endpoint's reported total, which is `jobs.query`'s paging shape and which only the deleted HTTP
transport ever reported. An ADBC read streams the whole result and `run` drains the reader to
exhaustion, so completeness IS the drain: a truncated stream is an `Err` rather than a short answer.
A LISTING still carries a total, because a dataset listing is a metadata document rather than a
result stream, and `ListingTotal` is unchanged.

**What replaced the page bound, because a bound was removed and something had to.** A result that
did not fit used to be recognised two ways - a page token, and `Incomplete`. An ADBC stream has no
pages; what it has is `MOST_RESULT_ROWS`, and a stream refused for crossing it is equally *the caller
cannot get this whatever it retries*. So `AdbcBigQuery` overrides `result_did_not_fit` for
`UnannouncedBatch::OverBound` alone: the port's default `false` would have reached a caller as a
`503`, the status a dead endpoint produces, inviting a retry that returns the same stream.

**The hole the interior's type pass closes, and it was measured rather than imagined.** `cell`
answers a null before it reads a column's type, so a result with NO rows never reaches it and a
column that is entirely null reaches it and is answered: a `TIMESTAMP` column came back as a
successful EMPTY result, and whether this workspace maps a type depended on what the data happened
to be. `sutura-exec-bigquery`'s own decoder had a schema pass that closed it and the engine did not.
It is in `ResultBatches::to_rows` now, which is the shape of this whole step: a check that existed
once and applied to one adapter now exists once and applies to all of them.

**Built: the port's signature, and step 3 is the consumer that made it worth paying for.**
`Warehouse::execute` returns `ResultBatches`. `ResultBatches::to_rows` is called once, at the
presentation edge in `sutura_app::answer`, so a decode failure that used to arrive wrapped in an
adapter's own error is `ServiceError::Unreadable` for every adapter at once.

**What that costs, because this record counted only the half that gains.** Two adapters stop
converting: `sutura-exec-bigquery` hands the ADBC driver's typed arrays straight through and the
engine returns the batches it collected. FOUR start - `DuckDB`, `Postgres`, Oracle and `ClickHouse`
speak rows, so on a SINGLE-source answer each now pays `arrow::of_row_set` out and `to_rows` back for
data that never left the process. The conversion did not disappear; it moved to the adapter that owns
the row-speaking driver, which is where a federated leg's cost had to be paid anyway.

**And one measurement refuted this record's argument for the row builder's inference limit.**
`arrow_column` said a mixed column round-trips as text and that no source produces one, *because a
data system declares a column's type*. A data system does - and a row-speaking adapter maps it PER
CELL: `DuckDB` and `Postgres` both answer a whole number that fits an `i64` as `Value::Integer` and
one that does not as exact `Value::Text`, so one column arrives mixed. Two conformance corpus cases
are exactly that shape (`wide-total-by-day`, `overflowing-integer-total-by-day`) and the first run of
the port change failed them on the production path, with `Integer(15)` rendered to `Text("15")`. So
`arrow_column` gained a `Decimal128(38, 0)` arm for a column mixing `Integer` with exact integral
text, which round-trips both halves because `to_rows`'s zero-scale arm widens a value that fits an
`i64` and leaves one that does not as its exact text - the pairing `sutura-exec-bigquery`'s
conformance fake already declared by hand. An all-TEXT column is never promoted, deliberately: a
postal code of `"01234"` would lose its leading zero.

**One row-handling path in `sutura-exec-bigquery` is deliberately untouched**, because it is not a
result decode: `importer.rs` renders a fixture CSV's cells into `GoogleSQL` literals behind the
default-off `fixtures` feature. That is a WRITE path with its own no-interpolation argument, and no
question reaches it.

**Why the guard is ours to write, and it is not a belt on a brace.** `RecordBatch::try_new`
validates positionally and by TYPE ONLY - it zips columns against fields and never reads a field
name - so a driver handing back two same-typed columns in the wrong order builds a perfectly valid
batch, and a consumer that counted columns would label those values with the announced schema's
names: a transposed answer under a certified metric name, with no error anywhere. A
differently-typed swap Arrow already refuses, which is why the mutation that matters, and the cell
that pins it, is a swap of two **same-typed** columns. Nothing in `DataFusion` closes it either:
`SchemaAdapter`/`SchemaMapper` are deprecated, the live `PhysicalExprAdapter` resolves by name on
the DATASOURCE path and is opt-in, and nothing validates a custom `ExecutionPlan`'s stream against
its declared schema at all.

**The ceiling is the caller's, and the two callers pass different values.** A foreign driver is what
the bound exists for - a federation leg carries no `LIMIT`, so nothing in the statement bounds what
the source streams - and `MOST_RESULT_ROWS` is that caller. The engine passes `usize::MAX`
deliberately: what protects this process from its own wide result is the memory pool in
`sutura-exec-datafusion`'s `pool`, an operator reservation, which is where 0009 puts it. So
`UnannouncedBatch::OverBound` is unreachable through the engine's own collection, and saying so is
cheaper than a second bound that would refuse an answer the pool had already granted.

**A limit on the row builder, stated where it is made.** `arrow::of_rows` - behind the `fixtures`
feature, for a fake and for an adapter whose source speaks rows - infers a column type: all-`Integer`
is `Int64`, all-`Real` is `Float64`, anything else is `Utf8` with each value rendered. So a MIXED
column does not round-trip: an `Integer` in a column that also holds `Text` comes back as `Text`.
That is a per-cell union meeting a per-column format, and it is acceptable because no data system
produces a mixed column - a source declares a column's type. The cell that pins the round-trip
asserts the mixed case as text rather than pretending otherwise.

**The one place that limit bites, and what the fixture does about it.** `sutura-exec-bigquery`'s
conformance fake is claiming what a data system would have SENT, and the corpus has columns that
answer `Integer` for one row and a wide `Text` for another - a real `BigQuery` reports one `NUMERIC`
for such a column either way. So that fake declares `Decimal128` with a PER-COLUMN scale rather than
using `of_rows`: at scale 0 the interior widens a whole number that fits an `i64` back to `Integer`
(`wide-total-by-day`), and at a positive scale it renders the exact text (`decimal-total-by-day`).
Getting that split wrong is what a run measured - a column declared at scale 0 lost `11.50`'s
fraction and answered null.

**Four measurements that bound it, taken on this branch on 2026-09-21.** Two of them correct how
this was scoped, one is what makes step 2 possible at all, and the last is what it cost:

- **The ADBC driver manager is on the engine's Arrow major.** `adbc_core 0.24.0` declares
  `arrow-array 59.2.0` and `arrow-schema 59.2.0` in `Cargo.lock`, which is the major
  `sutura-exec-datafusion` resolves through `datafusion`. So a batch the BigQuery driver produces can
  reach the interior with no conversion and no C data interface - which `unsafe_code = "forbid"`
  puts out of reach anyway.
- **The 58/59 split is still open, and it bounds which adapter can be Arrow-native rather than
  whether the port can be.** `duckdb 1.10505.0` - the pinned release, checked against
  `index.crates.io` - still declares `arrow ^58`, and `devco/arrow-majors-allow` tolerates the split
  as a DUPLICATE on the stated test *whether any first-party crate names the type*. The interior
  naming `arrow-array 59` keeps that answer NO for `sutura-exec-duckdb`, which converts through the
  domain's own row vocabulary and names no Arrow type - so this change does not convert the duplicate
  into a type boundary. What it does forbid until `duckdb-rs` releases its merged arrow-59 bump is a
  **DuckDB adapter that hands its native batches through**, which is the one adapter that must keep
  building rows by hand.
- **The port change is not a signature tweak, which is why it is not in this change.** On this tree:
  52 `fn execute` implementations, 59 `.execute(` call sites, 79 `RowSet::new` constructions and 94
  `.rows()` reads. Most are fakes, and the shape that makes it mechanical rather than a rewrite is
  the domain-side pair above - build batches from rows, read rows from batches - so an adapter or a
  fake that has rows changes one line.
- **`ALLOWED_IN_DOMAIN` is the cost that is not mechanical, and it is paid.** The domain naming
  `arrow-array` and `arrow-schema` took that allowlist's walk from 33 crates to 97 - so the entry
  added 64, which is the number a reader should carry. **Every other number here needs its venue
  named, because they disagree.** Under `cargo tree -p sutura-domain --all-features`, 21 of the 64
  are compiled and the remaining 43 are the over-broad kind that allowlist already carries -
  optional and platform-specific edges `cargo metadata` resolves for every target, including a
  `wasm-bindgen` pair and the `windows-*` family. Under the WORKSPACE-unified resolve that
  `just test` and `just lint` run in, more than 21 are compiled: one `arrow-array` serves the whole
  graph, `adbc_core` turns its optional `chrono-tz` on, and features unify, so the zone DATABASE
  compiles into the interior in the venue every gate uses. A per-package tree does not show that.
  **Two of the 21 are worth naming rather than counting**: a time-zone database (`chrono` ->
  `iana-time-zone`, which reads `/etc/localtime` or asks `CFTimeZoneCopySystem`) and an entropy
  source (`arrow-array` -> `ahash` -> `getrandom`, directly and not through `hashbrown`) are now in
  the hexagon's interior, reachable from no code this crate has. That last clause is a claim about
  current use and not a mechanism, so `clippy.toml` now bans the clock and entropy entry points by
  name; what no mechanism here reaches is the calls `arrow-array` and `ahash` make inside
  themselves. **No new lockfile entry**: both crates were already resolved at 59.2.0. Arrow is a
  data format rather than a runtime, a client or an engine, which is the line
  `xtask/src/boundaries/edges.rs` actually draws - and this record is what authorises the entry.

## Step 3, decided and BUILT: the combiner is a DataFusion plan behind a driven port

`sutura_domain::plan::FederatedPlan::combine` is deleted and replaced by a `DataFusion` plan over the
two legs' batches: two in-memory tables, one join, one aggregate, one projection, one sort.

**A PORT, and this step's first revision got that wrong.** It said the combine is *built in an
adapter crate* and that *`sutura-app` calls the combiner, above every adapter, exactly as it calls
`combine` today* - which is the shape `xtask check-boundaries`'s application rule refuses: an
application that names an engine is an application whose adapters are no longer a property of the
build. So the seam is the one `docs/adr/0007` designed and recorded as unbuilt:
`sutura_domain::plan::FederationCombiner` is declared in the domain beside `Warehouse` and
`SemanticCatalog`, `sutura-exec-datafusion::DataFusionCombiner` implements it,
`LocalService<W, S, B, C>` is generic in it, and a composition root chooses the implementor.
`-domain` names no engine: `ALLOWED_IN_DOMAIN`'s stated line is *no runtime, no client, no engine*,
and DataFusion is one. Arrow is a data FORMAT, which is why `ResultBatches` may be the currency on
both sides of that signature - the argument is step 2's and was paid there. `docs/adr/0007`'s
*Fifth amendment* carries the four mechanisms the port's shape is held by.

**Three seams existed for reaching the engine and only one was honest, so the rejected two are
recorded rather than assumed away.**
`xtask/src/boundaries/application.rs` refuses any `sutura-exec-*` name in `sutura-app`'s NORMAL
dependency tree, and `sutura-app` was `FederatedPlan::combine`'s only caller.

| Seam                                               | Verdict                                                                                                                                                                                                                                                                                                                                                               |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sutura-app` calling into `sutura-exec-datafusion` | Refused by name. A dev-dependency is exempt from that rule, and a combiner is not test support. The implementation does live in that crate - reached through the port from the composition root, never from `sutura-app`.                                                                                                                                             |
| a new **prefix-free** crate                        | PASSED, and that was the problem: the rule named `sutura-exec-*` rather than *an engine*, so this put DataFusion in `sutura-app`'s tree by picking a name the rule does not match. A gate end-run is not a design - so the rule now names the PROPERTY, as a `FORBIDDEN_EDGES` row forbidding `datafusion` in `sutura-app`'s normal tree. This seam is refused today. |
| a second **driven port**                           | The established shape, the one ADR 0007 designed, and what is BUILT: `sutura_domain::plan::FederationCombiner` with `sutura-exec-datafusion::DataFusionCombiner` as its first implementor.                                                                                                                                                                            |

**The byte budget survives, and this is the mechanism.** 0007 decided the working-set bound is
applied *as rows are converted*, because 0009's Decision 3 put the engine's memory pool on what its
own operators reserve and a finished `RowSet` is invisible to it. Under the DataFusion combiner the
join build side, the aggregate state and the sort - which is where the hand-written combine's
`ByteBudget` actually spent - ARE operator reservations: `crate::pool::environment` sizes a
`GreedyMemoryPool` to this call's own ceiling, with temporary files disabled so nothing spills, and
`pool::exhausted` recognises the refusal through the wrapping the engine adds. That is why the
combiner holds no session and builds one per combine - a pool is a property of a `RuntimeEnv` and the
ceiling is a per-question argument - and why `CombineError::Exhausted` carries the number that fired:
nothing else in the process knows which call it was.

**Stated as a limit rather than a claim, and the reach MOVED rather than widened.** That pool does not
count the in-memory tables the legs are registered as (they are batches the caller already holds, and
registering them copies no buffer), and it does not count `collect()` materialising the answer. The
hand-written combine counted the answer's own cells and none of the operators. So neither bound is a
superset of the other, and what still bounds the ANSWER's size is `plan::MAX_ROWS` and the response
bound, both applied by `sutura_app::federated` over the combined result. If the residual gap is not
acceptable the answer is a counted conversion beside the pool, not a wider claim.

**Two refusals are gone and two got stronger, and each is a consequence of the port rather than a
trade.** A leaf column mixing integer and real cells is not representable - an Arrow column has one
type. A leaf total past `i64::MAX` is answered EXACTLY rather than refused, through a 256-bit
accumulator, because `DataFusion`'s own sum accumulator adds with `add_wrapping` (measured in the
pinned 55.1.0 source) so the accumulator's width is the bound and a `checked_add`-and-refuse is not
available. A floating-point link key and two legs whose link types can never match are decided from
the legs' SCHEMAS now, so two EMPTY legs are judged too - the hand-written combine read the first
non-null cell it found and answered such a pair as *no rows*.

## Step 4, not built and blocked ahead of its caller: `datafusion-federation`

The instruction is to depend on the published crate directly - **no vendor**, superseding the
vendoring approach taken in a concurrent pull request. That is the mechanism requirement (c) is
about: federation pushdown, and later a multi-node move, arrive by adopting it rather than by
redeveloping it.

**It cannot land in this change, and the reason is a gate rather than a preference.**
`cargo xtask unused-deps` is a `Kind::Hygiene` task, so it runs in `just validate`'s hygiene leg, and
it fails a declared dependency no crate references. `datafusion-federation`'s caller is a
`FederationProvider`/`SQLExecutor` implementation with a per-subject `compute_context`, which is step
5's work; declaring the dependency ahead of it is a red leg, not groundwork.

**What is already measured about adopting it, so the next step does not re-derive it.**
`datafusion-federation 0.5.6`'s manifest declares `[dependencies.datafusion] version = "55"` with no
`default-features = false`, and Cargo unifies features additively, so adopting it turns datafusion's
`sql` feature back on no matter which of its APIs is called - a second SQL parser and an unparser in
the closure, beside `polyglot-sql`. `Cargo.toml`'s comment calls `sql` being off the strongest
sentence in that file, and this is what would end it. **So the ecosystem move comes first, and it is
already made:** the one-line upstream change setting `default-features = false` on that dependency is
proposed and OPEN against the crate's own repository, rather than patched or vendored here. That is
AGENTS.md's *engage the ecosystem instead of monkeypatching*, and step 1 of
`.agents/skills/sutura/dependencies`' escalation order - long before a `[patch]`, and longer still
before a vendor. Adopting the crate ahead of that change costs the `sql` feature and nineteen
lockfile entries; adopting it after costs three.

Its `compression` half is no longer part of that argument: this record turns the feature on
deliberately, so unification can no longer bring it in as a surprise.

## Step 5, built as the seam a provider plugs into: a per-subject compute context, wired

`identity::ComputeContext` is taken verbatim from `feat/datafusion-combiner-0007`, where it was held
back as orphaned dead code, and `DataFusionCombiner::for_subject` is its caller.

**What that is worth today, stated before the property it carries.** This build depends on no
federation provider - step 4 is blocked ahead of an upstream manifest change, and
`cargo xtask unused-deps` would fail a declared dependency no crate names - so nothing in this
process compares two contexts. What the wiring buys is that the seam exists where a provider plugs
in, and that the value reaching it is a digest: `ComputeContext::of` takes a `Subject` and there is
no other constructor, so a provider written against this cannot publish a raw one. The combiner's own
`Debug` prints whether a context is held and never its value.

**The property it holds, recorded here so step 4 does not have to rediscover it.**
`datafusion-federation`'s provider equality is `name() == name() && compute_context() ==
compute_context()`, so two providers for one source on behalf of two different subjects that compare
equal are fused by the optimizer into one federated node executed through one of them - one caller's
scan on the other caller's credential, with no bug on either caller's own path. The context must
therefore carry an **opaque per-subject digest**: unequal per subject so the fusion cannot happen,
and disclosing nothing, because the value is interpolated into plan text and a raw subject there is a
person's identifier in `EXPLAIN` output.

## Two things this record was asked to decide and finds already decided

**A value from a question still never reaches a statement as text, and nothing here trades that
away.** 0007:109 is the invariant and it holds on both paths today: a SQL adapter's leg is a
`GeneratedQuery` with statement and parameters in separate fields, and the engine does not render a
statement at all - `sutura-exec-datafusion`'s `translate::literal` turns a `ParamValue` into a
`datafusion::logical_expr::Expr` literal, so a value from a question is an expression node and
there is no text for it to be inside. **Binding EARLIER than the handoff is therefore already the
shape**, and it is the strongest available: DataFusion's `Expr::Placeholder` round-trips through the
unparser, which the route this branch takes never reaches. What would reopen the question is step 4

- `SQLExecutor`'s executing method takes one `&str` and no parameter list, so a pushed-down statement
  has nowhere but the text to carry a value, which ADR 0006 measured and this record inherits as the
  cost of that step rather than of this one.

**Oracle goes through `polyglot-sql`, and `transpile` is not the way to widen that.** Every Oracle
statement renders through `sutura_sql::generate` at `Dialect::Oracle` - `sutura-exec-oracle` compiles
and translates nothing - and `dialect-oracle` is already enabled. The `transpile` feature is
deliberately ABSENT and `Cargo.toml` carries the measurement: upstream's own Oracle `transform_expr`
renames `DATE_TRUNC` to `TRUNC` and leaves the arguments untouched, so it fixes neither the argument
order nor the bare-keyword-to-quoted-format difference - it would still be wrong, under a feature the
manifest says is not compiled. `dialect::DateTruncShape` builds the correct call directly instead.
**So "enable `transpile` if that is the clean route" has an answer and the answer is no**, and the
question only becomes live at step 4, where a statement DataFusion generated would have to be
translated to Oracle: that is a transpile by definition and it is the reason step 4 needs its own
measurement rather than this feature flag.

## What this does not buy

**Requirement (d) is unchanged by every step above, and neither the Arrow port nor the combiner
port advances it.** `sutura-exec-datafusion`'s `IMPERSONATION` is `NoPlaceForASubject` - one process,
one operating-system identity - so a leg it executes, including a combine, runs as the deployment and
not as the asker. What "always impersonation-capable" constrains is the shape: the combiner may not
become a place where two subjects' rows meet under one credential, which is why step 5's context is a
per-subject digest rather than a convenience. **And the digest is not yet a control**, because
nothing compares two of them in this build - step 4 is what would. `docs/where-identity-is-proven.md`
remains the record of which venue may be cited for which identity claim, and no step here changes a
row in it.

**Compressed sources are not a performance decision.** Nothing here measures read throughput for a
compressed file against a plain one, and the codecs differ by more than a constant. What is decided
is that a deployment may point a source at one.

## Consequences

- `deny.toml` carries one more licence, and `unused-allowed-license = "deny"` keeps it honest: the
  day nothing pulls `libbz2-rs-sys`, that entry fails the gate rather than lingering.
- Two dev-dependency compression writers exist so a test can produce what the engine reads. Neither
  is in a shipped artefact and nothing this workspace ships compresses anything.
- `sutura-cli`'s two file-source searches no longer spell their own format preference, so a format
  added to the engine reaches both without a change there - and a format added to `candidates`
  without an `attach_file` arm is a refusal at attach rather than a mis-read file.
- ADR 0006's federation probe paragraph said the bzip2 licence was one *the allowlist does not
  carry*, with `cargo deny` not run over the probe. That is corrected in place: the licence is
  carried now, deliberately, and `cargo deny check licenses` was run.

## Amendment, 2026-09-22: the residual gap is closed by the counted conversion this record named

Step 3's limit paragraph ends *if the residual gap is not acceptable the answer is a counted
conversion beside the pool, not a wider claim*. Round 7 of `telekom/sutura#929`'s review found it was
not acceptable and measured why: the engine called `DataFrame::collect`, retained every batch, passed
`usize::MAX` as its row ceiling, and then `ResultBatches::to_rows` built a second full copy. So the
counted conversion is built, and this record's own prescription is what it is.

**Where the count lives.** In `sutura_domain::warehouse::Accumulating::push` rather than beside the
pool - the guard that already applied the row ceiling and the schema check, which means every
`ResultBatches` in the workspace goes through it and no adapter has an unbudgeted door.
`RecordBatch::get_array_memory_size()` is charged twice, once for holding the batch and once for the
owned `Vec<Vec<Value>>` `to_rows` will build, plus that copy's own per-row and per-cell structure.
`collect()` is gone from both call sites in `sutura-exec-datafusion`; `execute_stream` plus
`StreamExt::next` is what makes the refusal land before the next batch is asked for.

**What the sentences in Step 3 and at line 173 now mean.** *It does not count `collect()`
materialising the answer* is spent: nothing calls `collect()`. *The engine passes `usize::MAX`* still
holds and is still deliberate - `docs/adr/0009` retired the row cap because a row count is not a
memory bound - but it is no longer the whole of the engine's own bound, because the byte budget beside
it is. **Neither bound is a superset of the other remains true**, and for the same reason: the pool
sees operators and not results, the budget sees the result and not operators. What is still not
counted is the `MemTable`s the legs are registered as, which are the caller's own already-budgeted
batches.
