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
The release installs the cluster-wide `SuturaWorkerPool` definition, in the API group `workerPool.apiGroup`.

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
