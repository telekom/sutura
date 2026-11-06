---
title: Workers read a split leg's partitions themselves, and each fails on its own limits
description: A federated leg on PostgreSQL or BigQuery is split into partitions that workers read, so no node holds the whole leg. The result-stream bound is per partition, a worker fails as soon as one of its own limits is crossed, the federated row ceiling stays global, and the task grant grows to name the leg, source and partition a worker reads (github.com/telekom/sutura#828).
---

# Workers read a split leg's partitions themselves, and each fails on its own limits

Status: **accepted**, 2026-11-02. This is the owner's decision, and so is each row of "Decided
with the design" below, accepted by the owner the same day. Partly built, under
`github.com/telekom/sutura#828`: a PostgreSQL lookup leg can be split, and every split is still
read on the coordinating node, because no split is offered a grantee yet. So nothing in production
grants a partition, no worker reads a source outside tests, and a BigQuery leg is read whole. A row
marked *inferred* was neither read in this repository nor run here. This record decides and proves
nothing.

## The question

`docs/adr/20261030182759-shuffle-across-nodes.md` spreads a combine's stages over several nodes, but
not the reads under them. Its Decision 3 reads each leg whole, at the coordinating node or at the
node that owns the source. That node holds the leg as one stream, bounded at 1,000,000 rows and
256 MiB while it is read (`MOST_RESULT_ROWS` and `MOST_RESULT_BYTES` in
`crates/sutura-exec-postgres/src/adbc/session.rs` and `crates/sutura-exec-bigquery/src/adbc.rs`,
held by `Accumulating::push` in `crates/sutura-domain/src/warehouse/arrow.rs`). The combine then
registers the leg as an in-memory table that its pool does not charge
(`crates/sutura-exec-datafusion/src/combine.rs`).

So adding nodes does not change how much one leg can be. The TPC-H corpus puts `customer` and `part`
in PostgreSQL (`.github/workflows/tpch-refusal-rate.yml`). At SF10 they hold 1.5 and 2 million rows,
the generator's sizes and not a run here, so an unfiltered lookup leg over either is over the bound
before any worker sees a row.

Where is a leg read, under which credential, and which limits does each node apply?

## The owner's answers

| Question                                          | Answer                                                                                                                               |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Q1. Where is a large leg read?                    | On workers. A leg is split into partitions that workers read, not materialised whole at the coordinating node                        |
| Q2. Is the 1,000,000-row / 256 MiB bound per leg? | Per stream. Each partition of a split leg gets it. No limit is raised for SF10's lookup legs                                         |
| Q3. Which data system first?                      | PostgreSQL (W1) and BigQuery (W2), in parallel                                                                                       |
| Q4. How is the federated row ceiling checked?     | Globally. A worker fails as soon as its own final-stage partition exceeds it, and the exact total stays at the coordinating node     |
| Q5. Which limits apply per worker?                | Those that protect one node's memory, and they fail while reading. Global policy limits are lifted only by the deployment's operator |

Decision 5, what the task grant carries and the worker's re-certification, was posted with these
answers as the design that follows from them, not asked as its own question.

## Decision

1. **A leg on PostgreSQL or BigQuery may be read in partitions, on workers** - and a lookup leg on
   Oracle, since the third amendment below. The coordinating node plans the leg as partitions, and
   each partition is one task of a source-reading stage. A worker reads its partition and feeds the
   rows to the combine's next stage on that node, so no node holds the whole leg. A leg on any
   other data system (for Oracle, any leg the third amendment does not split), and a remote leg, is
   still read whole where shuffle Decision 3 reads it. A stage that reads no source still holds no
   credential, and its worker still refuses a stage whose leaf reads a source. Which legs are split,
   and when, is decided below.
2. **The result-stream bound is per stream.** Each partition's stream is bounded at 1,000,000 rows
   and 256 MiB while it is read, as a leg's stream is today. A leg of N partitions can carry N times
   that. No constant is raised, and none becomes a setting. SF10's `customer` and `part` lookup
   legs are answered by splitting them, not by a larger bound.
3. **PostgreSQL and BigQuery proceed in parallel, each with its own split.**

   |                                | W1 PostgreSQL                                                                                                                                                         | W2 BigQuery                                                                                                                                          |
   | ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
   | A partition                    | A `ctid` range over the leg's root table, cut from `pg_class.relpages`, read by a TID range scan (PostgreSQL 14 or later; the tier runs `postgresql_18`)              | One Storage Read stream over the job's result                                                                                                        |
   | Where the statement runs       | On each reading worker: the leg's statement, restricted to its partition's range                                                                                      | Once, at the coordinating node, as one job. Workers read its result and run no statement                                                             |
   | One consistent read            | The coordinating node exports one snapshot (`pg_export_snapshot()`) and keeps its transaction open. Each worker's transaction imports it (`SET TRANSACTION SNAPSHOT`) | Every stream reads the one result the job wrote                                                                                                      |
   | Who the read runs as           | The shared service user, as every PostgreSQL leg runs (`crates/sutura-exec-postgres/src/lib.rs`)                                                                      | The caller's own pool principal, as the BigQuery adapter's exchange yields it (`crates/sutura-exec-bigquery/src/adbc/subject.rs`)                    |
   | What a reading worker holds    | The service user's credential, on every worker that reads PostgreSQL                                                                                                  | A short-lived source token, which the coordinating node exchanges once per question and forwards. Read-only scope if the exchange can narrow it      |
   | Inferred, not read or run here | That the snapshot import fits the session shape `crates/sutura-exec-postgres/src/adbc/session.rs` opens, and that the planner picks a TID range scan for the range    | That the exchange can narrow the scope: today it asks for the API client's own scopes (`subject.rs`). That a job's result is read by several streams |

   The raw caller assertion is never forwarded, and no worker holds the identity provider's client
   secret. Nothing in W1 is the caller's: a PostgreSQL source has no place for a subject.
4. **The federated row ceiling is global, and a worker fails on it early.** A worker that runs a
   task of the final stage counts that task's rows and fails as soon as they exceed the whole
   ceiling. The coordinating node still counts the exact total and refuses over it, as today. No
   worker gets a share of the ceiling. The early check is sound only on a stage whose tasks hold
   disjoint pieces of the combined set. It sits where the coordinating node's check sits today, on
   the combined set before `top` ranks it (`crates/sutura-app/src/federated.rs`), so a stage that
   regroups after it is not checked.

   | Limit                                                  | Kind              | Applied                                                                  |
   | ------------------------------------------------------ | ----------------- | ------------------------------------------------------------------------ |
   | The result-stream bound                                | One node's memory | Per stream, on the node that reads it, failing at the batch that crosses |
   | `runtime.working_set_max_bytes`                        | One node's memory | Per worker, as the shuffle record's Decision 6 applies it                |
   | A link's buffer share                                  | One node's memory | Per worker, as the shuffle record's Limits state it                      |
   | `governance.federated_row_ceiling`                     | Global policy     | Whole, early at a final-stage worker, exact at the coordinating node     |
   | The single-source cap (`MAX_ROWS`)                     | Global policy     | Unchanged. It stays compiled                                             |
   | The response byte bound (`ResponseByteLimit::DEFAULT`) | Global policy     | Unchanged, at the coordinating node. It stays compiled                   |
   | `governance.per_replica_spend_ceiling`                 | Global policy     | Unchanged, at the coordinating node's pre-flight, where a W2 job runs    |

   A limit that protects one node's memory fails while reading, on the node whose memory it
   protects. A global policy limit is never divided among workers, and splitting a leg never raises
   it. Only the deployment's operator lifts one, in its configuration, where it is a setting at all.
5. **The task grant grows.** A grant for a source-reading task carries:

   | Field        | Why                                                                                           |
   | ------------ | --------------------------------------------------------------------------------------------- |
   | Leg digest   | The worker reads only the leg the grant was issued for                                        |
   | Source       | The worker refuses a partition of a source the grant does not name                            |
   | Partition    | The `ctid` range (W1), the stream (W2) or the bucket at its SCN (W3, third amendment)         |
   | Expiry       | The worker refuses a grant past it                                                            |
   | Caller key   | For the worker's audit line and the source's labels, so a read names whose question it served |
   | Limit shares | The limits this task applies. Decision 4 fixes the federated ceiling's as the whole ceiling   |

   Before it reads, each worker re-certifies the leg against its own pinned catalog. That is the
   check a node runs on a remote leg: Decision 3 of
   `docs/adr/20261017032054-leg-transport-between-sutura-nodes.md` and its per-metric digest
   amendment. A worker refuses a leg its catalog does not certify, or certifies differently.

## What this changes in older records

| Record and statement                                                                                                                | Now                                                                                                                                                                                                                                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shuffle, Decision 3: only a node that reads a source holds the caller's credential, and a worker refuses a stage with a source leaf | Superseded for source-reading stages. A worker reads a split leg's partition under a grant that names it, holding the service user's credential (W1) or a token for the caller's principal (W2). Still holds: a stage that reads no source holds no credential and refuses a source leaf, and a leg that is not split is read where Decision 3 reads it |
| Shuffle, Decision 8: the ceiling applies once, at the coordinating node                                                             | The exact total still does. A final-stage worker also fails early (Decision 4)                                                                                                                                                                                                                                                                          |
| Shuffle, Decision 9: the size rule measures the legs the receiving node holds                                                       | A split leg is never held at one node, so the rule has nothing to measure for it. Whether to split is decided by the source's own estimate before reading, against the same threshold (below)                                                                                                                                                           |
| Shuffle, Limits: the grant does not expire, does not name the caller, and binds a question and not a partition                      | A source-reading grant carries an expiry, a caller key and a partition. A source-reading grant is signed under a key per reading task, sent only to the worker that reads it (below). A grant for a stage that reads no source is unchanged: an HMAC under a key every stage node of the question holds                                                 |
| Operator, Q6 and Decision 2: a worker has no source, no catalog and no source credential                                            | A worker that reads a partition holds the catalog it re-certifies against, and the service user's credential (W1) or a forwarded source token (W2). Still holds: a worker has no caller-facing route, and a stage that reads no source holds no credential                                                                                              |
| Operator, Decision 7: a worker refuses a configuration that declares a source or a catalog                                          | It cannot hold for a worker that reads. A reading worker declares the sources it reads and a catalog. A worker that only combines keeps the refusal (below)                                                                                                                                                                                             |
| Leg transport, Decision 3: the receiving node certifies a remote leg                                                                | Each reading worker runs the same check before it reads                                                                                                                                                                                                                                                                                                 |

## Decided with the design

Each row was posted as a proposal and accepted by the owner as a decision on 2026-11-02. Its text
is the proposal's, except a row marked *(2026-11-03)*, which records the owner's answer that day to
a question building it raised, and a clause marked *(third amendment)*, which that amendment added.

| Question                                                               | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| One worker pool, or a reading role beside a combining one?             | Two roles of one binary. A reading worker declares the sources it reads and a catalog. A combining worker keeps operator Decision 7's refusal, so a node that only combines holds no credential, as a stage that reads no source holds none. *(2026-11-03)* The coordinating node learns which nodes read which sources from its own configuration: `server.nodes` names each other node's role and the source aliases it reads, checked at load; readiness still comes from resolving the node |
| Which legs are split?                                                  | A lookup leg, and a fact leg whose partial groups are grouped again on the leg's own keys after a repartition, before they join anything. A fact leg's partial groups from two partitions joined to another fact leg would double a value                                                                                                                                                                                                                                                       |
| The ambiguous-link check over a split lookup leg                       | Plan it across, after a repartition on the link key, so a key duplicated across two partitions meets in one task. Today `refuse_ambiguous_link` scans one in-memory leg                                                                                                                                                                                                                                                                                                                         |
| When is a leg split, given the size rule measures legs one node holds? | W1: by the source's own estimate before reading, `pg_class.relpages` times the page size, against `server.nodes.across_above_bytes`. *(2026-11-03)* W2: by the job's result row count, which the driver patch reports from `ExecutePartitions`, against the per-stream bound. The ADBC adapter answers no dry run, so W2 has no estimate before the job runs                                                                                                                                    |
| How many partitions?                                                   | Enough that each partition's estimate (`pg_class.reltuples` over the range's pages, W2's row count over the streams, or *(third amendment)* W3's `NUM_ROWS` over the buckets) sits well under the per-stream bound, at most the ready reading workers. A partition over the bound is refused, not split again                                                                                                                                                                                   |
| Pages past `relpages`, and a root that is not a plain table            | The last range has no upper bound, because `relpages` is an estimate. A view, a partitioned table's parent or a foreign table is read whole, as today                                                                                                                                                                                                                                                                                                                                           |
| What bounds the exporting transaction (W1)?                            | The question's deadline. A worker that cannot import the snapshot fails the question                                                                                                                                                                                                                                                                                                                                                                                                            |
| How the coordinating node gets a source token to forward (W2)          | Today the driver runs the exchange itself from a loopback credential source, and sutura never holds the token. sutura runs the exchange and hands the token to the driver at the coordinating node and at each reading worker                                                                                                                                                                                                                                                                   |
| Can the exchange narrow the token to read-only (W2)?                   | *(2026-11-03)* A second exchange per question asks for a read-only scope, and only that token is forwarded to a reading worker. The coordinating node's job runs on the first exchange's token, because one exchange cannot serve both                                                                                                                                                                                                                                                          |
| What reads a partition of a job's result (W2)?                         | ADBC's partitioned execution, proposed to the driver upstream and carried as `nix/bigquery-adbc-partitions.patch` until a release has it. *(2026-11-03)* The patch also reports the result's row count                                                                                                                                                                                                                                                                                          |
| How the source token reaches a worker                                  | In a header of the call that installs the stage, as the task key does, never logged and never inside the grant                                                                                                                                                                                                                                                                                                                                                                                  |
| What the leg digest hashes                                             | The leg's wire document, which a lookup leg already ties to its metric's digest                                                                                                                                                                                                                                                                                                                                                                                                                 |
| The expiry's value                                                     | The question's deadline, read on the worker's own clock                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| The caller key's form                                                  | The masked subject a record already renders, never the full `SubjectKey`                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Which limits ride in the limit shares beyond the federated ceiling     | The per-stream bound only. A worker's memory limits stay its own configuration's                                                                                                                                                                                                                                                                                                                                                                                                                |
| Who can sign a source-reading grant                                    | A key per reading task, sent only to the worker that reads it, so a node that holds the question's key cannot sign a read it was not given                                                                                                                                                                                                                                                                                                                                                      |
| Which refusal reports a failed re-certification                        | `RemoteLegNotCertified`, naming the source, as for a remote leg. *(2026-11-03)* It crosses the link as its own typed code, and the planning node maps it to the refusal naming its own source                                                                                                                                                                                                                                                                                                   |
| How a worker's refusal reaches the caller                              | A typed code in the link's failure status. The row-ceiling refusal crosses as that code and reaches the caller as `RowsOverCeiling`, and a failed re-certification as `RemoteLegNotCertified`; any other failure relayed from another node still arrives as text and stays an internal error                                                                                                                                                                                                    |

## The refusals this adds

| Where              | Refused                                                                                                | Why        |
| ------------------ | ------------------------------------------------------------------------------------------------------ | ---------- |
| Reading worker     | A source-reading task with no grant, or whose grant does not name its leg digest, source and partition | Decision 5 |
| Reading worker     | A grant past its expiry                                                                                | Decision 5 |
| Reading worker     | A leg its own pinned catalog does not certify, or certifies differently                                | Decision 5 |
| Reading worker     | A source leaf other than a granted partition read                                                      | Decision 1 |
| Reading worker     | A partition's stream past 1,000,000 rows or 256 MiB, at the batch that crosses                         | Decision 2 |
| Final-stage worker | A task whose rows exceed the federated row ceiling                                                     | Decision 4 |

A new refusal enum is enrolled in `check-refusal-coverage`, which reads only enrolled enums. Each
refusal gets its own cell.

## Limits

- **A reading worker holds a standing PostgreSQL credential.** The grant bounds what an honest
  worker reads, not what the service user's credential reaches. A worker that leaks it leaks
  everything the service user may read, with no grant needed. Every node that reads PostgreSQL holds
  it, where today only serving nodes do.
- **A forwarded source token is a bearer token for the caller's principal.** For its lifetime it
  reads anything that principal's grants and the token's scope allow, not only its stream. Its
  read-only scope is inferred, not verified here.
- **A partition grant binds a partition only against a node without the question's key.** The grant
  is an HMAC under a per-question key that every stage node of the question holds. Such a node can
  sign a grant for another partition, or another source, of that question until the key per
  reading task decided above is built.
- **A forwarded source token is not refreshed.** A W2 read that outlives it fails.
- **A W2 worker re-certifies the leg, not the stream.** The job ran at the coordinating node, so the
  worker checks the leg it is told the stream holds and cannot check what the stream holds.
- **The early ceiling check is partial.** It fails a worker only on its own partition. Several
  final-stage workers each under the ceiling can together exceed it, and then only the coordinating
  node refuses, after it holds the rows within its result budget. A stage that is not the last one
  before the coordinating node is not checked against the ceiling at all.
- **The per-stream bound no longer bounds a leg.** A leg of N partitions carries up to N times the
  bound. Each node's working set and the federated ceiling on the answer are what bound the
  question.
- **A range splits pages, not rows.** A range whose pages hold wide or many rows can cross the
  per-stream bound while its neighbours are small. It is refused, not split again.
- **One read costs the source more.** A leg of N partitions opens N sessions plus the exporting one.
  Each partition's statement reads the leg's joined dimension tables again. The exporting
  transaction stays open for the whole read, which on PostgreSQL holds back cleanup of rows it
  can still see (inferred).
- **The expiry trusts each worker's clock.** Skew between nodes moves it.
- **Splitting adds no identity proof.** A W2 stream reads as whatever principal the BigQuery
  adapter's exchange yields, and `docs/where-identity-is-proven.md` decides what may be cited for
  that. W1 reads as the service user, never as the caller.
- **Not measured.** The read throughput, the load N sessions put on a source, or whether a TID range
  scan is chosen for every partition statement.

## Options considered

- **Each worker exchanges the caller's assertion** - not taken. The raw caller token would be
  forwarded to every reading worker, and every node would hold the identity provider's client
  secret.
- **Forward the pool-audience assertion to the workers** - not taken. Any holder exchanges it for a
  token of the caller's principal with the API client's full scopes.
- **W0: raise the row bound, or make it a setting** - not taken. It moves the bound on one node and
  does not spread the read, and the read is what has to scale.
- **A share of the federated ceiling per worker** - not taken. An answer under the ceiling whose
  rows land unevenly would be refused, and the share would change with the worker count.

## Out of scope

- Splitting a leg on any other data system. Its legs are read whole, as today.
- A question that reads one data system. It never goes across (shuffle Decision 9).
- A remote leg, read at the node that owns its source.

## Consequences

- Cells for every refusal above. An exact differential per data system: a split leg's answer equals
  the whole leg's. W1 runs in process against the PostgreSQL tier. W2 needs a live BigQuery job, so
  its cells run only in the BigQuery venue.
- A grant module that carries the fields of Decision 5, and a check that refuses a grant without
  them.
- The worker role in `sutura-config` gains a reading role beside the combining one, and the
  chart's worker values change with it.
- W2 waits for a partitioned read of a job's result.

## Amendment, 2026-11-05: a BigQuery source token travels with its own task, and only a pool source splits

The owner decided four questions that building W2's reading worker raised.

1. **The source token travels in its partition's own task, beside its reading key.** This replaces
   the row "How the source token reaches a worker" in "Decided with the design": a header of the
   call that installs a stage reaches every node of that stage, and so the token for one task
   travels only with that task. The worker takes the token once, when the read starts, and its
   `Debug` and `Display` show no value. The grant signs a digest of the token, so a worker refuses a
   token the grant was not issued with.
2. **A split question makes two exchanges.** The coordinating node runs the job on the first,
   asked for `PoolScope::Query` (the BigQuery client's own scope), so it can still cancel the job.
   The second asks for `PoolScope::ReadOnly` (`bigquery.readonly`), and only its token is
   forwarded. This names the scopes of the 2026-11-03 row on narrowing and does not change it.
3. **Only a BigQuery source that federates at a workload identity pool is split.** A
   `google_login`, `domain_delegation` or shared BigQuery source is read whole, as today. A reading
   worker whose configuration declares a BigQuery source of another kind does not start.
4. **A forwarded token that BigQuery refuses, or that has expired, fails the read as an internal
   error**, not as a refusal that names the source.

What is built: `PoolScope`, which `PoolExchange::exchange` takes. Every caller asks for
`PoolScope::Query`, and nothing is forwarded.

Limits:

- **`PoolScope::ReadOnly` is not narrowed yet.** `GoogleSts` asks for the full `bigquery` scope for
  both variants. The read-only scope lands with the forwarded token, in the next change of the
  BigQuery reading worker, so until then decision 2 is a decision and not behaviour.
- **A read-only token still starts queries.** `bigquery.readonly` accepts `jobs.query`, so a holder
  of a forwarded token can run any read query the caller's principal may run, until the token
  expires. It cannot cancel one, and nothing ends the token early. That Google accepts `jobs.query`
  under this scope was measured with a service account's own token
  (`crates/sutura-exec-bigquery/src/adbc/identity.rs`), not with a pool principal's.
- **A token in a task is as protected as the task.** Routing each task to its own worker and the
  link's TLS keep it from other nodes. A node that can read a task's document can read its token,
  as it can read the reading key beside it.
- **Taking the token once bounds the worker's copy, not the token.** It stays valid at BigQuery
  until it expires.
- **A worker cannot tell whose token it holds.** The token is opaque, so the digest ties it to the
  grant and not to the caller. The worker trusts the coordinating node for that.
- **An internal error is less helpful.** A caller whose grants do not cover the stream, or whose
  token expired during a long read, gets an internal error rather than `source_refused`.
- **Not measured: which principal BigQuery lets read a stream of a job's result.** Until a venue
  run observes a split read under two principals, a split BigQuery read is not cited for the
  caller's own access.

## Second amendment, 2026-11-05: the BigQuery adapter can split a pool leg, and nothing calls it

`GoogleSts` asks for `https://www.googleapis.com/auth/bigquery.readonly` for `PoolScope::ReadOnly`.
This replaces the first amendment's first limit. The BigQuery adapter has both halves of a split
leg, and no composition root calls either:

- `BigQueryWarehouse::split_leg` runs a leg once, for a subject at a source that federates at a
  pool, on a token asked for `PoolScope::Query`, labelled as a billed leg is. A second exchange asks
  for `PoolScope::ReadOnly`, and only that token is handed back to forward. Every other pairing is
  refused before either exchange. It reports when what it forwards ends: the question's end, or the
  forwarded token's expiry where that is earlier. That end is computed, not applied: no grant reads
  it yet, and `Grantee::grant` takes no earlier end, which the grant change adds. The job asks the
  driver for at most one stream per reading node, each of about half the per-stream bound, so each
  partition sits well under it as "How many partitions?" decides.
- `BigQueryWarehouse::read_partition` hands the driver the token it is handed as its only
  credential option, and only at a source that federates at a pool. `clippy.toml` bans the call, so
  its one caller will carry an `#[expect]`.

The `split_lookup` arm that offers a BigQuery leg for splitting lands with the reading worker's
BigQuery arm. Until a worker can read a granted BigQuery partition, a grant of one would fail a
question that reading the leg whole answers.

Limits, beside the first amendment's:

- **Neither the grant nor the token ends early yet.** Each split makes two tokens for the asking
  subject's principal. Both stay valid at Google until Google's own expiry, and nothing here
  shortens it. The forwarded token's expiry reaches no grant until the grant change applies it.
- **The adapter does not check whose credential it was handed.** It cannot read a subject key off
  the presented credential. The one tie between the caller a grant names and the token it forwards
  will be one layer up, once the `split_lookup` arm passes `split_leg` the presented credential it
  is handed: the federated leg reads both off the same request grant.
- **No cell sees the options reach the driver.** The cells check the options `split` and
  `forwarded` compute, and that a split or a partition read gets as far as the driver's load. That
  `split_leg` and `read_partition` hand those options to the driver, and no others, is held by
  review alone until the venue cell runs.
- **No driver ran it.** The cells run over fakes for the token service. Nothing has split a leg
  against BigQuery, and a stream read by a principal other than its creator is not yet measured.
- **Half the bound is a target, not a guarantee.** The Storage Read API may answer fewer streams
  than asked, each then larger, and a stream past 1,000,000 rows or 256 MiB is still refused when
  it is read. The byte bound plays no part in the stream count.

## Third amendment, 2026-11-06: an Oracle lookup leg splits by flashback query, under the shared service user

The owner decided four questions for Oracle as a third data system (W3), D1 to D4 below. Each item
also says what follows from it with the design; a sentence marked *(design)* is that, not the
owner's answer. This replaces the first row of "Out of scope" for Oracle only, and amends Decision
1, Decision 5's Partition row and the row "How many partitions?" above to name Oracle: a lookup leg
on an Oracle source may be read in partitions, on workers. A question that reads Oracle alone is
still pushed down whole, and a fact leg on Oracle is still read whole.

1. **The shared service user only (D1).** A reading worker reads as the source's declared service
   user and holds its credential, as W1 does. An `impersonation-at-source` Oracle source is read
   whole. A per-task source token, as W2 forwards one, stays possible and is not built.
2. **Each source opts in (D2).** `split: flashback` on a `kind: oracle` source allows its lookup legs
   to split. Without it, every leg is read whole. A source that declares it is probed at boot, and a
   failed probe refuses startup. The operator grants the service user the `FLASHBACK` object
   privilege on each table it reads, and not `FLASHBACK ANY TABLE`. *(design)* The key is refused on
   any other kind and on an `impersonation-at-source` source. The probe reads the privileges granted
   to the service user when it runs at boot: directly, through `PUBLIC`, and through roles granted
   to the service user. It refuses `FLASHBACK ANY TABLE`, `GRANT ANY PRIVILEGE` and `GRANT ANY
   ROLE` held that way, and a role granted to the service user that is not enabled when it signs
   in. It then reads each table the source's models name at a snapshot: the models' own tables only,
   not tables their joins reach.
3. **One snapshot per split leg, taken when the leg is split (D3).** The snapshot is
   `TIMESTAMP_TO_SCN(SYSTIMESTAMP)`, read once for the leg. Every table reference of every
   partition's statement carries `AS OF SCN` with that number, so the leg's partitions read one
   consistent state. Two split legs of one question read at two SCNs, and an Oracle leg of the same
   question that is read whole reads the database as it is when that leg runs. The snapshot is in
   the statement and not in the session: no partition calls `DBMS_FLASHBACK`.
4. **A reading worker refuses a stale snapshot (D4).** Before it reads, the worker asks the
   database when the snapshot was taken (`SCN_TO_TIMESTAMP`) and refuses a snapshot older than its
   own request budget. A snapshot the database can no longer read is refused, never read fresh
   instead. *(design)* The refusal is one typed error for `ORA-01555` (snapshot too old), `ORA-08181`
   (not a valid system change number) and `ORA-01466` (table definition has changed).

|                             | W3 Oracle                                                                                                                                               |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A partition                 | One hash bucket of one integer key of the leg's root table: `ORA_HASH(key, N - 1) = i`. A row whose key is NULL is in bucket 0 only                     |
| Where the statement runs    | On each reading worker: the leg's statement, `AS OF SCN` on every table, restricted to its bucket                                                       |
| One consistent read         | The one SCN read at split time                                                                                                                          |
| Who the read runs as        | The shared service user                                                                                                                                 |
| What a reading worker holds | The service user's credential                                                                                                                           |
| When the leg is split       | By the dictionary's estimate, `ALL_TABLES.NUM_ROWS` times `AVG_ROW_LEN`, against `server.nodes.across_above_bytes`. No row or no statistics: read whole |
| Which key is hashed         | The leg's first key that is a column of its root table declared `NUMBER` with scale 0. None: read whole                                                 |

The partition descriptor, version 1, in order: the version byte `1`; the SCN as eight big-endian
bytes, never zero; the hashed key's position in the leg as two big-endian bytes; the bucket's index
and the bucket count as four big-endian bytes each. A reading worker refuses a version it does not
read, bytes cut short or past the last field, an SCN of zero, fewer than two buckets, an index at or
past the count, and a position the leg holds no key at.

Limits:

- **The dictionary's statistics are an estimate.** A table whose statistics are stale or missing is
  read whole, or split into buckets whose real sizes differ from the estimate. A bucket past the
  per-stream bound is refused, not split again.
- **A bucket is not a range.** Every partition reads the whole root table and filters it, so a leg
  of N partitions reads it N times. `ORA_HASH` buckets are not uniform, and every NULL key lands in
  bucket 0.
- **The snapshot is a few seconds old when it is taken.** Oracle maps a time to an SCN at a
  granularity of about three seconds, so `TIMESTAMP_TO_SCN(SYSTIMESTAMP)` can name an SCN up to
  about three seconds before the split, and a row committed in those seconds is not read (D3).
- **The snapshot's age is approximate.** `SCN_TO_TIMESTAMP` is precise to about three seconds, so
  D4's refusal is that close to the budget and no closer. How long the database can read a snapshot
  is its undo retention, which the operator configures and nothing here sets.
- **A reading worker's credential reads history.** The `FLASHBACK` object privilege serves flashback
  version queries as well as flashback queries. A leaked service-user credential from any node
  that reads Oracle can read every version still in undo of the rows of each table the service user
  is granted `FLASHBACK` on or owns - an owner needs no grant - rows deleted or updated since
  included. D4 bounds only what an honest worker reads.
- **The boot probe reads the models' own tables.** A table that only a join reaches, with no
  `FLASHBACK` grant, passes the probe and fails the partition read.
- **Hashing one integer key is exact only for a lookup leg.** Every row of one group has the same
  key, so every group is in one bucket. A fact leg is not split.
- **Nothing is built yet.** This record decides to build the split in the Oracle adapter, the
  setting and the boot probe, and the two changes stacked on it deliver them: one adds the types
  with today's behaviour, the next the behaviour. Even then no composition root offers an Oracle
  leg for splitting, and no reading worker reads an Oracle partition, until the grant and the
  reading worker gain an Oracle arm.
