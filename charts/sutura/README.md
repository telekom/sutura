# sutura

sutura is a secure, multi-user way to chat with your data. It serves queries over HTTP and MCP.
By default, queries use certified metric definitions.
Data sources and semantic layers connect through adapters.
A query can combine data from several sources.

This chart deploys sutura.
It runs `sutura serve` over HTTP as one `Deployment`.
The chart also creates a `Service` and a settings `ConfigMap`.

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
