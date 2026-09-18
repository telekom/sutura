# The chart's own gate: `helm` and `kubeconform`, nix-pinned, running against `charts/sutura`
# inside the sandbox - #149 branch 1's own text: "Tools (helm, kubeconform, and a unit-test
# plugin if one is used) are nix pins, per the rule that nix owns any tool whose version
# changes what it reports." `nix/reuse.nix`'s pattern: `runCommand`, not a crane derivation,
# because this reads no Rust and should not wait on the dependency closure.
#
# WHAT IT CHECKS - #149 branch 2 folded into branch 1's own derivation rather than a second
# check, because it is the same tools over the same chart and `just ci`'s loop names checks by
# job, not by branch:
#
#   1. `helm lint` - Chart.yaml/values.yaml well-formed. MEASURED NOT TO CATCH a broken
#      template: linting the chart with every required value left at its refusing default
#      still reports zero failures, because `helm lint` swallows a `fail()` inside a template
#      as an INFO line rather than a lint error. Kept anyway - it is what catches a malformed
#      Chart.yaml - but it is not this gate's only leg because of that gap.
#   2. `helm template` with NO values override - the refusal itself, and its wording pinned as
#      a golden (`testdata/golden/refusal-no-values.txt`): `_helpers.tpl`'s `fail()` calls
#      mirror `Settings::refusals`, and this asserts the render exits non-zero AND that the
#      message an operator reads has not silently changed, rather than only checking the guard
#      still fires.
#   3. `helm template` over every values file in `charts/sutura/testdata/values/`, each
#      compared against its committed golden in `testdata/golden/` - `diff -u`, so a mismatch
#      prints the reviewable diff a human would see, not a hash. **This is the coverage that
#      matters**: `helm lint` cannot see a broken template (point 1), so a silently changed
#      render is caught here or nowhere. Every values file needs a golden and every golden
#      needs a values file - an orphan either way is this gate's own failure, not the chart's.
#   4. Each of those renders, piped into `kubeconform` against the Kubernetes API schemas
#      vendored below, once per pinned version - the only thing here that catches an
#      apiVersion that moved.
#
# NO NETWORK AT CHECK TIME. `kubeconform`'s default schema source is a URL fetch, which a nix
# sandbox refuses - so `schemas` below vendors the three schema files this chart's own Kinds
# need (Service, ConfigMap, Deployment) as fixed-output derivations, each pinned by content
# hash rather than by trusting the tag. `-strict` still passes against them - checked by
# rendering this chart's own complete-values output through kubeconform locally before this
# comment was written, not asserted from the tool's docs. `ServiceMonitor` is a Prometheus
# Operator CRD with no schema in this set; `-skip ServiceMonitor` is the documented answer
# for a CRD with no vendored schema, not a workaround for a hole in this pin.
#
# TWO KUBERNETES VERSIONS, not several: neither Kind this chart renders (`apps/v1` since 1.9,
# `v1` Service/ConfigMap since 1.0) has moved its `apiVersion` across any version this chart
# could plausibly target, so a wider matrix would multiply the schema pins without multiplying
# what they can catch. 1.29.0 and 1.31.0 bracket the range - the older widely-run minor and the
# one branch 1 already pinned - so a future move inside it is caught rather than assumed away;
# widen this table, not the reasoning, the day a template starts reading `.Capabilities`.
{ pkgs, src }:

let
  # yannh/kubernetes-json-schema, pinned to one commit on its only branch (`master`) rather
  # than to a moving ref - the schema files at each of these paths are self-contained
  # ("standalone": no external `$ref`), so three files per version cover this chart's three
  # plain Kinds with nothing pulled transitively.
  schemaCommit = "1360e239a56dcf2e5c7f99e61ccbaca1ea07036a";
  schemaFile = version: name: hash:
    pkgs.fetchurl {
      url = "https://raw.githubusercontent.com/yannh/kubernetes-json-schema/${schemaCommit}/${version}/${name}";
      inherit hash;
    };
  schemas = pkgs.runCommand "sutura-chart-schemas" { } ''
    mkdir -p "$out/v1.29.0-standalone-strict" "$out/v1.31.0-standalone-strict"
    cp ${schemaFile "v1.29.0-standalone-strict" "configmap-v1.json" "sha256-HbLE/ss00c9MWpW464XEWmxdoXAKOzmS9v6hyj+RhbA="} \
      "$out/v1.29.0-standalone-strict/configmap-v1.json"
    cp ${schemaFile "v1.29.0-standalone-strict" "deployment-apps-v1.json" "sha256-QVlNmJIY2/4SGQszLrJtzBskdK2SDo0VDXYwoND7fZ4="} \
      "$out/v1.29.0-standalone-strict/deployment-apps-v1.json"
    cp ${schemaFile "v1.29.0-standalone-strict" "service-v1.json" "sha256-N4ivpQcOIs/rPkjQJWLju3HM17Yy2EuUpQWgW6sav0w="} \
      "$out/v1.29.0-standalone-strict/service-v1.json"
    cp ${schemaFile "v1.31.0-standalone-strict" "configmap-v1.json" "sha256-4Ord69Z3wIqgkrLaImTYasT8NO7RErn6wpRbPwDB6bE="} \
      "$out/v1.31.0-standalone-strict/configmap-v1.json"
    cp ${schemaFile "v1.31.0-standalone-strict" "deployment-apps-v1.json" "sha256-PjAI9mpfaM7jmESFrBiS2+3H8HKzqHBkEWurKUh06Z4="} \
      "$out/v1.31.0-standalone-strict/deployment-apps-v1.json"
    cp ${schemaFile "v1.31.0-standalone-strict" "service-v1.json" "sha256-9InWECZ1I4uROJjK9v729HJAOVD8nliV73GPPE8cQ1E="} \
      "$out/v1.31.0-standalone-strict/service-v1.json"
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
    testdata="$chart/testdata"

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
    if ! diff -u "$testdata/golden/refusal-no-values.txt" "$work/refusal.log"; then
      echo "sutura-chart: the no-values refusal's own wording moved - see the diff above." \
        "Update testdata/golden/refusal-no-values.txt if the new wording is deliberate." >&2
      exit 1
    fi

    kubeVersions="1.29.0 1.31.0"
    failed=0

    for valuesFile in "$testdata"/values/*.yaml; do
      name="$(basename "$valuesFile" .yaml)"
      golden="$testdata/golden/$name.yaml"
      if [ ! -f "$golden" ]; then
        echo "sutura-chart: $valuesFile has no golden at $golden" >&2
        failed=1
        continue
      fi
      rendered="$work/$name.rendered.yaml"
      helm template test-release "$chart" -f "$valuesFile" > "$rendered"
      if ! diff -u "$golden" "$rendered"; then
        echo "sutura-chart: $name's render drifted from its golden (diff above)." \
          "Re-render and update testdata/golden/$name.yaml if this is deliberate." >&2
        failed=1
        continue
      fi
      for kv in $kubeVersions; do
        kubeconform -strict -summary \
          -kubernetes-version "$kv" \
          -schema-location '${schemas}/{{ .NormalizedKubernetesVersion }}-standalone-strict/{{ .ResourceKind }}{{ .KindSuffix }}.json' \
          -skip ServiceMonitor \
          "$rendered"
      done
    done

    # The other direction: a golden with no values file left it orphaned rather than removed.
    for goldenFile in "$testdata"/golden/*.yaml; do
      name="$(basename "$goldenFile" .yaml)"
      if [ ! -f "$testdata/values/$name.yaml" ]; then
        echo "sutura-chart: $goldenFile has no matching values file at testdata/values/$name.yaml" >&2
        failed=1
      fi
    done

    test "$failed" -eq 0
    touch "$out"
  '';
}
