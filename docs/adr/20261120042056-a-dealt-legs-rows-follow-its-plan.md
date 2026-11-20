---
title: A dealt leg's rows follow its plan
description: A leg whose rows a node holds is dealt across the stage's tasks as a feed on the stream each task's plan was set over, so the plan carries a schema and a feed declaration and no row, and a feed that stops short fails the task as a lost node.
---

# A dealt leg's rows follow its plan

Status: **proposed**, 2026-11-20. Built in the change that adds this record. It decides and proves
nothing beyond what "Limits" lists.

## The question

`docs/adr/20261030182759-shuffle-across-nodes.md` deals a leg's rows between the tasks of the stage
that scans it, and sent each task its share inside the stage's plan. A plan then grew with the leg:
in the multi-node TPC-H check it reached 222 to 359 MB per task, sending it took up to 10.1 s at 16 to
44 MB/s, and a consumer's `execute_task` gave up after the engine's fixed 10 s wait for that plan.
The check failed on that wait in 4 of 31 runs on the base, and a change that only raised the thread
count did not prevent it.

## Decision

1. **A dealt leg's scan is planned as a leaf that holds the leg's schema and a work-unit feed's
   declaration** (`FedRowsExec`, `crates/sutura-exec-datafusion/src/combine/across/fed.rs`). The
   rows stay at the node that planned the question. Each task is sent its share after its plan, on
   the stream the plan was set over, as `datafusion-distributed`'s own work units: one unit is one
   source batch as Arrow IPC. The engine and its wire protocol are unchanged.
2. **A feed is read only by the task its call installed.** The worker keys a feed's receivers by feed
   id and partition inside that task's own session, and the units arrive on the `CoordinatorChannel`
   call that installed it, which the task grant (Decision 5 of the shuffle record) admits only for a
   node holding the question's key. No call addresses a feed from outside its own stream. The leaf is
   admitted by the receiving node's refusal of source-reading leaves as a fifth carried kind: it
   reads no source and no credential.
3. **A share is whole or the task fails.** Each partition's feed ends with a marker that carries the
   number of batches sent. A stream that closes before it, or a count that is not what arrived, fails
   the task under the code `sutura.refusal.feed-lost`, which the planning node reads back as
   `CombineError::NodeLost`: the question may be asked again, and no partial answer is made. A unit
   that is not one IPC message, or whose columns are not the leg's, fails the task as the engine's
   failure.
4. **The stage's task count is checked at the feed.** The rows are dealt across the count the
   planner chose; a stage that runs another count refuses to feed.

## Limits

- The planning node still holds every leg's rows until the question ends, and encodes a remote
  task's share as it sends it. The receiving worker queues what it was sent in an unbounded channel
  outside its pool, as the plan that carried the rows was held. This decision moves where rows wait.
  It does not bound them.
- A feed is read once per partition. A stage that executes a fed partition twice fails.
- A node holding the question's key can still feed rows that are not the leg's, as it could carry
  them in a plan. The check at the leaf is the leg's own schema.
- No cell measures the bytes on a link. The leaf's declaration is measured to be the same size for
  ten rows as for a hundred thousand, and the engine's own `plan_bytes` log in the multi-node check
  is the measure at the worker. The effect on that check's failure rate is measured only by running
  it.
- A message holds the units ready at one poll, which the provider keeps near one per partition by
  yielding between units. A unit is a source batch, so a message is as large as the batches a source
  streams.
