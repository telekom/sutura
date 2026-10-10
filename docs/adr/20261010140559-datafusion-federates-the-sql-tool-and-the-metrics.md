---
title: DataFusion federates the SQL tool and the metrics
description: DataFusion plans a federated question over table providers built from each data system's own schema and pinned to the bundle the question resolved against. The metric path keeps every question value a bound parameter, and the SQL tool gets cross-source SQL under the raw tool's guards per leg. Every per-leg guard stays in the application and the adapter calls it through a port. Nothing here is built (telekom/sutura#828).
---

# DataFusion federates the SQL tool and the metrics

Status: **proposed** (issue #828). **Nothing in this record is built.** The owner reads it before
any build starts. It replaces #828's M1 with the staged plan in decision 5. Three questions need
the owner, and each one is marked *Open question for the owner*, with options and a
recommendation.

It amends three records where it names them: [ADR 0007](0007-federating-across-different-data-systems.md)
(how a leg reaches its source), [ADR 0013](0013-a-raw-sql-tool-off-by-default.md) (a second SQL
tool), and [ADR 0039](0039-arrow-and-datafusion-override-the-hand-written-combiner.md) step 4 (the
caller of `datafusion-federation`). The amendments are proposed with this record. They are not yet
written into those files.

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

Four more facts, measured for this record:

- `datafusion-federation 0.5.7` declares `sql = ["datafusion/sql"]` and no default feature. The
  provider, the analyzer, `FederatedPlanNode` and `FederationPlanner` build without datafusion's
  `sql` feature. #828's M1 spike measured the same.
- `FederationPlanner::plan_federation(&self, node: &FederatedPlanNode, ..)` gets the cut
  `LogicalPlan` subtree. The implementor decides how that subtree runs.
- The `datafusion-sql 55.1.0` unparser has these dialects: default, PostgreSQL, DuckDB, MySQL,
  SQLite, BigQuery, Snowflake and custom. It has no Oracle and no ClickHouse dialect.
- `sutura_domain::catalog::Column::data_type` is descriptive text and never a cast. So the catalog
  alone cannot give an Arrow schema.

This record uses the posture names of the code: `shared-service-user` and
`impersonation-at-source`. The published pages call the second one *secure-impersonation*.

## Decision 1: a table provider comes from the data system's own schema, pinned to the bundle

**One provider for each source and each subject.** The provider's `name()` is the source alias.
Its `compute_context()` is `ComputeContext::of(subject)`, the digest ADR 0039 step 5 built for this
use. So the optimizer cannot fuse two subjects' scans into one node. Each request builds its own
session. So two subjects never share a session either.

**Two kinds of metadata build a provider, and each has one job.**

| Part of the provider           | Comes from                                                                                                                                             | Why                                                                                                           |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| Which tables and columns exist | The pinned bundle: `Model { source, table, columns, audience }` in the `PinnedDefinitions` the question resolved against                               | The bundle is the one list of what a caller may address. A table that is not in the bundle is not a provider. |
| The Arrow type of each column  | The data system, through its adapter: a new `Warehouse` method that returns the table's Arrow schema (ADBC `get_table_schema` where the driver has it) | The catalog's `data_type` is text for a person. The data system knows what it returns.                        |

- A column that the bundle names and the data system does not return refuses the load, in the
  same family as the anchor check. A column that the data system returns and the bundle does not
  name is not registered.
- **Pinning.** The adapter reads the schemas when the catalog loads and at each refresh. It reads
  them under the deployment's configured identity, for `verify_anchor`'s reason: no caller exists
  at load. The set is held beside the `PinnedDefinitions` of the same load, keyed by its
  `DefinitionDigest`. A question plans against the set of the digest its `Provenance` names. A
  refresh makes a new set. A question that is in flight keeps the set it started with.
- **Limit.** A type change at the source between two refreshes is not seen at planning. It is seen
  at execution, where `Accumulating::push` refuses a batch whose schema does not agree.
- **Limit.** The load reads what the deployment identity can see. Under `impersonation-at-source` a
  subject can see less. The source still decides at execution, and a leg the subject may not read is
  `SourceRefused`. The schema can name a column that this subject may not read. That name is already
  in the bundle. The audience cut (`ScopedView::models`) is what limits the names a caller can
  address.
- **Limit.** Which ADBC drivers return a table schema is not measured. An adapter that cannot
  return one cannot be a provider. Its source stays on today's path.

## Decision 2: the SQL tool federates across sources, under the raw tool's guards per leg

**What changes.** DataFusion's SQL frontend parses the caller's statement. It plans the statement
over the providers of decision 1. `datafusion-federation` cuts the plan into one subtree for each
source, and its `sql` module writes each subtree as one statement. Each source runs its own
statement. DataFusion joins the results locally.

**Who may run it.** A caller that holds the tool's capability scope. A caller without the scope
does not see the tool advertised, as ADR 0013 decides for `run_sql`. The session registers only
the models that the caller's `ScopedView::models` lists. A model the caller may not see is not a
table in that session.

**Identity, per leg.** The rule of ADR 0013 applies to each leg and not to the deployment as a
whole:

| Deployment    | A leg over a `shared-service-user` source   | A leg over an `impersonation-at-source` source |
| ------------- | ------------------------------------------- | ---------------------------------------------- |
| `single-user` | Allowed. The operator chose the credential. | Allowed. The source authorizes the subject.    |
| `multi-user`  | **Startup refusal**, one for each source.   | Allowed. The source authorizes the subject.    |

- The tool runs over the sources that `tools` lists for it by alias. Nothing is inferred. A list
  with no source is a startup refusal. This replaces *the sole registered data system*.
- **Limit.** Today no linked adapter both accepts a raw statement and carries a per-subject
  credential (the `RunSqlEnabledInMultiUserMode` doc says so). Postgres and DuckDB accept raw
  statements and run as the deployment. BigQuery runs as the subject and does not accept raw
  statements. So in `multi-user` mode the tool has no source until an `impersonation-at-source`
  adapter accepts raw statements. BigQuery is the first candidate.

**The guards of ADR 0013 carry over per leg, because each leg goes through the raw port.** The
`SQLExecutor` that sutura implements gets one statement for one source. It parses that text as a
`RawStatement` and gives it to `Warehouse::execute_raw` through the leg runner of decision 4.

| Guard of `run_sql`                                        | In the federated tool                                                                                 |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Off by default                                            | Its own switch, off by default                                                                        |
| `RunSqlEnabledInMultiUserMode`                            | One startup refusal for each listed `shared-service-user` source in `multi-user` mode                 |
| `RawStatement::parse`: at most 64 KiB, not empty, no NUL  | On the caller's statement, and again on each leg's statement                                          |
| Mint once, grant fits, posture agrees                     | One mint for the listed sources that the plan names; posture agreement per leg                        |
| Read-only transaction (Postgres), read-only open (DuckDB) | Per leg, unchanged, because the adapter's `execute_raw` applies them                                  |
| `TooManyRows`, `ResultTooLarge`                           | Per leg, and on the joined answer                                                                     |
| `StatementFailed` carries no driver text                  | It also carries no planner text. DataFusion's planning errors name fields, so they go to the log only |
| `RawOutcome` has no `Provenance`                          | Unchanged. A federated SQL answer is never certified                                                  |
| Deadline checked before the call and passed to the port   | One request `Deadline` for the whole statement, checked before each leg                               |

**Two guards are new.**

- DataFusion plans with `SQLOptions` that allow no DDL, no DML and no other statement. So `CREATE`,
  `INSERT`, `COPY` and `SET` are refused before a leg exists. **This is not the read-only boundary.**
  ADR 0013 still holds: sutura does not decide read-only by reading the text. The boundary per leg
  is the source's grant and the adapter's read-only transaction.
- The local join runs in a memory pool sized to the working-set ceiling, as the combine does today
  (`pool::environment`).

**Limits.**

- The caller writes DataFusion's SQL dialect, not the source's. A function that the source has and
  DataFusion does not know is refused at planning. `run_sql` passes such a function through. This is
  the reason for the first open question.
- The raw path does not charge the spend ledger today, and this tool does not either. A leg that
  prices nothing is *not counted*, per ADR 0030, never *free*. The ledger applies when an adapter
  can price a raw statement.
- The per-source statement is the unparser's text, with the caller's literals inlined. ADR 0007's
  invariant is about a value from a certified question, and this path has none. The caller already
  writes the whole statement. An unparser fault can change what a leg runs. It cannot give the
  caller more than the leg's identity may do at that source, and the identity table above bounds
  that.
- `insert_into` on `datafusion-federation`'s table adaptor returns *not implemented* when no inner
  provider is given. sutura gives none. `SQLOptions` refuses DML before that point.

**Open question for the owner (Q1): one SQL tool or two?**

- (a) A second tool, with its own capability and its own switch, beside today's `run_sql`.
  `run_sql` stays as it is: one source, the source's own dialect, the text passed through unparsed.
- (b) One tool. DataFusion parses every statement, also for one source. Pass-through ends.

**Recommendation: (a).** The two tools differ in who parses, in the dialect the caller writes and in
what each can refuse. One switch for both lets a deployment turn on a parser that it did not
choose.

**Open question for the owner (Q2): does the shipped artefact carry DataFusion's SQL frontend?**
The tool needs datafusion's `sql` feature. That brings `sqlparser` and `datafusion-sql` into the
closure of each build that links the tool. ADR 0006, ADR 0007 and ADR 0013 call *not having a
parser* a property. That property ends for such a build.

- (a) A cargo feature, off in the default build. An image that the owner names turns it on.
- (b) On in every build.

**Recommendation: (a).** The default artefact keeps the property, and the decision to give it up
is visible in a diff.

## Decision 3: DataFusion plans the certified metric question, and every question value stays a bound parameter

**What changes.** The question stays a domain `FederatedPlan`. The domain names no engine.
`sutura-exec-datafusion` builds one DataFusion `LogicalPlan` from it over the providers of
decision 1. `datafusion-federation`'s analyzer cuts each largest one-provider subtree into a
`FederatedPlanNode`. The rest (the join, the re-aggregation, the `Above` division and the sort) runs
locally. That rest is today's combine plan. The change is that each leg is now a scan in the same
plan, and not a `MemTable` of a finished result.

**How ADR 0007's invariant holds.** A value from the question enters the `LogicalPlan` as an
`Expr::Placeholder`, never as an `Expr::Literal`. The values stay beside the plan as the domain's
`ParamValue` list. sutura implements the `FederationPlanner`. It does not use the crate's `sql`
module on this path, because that module's executor has no parameter list. For each cut node,
sutura's planner writes the statement for that source with one bind placeholder for each value. It
gives the statement and its values as a `GeneratedQuery` to the leg runner of decision 4. The
adapter binds the values, as the Postgres and BigQuery ADBC adapters do today. **So 0007 is not
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

**Open question for the owner (Q3): which generator writes the text around the placeholders?**

- (a) **sutura's own generator.** The planner maps each cut subtree back to a domain `LegPlan`, and
  `sutura_sql::generate` writes it through `polyglot-sql`, as today. No second parser enters the
  closure. Each row of the invariants table stays as it is. Oracle keeps `DateTruncShape`.
  **Cost:** the map accepts only the node shapes that sutura's own plan makes (scan, filter,
  projection, join on declared keys, the closed `Aggregate` set, sort, limit). It refuses any other
  cut by name, and the golden suite shows a refusal that an engine upgrade causes.
- (b) **DataFusion's unparser,** with placeholders. This needs the `sql` feature in every build,
  because the metric path is the core. It has no Oracle and no ClickHouse dialect. It amends ADR
  0007's sentence *the generator is never DataFusion*, and it needs a new per-dialect placeholder
  count cell. A leg is then a statement and not a `LegPlan`, so `Warehouse::dry_run` needs a new
  `Executable` for it.

**Recommendation: (a) first.** It keeps every mechanism that holds today. (b) can follow as a
measured change with a differential against (a), after Q2 has an answer.

## Decision 4: every per-leg guard stays in the application, and the adapter calls it through a port

**The rule.** An adapter never calls another adapter. So `sutura-exec-datafusion` does not call
`Warehouse` or `CredentialBroker`. The domain declares a second port beside `FederationCombiner`,
called *the leg runner* here. `sutura-app` implements it over today's `dry_run_leg` and `run_leg`.
The `FederationPlanner` calls the leg runner for each cut node. The port arrives with its first
implementor, in the PR that first calls it.

**Two calls, so that the ledger still comes first.** The legs exist only after DataFusion's
optimizer cuts the plan. So the combiner port splits into two calls:

1. `plan` returns the cut: one typed leg for each source, and the local rest. It executes nothing.
2. `sutura-app` dry-runs each leg and charges the sum, as `preflight_and_charge` does today.
3. `execute` takes the cut from step 1 and the leg runner. It cannot plan again, because the cut is
   a value that only `plan` makes and `execute` consumes.

| Guard                        | Today                                                                                       | After the move                                                                                                                                             |
| ---------------------------- | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Spend ledger, all-or-nothing | `preflight_and_charge` dry-runs every leg, then charges, before any leg runs (ADR 0030)     | The same function, on the legs of the cut, between `plan` and `execute`. No leg runs before the charge.                                                    |
| Credential per leg           | One mint for the plan's `SourceSet`; `presented_for`, `still_usable_at` and posture per leg | The same calls in the same functions. The `SourceSet` comes from the cut's sources.                                                                        |
| Subjects never fuse          | No provider exists                                                                          | `compute_context()` is the subject's digest, and each request builds its own session.                                                                      |
| Deadline                     | One request `Deadline`, checked before each leg and passed to `execute`                     | The same. The leg runner checks it before each leg. The local rest must stop at the same deadline.                                                         |
| Refusal mapping              | `run_leg` maps exhaustion, size, deadline and `SourceRefused` in that order; else `503`     | The same function, the same order. The adapter carries a leg's refusal out unchanged and never maps it again. A leg's refusal wins over a combine failure. |
| Byte budget                  | The pool for operators; `Accumulating::push` for each leg's result                          | The same two.                                                                                                                                              |
| Cross-posture disclosure     | One `executed_as` entry for each leg (ADR 0040)                                             | One entry for each leg of the cut.                                                                                                                         |

**Limit.** The leg runner is synchronous today and DataFusion executes asynchronously. The planner
calls the runner on a blocking thread. That the deadline still cancels a leg there is not measured.

## Decision 5: the staging, and what #828 M2 to M5 become

Each PR carries its own cells. A cell runs against a real adapter in a venue that starts that
adapter. A new port arrives in one PR with today's behaviour, and the new behaviour follows in the
next PR, so that `just causality` can build the base.

| PR | Delivers                                                                                                                       | Its cells show                                                                                                                                                                |
| -- | ------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1  | This record                                                                                                                    | Nothing to run                                                                                                                                                                |
| 2  | The schema method on `Warehouse` (DuckDB and Postgres first, BigQuery next) and the set pinned by digest                       | A column that the bundle names and the source lacks refuses the load. A refresh swaps the set. A question in flight keeps its set.                                            |
| 3  | `datafusion-federation` 0.5.7 without `sql`, the two-call combiner port and the leg runner, with the cut fixed to today's legs | Every federated golden gives the same answer as before. The ledger charges before any leg runs. Two subjects' providers are not equal.                                        |
| 4  | DataFusion cuts the metric plan (decision 3), after Q3                                                                         | A differential against PR 3's answers on DuckDB and Postgres. One placeholder for each value in each pushed statement. A cut that the map does not accept is refused by name. |
| 5  | The federated SQL tool (decision 2), after Q1 and Q2                                                                           | Each startup refusal. DDL and DML refused at planning. The raw guards per leg. No planner or driver text reaches a caller.                                                    |
| 6  | Only if Q3 is (b): the unparser as the generator                                                                               | A differential against (a), per dialect.                                                                                                                                      |

What #828's milestones become:

- **M1** is replaced by PRs 2 to 4.
- **M2** (CI slice) is unchanged and does not depend on this record.
- **M3** (plan-time heuristic) reads PR 4's cut (the legs per source and the local rest) and not
  today's fixed two-leg split.
- **M4** (elastic multi-node) plugs in where the `FederationPlanner` runs the local rest. A worker
  must call the leg runner, so each guard of decision 4 still runs once per leg. How the subject's
  credential reaches a worker stays open, as #828 states.
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
