# #149 branch 5: a `kind` cluster, this chart installed, one wait on the Deployment's own
# readiness, then teardown - the smallest honest version of "does the published chart actually
# deploy", named `apps.kind-smoke` in flake.nix.
#
# NOT A NIX CHECK. `kind` needs a container runtime for its own node backend and a real network
# namespace to bind the cluster's API server on, and a build sandbox has neither - the same
# argument `apps.bigquery-acceptance` makes for needing network. So this follows that app's
# shape: `nix run` puts the pinned tools on PATH and a CI job invokes the result directly, on a
# runner that has docker - `demo-container.yml`'s job, not `checks.*`.
#
# `kind` and `kubectl` are nix-pinned because their versions decide what a cluster looks like -
# `kind`'s own node image pins a Kubernetes minor version, and a bump changes what gets tested
# without anyone editing this file. `helm` is the SAME pinned binary `checks.helm-chart`
# (`nix/helm-chart.nix`) already lints and renders the chart with, passed in rather than taken
# from `pkgs` a second time. `docker` is deliberately NOT pinned, on the rule the release and
# demo workflows already follow: it is the runner's own daemon, not a build input.
#
# WHAT THIS PROVES, AND WHAT IT DOES NOT. It proves the chart this repository ships actually
# becomes a Ready Deployment on a real Kubernetes control plane, once - the smallest honest
# version the issue asked for. It does NOT answer a question through the Service the way
# `examples/single-player` would: that needs the catalog's data mounted, a working config.base,
# and a real query round trip, which is a materially bigger fixture than "does it come up", and
# building that here would be shipping a smaller version of THAT ambition silently rather than
# saying so - so it is left for whoever picks that up next.
{ pkgs, chartHelm, ociImage }:

pkgs.writeShellApplication {
  name = "sutura-kind-smoke";
  runtimeInputs = [ pkgs.kind pkgs.kubectl chartHelm ];
  text = ''
    if ! command -v docker >/dev/null 2>&1; then
      echo "sutura-kind-smoke: no docker on PATH - kind's own node backend needs a container runtime, the same one demo-container.yml assumes. Run this where docker is available." >&2
      exit 1
    fi

    # `charts/sutura` resolved by walking up from the working directory, on `apps.fuzz`'s idiom:
    # `$0` cannot be used here either, since `writeShellApplication` runs from the nix store.
    root="$PWD"
    while [ ! -f "$root/charts/sutura/Chart.yaml" ]; do
      parent="$(dirname "$root")"
      if [ "$parent" = "$root" ]; then
        echo "sutura-kind-smoke: no charts/sutura/Chart.yaml at or above $PWD - run this from inside the repository" >&2
        exit 1
      fi
      root="$parent"
    done

    work="$(mktemp -d)"
    cluster="sutura-smoke-$$"
    # Own kubeconfig, never the caller's `~/.kube/config`: a smoke test that quietly rewrote a
    # developer's default context would be a surprising thing for this app to do.
    export KUBECONFIG="$work/kubeconfig"
    trap 'kind delete cluster --name "$cluster" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

    echo "sutura-kind-smoke: loading the native image into docker"
    "${ociImage}" > "$work/oci.tar"
    docker load -i "$work/oci.tar"

    echo "sutura-kind-smoke: creating cluster $cluster"
    kind create cluster --name "$cluster" --wait 120s
    kind load docker-image sutura:latest --name "$cluster"

    kubectl create secret generic sutura-access-token --from-literal=token=smoke-token

    echo "sutura-kind-smoke: installing the chart"
    helm install sutura "$root/charts/sutura" \
      --set environment=development \
      --set image.repository=sutura \
      --set image.tag=latest \
      --set image.pullPolicy=Never \
      --set security.accessToken.secretName=sutura-access-token \
      --set security.tlsTermination=sidecar

    echo "sutura-kind-smoke: waiting for the deployment to become ready"
    kubectl rollout status deployment/sutura --timeout=180s

    echo "sutura-kind-smoke: sutura became ready on kind - tearing down"
  '';
}
