# sutura

This chart deploys sutura, an identity-aware semantic data runtime.
It runs `sutura serve` over HTTP as one `Deployment`.
The chart also creates a `Service` and a settings `ConfigMap`.

## Install

Install a released version of the chart from the OCI registry:

```sh
helm install sutura oci://ghcr.io/telekom/charts/sutura --version <version> -f values.yaml
```

The values file must set `environment`, `security.accessToken.secretName`,
`security.metricsToken.secretName` and `security.tlsTermination`.
Each secret name refers to a Kubernetes Secret.

## Configure

Set every option in `values.yaml`.
Each key has a one-line comment.
The chart passes `config.base` and `config.environment` to sutura as its settings files.

## Learn more

- [Configuration](https://telekom.github.io/sutura/latest/configuration/)
- [Serving over HTTP](https://telekom.github.io/sutura/latest/serving/)
- [Verify a release (images and chart)](https://telekom.github.io/sutura/latest/verifying-a-release/)
