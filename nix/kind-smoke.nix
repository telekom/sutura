# #149 branch 5: a `kind` cluster, this chart installed, one wait on the Deployment's own
# readiness, then exact query round-trips through its Service selection and each of two replicas.
# Named `apps.kind-smoke` in flake.nix.
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
# WHAT THIS PROVES, AND WHAT IT DOES NOT. It proves the chart this repository ships becomes a
# Ready two-replica Deployment on a real Kubernetes control plane, serving `examples/single-player`'s
# catalog and data over one `files` source - so boot got past `Settings::refusals`, loaded a
# catalog and passed the `/health` startup probe. The binary refuses to boot with no catalog
# documents, an access token under 32 characters, or no metrics token off-host, so a smoke
# without all three measures a crash loop, not the chart. An exact answer is checked through a
# Service port-forward and each pod separately. A Service port-forward selects a pod; it does not
# measure the cluster Service dataplane. These replicas compute independently over fixture files:
# this proves neither a distributed combine nor inter-node identity, network policy or mTLS.
{ pkgs, chartHelm, ociImage }:

pkgs.writeShellApplication {
  name = "sutura-kind-smoke";
  runtimeInputs = [ pkgs.kind pkgs.kubectl chartHelm pkgs.coreutils pkgs.findutils pkgs.curl pkgs.jq pkgs.gnused ];
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
    forward_pid=
    trap '[ -z "$forward_pid" ] || kill "$forward_pid" 2>/dev/null || true; kind delete cluster --name "$cluster" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

    echo "sutura-kind-smoke: loading the native image into docker"
    "${ociImage}" > "$work/oci.tar"
    docker load -i "$work/oci.tar"

    echo "sutura-kind-smoke: creating cluster $cluster"
    kind create cluster --name "$cluster" --wait 120s
    kind load docker-image sutura:latest --name "$cluster"

    # Two distinct random tokens: the binary refuses one under 32 characters, and refuses a
    # metrics token equal to the access token.
    token() { od -An -tx1 -N32 /dev/urandom | tr -d ' \n'; }
    access_token="$(token)"
    kubectl create secret generic sutura-access-token --from-literal=token="$access_token"
    kubectl create secret generic sutura-metrics-token --from-literal=token="$(token)"

    # A ConfigMap is flat, so each file's relative path becomes its key with `/` spelled `__`,
    # and the volume's `items` put it back at that path under the mount.
    fixture="$root/examples/single-player"
    values="$work/values.yaml"
    cat > "$values" <<'YAML'
    config:
      base:
        catalogs:
          - name: model
            kind: markdown
            dir: /srv/sutura/catalog
            data_dir: /srv/sutura/data
            version: kind-smoke
        sources:
          local:
            kind: files
            data_dir: /srv/sutura/data
            posture: shared-service-user
        security:
          identity: single-user
          single_user_because: "a smoke test reads the example's own fixture files as one identity"
    extraVolumeMounts:
      - name: smoke-catalog
        mountPath: /srv/sutura/catalog
        readOnly: true
      - name: smoke-data
        mountPath: /srv/sutura/data
        readOnly: true
    extraVolumes:
    YAML
    for part in catalog data; do
      files=()
      while IFS= read -r rel; do
        files+=("--from-file=''${rel//\//__}=$fixture/$part/$rel")
      done < <(cd "$fixture/$part" && find . -type f -printf '%P\n' | sort)
      kubectl create configmap "sutura-smoke-$part" "''${files[@]}"
      printf '  - name: smoke-%s\n    configMap:\n      name: sutura-smoke-%s\n      items:\n' "$part" "$part" >> "$values"
      (cd "$fixture/$part" && find . -type f -printf '%P\n' | sort) | while IFS= read -r rel; do
        printf '        - key: %s\n          path: %s\n' "''${rel//\//__}" "$rel" >> "$values"
      done
    done

    echo "sutura-kind-smoke: installing the chart"
    started_at=$(date +%s)
    helm install sutura "$root/charts/sutura" \
      -f "$values" \
      --set replicaCount=2 \
      --set environment=development \
      --set image.repository=sutura \
      --set image.tag=latest \
      --set image.pullPolicy=Never \
      --set security.accessToken.secretName=sutura-access-token \
      --set security.metricsToken.secretName=sutura-metrics-token \
      --set security.tlsTermination=sidecar

    echo "sutura-kind-smoke: waiting for the deployment to become ready"
    if ! kubectl rollout status deployment/sutura --timeout=180s; then
      echo "sutura-kind-smoke: not ready - the pod's state and its own output follow" >&2
      selector=app.kubernetes.io/instance=sutura
      kubectl describe pods -l "$selector" >&2 || true
      kubectl logs -l "$selector" --tail=200 >&2 || true
      kubectl logs -l "$selector" --tail=200 --previous >&2 || true
      exit 1
    fi

    # Exact fixture output, shared by the Service-selected pod and both directly selected pods.
    # The forwarded socket is loopback-only; the access token is passed in a header, never a URL.
    question='{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
    expected='{"outcome":"answer","columns":["period","recurring_revenue"],"rows":[["2026-06-01","202121"]]}'
    ask() {
      local resource=$1 port="" actual=""
      : > "$work/forward.log"
      kubectl port-forward --address=127.0.0.1 "$resource" :8080 >> "$work/forward.log" 2>&1 &
      forward_pid=$!
      for _ in $(seq 1 100); do
        port=$(sed -n 's/^Forwarding from 127\.0\.0\.1:\([0-9]*\) ->.*/\1/p' "$work/forward.log")
        [ -z "$port" ] || break
        kill -0 "$forward_pid" 2>/dev/null || break
        sleep 0.1
      done
      if [ -z "$port" ]; then
        cat "$work/forward.log" >&2
        echo "sutura-kind-smoke: $resource did not open its forwarded port" >&2
        exit 1
      fi
      actual=$(curl --silent --show-error --fail --max-time 30 \
        --header "authorization: Bearer $access_token" \
        --header 'content-type: application/json' \
        --data "$question" "http://127.0.0.1:$port/v1/query" | \
        jq --compact-output '{outcome, columns, rows}')
      if [ "$actual" != "$expected" ]; then
        printf 'sutura-kind-smoke: %s expected %s, got %s\n' "$resource" "$expected" "$actual" >&2
        exit 1
      fi
      kill "$forward_pid"
      wait "$forward_pid" 2>/dev/null || true
      forward_pid=
      echo "sutura-kind-smoke: $resource answered with the expected rows"
    }

    ask service/sutura
    echo "sutura-kind-smoke: chart install to first answer: $(( $(date +%s) - started_at )) seconds"
    mapfile -t pods < <(kubectl get pods -l app.kubernetes.io/instance=sutura -o name)
    if [ "''${#pods[@]}" -ne 2 ]; then
      echo "sutura-kind-smoke: expected two replicas, found ''${#pods[@]}" >&2
      exit 1
    fi
    for pod in "''${pods[@]}"; do
      ask "$pod"
    done
    echo "sutura-kind-smoke: both replicas answered exactly - tearing down"
  '';
}
