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
2. **A feed is read only by the task its call installed.** The engine gives each
   `CoordinatorChannel` call its own receiver map, keyed by feed id and partition, puts the units
   that call carries in that map alone, and refuses a second install for the same task. The task
   grant does not narrow this: a node that installs a stage supplies the question's key itself
   (Decision 5 of the shuffle record), so what keeps one task's rows from another is the per-call
   map. The leaf is admitted by the receiving node's refusal of source-reading leaves as a fifth
   carried kind: it reads no source and no credential.
3. **A share is whole or the task fails.** Each partition's feed ends with a marker that carries the
   number of batches sent. A stream that closes before it, or a count that is not what arrived, fails
   the task under the code `sutura.refusal.feed-lost`, which the planning node reads back as
   `CombineError::NodeLost`: the question may be asked again, and no partial answer is made. A unit
   that is not one IPC message, or whose columns are not the leg's, fails the task as the engine's
   failure. A share the planning node could not write is marked in the feed and fails the task the
   same way, as the planning node's own failure and no lost node.
4. **The stage's task count is checked at the feed.** The rows are dealt across the count the
   planner chose; a stage that runs another count refuses to feed.

## Limits

- The planning node keeps every leg's rows until the question ends, and the engine fills an
  unbounded outbound channel for each task from its feed: the IPC copy of every share not yet sent
  waits there, outside any pool, beside the batches themselves. Yielding between units bounds a
  message's size, not memory, and never waits on the network. This is no worse than the plan that
  carried the same rows. Bounding it takes a change to the vendored engine, not made here. The
  receiving worker's queue is unbounded and outside its pool as well.
- The engine ends a feed's stream when the planning node fails, so a failure there that is not
  marked in the feed reads at the worker as a cut feed. The planning node's own error is the one its
  caller is told.
- A feed is read once per partition. A stage that executes a fed partition twice fails.
- A node holding the question's key can still feed rows that are not the leg's, as it could carry
  them in a plan. The check at the leaf is the leg's own schema.
- The size of a stage's plan is measured on the wire by
  `a_stage_sent_to_another_node_is_no_larger_for_a_leg_of_a_hundred_thousand_rows`, which reads the
  length of the first gRPC message of each install. The effect on the multi-node check's failure rate
  is measured only by running it.
- A message holds the units ready at one poll, which the provider keeps near one per partition by
  yielding between units. A unit is a source batch, so a message is as large as the batches a source
  streams.
