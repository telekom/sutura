# sutura

A Helm chart for `sutura serve` - the identity-aware semantic data runtime, over HTTP.

## What this chart is not

**Single player only.** Every query this deployment answers runs as one shared service-user
identity, whatever `config.base`/`config.environment` declare it to be. `security.inbound`
(off by default) establishes WHO is asking a question - leg 1, `docs/adr/0014` - and nothing
more: no adapter in the published binary can execute a query AS the caller (leg 2), so turning
`security.inbound` on narrows nothing about which rows a source returns. Do not read a
`ServiceAccount` per tenant, a `NetworkPolicy` per namespace or anything else here as
multi-tenant isolation - none of that changes what identity a source sees.

**Not an operator.** This chart deploys one `Deployment`; it does not reconcile a catalog or a
source declaration. Multiple replicas share nothing - no budget, no replay window - so scaling
`replicaCount` up gives N independent processes, not a shared ceiling on either. See
`docs/adr/0030` and `crates/sutura-http/src/inbound/token.rs` if you were about to assume
otherwise.

## What it configures, and how

The settings tree this chart feeds is `sutura-config`'s own: a directory of files, then
`SUTURA__*` environment variables, both layered on top of the binary's embedded defaults
(`crates/sutura-config/src/defaults.yaml`). This chart adds nothing to that surface - see
`values.yaml`'s own comments for what each key maps to and why some settings-tree keys (any
list or map: `security.inbound.algorithms`, `rate_limit.trusted_proxies`, `sources`,
`catalogs`) are only reachable through `config.base`, never through a chart value, because the
environment-variable layer here carries no list separator.

Every credential-shaped setting is a Kubernetes Secret, mounted or injected, never a literal in
`values.yaml`:

| Value                               | Settings key                                | Kubernetes shape                     |
| ----------------------------------- | ------------------------------------------- | ------------------------------------ |
| `security.accessToken`              | `security.access_token`                     | env, from a Secret key               |
| `security.metricsToken`             | `security.metrics_token`                    | env, from a Secret key               |
| `tls.secretName`                    | `server.tls_certificate` / `server.tls_key` | volume, a `kubernetes.io/tls` Secret |
| `security.inbound.keySetSecretName` | `security.inbound.key_set_file`             | volume, an opaque Secret             |

A source's own credential (`sources.<alias>.credential_file`, `password_file`,
`client_certificate`, `client_key`) has no dedicated value: point `extraVolumes` /
`extraVolumeMounts` at a Secret, then name the mount path in `config.base.sources.<alias>`.

## The refusals this chart mirrors before `kubectl apply` does

This chart's `Service` always makes the pod reachable off-host, which is exactly the shape
`crates/sutura-config/src/settings.rs`'s `Settings::refusals` refuses to start without a
declared TLS termination, an access token and an enabled rate limiter. `helm template` (and
`helm install`) fails the render with the same three checks - `templates/_helpers.tpl`'s
`sutura.requireOffHostPosture` - rather than shipping a `Deployment` that crash-loops on the
binary's own refusal five seconds later. This is a chart-side heuristic that reads only
`values.yaml`, not the binary's own check: it cannot see a Secret's contents or an environment
override file, so it is a lower bound on what will actually refuse, not a replacement for it.

## Probes

`GET /health` is the liveness path, and there is deliberately no readiness route - the boot
sequence re-verifies every catalog anchor before the listener opens, so an open port already
means a verified bundle (`docs/serving.md`, "What is not built"). This chart wires `/health` as
both the `startupProbe` (with a generous failure budget: anchor verification against a
networked source is a round trip per anchor) and the `livenessProbe`, and configures no
`readinessProbe`.

## Resource limits size the runtime

Setting `resources.limits.cpu`/`.memory` derives `runtime.engineWorkerThreads` /
`workingSetMaxBytes` when those are left empty - `templates/_helpers.tpl`'s
`sutura.engineWorkerThreads`/`workingSetMaxBytes`, exercised by
`testdata/values/resource-limits.yaml`. A CPU limit rounds up to a whole thread; a memory limit
keeps 75% of itself as the ceiling, leaving headroom for what that ceiling does not count
(the process's own RSS) so it trips before the kernel OOM-kills the container at the cgroup
limit - `docs/serving.md`'s "422 rather than 503" distinction. Setting either `runtime.*` key
directly always overrides the derivation.

## What is not in this branch

- No Ingress: this chart declares no opinion about how traffic reaches the cluster edge: set
  `security.tlsTermination: ingress` and bring your own.

## Distribution

Published as an OCI artefact on `ghcr.io`, beside the images - owner decision on #149, recorded
so it is not re-derived: same registry, same auth as `sutura`'s own images, no second
distribution surface. A tagged release pushes `sutura-<version>.tgz` (version = the release tag)
with the SAME pinned `helm` `checks.helm-chart` lints and renders this chart with, and signs the
pushed reference with `cosign` the way every image reference already is -
`.github/workflows/release.yml`.

```
helm install sutura oci://ghcr.io/telekom/charts/sutura --version <version>
```

**Artifact Hub indexes it for discovery; it does not host it.** Two things only a human with
console access can do, and CI cannot fake either:

1. Register `oci://ghcr.io/telekom/charts/sutura` as a repository in the Artifact Hub console,
   signed in as the owning account. That alone makes verified-publisher status available -
   nothing here needs to *claim* a repository nobody else has added.
2. Commit the `repositoryID` Artifact Hub then issues into `charts/sutura/artifacthub-repo.yml`
   (that key only - no `owners` block: `AGENTS.md` refuses a name or email in this public repo,
   and a console-registered repository has nothing to claim). Once that file exists,
   `release.yml` pushes it under the registry's `artifacthub.io` tag on every subsequent
   release; until it exists, that step logs why it did nothing and does not fail the release.
3. Make the `ghcr.io/telekom/charts` package publicly readable - private by default even for a
   public repository, and the most likely reason a first attempt looks broken.
