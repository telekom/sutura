# This chart installed in a single-node k3s cluster inside a NixOS VM, scaled by its CPU autoscaler
# (`telekom/sutura#828`). k3s runs its own metrics-server from the pinned airgap images, and the
# image is `.#oci`, the one `kind-smoke` loads.
#
# What is observed: the HorizontalPodAutoscaler reads each pod's CPU, scales the Deployment from one
# replica to three while eight request loops run, and back to one after they stop. Each of the
# three replicas answers the fixture question exactly. A `helm upgrade` with the same values leaves
# the autoscaler's replica count alone, which it would not if the chart still rendered `replicas`.
#
# What this does not show: the questions-in-flight target, which needs a custom-metrics adapter
# nobody ships here. One node, so nothing about scheduling across nodes. The downscale
# stabilization window is 30s rather than Kubernetes' five minutes, so the test ends in minutes.
# The CPU request is 50m so that eight loops are enough load. Neither number is a production default.
{ pkgs, chartHelm, ociImage, chart, example }:
let
  # A file name ending in `.tar`, which is what k3s imports from its images directory.
  image = pkgs.runCommand "sutura-image.tar" { } "${ociImage} > $out";
  question = pkgs.writeText "question.json" (builtins.toJSON {
    metrics = [ "recurring_revenue" ];
    grain = "month";
    range = { start = "2026-06-01"; end = "2026-07-01"; };
  });
  values = pkgs.writeText "k3s-autoscaling-values.json" (builtins.toJSON {
    environment = "development";
    image = { repository = "sutura"; tag = "latest"; pullPolicy = "Never"; };
    security = {
      accessToken.secretName = "sutura-access-token";
      metricsToken.secretName = "sutura-metrics-token";
      tlsTermination = "sidecar";
    };
    resources.requests.cpu = "50m";
    autoscaling = { enabled = true; minReplicas = 1; maxReplicas = 3; targetCPUUtilizationPercentage = 50; };
    config.base = {
      catalogs = [{ name = "model"; kind = "markdown"; dir = "/srv/sutura/catalog"; data_dir = "/srv/sutura/data"; version = "k3s-autoscaling"; }];
      sources.local = { kind = "files"; data_dir = "/srv/sutura/data"; posture = "shared-service-user"; };
      security = { identity = "single-user"; single_user_because = "an autoscaling test reads the example's own fixture files as one identity"; };
      # Every load request arrives from the node's one address, so the default bucket would answer
      # most of them with 429 and the pods would not be busy.
      rate_limit = { api_per_second = 100000; api_burst = 100000; };
    };
    extraVolumes = [{ name = "example"; hostPath = { path = "${example}"; type = "Directory"; }; }];
    extraVolumeMounts = [{ name = "example"; mountPath = "/srv/sutura"; readOnly = true; }];
  });
  load = pkgs.writeShellScript "k3s-autoscaling-load" ''
    for _ in 1 2 3 4 5 6 7 8; do
      while :; do
        ${pkgs.curl}/bin/curl -s -o /dev/null -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' --data @${question} "http://$1:8080/v1/query" || :
      done &
    done
    wait
  '';
in
pkgs.testers.runNixOSTest {
  name = "k3s-autoscaling";

  nodes.machine = {
    virtualisation = { memorySize = 4096; cores = 4; diskSize = 8192; };
    # Pods reach the API server and metrics-server reaches the kubelet through the node's own address.
    networking.firewall.enable = false;
    environment.systemPackages = [ pkgs.kubectl chartHelm pkgs.curl ];
    environment.sessionVariables.KUBECONFIG = "/etc/rancher/k3s/k3s.yaml";
    services.k3s = {
      enable = true;
      role = "server";
      disable = [ "traefik" "servicelb" "local-storage" ];
      images = [ pkgs.k3s.airgap-images image ];
      extraFlags = [ "--kube-controller-manager-arg=horizontal-pod-autoscaler-downscale-stabilization=30s" ];
    };
  };

  testScript = ''
    import json
    import secrets
    import time

    EXPECTED = {"outcome": "answer", "columns": ["period", "recurring_revenue"], "rows": [["2026-06-01", "202121"]]}
    SELECTOR = "app.kubernetes.io/instance=sutura"

    def deployment(field):
        return machine.succeed(f"kubectl get deployment sutura -o jsonpath='{{.{field}}}'").strip()

    def ask(address):
        reply = json.loads(machine.succeed(
            f"curl -fsS --max-time 30 -H 'authorization: Bearer {access_token}' -H 'content-type: application/json' --data @${question} http://{address}:8080/v1/query"
        ))
        got = {key: reply.get(key) for key in EXPECTED}
        assert got == EXPECTED, f"{address} answered {got}"

    machine.wait_for_unit("k3s")
    machine.wait_until_succeeds("kubectl get serviceaccount default", timeout=300)
    machine.wait_until_succeeds("kubectl wait --for=condition=Available apiservice/v1beta1.metrics.k8s.io --timeout=10s", timeout=300)

    access_token = secrets.token_hex(32)
    machine.succeed(f"kubectl create secret generic sutura-access-token --from-literal=token={access_token}")
    machine.succeed(f"kubectl create secret generic sutura-metrics-token --from-literal=token={secrets.token_hex(32)}")

    try:
        machine.succeed("helm install sutura ${chart} -f ${values}")
        machine.succeed("kubectl rollout status deployment/sutura --timeout=300s")
        cluster_ip = machine.succeed("kubectl get service sutura -o jsonpath='{.spec.clusterIP}'").strip()

        with subtest("the autoscaler reads each pod's CPU before any load"):
            machine.wait_until_succeeds("kubectl get hpa sutura -o jsonpath='{.status.currentMetrics[0].resource.current.averageUtilization}' | grep -q .", timeout=300)
            assert deployment("spec.replicas") == "1"

        with subtest("under load it scales to three replicas, and each answers exactly"):
            started = time.monotonic()
            machine.succeed(f"systemd-run --unit=load --setenv=TOKEN={access_token} ${load} {cluster_ip}")
            machine.wait_until_succeeds("test \"$(kubectl get deployment sutura -o jsonpath='{.status.readyReplicas}')\" = 3", timeout=300)
            print(f"load start to three ready replicas: {time.monotonic() - started:.0f}s")
            pods = machine.succeed(f"kubectl get pods -l {SELECTOR} -o jsonpath='{{.items[*].status.podIP}}'").split()
            assert len(pods) == 3, pods
            for pod in pods:
                ask(pod)

        with subtest("helm upgrade leaves the autoscaler's replica count alone"):
            machine.succeed("helm upgrade sutura ${chart} -f ${values}")
            assert deployment("spec.replicas") == "3"

        with subtest("without load it scales back to one"):
            machine.succeed("systemctl stop load")
            machine.wait_until_succeeds("test \"$(kubectl get deployment sutura -o jsonpath='{.spec.replicas}')\" = 1", timeout=300)
    except Exception:
        print(machine.execute(f"kubectl describe hpa sutura; kubectl describe pods -l {SELECTOR}; kubectl logs -l {SELECTOR} --tail=100")[1])
        raise
  '';
}
