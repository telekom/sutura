# The chart's own gate: `helm` and `kubeconform`, nix-pinned, running against `charts/sutura`
# inside the sandbox - #149 branch 1's own text: "Tools (helm, kubeconform, and a unit-test
# plugin if one is used) are nix pins, per the rule that nix owns any tool whose version
# changes what it reports." `nix/reuse.nix`'s pattern: `runCommand`, not a crane derivation,
# because this reads no Rust and should not wait on the dependency closure.
#
# WHAT IT CHECKS, deliberately short of #149 branch 2's own scope (the golden matrix over
# several values files and several pinned Kubernetes versions - that is `build/chart-goldens`,
# not this branch):
#
#   1. `helm lint` - Chart.yaml/values.yaml well-formed. MEASURED NOT TO CATCH a broken
#      template: linting the chart with every required value left at its refusing default
#      still reports zero failures, because `helm lint` swallows a `fail()` inside a template
#      as an INFO line rather than a lint error. Kept anyway - it is what catches a malformed
#      Chart.yaml - but it is not this gate's only leg because of that gap.
#   2. `helm template` with NO values override - the refusal itself. `_helpers.tpl`'s `fail()`
#      calls mirror `Settings::refusals`, and this asserts the render exits non-zero rather
#      than assuming the guard clause still reads the value it is named for.
#   3. `helm template` with a complete values set, piped into `kubeconform` against the
#      Kubernetes API schemas vendored below - the only thing here that catches an apiVersion
#      that moved.
#
# NO NETWORK AT CHECK TIME. `kubeconform`'s default schema source is a URL fetch, which a nix
# sandbox refuses - so `schemas` below vendors the three schema files this chart's own Kinds
# need (Service, ConfigMap, Deployment) as fixed-output derivations, each pinned by content
# hash rather than by trusting the tag. `-strict` still passes against them - checked by
# rendering this chart's own complete-values output through kubeconform locally before this
# comment was written, not asserted from the tool's docs. `ServiceMonitor` is a Prometheus
# Operator CRD with no schema in this set; `-skip ServiceMonitor` is the documented answer
# for a CRD with no vendored schema, not a workaround for a hole in this pin.
{ pkgs, src }:

let
  schemaVersion = "v1.31.0-standalone-strict";
  # yannh/kubernetes-json-schema, pinned to one commit on its only branch (`master`) rather
  # than to a moving ref - the schema files at this path are self-contained ("standalone": no
  # external `$ref`), so three files cover this chart's three plain Kinds with nothing pulled
  # transitively.
  schemaCommit = "1360e239a56dcf2e5c7f99e61ccbaca1ea07036a";
  schemaFile = name: hash:
    pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/yannh/kubernetes-json-schema/${schemaCommit}/${schemaVersion}/${name}";
      inherit hash;
    };
  schemas = pkgs.runCommand "sutura-chart-schemas" { } ''
    mkdir -p "$out/${schemaVersion}"
    cp ${schemaFile "configmap-v1.json" "sha256-4Ord69Z3wIqgkrLaImTYasT8NO7RErn6wpRbPwDB6bE="} \
      "$out/${schemaVersion}/configmap-v1.json"
    cp ${schemaFile "deployment-apps-v1.json" "sha256-PjAI9mpfaM7jmESFrBiS2+3H8HKzqHBkEWurKUh06Z4="} \
      "$out/${schemaVersion}/deployment-apps-v1.json"
    cp ${schemaFile "service-v1.json" "sha256-9InWECZ1I4uROJjK9v729HJAOVD8nliV73GPPE8cQ1E="} \
      "$out/${schemaVersion}/service-v1.json"
  '';
in
{
  # Named here too, so `apps.helm`/`apps.kubeconform` (if either is ever added) cannot resolve
  # to a different version than this check runs.
  helm = pkgs.kubernetes-helm;
  kubeconform = pkgs.kubeconform;

  check = pkgs.runCommand "sutura-chart"
    {
      inherit src;
      nativeBuildInputs = [ pkgs.kubernetes-helm pkgs.kubeconform ];
    } ''
    # A writable directory: `$src` is the read-only store copy, so every intermediate file
    # below (helm's own render, the refusal capture) has to land beside it rather than inside.
    work="$PWD"
    chart="$src/charts/sutura"

    # From the derivation, not the caller's shell - the evidence this gate produces is about
    # the pin, not about whoever's PATH happened to run it.
    echo "pinned helm:       $(helm version --short)"
    echo "pinned kubeconform: $(kubeconform -v)"

    helm lint "$chart"

    if helm template "$chart" > /dev/null 2> "$work/refusal.log"; then
      echo "sutura-chart: rendered with no values set - Settings::refusals' fail() guard did not fire" >&2
      cat "$work/refusal.log" >&2
      exit 1
    fi
    grep -q "environment is required" "$work/refusal.log"

    helm template test-release "$chart" \
      --set environment=production \
      --set security.accessToken.secretName=sutura-access-token \
      --set security.tlsTermination=ingress \
      > "$work/rendered.yaml"

    kubeconform -strict -summary \
      -kubernetes-version 1.31.0 \
      -schema-location '${schemas}/{{ .NormalizedKubernetesVersion }}-standalone-strict/{{ .ResourceKind }}{{ .KindSuffix }}.json' \
      -skip ServiceMonitor \
      "$work/rendered.yaml"

    touch "$out"
  '';
}
