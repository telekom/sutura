---
title: DataFusion federates the SQL tool and the metrics
description: DataFusion plans a federated question over table providers built from the metadata catalog's schema and pinned to the bundle the question resolved against. The metric path keeps every question value a bound parameter, and the SQL tool gets cross-source SQL under the raw tool's guards per leg. Every per-leg guard stays in the application and the adapter calls it through a port (telekom/sutura#828).
---

# DataFusion federates the SQL tool and the metrics

Status: **proposed** (issue #828). It replaces #828's M1 with the staged plan in decision 5. The
owner decided its three open questions on 2026-10-10. Each one is recorded where it applies, as
*Owner decision 2026-10-10*. The identity design and four more owner decisions of 2026-10-12 are in
*Amendment, 2026-10-12* at the end. Each one is also marked where it applies.

It amends three records where it names them.
[ADR 0007](0007-federating-across-different-data-systems.md): how a leg reaches its source, and its
sentence that DataFusion's unparser *stays declined for the statement that reaches a data system*.
Decision 2 amends that sentence for the SQL tool, whatever Q3 decides.
[ADR 0013](0013-a-raw-sql-tool-off-by-default.md): a federated SQL tool, and `run_sql` over an
acknowledged `shared-service-user` source in `multi-user` mode.
[ADR 0039](0039-arrow-and-datafusion-override-the-hand-written-combiner.md) step 4: the caller of
`datafusion-federation`. The amendments are proposed with this record. They are not yet written
into those files.

## The facts this record starts from

Each fact was read on `origin/main` at `6f4eb2460`, or in the cached crate source named in its row.

| Fact                                                                                                                             | Read on the base                                                                                                                                                                                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `FederationCombiner::combine` (`crates/sutura-domain/src/plan/federated/combiner.rs:242`) receives leg results.                  | **True.** It takes `Legs<'_>`, the batches the legs already returned.                                                                                                                                                                                                                                       |
| `run_leg` (`crates/sutura-app/src/federated/leg.rs`) runs each leg at its source as a compiled, parameter-bound statement.       | **True.** It calls `Warehouse::execute(Executable::Leg(..))`. The Postgres and BigQuery ADBC adapters bind the values with ADBC `Statement::bind`.                                                                                                                                                          |
| `datafusion-federation` fuses subtrees of one provider only.                                                                     | **True in 0.5.7.** Its scan walk gives `ScanResult::Distinct` for one provider and `Ambiguous` for more. Two providers are equal when `name()` and `compute_context()` are equal. ADR 0039's third amendment did not re-read this. This record did.                                                         |
| A provider-driven leg moves leg execution into the combiner, and that moves the ledger, credential, deadline and refusal guards. | **True.** ADR 0039's phrase *not a signature tweak* is about the Arrow port change in its step 2. It is not about this move.                                                                                                                                                                                |
| The crate's SQL path unparses a plan to SQL text with literals inlined.                                                          | **True for its `sql` module.** `SQLExecutor::execute(&self, query: &str, schema, filters)` takes no parameter list. One refinement: the `datafusion-sql 55.1.0` unparser writes `Expr::Placeholder` as its id, unchanged. So the unparser can keep a placeholder. The executor has no place for the values. |
| Only the raw SQL tool runs caller text today.                                                                                    | **True.** And `sutura_app::raw::run_sql` runs on one source only. It refuses with `RunSqlError::NoAcceptingSource` when more than one data system is open.                                                                                                                                                  |

Five more facts, measured for this record:

- `datafusion-federation 0.5.7` declares `sql = ["datafusion/sql"]` and no default feature. The
  provider, the optimizer rule, `FederatedPlanNode` and `FederationPlanner` build without datafusion's
  `sql` feature. #828's M1 spike measured the same.
- `FederationPlanner::plan_federation(&self, node: &FederatedPlanNode, ..)` gets the cut
  `LogicalPlan` subtree. The implementor decides how that subtree runs.
- The cut is `FederationOptimizerRule`, an `OptimizerRule`, and not an analyzer. DataFusion runs the
  optimizer again in `SessionState::create_physical_plan` (`datafusion 55.1.0`).
- The `datafusion-sql 55.1.0` unparser has these dialects: default, PostgreSQL, DuckDB, MySQL,
  SQLite, BigQuery, Snowflake and custom. It has no Oracle and no ClickHouse dialect.
- `sutura_domain::catalog::Column::data_type` is optional descriptive text and never a cast. So the
  catalog gives an Arrow schema only through a mapping from that text to an Arrow type (decision 1).

This record uses the posture names of the code: `shared-service-user` and
`impersonation-at-source`. The published pages call the second one *secure-impersonation*. A source
with the second posture declares how the caller reaches it: a token exchange, an ordered chain of
exchanges (hops), or, for ClickHouse with no `delegation`, `EXECUTE AS`. *Amendment, 2026-10-12*
names the declaration of each source.

## Decision 1: a table provider comes from the catalog's schema, pinned to the bundle

**One provider for each source and each subject.** The provider's `name()` is the source alias.
Its `compute_context()` is `ComputeContext::of(source, subject)`, the digest ADR 0039 step 5 built
for this use. It covers the source and the subject. So the optimizer cannot fuse two subjects' scans into one node. Each request builds its own
session. So two subjects never share a session either.

**Two kinds of metadata build a provider, and each has one job.**

| Part of the provider           | Comes from                                                                                                                             | Why                                                                                                           |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Which tables and columns exist | The pinned bundle: `Model { source, table, columns, audience }` in the `PinnedDefinitions` the question resolved against               | The bundle is the one list of what a caller may address. A table that is not in the bundle is not a provider. |
| The Arrow type of each column  | The metadata catalog, for example the DataHub catalog adapter: the table's column names and types, read under the catalog's read token | The planner needs no credential for the data source to learn a schema.                                        |

- A column that the bundle names and the catalog does not return refuses the load, in the same
  family as the anchor check. A column that the catalog returns and the bundle does not name is not
  registered.
- **Fully qualified names.** Every federated table is named fully qualified. For BigQuery that is
  `project.dataset.table`, so a table in another project works.
- **Pinning.** The catalog's load and each refresh give the schemas. They are read under the
  catalog's read token, because no caller exists at load or at a refresh (*Owner decision
  2026-10-12*, the schema source). No data source gives the planner a schema at load. A source needs
  no deployment credential for that: for BigQuery there is no load-time `tables.get`. The read
  carries names and types only, and no row. The set carries its own digest. It is held in one
  snapshot with the `PinnedDefinitions` of the same load. `DefinitionDigest` is a content hash, so
  an unchanged bundle over a changed schema keeps its `DefinitionDigest`. So the snapshot key is
  both digests, and `Provenance` names both. A question holds its snapshot from the start, so a
  question in flight keeps its set.
- **Limit.** A type change at the source that the catalog has not yet read is not seen at planning.
  It is seen at execution, where `Accumulating::push` refuses a batch whose schema does not agree.
- **Limit.** The catalog shows what it holds, and not what a subject may read. Under
  `impersonation-at-source` a subject can see less. The source still decides at execution, and a
  leg the subject may not read is `SourceRefused`. The schema can name a column that this subject
  may not read. That name is already in the bundle. The audience cut (`ScopedView::models`) is what
  limits the names a caller can address. Every data leg runs as the caller, through the exchange or
  the hop chain that its source declares, and a ClickHouse source with no `delegation` runs no
  federated leg. A leg over a source that the operator acknowledged as `shared-service-user` is the
  one exception (*Amendment, 2026-10-12*).
- **Limit.** The catalog's `data_type` is optional descriptive text (see the facts above). The
  planner needs a mapping from that text to an Arrow type. A table with a column that has no mapped
  type cannot be a provider. Its source stays on today's path.

## Decision 2: the SQL tool federates across sources, under the raw tool's guards per leg

**What changes.** DataFusion's SQL frontend parses the caller's statement. It plans the statement
over the providers of decision 1. `datafusion-federation` cuts the plan into its largest
one-provider subtrees, and its `sql` module writes each subtree as one statement. One source can
give more than one subtree: in `a1 JOIN b JOIN a2`, with `a1` and `a2` on source A, A gives two.
Each subtree runs at its own source. DataFusion joins the results locally.

**One tool, with a federation switch (Owner decision 2026-10-10, Q1).** `run_sql` stays the one SQL
tool. `tools.run_sql.federation_allowed` decides whether one statement may name tables in more than
one source. **The default is `false`.**

- **Where the switch lives: per deployment, under `tools.run_sql`.** The tool's source list already
  names each source explicitly, so a per-source flag would be a second list that can disagree with
  the first.
- **`false`:** DataFusion parses and plans the statement. A plan whose scans name more than one
  source is refused with a typed refusal, a new `RawRefusalReason` variant, before any leg exists.
  A plan over one source runs as one statement at that source.
- **`true`:** the statement is federated as this decision describes.
- `true` in a build without the `sql` cargo feature is a startup refusal. Such a build keeps today's
  `run_sql`: one source, the text passed through unparsed.
- **Limit.** The typed refusal needs the parser. So it exists only in a build with the `sql`
  feature. A build without it allows one source, and that source's own error answers a table name
  it does not know, as `StatementFailed`. The two builds also read different tables. Without the
  feature, `run_sql` passes the text through, so a statement reads any table that the source's grant
  allows, as today. With it, only the models that the caller's `ScopedView::models` lists are
  tables. So one statement can answer in one build and be refused in the other.

**Who may run it.** A caller that holds `run_sql`'s capability scope, as today. A caller without the scope
does not see the tool advertised, as ADR 0013 decides for `run_sql`. The session registers only
the models that the caller's `ScopedView::models` lists. A model the caller may not see is not a
table in that session.

**Identity, per leg.** The rule of ADR 0013 applies to each leg and not to the deployment as a
whole:

| Deployment    | A leg over a `shared-service-user` source                                                               | A leg over an `impersonation-at-source` source                                                                                                                  |
| ------------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `single-user` | Allowed. The operator chose the credential. The written reason of the mode is the acknowledgement.      | Allowed. The source authorizes the subject. The leg runs as the caller, through the source's declared exchange. A source with no declared exchange runs no leg. |
| `multi-user`  | Allowed only when the operator acknowledges that source. Else **startup refusal**, one for each source. | Allowed. The source authorizes the subject. The leg runs as the caller, through the source's declared exchange. A source with no declared exchange runs no leg. |

The table holds for each federated plan on both paths. A leg over a `shared-service-user` source
does not run as the caller. This record adds no other way for such a leg into a plan. **Limit.**
sutura does not refuse a plan that mixes this leg with a leg that runs as the caller. The mix is the
operator's decision, and the answer discloses each leg's identity (*Owner decision 2026-10-12*, the
shared-account leg). In `multi-user` mode the acknowledgement is `acknowledged_because` on that
source (*Owner decision 2026-10-12*, `run_sql` over a shared source).

- The tool runs over the sources that `tools.run_sql` lists by alias. Nothing is inferred. A list
  with no source is a startup refusal. The list replaces `run_sql`'s *sole registered data system*.
  In a build without the `sql` feature, a list with more than one source is a startup refusal.
- **Limit.** Today no linked adapter both accepts a raw statement and carries a per-subject
  credential. Postgres and DuckDB accept raw statements. They have no place for a subject, so each
  runs under one declared account. BigQuery, Oracle, ClickHouse and Cube carry a per-subject
  credential (*Amendment, 2026-10-12*). None of them accepts raw statements. So in `multi-user` mode
  the tool runs only over an acknowledged `shared-service-user` source, until an adapter that runs
  as the caller accepts raw statements. BigQuery is the first candidate.

**The guards of ADR 0013 carry over per leg, because each leg goes through the raw port.** The
`SQLExecutor` that sutura implements gets one statement for one source. It parses that text as a
`RawStatement` and gives it to `Warehouse::execute_raw` through the leg runner of decision 4.

| Guard of `run_sql`                                        | In `run_sql` with `federation_allowed` set to `true`                                                                                                                                              |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Off by default                                            | `run_sql` stays off by default, and `federation_allowed` is `false` by default                                                                                                                    |
| `RunSqlEnabledInMultiUserMode`                            | A startup refusal of `run_sql` in `multi-user` mode while no source is an acknowledged `shared-service-user`                                                                                      |
| `RawStatement::parse`: at most 64 KiB, not empty, no NUL  | On the caller's statement, and again on each leg's statement                                                                                                                                      |
| Mint once, grant fits, posture agrees                     | One mint for the listed sources that the plan names, each by the exchange or hop chain that its source declares, and none for a ClickHouse source with no `delegation`; posture agreement per leg |
| Read-only transaction (Postgres), read-only open (DuckDB) | Per leg, unchanged, because the adapter's `execute_raw` applies them                                                                                                                              |
| `TooManyRows`, `ResultTooLarge`                           | Per leg, and on the joined answer                                                                                                                                                                 |
| `StatementFailed` carries no driver text                  | It also carries no planner text. DataFusion's planning errors name fields, so they go to the log only                                                                                             |
| `RawOutcome` has no `Provenance`                          | Unchanged. A federated SQL answer is never certified                                                                                                                                              |
| Deadline checked before the call and passed to the port   | One request `Deadline` for the whole statement, checked before each leg                                                                                                                           |

**Three guards are new.**

- DataFusion plans with `SQLOptions` that allow no DDL, no DML and no other statement. So `CREATE`,
  `INSERT`, `COPY` and `SET` are refused before a leg exists. **This is not the read-only boundary.**
  ADR 0013 still holds: sutura does not decide read-only by reading the text. The boundary per leg
  is the source's grant and the adapter's read-only transaction.
- The local join runs in a memory pool sized to the working-set ceiling, as the combine does today
  (`pool::environment`).
- **Cross-posture disclosure.** In `single-user` mode one statement can join a `shared-service-user`
  leg and an `impersonation-at-source` leg. `RawOutcome` has no `Provenance` and no `executed_as`
  today. `run_sql` with `federation_allowed` set to `true` adds one `executed_as` entry for each leg
  to its outcome, outside any `Provenance`. So the answer is disclosed per leg, as ADR 0040 does for
  a metric, and it still claims no certification.

**Limits.**

- The caller writes DataFusion's SQL dialect, not the source's. A function that the source has and
  DataFusion does not know is refused at planning. In a build with the `sql` feature this holds for
  one source too, so pass-through ends there. A build without the feature keeps pass-through over
  one source.
- The raw path does not charge the spend ledger today, and this tool does not either. A leg that
  prices nothing is *not counted*, per ADR 0030, never *free*. The ledger applies when an adapter
  can price a raw statement.
- The per-source statement is the unparser's text, with the caller's literals inlined. ADR 0007's
  invariant is about a value from a certified question, and this path has none. The caller already
  writes the whole statement. An unparser fault can change what a leg runs. It cannot give the
  caller more than the leg's identity may do at that source, and the identity table above bounds
  that. **This amends ADR 0007's sentence that DataFusion's unparser stays declined for the
  statement that reaches a data system**, for the SQL tool, whatever Q3 decides.
- `insert_into` on `datafusion-federation`'s table adaptor returns *not implemented* when no inner
  provider is given. sutura gives none. `SQLOptions` refuses DML before that point.

**Owner decision 2026-10-10 (Q2): DataFusion's SQL frontend is a cargo feature, off by default in
shipped builds.** Federation in `run_sql` needs datafusion's `sql` feature. That brings `sqlparser` and `datafusion-sql` into the
closure of each build that links the tool. ADR 0006, ADR 0007 and ADR 0013 call *not having a
parser* a property. That property ends for such a build.

An image that the owner names turns it on. So the default artefact keeps the property, and the
decision to give it up is visible in a diff. Rejected: on in every build.

**Limit.** No check holds the default build free of a parser yet. ADR 0013's evidence is that
neither crate is in `Cargo.lock`. `Cargo.lock` resolves every feature, so that evidence ends with
this feature too. PR 5b adds a gate over the default build's resolved tree that fails when `sqlparser` or
`datafusion-sql` is in it.

## Decision 3: DataFusion plans the certified metric question, and every question value stays a bound parameter

**What changes.** The question stays a domain `FederatedPlan`. The domain names no engine.
`sutura-exec-datafusion` builds one DataFusion `LogicalPlan` from it over the providers of
decision 1. `datafusion-federation`'s optimizer rule cuts each largest one-provider subtree into a
`FederatedPlanNode`. One source can give more than one node. The rest (the join, the re-aggregation, the `Above` division and the sort) runs
locally. That rest is today's combine plan. The change is that each leg is now a scan in the same
plan, and not a `MemTable` of a finished result.

**How ADR 0007's invariant holds.** A value from the question enters the `LogicalPlan` as an
`Expr::Placeholder`, never as an `Expr::Literal`. The values stay beside the plan as the domain's
`ParamValue` list. sutura implements the `FederationPlanner`. It does not use the crate's `sql`
module on this path, because that module's executor has no parameter list. For each cut node,
sutura's planner gives one leg and its values to the leg runner of decision 4. Under Q3 (a) the leg
is a domain `LegPlan`, and the adapter writes it with `sutura_sql::generate_leg`, as today (for
example `crates/sutura-exec-postgres/src/adbc.rs:242`). Under Q3 (b) the planner writes the
statement and gives a `GeneratedQuery`. In both, each value is one bind placeholder, and the adapter
binds the values, as the Postgres and BigQuery ADBC adapters do today. **So 0007's invariant is not
amended.** The invariants row *one bind placeholder per plan value, counted per dialect and
compared against the plan's parameter list* applies to each pushed statement.

- The caller writes no SQL on this path. `Query` has no SQL field, and `deny_unknown_fields`
  names the attempt. The per-source SQL is generated.
- A leg that `sutura-exec-datafusion` runs itself over files has no SQL, and `translate::literal`
  stays.
- **Limit.** The optimizer may move a placeholder or fold it. That a placeholder survives
  DataFusion's optimizer rules unchanged is not measured. The placeholder count per pushed statement
  is the check that sees a lost value.
- **Limit, for Q3 (b) only.** The unparser writes a placeholder id as given. The id is a string
  that sutura chooses, so a per-dialect spelling is possible. Which spelling each target accepts is
  not measured.

**Owner decision 2026-10-10 (Q3): `polyglot-sql` writes the pushed metric statement first, not
DataFusion's unparser.** The two options were:

- (a) **sutura's own generator.** The planner maps each cut subtree back to a domain `LegPlan`, and
  `sutura_sql::generate_leg` writes it through `polyglot-sql`, as today. No second parser enters the
  closure. Each row of the invariants table stays as it is. Oracle keeps `DateTruncShape`.
  **Cost:** the map accepts only the node shapes that sutura's own plan makes (scan, filter,
  projection, join on declared keys, the closed `Aggregate` set, sort, limit). It refuses any other
  cut by name, and the golden suite shows a refusal that an engine upgrade causes.
- (b) **DataFusion's unparser,** with placeholders. This needs the `sql` feature in every build,
  because the metric path is the core. It has no Oracle and no ClickHouse dialect. It also amends ADR 0007's sentence *the generator is never DataFusion* for the metric path, and it needs a new per-dialect placeholder
  count cell. A leg is then a statement and not a `LegPlan`, so `Warehouse::dry_run` needs a new
  `Executable` for it.

(a) is decided. It keeps every mechanism that holds today. (b) is not planned. It would need its own
record and a differential against (a).

## Decision 4: every per-leg guard stays in the application, and the adapter calls it through a port

**The rule.** An adapter never calls another adapter. So `sutura-exec-datafusion` does not call
`Warehouse` or `CredentialBroker`. The domain declares a second port beside `FederationCombiner`,
called *the leg runner* here. `sutura-app` implements it over today's `dry_run_leg` and `run_leg`.
The `FederationPlanner` calls the leg runner for each cut node. The port arrives with its first
implementor, in the PR that first calls it.

**Two calls, so that the ledger still comes first.** The legs exist only after DataFusion's
optimizer cuts the plan. So the combiner port splits into two calls:

1. `plan` returns the cut: one typed leg for each cut node (one or more for each source), and the
   local rest. It executes nothing.
2. `sutura-app` dry-runs each leg and charges the sum, as `preflight_and_charge` does today.
3. `execute` takes the cut from step 1 and the leg runner. The cut is a value that only `plan` makes
   and `execute` consumes.

**The type holds which value `execute` takes. It does not hold which legs run.** DataFusion runs
the optimizer again in `SessionState::create_physical_plan`, and `plan_federation` runs at physical
planning. So the charged cut gives out one token for each leg it charged. The leg runner accepts
only a leg with a token from the charged cut. It refuses any other leg, as this workspace's own
defect, before it reads a credential. 0.5.7 skips a node that is already cut, but that is the
crate's code and not a sutura mechanism. **Limit.** This was read in the crate source, not run.

| Guard                        | Today                                                                                       | After the move                                                                                                                                                             |
| ---------------------------- | ------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Spend ledger, all-or-nothing | `preflight_and_charge` dry-runs every leg, then charges, before any leg runs (ADR 0030)     | The same function, on the legs of the cut, between `plan` and `execute`. No leg runs before the charge. The leg runner refuses a leg that is not among the cut's own legs. |
| Credential per leg           | One mint for the plan's `SourceSet`; `presented_for`, `still_usable_at` and posture per leg | The same calls in the same functions. The `SourceSet` comes from the cut's sources.                                                                                        |
| Subjects never fuse          | No provider exists                                                                          | `compute_context()` is the digest of the source and the subject, and each request builds its own session.                                                                  |
| Deadline                     | One request `Deadline`, checked before each leg and passed to `execute`                     | The same. The leg runner checks it before each leg. The local rest must stop at the same deadline.                                                                         |
| Refusal mapping              | `run_leg` maps exhaustion, size, deadline and `SourceRefused` in that order; else `503`     | The same function, the same order. The adapter carries a leg's refusal out unchanged and never maps it again. A leg's refusal wins over a combine failure.                 |
| Byte budget                  | The pool for operators; `Accumulating::push` for each leg's result                          | The same two.                                                                                                                                                              |
| Cross-posture disclosure     | One `executed_as` entry for each leg (ADR 0040)                                             | One entry for each leg of the cut.                                                                                                                                         |

**Limit.** The leg runner is synchronous today and DataFusion executes asynchronously. The planner
calls the runner on a blocking thread. That the deadline still cancels a leg there is not measured.

## Decision 5: the staging, and what #828 M2 to M5 become

Each PR carries its own cells. A cell runs against a real adapter in a venue that starts that
adapter. A new port arrives in one PR with today's behaviour, and the new behaviour follows in the
next PR, so that `just causality` can build the base.

| PR | Delivers                                                                                                                                                                                                                                                                                                                                                                                                              | Its cells show                                                                                                                                                                                                                                                                                                                           |
| -- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1  | This record                                                                                                                                                                                                                                                                                                                                                                                                           | Nothing to run                                                                                                                                                                                                                                                                                                                           |
| 2  | The catalog's table schemas as typed columns, and a fully qualified name for each federated table. It deletes `Warehouse::table_schemas` with its DuckDB and Postgres overrides and its wrappers, the call site in the load, the method's `clippy.toml` row and the Postgres conformance cell. The pinned set (`PinnedSchemas`, `SchemaDigest`) stays, pinned by both digests, and takes its columns from the catalog | A column that the bundle names and the catalog lacks refuses the load. A refresh swaps the set. A question in flight keeps its set.                                                                                                                                                                                                      |
| 3a | `datafusion-federation` 0.5.7 without `sql`, the two-call combiner port and the leg runner, with the cut fixed to today's legs and `compute_context()` = `ComputeContext::of(source, subject)`                                                                                                                                                                                                                        | Every federated golden gives the same answer as before. The ledger charges before any leg runs.                                                                                                                                                                                                                                          |
| 3b | The leg runner's refusal of a leg that is not among the cut's own legs                                                                                                                                                                                                                                                                                                                                                | Two subjects' providers are not equal. An uncharged leg is refused.                                                                                                                                                                                                                                                                      |
| 4  | DataFusion cuts the metric plan (decision 3), with `polyglot-sql` as the generator                                                                                                                                                                                                                                                                                                                                    | A differential against PR 3's answers on DuckDB and Postgres. One placeholder for each value in each pushed statement. A cut that the map does not accept is refused by name.                                                                                                                                                            |
| 5a | The new `RawRefusalReason` variant, which nothing produces yet, its enrolled variant count in `xtask check-refusal-coverage`, and a dated entry for it in `devco/raw-refusals-unprovoked-allow`                                                                                                                                                                                                                       | Nothing new to run                                                                                                                                                                                                                                                                                                                       |
| 5b | `federation_allowed` on `run_sql` (decision 2), behind the `sql` cargo feature, and a gate that fails when `sqlparser` or `datafusion-sql` is in the default build's resolved tree. It removes 5a's allow entry                                                                                                                                                                                                       | `false`: a statement over two sources is the typed refusal. `true`: the same statement over DuckDB and Postgres gives the answer that one DuckDB with both tables gives. Each startup refusal. DDL and DML refused at planning. The raw guards per leg. No planner or driver text reaches a caller. The default build has neither crate. |

**How the cells of PR 3 and PR 5 pass `just causality`.** A `devco/causality-no-base-exemptions`
entry does not cover a cell that cannot build on the base. The entry exempts a test that the base
run gave no result for. A test file that names a type the base does not have stops the base build,
and then the whole run is inconclusive (exit 3). So the rule above splits PR 3 and PR 5:

- **3a** keeps today's behaviour, so its two cells pass on the base. Each one declares
  `Claim-Cell:` with a killing mutation under `devco/claim-mutations/`.
- **3b**'s two cells name the port that 3a adds, so they build on 3a. *An uncharged leg is
  refused* is red there. 3a already builds each provider with `ComputeContext::of(source, subject)`,
  so *two subjects' providers are not equal* holds on 3a. It declares `Claim-Cell:` with the killing
  mutation *`compute_context()` returns `None`* under `devco/claim-mutations/`.
- **5b**'s two switch cells name the refusal and the answer by their wire names only. So they
  compile on the base, and they are red there, because the base has no `federation_allowed` key
  and runs `run_sql` over one source. A cell that names the new `RawRefusalReason` variant builds
  on 5a, and it is red there, because 5a produces the variant nowhere. 5b removes 5a's allow
  entry, because `xtask check-refusal-coverage` fails an entry whose variant now has evidence.

Three cells of 5b need a build without the `sql` feature: *`true` without the feature*, *more than
one source without the feature*, and *the default build has neither crate*. `just test` and
`just causality` build with every feature, so neither one runs them. Their venue is the
default-feature lane: `just check-default-features`, and `just gates`, which runs the same two
checks. `just validate` does not run them. PR 5b adds its resolved-tree gate to that lane.

What #828's milestones become:

- **M1** is replaced by PRs 2 to 4.
- **M2** (CI slice) is unchanged and does not depend on this record.
- **M3** (plan-time heuristic) reads PR 4's cut (the legs, one or more for each source, and the local rest) and not
  today's fixed two-leg split.
- **M4** (elastic multi-node) plugs in where the `FederationPlanner` runs the local rest. A worker
  must call the leg runner, so each guard of decision 4 still runs once per leg. A worker gets the
  caller's credential only through an RFC 8693 token exchange or the declared hop chain. The
  credential is scoped to that worker and is short-lived. The caller's raw token never goes to a
  worker, and no worker account is shared (*Owner decision 2026-10-12*, the worker credential).
- **M5** (per-subject result cache) adds the pinned schema set's digest to its key, beside the
  catalog version.

## Rejected

| Option                                                                                                                    | Why it is rejected                                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **A: the lazy-leg seam with no caller.** A provider whose scan runs today's compiled leg, added before anything calls it. | It pushes nothing down that today's legs do not. A seam that nothing calls is dead code, and `cargo xtask unused-deps` fails a dependency that no crate names.                                          |
| **Goldens over plain `MemTable`s.**                                                                                       | Both sides read the same batches in memory. So the differential tests the combine twice and the pushdown never. The goldens run against the ADBC adapters.                                              |
| The crate's `sql` module and its `SQLExecutor` on the metric path                                                         | `execute(&str, ..)` has no parameter list, so each value is inlined text. sutura can propose a parameter list upstream. Until a release has one, the metric path uses sutura's own `FederationPlanner`. |
| A typed escaping rule per dialect for inlined literals                                                                    | The adapters already bind values. An escaper is a second mechanism with its own differential to keep true, and it gives nothing that binding does not.                                                  |
| Amend ADR 0007's invariant                                                                                                | Not needed. Decision 3 keeps it.                                                                                                                                                                        |
| One provider for each source with no subject in its context                                                               | `compute_context() == None` on two providers makes them equal. The optimizer then fuses two subjects' scans into one node under one credential.                                                         |
| Two SQL tools, a federated one beside `run_sql` (this record's first recommendation)                                      | Owner decision 2026-10-10. One tool keeps one capability, one advertisement and one audit path. The switch makes federation visible in a diff.                                                          |

## Amendment, 2026-10-12: each federated leg runs as the caller

**The identity design.** Each federated leg runs as the caller. Its credential reaches the source
through the token exchange or the hop chain that the source declares. A ClickHouse source with no
`delegation` does not run a federated leg (see the ClickHouse decision below). There is no service
account for each caller, and sutura holds no list of the callers that may run. The source's own
grants decide what a caller reads. A caller with no grant is refused by the source.

**What each source declares.** The table names what the code does today.

| Source                                | How the caller's credential reaches it                                                                                                                                                                                   |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| BigQuery                              | The caller's verified token goes through the hops under `workload_identity.delegation`, if the source declares any. The declared workload identity pool federates it. The query runs as the caller's own pool principal. |
| Oracle                                | The caller's verified token goes through the hops under `delegation`. The exchanged token opens the session for each question. The database maps it to a user.                                                           |
| ClickHouse                            | With `delegation`, each caller signs in with the token that the hops return. Without it, the deployment's user sends `EXECUTE AS` for the user that the caller's verified subject names.                                 |
| Cube                                  | The caller's verified token goes through the hops under `delegation`. Each request sends the exchanged token.                                                                                                            |
| Postgres, DuckDB and the files engine | No place for a subject. Each leg runs under one declared account.                                                                                                                                                        |

ClickHouse and Cube cannot run a leg. A plan that names one is refused as
`federation_not_executable` before any credential is minted.

**Decision: ClickHouse with no `delegation`.** A ClickHouse source with no `delegation` does not run
a federated leg, also when ClickHouse can run legs. Without `delegation`, the deployment's own user
sends `EXECUTE AS`. That is the deployment's credential switching identity, and not the caller's
credential. A ClickHouse source that declares `delegation` runs as the caller, through the hops it
declares.

**Owner decision 2026-10-12: the shared-account leg.** A leg over a source that runs under one shared
account (Postgres, embedded DuckDB) is allowed in a federated plan only when the operator declares
that source `shared-service-user` and acknowledges it. The same rule holds for a single-source
query. **Limit.** Such a leg does not run as the caller. Cross-posture is the operator's decision,
and sutura adds no rule for the mix beyond the per-leg disclosure of ADR 0040.

**Owner decision 2026-10-12: `run_sql` over a shared source.** In `multi-user` mode, `run_sql` over
a `shared-service-user` source is allowed only when the operator acknowledges that source. Without
the acknowledgement, it is refused. The acknowledgement is the per-source setting
`acknowledged_because`, written on that source's entry. It is never an implicit default. This
changes the guard of ADR 0013, which refuses `run_sql` over a shared account in `multi-user` mode.
**Limit.** Every caller can then read what the shared account can read.

**Owner decision 2026-10-12: the schema source.** The federation planner takes the schema of each
federated table (names and types) from the metadata catalog, for example the DataHub catalog
adapter. It reads the schema under the catalog's read token. No data source gives the planner a
schema at load, so a source needs no deployment credential for that. For BigQuery there is no
load-time `tables.get`. Every federated table is named fully qualified: for BigQuery,
`project.dataset.table`. So a table in another project works. At query time, every data leg runs
as the caller. **Limit.** The catalog shows what it holds, and not what the caller may read.

**Owner decision 2026-10-12: the worker credential.** A worker gets the caller's credential only
through an RFC 8693 token exchange or the declared hop chain. The credential is scoped to that
worker and is short-lived. The caller's raw token is never forwarded to a worker. No worker account
is shared.

**Rejected.** A service account for each caller, and a list in sutura of the callers that may run.
Each one puts a second identity, or a second list, between the caller and the source. The source's
grants already decide the same question (ADR 0032, fourth amendment). Also rejected: a load-time
read of each data source's schema under the deployment identity. It needs a deployment credential
at every source. The catalog read needs none at a source. PR 2 deletes the code of that read
(decision 5).

## Second amendment, 2026-10-13: ClickHouse runs a leg

ClickHouse now declares `EXECUTES_LEGS`, so the sentence *ClickHouse and Cube cannot run a leg* in
the amendment above holds for Cube only. The ClickHouse decision above holds in the adapter: a
leg presented as a principal with no token would run through `EXECUTE AS`, and the adapter refuses
it as `ClickHouseError::LegUnderExecuteAs` before it sends anything. A leg runs as the shared
`user` of a `shared-service-user` source, or signed in with the caller's `delegation` token.
**Limit.** The adapter refuses that leg after the mint, so the answer fails with the adapter's
error and not with the `federation_not_executable` refusal.
