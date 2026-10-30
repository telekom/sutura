---
title: An operator sizes the pool of nodes that run a combine's stages
description: How the nodes that run a cross-node combine come and go on Kubernetes (#828). A Rust operator in this workspace keeps a pool of worker nodes sized to what the serving nodes report they need, plus a warm reserve. A chart value switches it on, the pool may shrink to no worker at all, and chart values bound it. It removes idle workers first, and it refuses a worker configuration that declares a source.
---

# An operator sizes the pool of nodes that run a combine's stages

Status: **proposed**, 2026-10-30. The owner chose an operator, the venue, the certificate
authorities, the pool shape, what a question does while workers are short, and a chart switch with
a pool that may be empty: the answers below. The rest is proposed here. Nothing is built or
compiled. This record decides; it proves nothing.

## The question

`docs/adr/20261030182759-shuffle-across-nodes.md` spreads one question's combine over several
nodes. The join then runs on nodes this deployment pays for, and how many of them exist at a given
moment decides whether a question that one node refuses for its working set can run at all.

The chart's autoscaler scales the serving Deployment on CPU and on questions in flight. It cannot
keep idle capacity ready, cannot tell which pod is idle when it scales down, and cannot check what
a pod is configured to hold. Who decides how many nodes run combine stages, which of them go away,
and what they may hold?

## The owner's answers

| Question                                      | Answer                                                                         |
| --------------------------------------------- | ------------------------------------------------------------------------------ |
| Q1. Does Kubernetes scaling need an operator? | Yes. The join runs on this deployment's own nodes, and an operator scales them |
| Q2. Where is it verified?                     | k3s inside a NixOS VM, gated by the paths of the chart and the operator        |
| Q3. Which authority issues node certificates? | nix in the VM venue, cert-manager in the k3s venue                             |
| Q4. What scales the serving nodes?            | The chart's autoscaler, on CPU and on questions in flight                      |
| Q5. Does the operator own the serving nodes?  | No, only the workers                                                           |
| Q6. A separate worker pool, or every node?    | A separate pool, with no source credential                                     |
| Q7. A question finds too few ready workers    | It waits for them, bounded, and is refused if they are not ready in time       |
| Q8. One certificate per worker or per pool?   | Per worker, from cert-manager's CSI driver                                     |
| Q9. How is the pool switched on and bounded?  | A chart value. The pool may scale to zero, within limits set in chart values   |

## Decision

1. **The operator is a Rust binary in this workspace**, on `kube` and `k8s-openapi`, with its own
   image. It shares the toolchain, the nix build and `cargo-deny` with the rest of the workspace.
   `sutura` does not link it, and nothing on the answer path depends on it.
2. **Two kinds of node, one binary.** A *serving* node is today's chart Deployment: it answers
   callers, reads sources and coordinates its questions. A *worker* runs combine stages and nothing
   else: no caller-facing route, no source, no catalog, and no source credential mounted. Both run
   the `sutura` image. The operator owns the workers. The serving nodes stay with the chart and its
   autoscaler.
3. **One namespaced resource, `SuturaWorkerPool`.** Its spec names the worker configuration, the
   image, the resources, and four numbers: `min`, `max`, `warm` and `idleAfter`. The operator
   renders a Deployment and a headless Service from it, and reports what it did in the resource's
   status.
4. **The size is demand plus the warm reserve, clamped to `min` and `max`.** Demand is a gauge each
   serving node exports on `/metrics`: the worker slots its cross-node questions hold or are about
   to ask for. The operator reads it from every serving pod with the metrics token. A sutura pod gets
   no Kubernetes API permission, so a serving node cannot create a pod. Surplus above the target is
   removed only after it has been idle for `idleAfter`.
5. **Idle workers go first.** Each worker exports its stages in flight. Before it lowers the
   replica count, the operator sets `controller.kubernetes.io/pod-deletion-cost` on each worker from
   that count, so the Deployment controller removes the idle ones. A worker asked to stop takes no
   new stage and finishes the ones it runs within its stop budget. Its
   `runtime.shutdown_grace_seconds` and the pod's termination grace period cover the longest
   question deadline, not the 15s default.
6. **Serving nodes find workers through the headless Service's DNS name**, resolved when a question
   is planned. Kubernetes drops a stopping pod from it. That settles the worker list the shuffle
   record left to the operator, with no API access from a sutura pod.
7. **A worker configuration is checked before it rolls out.** The operator parses it with
   `sutura-config`. A configuration that crate refuses, or one that declares a source or a catalog,
   is refused: the refusal text goes into the resource's status, and nothing rolls out. The serving
   nodes' configuration stays the chart's.
8. **No consensus.** The operator is a reconciler whose state lives in the Kubernetes API, which
   already runs its own consensus. One replica runs. While it is down, workers keep serving and only
   resizing stops, so there is no leader election until a second replica is wanted.
9. **Least privilege.** The operator's Role is namespaced: it manages Deployments and Services it
   owns, patches its own workers' annotations, reads the metrics token Secret, and writes its
   resource's status. Only the custom resource definition is cluster-scoped, and the chart installs
   it.
10. **Verified in the k3s VM venue.** The test that already scales the chart also installs the
    operator and one `SuturaWorkerPool`, and checks the warm reserve, scaling on demand from an empty
    pool and back to it, idle-first removal and a refused worker configuration. The operator's paths
    select it.
11. **A question waits for workers, bounded.** A cross-node question that finds fewer ready workers
    than it needs waits for them, up to a setting below its deadline, and its demand counts in the
    gauge while it waits. If they are not ready in time, it is refused, as a refusal the caller may
    retry.
12. **Each worker has its own certificate**, from cert-manager's CSI driver, issued by the
    deployment's authority. A leaked key names one pod. The cluster needs the driver installed.
13. **A chart value switches the pool on, and the pool may be empty.** Off, the default, the chart
    renders no resource definition, operator, Role or `SuturaWorkerPool`, and every question runs
    on one serving node as today; one that node refuses for its working set stays refused. On, the
    chart renders all four, with `min`, `max`, `warm`, `idleAfter` and the worker resources taken
    from its values. `min` and `warm` may be 0: no worker pod then runs until a serving node reports
    demand, and the pool returns to none after `idleAfter` without it. Only the operator's pod
    stays. The definition is a template under the switch, not a file in the chart's `crds/`
    directory, which Helm installs regardless of values and never upgrades. Switching off deletes
    it, and with it the pool and its workers.

## Open, with a proposal

| Question                                 | Proposal                                                        |
| ---------------------------------------- | --------------------------------------------------------------- |
| The resource's API group                 | A domain the project controls. None is named here               |
| Which release triples ship the operator? | The two Linux image triples only. It runs nowhere but a cluster |

## The refusals this adds

| Where        | Refused                                                           | Why         |
| ------------ | ----------------------------------------------------------------- | ----------- |
| Operator     | A worker configuration `sutura-config` refuses                    | Decision 7  |
| Operator     | A worker configuration that declares a source or a catalog        | Decision 7  |
| Operator     | A `SuturaWorkerPool` whose `min` is above its `max`               | Decision 4  |
| Serving node | A cross-node question whose workers are not ready within its wait | Decision 11 |

A new refusal enum is enrolled in `check-refusal-coverage`, and each refusal gets its own cell.

## Limits

- **Demand lags.** It is polled, and a new pod takes seconds to start. The warm reserve hides that
  only up to its size.
- **Deletion cost is a preference.** The Deployment controller honours it, and a stage can still
  start on a worker between the operator's read and the removal. The stop budget covers that stage
  only if it ends in time; otherwise its question fails, as the shuffle record's lost node does.
- **An empty pool starts cold.** With `min` and `warm` at 0, the first cross-node question waits
  for a pod to be scheduled, its image pulled and its certificate issued. A wait shorter than that
  refuses it, and so does a pool already at `max`.
- **A waiting question holds its serving node's slot.** It counts against that node's concurrency
  bound while it waits, so a burst of cross-node questions can fill a serving node before any
  worker is ready.
- **The operator trusts the serving nodes' gauge.** A serving node that reports too much grows the
  pool to `max` and no further. `max` is a cost bound, not a security one.
- **The operator reads the metrics token.** That token shows counters, not rows or credentials
  (`docs/adr/0015-an-authenticated-metrics-endpoint.md`).
- **The worker check reads the configuration, not the pod.** A volume added by hand to the
  Deployment is not seen until the operator renders it again.
- **Nothing is measured.** Not the reconcile period, the time from demand to a ready worker, or the
  operator's footprint.

## Options considered

- **The autoscaler on the worker Deployment, with a custom metric** - not taken. It needs a metrics
  pipeline and an adapter, keeps no warm reserve, and removes pods without knowing which are idle.
- **KEDA** - not taken. It scales a replica count on a metric without a pipeline, but it keeps no
  reserve above demand, removes pods without knowing which are idle, and cannot check a worker
  configuration. It is also one more component to install.
- **The Operator SDK's Helm operator** - not taken. It renders a chart from a resource and has no
  demand logic.
- **An operator in Go** - not taken. A second toolchain in the hermetic build and a second
  dependency audit.
- **Serving nodes claim workers through the Kubernetes API** - not taken. A pod that answers callers
  would hold a credential that creates resources.

## Out of scope

- The engine, the codec, the task grant and mutual TLS between nodes: the shuffle record.
- The serving nodes' autoscaler, which the chart already renders.

## Consequences

- A new binary crate, its image, and its `cargo-deny` review.
- Two gauges that do not exist yet: worker slots wanted on a serving node, stages in flight on a
  worker.
- A worker role in `sutura-config`, so that a worker's configuration is refused at startup if it
  declares a source, as well as by the operator.
- A chart switch and the pool's limits in the chart's values, with a chart refusal cell for each
  value it refuses.
- The k3s VM test grows the operator's cells, and the operator's paths join the gating area that
  selects it.
