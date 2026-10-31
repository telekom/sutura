---
title: One question's combine runs across nodes, and a stream between them carries a node identity and a task grant
description: How a federated question whose combine is too big for one node spreads over several (#828). datafusion-distributed runs the combine's stages on several nodes, with no coordinator service. Only a node that reads a source holds the caller's credential. A stream in the middle of the combine is authenticated by mutual TLS between nodes, plus a grant from the coordinating node that names the question, the stage and the caller.
---

# One question's combine runs across nodes, and a stream between them carries a node identity and a task grant

Status: **accepted**, 2026-10-31, scoped to a question whose join or aggregation needs more than
one node. The owner chose the engine family, the stream identity and when a question goes across,
the answers below. Decision 5, the task grant, is deferred. This record decides; it proves nothing.

## The question

A federated question's legs deliver rows to node A, and A's combiner joins them
(`crates/sutura-exec-datafusion/src/combine.rs`). When that join needs more memory than A's
`runtime.working_set_max_bytes`, A refuses. A pool never spills (`crates/sutura-exec-datafusion/src/pool.rs`).
A second node does not help today. A applies its own budget to a remote leg's reply
(`crates/sutura-exec-node/src/call.rs`), and the combine runs at A alone.

How does one combine spread over several nodes? And how does each stream between them prove which
caller's question it belongs to?

## The owner's answers

| Question                                                                           | Answer                                                                                                                                                                                                     |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1. How is one question split across nodes?                                        | A shuffle engine: nodes exchange rows all-to-all                                                                                                                                                           |
| Q2. How does a stream in the middle of the combine prove whose question it serves? | Node identity plus a task grant. Only a node that reads a source holds the caller's credential. A stream between nodes is authenticated by mutual TLS, plus a short-lived grant from the coordinating node |
| Q3. Which questions go across nodes?                                               | Only one whose join or aggregation one node cannot hold. A question that reads one data system never does                                                                                                  |

## Decision

1. **The engine is datafusion-distributed**, version 4, with default features off and only `grpc`
   on. On 2026-10-30, a trial resolution against this tree's lockfile kept one copy each of
   DataFusion 55.1, arrow and arrow-flight 59.2, and tonic 0.14.6. With `sql` off, DataFusion's SQL
   frontend is in no built dependency tree, which keeps the property the `datafusion-federation`
   entry in `Cargo.toml` states. Nothing was compiled.
2. **No coordinator service, and no consensus.** The node that receives a question plans it and
   coordinates that question's stages. Questions share no state, so nothing needs raft or paxos.
   Each node admits work under its own concurrency bounds. A node finds its peers through the
   engine's worker resolver, backed by a declared list. A Kubernetes-backed list belongs with the
   operator.
3. **Only a node that reads a source holds the caller's credential.** A stage that reads a source
   runs where a leg runs today: at the coordinating node, through its own broker, or at the node
   that owns the source, through a remote leg and its one exchange
   (`docs/adr/20261017032054-leg-transport-between-sutura-nodes.md`). A node that runs a stage in
   the middle of the combine holds no caller credential and reads no source. Every node's worker
   refuses a stage it is sent whose leaf reads anything but another node's stage or rows the plan
   itself carries. The check sits at the receiving node, on the decoded plan, because a check at
   the coordinating node does not stop a node that builds a plan of its own.
4. **A stream between nodes is authenticated by mutual TLS.** Both ends present a node certificate
   from the deployment's own authority: nix-generated in the VM venue, cert-manager in the k3s
   venue. A call to the engine's worker service without a client certificate is refused. The
   listener's other routes keep accepting callers that present none.
5. **Each stream also carries a task grant.** The coordinating node signs it. It names the question,
   the stage, the partition, the caller's verified `SubjectKey`, and an expiry no later than the
   question's deadline. The receiving node checks the grant on every call, in a layer in front of
   the worker service. That handler discards request metadata, so the check cannot sit inside it.
   A call is refused when its grant does not verify, has expired, or names another task. The grant
   grants no source access. It ties a stream to one question, so that a node with a valid
   certificate cannot pull another question's rows.
6. **Rows stay in memory and are never stored.** Each node runs its stages under its own
   working-set pool, with the same never-spill policy. A node over its budget refuses, and the
   question is refused. The engine streams between nodes, and its source, read on 2026-10-30,
   writes no shuffle file. So the rule "no row is stored at A or at B" is meant to hold for every
   node. `check-answer-path-caches` gains the crate that hosts the worker service. That gate finds
   only a map holding an answer-path type, so it does not prove the engine stores nothing.
7. **A lost node fails the question.** The engine does not retry, and neither does sutura. The
   caller gets a refusal and may ask again.
8. **What scales is the combine's working set, not the answer.** The federated row ceiling still
   applies once, at the coordinating node, to the answer.
9. **A question goes across only when one node is too small for it.** A question that reads one
   data system never goes across. One that reads several goes across at once when its legs hold
   more than the declared `server.nodes.across_above_bytes`, or more than its working-set ceiling
   when that is unset. Otherwise the receiving node combines it, and plans it across only when that
   combine runs out of its ceiling. Every link adds a round trip a small question does not need.
10. **One listener.** The worker service is mounted on `sutura-http`'s listener, behind its own
    feature, as Flight is behind `flight`. That listener asks for no client certificate today
    (`crates/sutura-http/src/tls.rs`). It gains an optional client certificate, which only the
    worker service requires.

## Open, with a proposal

| Question                                        | Proposal                                                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Which key signs a grant?                        | The coordinating node's certificate key, verified against the deployment's authority. A key shared by the cluster would let any node mint a grant. Every node holds such a key too, so any node of the authority mints a grant as well: the grant adds an expiry and an audit line, and restricts nothing, unless only coordinating nodes hold a signing authority of their own. Deferred by the owner on 2026-10-31 |
| Which crate hosts the worker service and codec? | `sutura-exec-datafusion`, behind a default-off `distributed` feature. The engine plans the combine's own session, and the crate-boundary gate refuses an edge between two `sutura-exec-` crates                                                                                                                                                                                                                      |

## The refusals this adds

| Where          | Refused                                                                       | Why        |
| -------------- | ----------------------------------------------------------------------------- | ---------- |
| Receiving node | A stage it is sent with a leaf that reads a source                            | Decision 3 |
| Receiving node | A worker call with no client certificate, or one the authority did not sign   | Decision 4 |
| Receiving node | A worker call whose grant does not verify, has expired, or names another task | Decision 5 |
| Any node       | A stage over that node's working-set budget                                   | Decision 6 |
| Coordinator    | A question that lost a node mid-run                                           | Decision 7 |

A new refusal enum is enrolled in `check-refusal-coverage`, which reads only enrolled enums. A
refusal with one mode is a struct, which that gate does not read. Each refusal gets its own cell.

## Limits

- **The engine is young.** It is a contrib project, not part of Apache DataFusion. Its 4.0.0
  changed the codec signature a sutura plan would use. It lists no production users. Each
  DataFusion upgrade now also waits for its release.
- **A node in the middle of the combine sees that question's rows.** The grant stops a node from
  pulling a stream for a question it was not given. It does not hide the rows of the question the
  node was given. A node with the authority's certificate is trusted with what it is sent.
- **The grant does not recheck the subject's grants at a source.** The rows were read as the
  caller, by the node that read the source. The grant carries the subject so that audit lines on
  every node name it, not to authorize the read again.
- **One node's failure costs the whole question.** Nothing resumes a stage.
- **The working set is checked per node, not per question.** Several nodes each under budget can
  together hold more than one node's budget. That is the point, and it also means the question's
  total memory is bounded only by the number of nodes.
- **The leg size is an estimate.** It is Arrow's in-memory measure of the legs, which counts a
  buffer two arrays share twice, so it errs towards going across. A combine that ran out on one node
  is started again across, and the work it did there is lost.
- **Not measured.** The engine's throughput, its memory at a large hash join, and the extra round
  trips per stage.

## Options considered

- **Apache DataFusion Ballista** - not taken. It needs a central scheduler that keeps its state in
  memory, and it has no high availability. It writes shuffle rows to each executor's disk, which
  breaks the rule that no node stores rows. Its executors serve shuffle data without checking any
  per-question credential.
- **The caller's token exchanged once for an audience every node accepts** - not taken. A token
  leaked from one node works at every node.
- **The caller's token exchanged for each pair of nodes** - not taken. The number of exchanges grows
  with the square of the node count, and every pair becomes a second hop, which the leg transport
  record refuses.
- **A shuffle of our own on Flight `DoExchange`** - not taken. It means writing the stage planner,
  partitioning and exchange that the engine already has.
- **A larger single node** - not taken as the answer. It moves the bound and does not remove it.
  It stays the first thing an operator can do.

## Out of scope

- The operator, autoscaling and warm capacity (#828). They decide how many nodes there are, not how
  a question uses them.
- Certificate-bound tokens for remote legs. The leg transport record keeps that as a later
  decision. Mutual TLS here authenticates nodes to each other, not a caller's token.

## Consequences

- Cells for every refusal above. Also an exact differential, in process over three localhost nodes
  linked by mutual TLS: a question one node refuses for its working set is answered on three nodes,
  each with that same working set, with exactly what one node with a larger one answers. The
  multi-node VM venue proves the links between hosts, not a ceiling.
- A node certificate per node in both venues, and a client-certificate path on the listener.
- The `Cargo.toml` entry for the engine states why `sql` is off, as the `datafusion-federation`
  entry does.
