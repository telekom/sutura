---
title: An operator sizes the pool of nodes that run a combine's stages
description: How the nodes that run a cross-node combine come and go on Kubernetes (#828). A Rust operator in this workspace keeps a pool of worker nodes sized to what the serving nodes report they need, plus a warm reserve. A chart value switches it on, the pool may shrink to no worker at all, and chart values bound it. It removes idle workers first, and a worker refuses, at startup, a configuration that declares a source.
---

# An operator sizes the pool of nodes that run a combine's stages

Status: **accepted**, 2026-11-01. The owner chose an operator, the venue, the certificate
authorities, the pool shape, what a question does while workers are short, a chart switch with a
pool that may be empty, and on 2026-10-31 the resource's API group and how the operator ships: the
answers below. Decisions 1, 3, 7 and 9 were amended on 2026-11-01: the chart renders the workers,
and the operator only sizes them. Every decision is built: the operator and the pool's size (#1660,
#1665, #1667), the bounded wait (#1662), both gauges (#1663, #1664), the scrape over node TLS
(#1670), the chart switch (#1672) and the release (#1669). The k3s VM test (#1673) starts with no
worker, answers a one-system question with none, starts one for a federated question whose stage
reads over a node link, and returns to none when idle. It does not run a warm reserve, idle-first
removal or a refused worker configuration in the cluster. This record decides; it proves nothing.

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
| Q10. The resource's API group?                | A setting, defaulting to a name under the project's public GitHub Pages domain |
| Q11. How does the operator ship?              | As every shipped binary does, through the standard release                     |

## Decision

1. **The operator is a Rust binary in this workspace**, on `k8s-openapi`'s types and the
   workspace's `ureq` client, with its own image. It polls the API server with its pod's service
   account. It shares the toolchain, the nix build and `cargo-deny` with the rest of the workspace.
   `sutura` does not link it, and nothing on the answer path depends on it.
2. **Two kinds of node, one binary.** A *serving* node is today's chart Deployment: it answers
   callers, reads sources and coordinates its questions. A *worker* runs combine stages and nothing
   else: no caller-facing route, no source, no catalog, and no source credential mounted. Both run
   the `sutura` image. The operator owns the workers. The serving nodes stay with the chart and its
   autoscaler.
3. **One namespaced resource, `SuturaWorkerPool`.** Its spec names the worker Deployment, the
   serving Deployment whose demand it serves, and four numbers: `min`, `max`, `warm` and
   `idleAfter`. The chart renders the worker Deployment and its headless Service from the values it
   renders the serving nodes from, with the worker role. The operator sets that Deployment's replica
   count and reports what it did in the resource's status.
4. **The size is demand plus the warm reserve, clamped to `min` and `max`.** Demand is a gauge each
   serving node exports on `/metrics`: the worker slots its cross-node questions hold or are about
   to ask for. The operator reads it, with the metrics token, from every ready pod the serving
   Deployment's selector matches. A sutura pod gets no Kubernetes API permission, so a serving node
   cannot create a pod. Surplus above the target is removed only after it has been idle for
   `idleAfter`.
5. **Idle workers go first.** Each worker exports its stages in flight. Before it lowers the
   replica count, the operator sets `controller.kubernetes.io/pod-deletion-cost` on each worker from
   that count, so the Deployment controller removes the idle ones. A worker asked to stop takes no
   new stage and finishes the ones it runs within its stop budget. Its
   `runtime.shutdown_grace_seconds` and the pod's termination grace period cover the longest
   question deadline, not the 15s default.
6. **Serving nodes find workers through the headless Service's DNS name**, resolved when a question
   is planned. Kubernetes drops a stopping pod from it. That settles the worker list the shuffle
   record left to the operator, with no API access from a sutura pod.
7. **A worker refuses its own configuration.** At startup, a worker refuses a configuration that
   declares a source or a catalog, as `sutura-config` refuses any configuration a node cannot
   serve, and exits. The chart sets `terminationMessagePolicy: FallbackToLogsOnError`, so the
   refusal is the container's termination message. The pod never becomes ready, and with
   `maxUnavailable: 0` the rollout stalls while the old workers keep serving. The operator does not
   read the worker configuration. The serving nodes' configuration stays the chart's.
8. **No consensus.** The operator is a reconciler whose state lives in the Kubernetes API, which
   already runs its own consensus. One replica runs. While it is down, workers keep serving and only
   resizing stops, so there is no leader election until a second replica is wanted.
9. **Least privilege.** The operator's Role is namespaced and creates nothing: it reads its
   resources and writes their status, reads the scale of the serving and worker Deployments and
   writes the worker one's, by name, and lists pods and patches their annotations. The metrics token
   is mounted into its pod, not read through the API. Only the custom resource definition is
   cluster-scoped, and the chart installs it.

   **Amended 2026-11-01** (`github.com/telekom/sutura#1680` finding 4): `deployments/scale`'s `get`
   rule names both Deployments by `resourceNames` (`<fullname>` and `<fullname>-workers`), and its
   `patch` rule names only `<fullname>-workers`. The names come from `operator.workers`/
   `operator.serving`, two settings the chart sets and `sutura-operator run` refuses to start
   without. Naming the rule does not make the operator trust a `SuturaWorkerPool`'s own `spec`: it
   is refused, before either Deployment's scale is ever read, if it names anything other than those
   same two Deployments (`InvalidPool::ForeignWorkers`/`ForeignServing`, reported through the
   resource's existing `status.refusal` field). A pool refused before it was ever sized has no
   `status.replicas` yet, so the definition's status schema no longer requires that field - only
   `required: [spec]` applies to the resource as a whole - and the merge-patch writing `refusal`
   carries no invented replica count. No schema `default` is added either, which would let the API
   server invent a size that was never read. A pool refused after it was already sized keeps its
   last `status.replicas` alongside the new `refusal`, since the patch only ever sets fields it has
   a value for. The operator also holds only the
   CA bundle from its node certificate volume (`subPath: ca.crt`) and requests no `client auth` key
   usage on its own certificate: scraping a pod's `/metrics` only verifies that pod's server
   certificate against the CA, and the operator never presents one of its own as a client. A
   serverAuth-only certificate would in any case be refused if it were ever presented as a client
   one - every node's own TLS gate checks a client certificate's extended key usage as
   required-if-present. Per-worker certificates (Decision 12) are unaffected.
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
    renders no resource definition, operator, Role, `SuturaWorkerPool` or worker, and every question
    runs on one serving node as today; one that node refuses for its working set stays refused. On,
    the chart renders them all, with `min`, `max`, `warm`, `idleAfter` and the worker resources taken
    from its values. `min` and `warm` may be 0: no worker pod then runs until a serving node reports
    demand, and the pool returns to none after `idleAfter` without it. Only the operator's pod
    stays. The definition is a template under the switch, not a file in the chart's `crds/`
    directory, which Helm installs regardless of values and never upgrades. Switching off deletes
    it, and with it the pool and its workers.

14. **The resource's API group is a setting**, `operator.api_group` in the operator's sutura
    configuration, defaulting to `sutura.telekom.github.io`: a name under the project's public
    GitHub Pages domain. The chart renders the resource definition under the same value, and the
    operator addresses the resource by the group it starts with, not one fixed when it is compiled.
    A group that is not a lowercase DNS subdomain is refused at startup.
15. **The operator ships as every shipped binary does.** The release builds it at the same four
    triples, as a tarball and an image for each, with the same SBOMs and signatures. The chart's
    operator image is that release's.

## The refusals this adds

| Where        | Refused                                                                          | Why         |
| ------------ | -------------------------------------------------------------------------------- | ----------- |
| Worker       | A configuration `sutura-config` refuses, at startup                              | Decision 7  |
| Worker       | A configuration that declares a source or a catalog, at startup                  | Decision 7  |
| Operator     | A `SuturaWorkerPool` whose `min` is above its `max`                              | Decision 4  |
| Operator     | An API group that is not a lowercase DNS subdomain                               | Decision 14 |
| Operator     | A `SuturaWorkerPool` naming a Deployment it does not own (2026-11-01, finding 4) | Decision 9  |
| Serving node | A cross-node question whose workers are not ready within its wait                | Decision 11 |

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
- **A worker checks its configuration, not its pod.** A credential mounted into the worker
  Deployment by hand is not refused. The chart mounts none.
- **The chart leaves the worker count unset**, so an upgrade keeps the count the operator set. A
  new worker Deployment starts with one worker, removed after `idleAfter` if nothing asks for it.
- **Pod patches are namespace-wide, and a pod patch reaches more than its deletion cost.** RBAC
  cannot narrow `list`/`patch` on `pods` by `resourceNames` or a label selector, so the operator's
  token may list and patch any pod in its namespace, not only the workers'. The operator itself
  writes only `controller.kubernetes.io/pod-deletion-cost`, but the grant does not stop there: a pod
  patch can set any metadata (labels, annotations, owner references, finalizers) and, in spec, each
  container's or init container's `image`, `activeDeadlineSeconds`, add tolerations, and change
  `terminationGracePeriodSeconds` only from a negative value to `1`. Changing a
  container's image restarts that container under the pod's own service account and existing
  volume mounts, which can reach whatever source credential or node certificate that pod already
  holds; changing its labels can pull it out of its ReplicaSet's or Service's selector, detaching it
  from traffic or from the controller that would otherwise replace it. **(Amended 2026-11-01,
  finding 4)** `deployments/scale`, unlike `pods`, *is* narrowed: `get` names the serving and worker
  Deployments, `patch` names only the worker one, by `resourceNames`, which the API server itself
  enforces with a 403 - RBAC does not need the operator's cooperation for that. The operator
  additionally refuses, in its own reconcile loop, a `SuturaWorkerPool` naming any other Deployment:
  a second, independent check that keeps refusing and reporting through `status.refusal` even if the
  Role above is later widened to drop `resourceNames`, which `resourceNames` alone cannot do for
  itself.
- **The operator's own certificate holds no client identity - its RBAC token is a separate matter.**
  Mounting only `ca.crt` and requesting no `client auth` usage (above, Decision 9's amendment) means
  the operator's own pod can verify a scraped pod's server certificate but can never present that
  certificate as a client one to anything that checks for it; every node's own TLS gate would refuse
  it there regardless, since webpki checks a client certificate's extended key usage as
  required-if-present. This covers only the operator's own certificate. It says nothing about its
  RBAC token, which - through the pod-patch reach described above - can still restart some other
  pod's container under an image of the attacker's choosing, reaching that other pod's own node
  certificate indirectly. cert-manager's CSI driver also still issues a full key pair into the
  operator's own pod volume outside the running container regardless of which usages are requested;
  `subPath: ca.crt` only limits what the mounted file exposes, and a renewed certificate is not
  re-read through that `subPath` - acceptable here because the operator reads the authority once, at
  process start, not on every scrape.
- **The default group is a name, not a domain anyone checks.** Kubernetes does not ask whoever
  installs a definition to prove control of its group. The definition is cluster-scoped, so two
  releases in one cluster need different groups. Helm is expected to refuse the second install of a
  definition another release owns; that has not been run here.
- **Changing the group on an installed release is a new definition.** The pool under the old group
  and its workers go with the old one, as Decision 13's switch-off does.
- **Nothing is measured.** Not the reconcile period, the time from demand to a ready worker, or the
  operator's footprint.

## Options considered

- **The autoscaler on the worker Deployment, with a custom metric** - not taken. It needs a metrics
  pipeline and an adapter, keeps no warm reserve, and removes pods without knowing which are idle.
- **The operator renders the worker Deployment from a spec that carries the image, the
  configuration and the resources** - not taken. It repeats what the chart renders for the serving
  nodes, and it needs a configuration parse in the operator for a check the worker makes itself.
- **KEDA** - not taken. It scales a replica count on a metric without a pipeline, but it keeps no
  reserve above demand, removes pods without knowing which are idle, and cannot check a worker
  configuration. It is also one more component to install.
- **The Operator SDK's Helm operator** - not taken. It renders a chart from a resource and has no
  demand logic.
- **`kube`** - not taken. Its client brings an async stack and a second `k8s-openapi` version for
  the few requests a poll makes.
- **An operator in Go** - not taken. A second toolchain in the hermetic build and a second
  dependency audit.
- **Serving nodes claim workers through the Kubernetes API** - not taken. A pod that answers callers
  would hold a credential that creates resources.

## Out of scope

- The engine, the codec, the task grant and mutual TLS between nodes: the shuffle record.
- The serving nodes' autoscaler, which the chart already renders.

## Consequences

- A new binary crate, its image, and its `cargo-deny` review. The release workflow ships a second
  binary at every triple.
- Two gauges: worker slots wanted on a serving node, stages in flight on a
  worker.
- A worker role in `sutura-config`, so that a worker's configuration is refused at startup if it
  declares a source. The operator does not check it again.
- A chart switch and the pool's limits in the chart's values, with a chart refusal cell for each
  value it refuses.
- The k3s VM test grows the operator's cells, and the operator's paths join the gating area that
  selects it.
