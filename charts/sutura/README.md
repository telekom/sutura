# sutura

sutura is a secure, multi-user way to chat with your data. It serves queries over HTTP and MCP.
By default, queries use certified metric definitions.
Data sources and semantic layers connect through adapters.
A query can combine data from several sources.

This chart deploys sutura.
It runs `sutura serve` over HTTP as one `Deployment`.
The chart also creates a dedicated `ServiceAccount`, a `Service`, and a settings `ConfigMap`.

## Service account

The chart creates a service account named after the release and sets
`automountServiceAccountToken: false`. This prevents the pod from receiving the namespace default
Kubernetes API token. To use a projected token for an external Google credential, add an explicit
`serviceAccountToken` volume and mount it with `extraVolumes` and `extraVolumeMounts`; the chart's
default is intentionally still no token. Set `serviceAccount.name` when using an existing account,
and set `serviceAccount.create: false` to keep the chart from creating one.

## Install

Install a released version of the chart from the OCI registry:

```sh
helm install sutura oci://ghcr.io/telekom/charts/sutura --version <version> -f values.yaml
```

The values file must set `environment` and `security.metricsToken.secretName`.
It must set `security.accessToken.secretName` unless `config.base` or `config.environment` sets
`security.inbound.mode`.
With `security.inbound.mode: direct`, it must not set an access token.
Without `security.inbound.mode`, it must set `security.identity: single-user` and a reason in
`security.single_user_because`, and no source may be `impersonation-at-source`.
To serve more than one user, declare `security.inbound` instead.
It must also declare TLS: set `security.tlsTermination`, or set `tls.enabled: true` with `tls.secretName`.
Each secret name refers to a Kubernetes Secret.
A `security.inbound` block must set `mode` to `direct` or `behind-gateway`.

## Image

The chart runs the musl image by default.
The tag is the app version with the suffix `-musl`.
Set `image.variant: glibc` to run the glibc image, which has no libc suffix.
Any other `image.variant` fails the render.
An upgrade from a chart that ran the glibc image changes the image: set `image.variant: glibc` to keep it.
`image.performance: true` adds `-performance` before the variant suffix.
`image.tag` sets the tag as it is.
Set `image.digest` to pin the exact image instead of a tag (github.com/telekom/sutura#1680
finding 7), as `sha256:` followed by 64 lower-case hex digits; the chart then renders
`image.repository@digest` and ignores `image.tag`, `image.variant` and `image.performance`. Any
other `image.digest` fails the render. `workerPool.operator.image.digest` pins the operator's image
the same way, independent of the app image's own tag or digest.

## Logs

`telemetry.format` is `bunyan` (default, one JSON object per line) or `pretty`.
`telemetry.filter` is a log filter directive, for example `info` (default) or `sutura=debug`.
The chart writes both to `base.yaml`.
A `telemetry` key in `config.base` or `config.environment` overrides them.
The chart does not check either value: sutura refuses to start with a bad one.

## Worker pool

`workerPool.enabled: true` adds workers for questions that read several data systems.
A question that reads one data system never uses them.
`sutura-operator` sizes the workers between `workerPool.min` and `workerPool.max`, from the worker slots the serving pods want.
With `min: 0` no worker runs until a question wants one.
The pool needs cert-manager and its CSI driver, and a CA issuer named in `workerPool.issuer`.
Every pod gets its own certificate from that issuer and ends TLS itself, so the chart refuses `tls.enabled` or a `security.tlsTermination` other than `in-process`.
A pod reads its certificate once, at start: keep `workerPool.certificateDuration` longer than a pod runs.
`workerPool.addressFamily` is `auto` (the default), `ipv4` or `ipv6`: which family of address the serving pod dials a worker's host name at, written as `server.nodes.address_family`.
`auto` takes one family per host, its IPv4 addresses when it has any, so a dual-stack pod is not two workers; `ipv6` suits an IPv6-only cluster.
The chart refuses any other spelling at render time.
It chooses among the addresses the name resolves to and does not make a worker reachable over a family the cluster network does not carry.
The release installs the cluster-wide `SuturaWorkerPool` definition, in the API group `workerPool.apiGroup`.
The operator's Role may `get` the serving and worker Deployments' `scale` by name and `patch` only the
worker's, both narrowed to those two names by `resourceNames`, which the API server itself enforces
(github.com/telekom/sutura#1680 finding 4); it also refuses, in its own reconcile loop, a
`SuturaWorkerPool` naming any other Deployment before reading either one's scale, a check that keeps
holding even if the Role is later widened. `list`/`patch` on `pods` cannot be scoped by `resourceNames`
or a label selector, so that grant reaches every pod in the release's namespace: any field a pod patch
can reach there, not only `controller.kubernetes.io/pod-deletion-cost`, which is all the operator
itself ever writes (see the ADR's Limits for what that allows). The operator's own pod mounts only the
CA bundle from its node certificate (`ca.crt`, via `subPath`) and requests no `client auth` usage: its
own certificate can verify a scraped pod's server certificate and can never be presented as a client
one - a property of that certificate, not of the RBAC token above
(`docs/adr/20261030193029-operator-sizes-the-worker-pool.md`, Limits).

### Node issuer

Any certificate signed by `workerPool.issuer` is treated as a node: the server checks the chain,
not the name, so a workload holding one of this issuer's certificates can run stages on every node
of this release. Dedicate the issuer to this release - never point two releases, or a workload this
release does not run, at the same CA. The chart refuses `workerPool.issuer.kind: ClusterIssuer`
unless `workerPool.issuer.acknowledgeSharedIssuer: true` is also set, because a `ClusterIssuer` can
sign for every namespace in the cluster. **Stated limit:** the chart cannot check that a namespaced
`Issuer` is dedicated to this release rather than shared by name with another release or hand-issued
elsewhere - it refuses only the `ClusterIssuer` case it can see from the values file. It also does
not check the certificate's Subject Alternative Name at the TLS layer; that check, if added, belongs
in the server (`crates/sutura-http/src/tls/authority.rs`), not in this chart.

## Reading workers

`readingWorkers.groups` adds workers that read data sources themselves, so a lookup leg of a question can be read in partitions on the nodes that read its source.
A group is a name and `sources`, `catalogs`, `replicas`, `resources`, `extraVolumes` and `extraVolumeMounts`.
Each group is a `Deployment` with `server.role: reading-worker`, a headless `Service` and a settings file, all named `<release>-reading-<group>`.
The serving pod names every group under `server.nodes`, with the source aliases the group reads.
A group needs `workerPool.enabled: true`: its certificate comes from `workerPool.issuer` and it ends TLS the way the workers do, so the same restrictions apply.

A group opens only the sources it names.
Its settings file holds the `sources` entries of those aliases, taken from `config.base` and `config.environment`, and `security.identity`, `security.single_user_because`, `security.inbound` and the group's own `catalogs`.
Nothing else of `config.base` or `config.environment` reaches it, so a setting a source depends on, such as `security.outbound`, is not carried.
The release's own `extraVolumes`, `extraVolumeMounts` and `extraEnv` never reach a group.
Mount a source's credential into the group that reads it with that group's `extraVolumes` and `extraVolumeMounts`, and point the source's `password_file` at the mount.
The group's pod mounts the key set of `security.inbound.keySetSecretName`, a public key set, as the serving pod does.

The chart refuses a render that the binary would refuse to start:

- no `security.inbound.mode`: a lookup leg a reading worker reads is granted for a verified caller;
- a group that names no source, or a source `sources` does not declare;
- a group with no `catalogs`, a group name that is not a DNS label, and `readingWorkers.groups` without `workerPool.enabled`.

Limits.
The chart deploys the nodes and names them; what a served question reads on them is the binary's, and [Serving over HTTP](https://telekom.github.io/sutura/latest/serving/) states it for each release.
`sutura-operator` sizes the combining workers only: `replicas` is the count of a group, and a `SuturaWorkerPool` does not name one.
The chart cannot check that a source's connection settings are right or that a group's mounted credential holds what the source reads: the group's pod refuses to start, or a question refuses, when they are not.
A group's `catalogs` is written out for the group and is not checked against the serving pod's own catalog.
The group keeps the release's `serviceAccount`, `nodeSelector`, `affinity` and `tolerations`.
The group's mounted Secret is readable by every process in the group's pod, which is why the credentials of a source belong in the group that reads it and in no other.
With `networkPolicy.enabled: true`, the reading workers accept their port from the serving pods, the combining workers and each other, as the workers do.
The chart does not restrict their egress to a data source.

## Network policy

`networkPolicy.enabled: true` adds `NetworkPolicy` objects for the serving, worker and operator
pods (github.com/telekom/sutura#1680 finding 6): workers accept their port only from this
release's serving pods and each other, the operator's egress is limited to DNS and the Kubernetes
API's common ports, and serving pods get an additive policy that keeps their port reachable even if
the namespace later adds a default-deny policy.
**Off by default** - the chart cannot tell whether the cluster's CNI enforces `NetworkPolicy`, or
whether something outside the chart's view (a service mesh, a cross-namespace ingress controller, a
scrape target) already depends on reachability these rules would remove. Judge against the
cluster's own network setup before turning it on; it does not restrict a serving pod's own egress,
or a worker's egress to a data source or a sibling worker, because this chart cannot know a data
source's address.

## Configure

Set every option in `values.yaml`.
Each section has a one-line comment.
The chart passes `config.base` and `config.environment` to sutura as its settings files.
The chart refuses an `extraEnv` name that sutura reads as a `security` setting, in any letter case, with `__`, `.` or `[` after `SUTURA__SECURITY`.
Set that setting in `config.environment`, or with `security.accessToken`, `security.metricsToken` or `security.inbound.keySetSecretName`.
The chart also refuses an `extraEnv` name that sutura reads as a variable the chart sets itself, such as `SUTURA_ENVIRONMENT`.

## Learn more

- [Configuration](https://telekom.github.io/sutura/latest/configuration/)
- [Serving over HTTP](https://telekom.github.io/sutura/latest/serving/)
- [Verify a release (images and chart)](https://telekom.github.io/sutura/latest/verifying-a-release/)
