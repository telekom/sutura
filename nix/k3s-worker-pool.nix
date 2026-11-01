# This chart's worker pool in a single-node k3s cluster inside a NixOS VM, sized by `sutura-operator`
# (`telekom/sutura#828`). cert-manager and its CSI driver run from pinned images, and a CA issuer they
# bootstrap signs every pod's node certificate. The images are `.#oci` and `.#oci-operator`; the
# operator's is retagged into `.#oci`'s repository, the one `image.repository` names for both.
#
# The catalog is the example's with `customers` moved onto a second `files` source over the same
# data, the rewrite `nix/multi-node.nix` makes, so recurring revenue by region reads two data
# systems. The leg size above which a question goes across is one byte.
#
# What is observed: the release starts with no worker. A question one data system answers whole is
# answered at once, with none. The federated question waits, the operator reads the serving pod's
# demand and starts a worker, and the answer equals the one-source golden. The worker then holds a
# link it dialled to a node's listener, which a node dials only to read the input of a stage it was
# sent. Idle, the pool goes back to no worker, and the pool's status reports each count.
#
# What this does not show: one node, so the worker shares the serving pod's machine and nothing is
# scheduled across machines. One question at a time: nothing measures load, the ceiling, or a worker
# going away mid-question. The idle window is 60s rather than the chart's 300s so the test ends in
# minutes. Nobody signs in - the deployment is single-user.
{ pkgs, chartHelm, ociImage, operatorImage, chart, example, golden }:
let
  # File names ending in `.tar`, which is what k3s imports from its images directory.
  image = pkgs.runCommand "sutura-image.tar" { } "${ociImage} > $out";
  operator = pkgs.runCommand "sutura-operator-image.tar" { } "${operatorImage} > $out";
  jetstack = name: tag: imageDigest: hash: pkgs.dockerTools.pullImage {
    imageName = "quay.io/jetstack/${name}";
    inherit imageDigest hash;
    finalImageTag = tag;
  };
  certManagerImages = [
    (jetstack "cert-manager-controller" "v1.21.3" "sha256:9f4f96a33c7774cef24466fcd85031997531a03a8d117f151126baa8d1290dcc" "sha256-P2gawOAOZPAICFXzRKhdlWvsk+LX52tOzQOSsCMbmhQ=")
    (jetstack "cert-manager-webhook" "v1.21.3" "sha256:055ac8e9de6a0ba6763794838fe38944acc650f68ebe1b01dd8be079e580f458" "sha256-uYEkHcw+YLilkRkvuoyjFTiWY4SBwh+p/Tv+VC5Tq38=")
    (jetstack "cert-manager-cainjector" "v1.21.3" "sha256:083d0e906f8eb44e255fe4ff817ae54e49f9ed82e22f1011bb4e9e4f4b8a641f" "sha256-t1nsNeypTkr/Y1yYo5I2NOH/m4WaAOYiJBdL7ujROzw=")
    (jetstack "cert-manager-csi-driver" "v0.16.1" "sha256:00493ba0324911cd93db80e9012076931053100b4d53bedd5fb574fc4c6aca8b" "sha256-bioKyXaN/4bcAqL/J5BlHVYkYF41QsxwWymHPpsOUco=")
    (jetstack "csi-node-driver-registrar" "v2.18.0" "sha256:b7fefd08651f00ac4df1a196ed4c621c2451fbb899297fe9fad9e36c667af0c0" "sha256-PhJvFFrOU3rruolaWvLTiPa43j/OQq88MNvkVw0QWC0=")
    (jetstack "livenessprobe" "v2.20.0" "sha256:19f2cf2f40e1987c7943945deca5f65cd4e4bf5cc0657fc78a5df131861578e8" "sha256-hx9cvHJ9w76H2uhoY0PQob+J9xCFhCl2oY1deR3Sob4=")
  ];
  certManager = pkgs.fetchurl {
    url = "https://github.com/cert-manager/cert-manager/releases/download/v1.21.3/cert-manager.yaml";
    hash = "sha256-wEpRKVrdorGndRRnUflz54cZB3jH81GAhh2WeIduXHQ=";
  };
  csiDriver = pkgs.fetchurl {
    url = "https://charts.jetstack.io/charts/cert-manager-csi-driver-v0.16.1.tgz";
    hash = "sha256-bPS8HsM7DAiQqlS+yhQkLezs/sI3FPOtGdUoneuWpqk=";
  };
  # A self-signed issuer signs the CA certificate, and the CA issuer the chart names signs each pod's.
  issuer = pkgs.writeText "k3s-worker-pool-issuer.json" (builtins.toJSON {
    apiVersion = "v1";
    kind = "List";
    items = [
      { apiVersion = "cert-manager.io/v1"; kind = "Issuer"; metadata.name = "bootstrap"; spec.selfSigned = { }; }
      {
        apiVersion = "cert-manager.io/v1";
        kind = "Certificate";
        metadata.name = "sutura-nodes-ca";
        spec = {
          isCA = true;
          commonName = "sutura-nodes";
          secretName = "sutura-nodes-ca";
          privateKey = { algorithm = "ECDSA"; size = 256; };
          issuerRef = { name = "bootstrap"; kind = "Issuer"; };
        };
      }
      { apiVersion = "cert-manager.io/v1"; kind = "Issuer"; metadata.name = "sutura-nodes"; spec.ca.secretName = "sutura-nodes-ca"; }
    ];
  });

  # `nix/multi-node.nix`'s rewrite: a block that no longer matches fails the build.
  chained = "  - name: sales_area\n    column: sales_area\n    via: [subscription_customer, customer_region]\n    values: [central, north_east, south_west]\n    description: >\n      Which sales area the customer's region rolls up into. The one dimension here\n      reached through a chain of two relationships rather than one.\n";
  catalog = pkgs.runCommand "k3s-worker-pool-catalog" { } ''
    cp -r ${example}/catalog "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/models/customers.md" --replace-fail 'source: local' 'source: sales'
    substituteInPlace "$out/metrics/recurring_revenue.md" --replace-fail ${pkgs.lib.escapeShellArg chained} ""
    rm "$out/knowledge/caveats/sales-area-as-of-today.md"
  '';
  files = { kind = "files"; data_dir = "/srv/sutura/data"; posture = "shared-service-user"; };
  values = pkgs.writeText "k3s-worker-pool-values.json" (builtins.toJSON {
    environment = "development";
    image = { repository = "sutura"; tag = "latest"; pullPolicy = "Never"; };
    security = {
      accessToken.secretName = "sutura-access-token";
      metricsToken.secretName = "sutura-metrics-token";
    };
    workerPool = {
      enabled = true;
      issuer.name = "sutura-nodes";
      min = 0;
      max = 2;
      idleAfterSeconds = 60;
      acrossAboveBytes = "1";
      # Long enough for the operator's next pass and a worker's start.
      waitForWorkersSeconds = 90;
      operator.image.tag = "operator";
    };
    config.base = {
      server.request_timeout_seconds = 120;
      catalogs = [{ name = "model"; kind = "markdown"; dir = "/srv/sutura/catalog"; data_dir = "/srv/sutura/data"; version = "k3s-worker-pool"; }];
      sources = { local = files; sales = files; };
      security = { identity = "single-user"; single_user_because = "a worker pool test reads the example's own fixture files as one identity"; };
    };
    extraVolumes = [
      { name = "catalog"; hostPath = { path = "${catalog}"; type = "Directory"; }; }
      { name = "data"; hostPath = { path = "${example}/data"; type = "Directory"; }; }
    ];
    extraVolumeMounts = [
      { name = "catalog"; mountPath = "/srv/sutura/catalog"; readOnly = true; }
      { name = "data"; mountPath = "/srv/sutura/data"; readOnly = true; }
    ];
  });
in
pkgs.testers.runNixOSTest {
  name = "k3s-worker-pool";

  nodes.machine = {
    virtualisation = { memorySize = 6144; cores = 4; diskSize = 12288; };
    networking.firewall.enable = false;
    environment.systemPackages = [ pkgs.kubectl chartHelm pkgs.curl ];
    environment.sessionVariables.KUBECONFIG = "/etc/rancher/k3s/k3s.yaml";
    services.k3s = {
      enable = true;
      role = "server";
      disable = [ "traefik" "servicelb" "local-storage" "metrics-server" ];
      images = [ pkgs.k3s.airgap-images image operator ] ++ certManagerImages;
    };
  };

  testScript = ''
    import json
    import secrets
    import time

    REGION = '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
    WHOLE = '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'


    def cell(raw: str) -> str:
        return "null" if raw == "Null" else raw.split("(", 1)[1].removesuffix(")")


    def golden() -> dict[str, int]:
        with open("${golden}") as handle:
            header, *body = handle.read().split("---\n")[2].strip().splitlines()
        rows = [dict(zip(header.split("\t"), map(cell, line.split("\t")))) for line in body]
        return {row["region"]: int(row["recurring_revenue"]) for row in rows}


    def workers() -> int:
        return int(machine.succeed("kubectl get deployment sutura-workers -o jsonpath='{.spec.replicas}'"))


    def reported(count: int) -> None:
        machine.wait_until_succeeds(f"test \"$(kubectl get suturaworkerpools sutura -o jsonpath='{{.status.replicas}}')\" = {count}", timeout=120)


    def ask(question: str) -> tuple[dict, float]:
        started = time.monotonic()
        reply = json.loads(machine.succeed(
            f"curl -fsS --max-time 150 --cacert /tmp/ca.pem --resolve sutura.default.svc:8080:{address} -H 'authorization: Bearer {access_token}' -H 'content-type: application/json' --data '{question}' https://sutura.default.svc:8080/v1/query"
        ))
        assert reply.get("outcome") == "answer", reply
        return reply, time.monotonic() - started


    machine.wait_for_unit("k3s")
    machine.wait_until_succeeds("kubectl get serviceaccount default", timeout=300)
    machine.wait_until_succeeds("k3s ctr -n k8s.io images tag docker.io/library/sutura-operator:latest docker.io/library/sutura:operator", timeout=120)

    try:
        machine.succeed("kubectl apply --server-side -f ${certManager}")
        machine.succeed("kubectl -n cert-manager wait --for=condition=Available deployment --all --timeout=300s")
        machine.succeed("helm install csi ${csiDriver} -n cert-manager --wait --timeout 5m")
        machine.wait_until_succeeds("kubectl apply -f ${issuer}", timeout=120)
        machine.succeed("kubectl wait --for=condition=Ready issuer/sutura-nodes --timeout=120s")
        machine.succeed("kubectl get secret sutura-nodes-ca -o jsonpath='{.data.ca\\.crt}' | base64 -d > /tmp/ca.pem")

        access_token = secrets.token_hex(32)
        machine.succeed(f"kubectl create secret generic sutura-access-token --from-literal=token={access_token}")
        machine.succeed(f"kubectl create secret generic sutura-metrics-token --from-literal=token={secrets.token_hex(32)}")
        # `--wait` holds the pool's hook until its definition is established.
        machine.succeed("helm install sutura ${chart} -f ${values} --wait --timeout 5m")
        address = machine.succeed("kubectl get service sutura -o jsonpath='{.spec.clusterIP}'").strip()

        with subtest("the release starts with no worker"):
            reported(0)
            assert workers() == 0

        with subtest("a question one data system answers is answered at once, with no worker"):
            reply, took = ask(WHOLE)
            assert reply["rows"] == [["2026-06-01", "202121"]], reply
            assert took < 10, f"one data system took {took:.0f}s"
            assert workers() == 0

        with subtest("the federated question starts a worker and gets the one-source answer"):
            reply, took = ask(REGION)
            print(f"federated question with no worker to its answer: {took:.0f}s")
            region, revenue = reply["columns"].index("region"), reply["columns"].index("recurring_revenue")
            expected = golden()
            assert {row[region]: int(row[revenue]) for row in reply["rows"]} == expected, expected
            assert workers() >= 1
            reported(workers())

        with subtest("the worker ran a stage, reading its input over a node link"):
            container = machine.succeed("k3s crictl ps --name '^worker$' -q").split()[0]
            pid = json.loads(machine.succeed(f"k3s crictl inspect {container}"))["info"]["pid"]
            machine.succeed(f"nsenter -t {pid} -n ss -Htn state established dport = :8080 | grep -q .")

        with subtest("idle, the pool goes back to no worker"):
            machine.wait_until_succeeds("test \"$(kubectl get deployment sutura-workers -o jsonpath='{.spec.replicas}')\" = 0", timeout=300)
            reported(0)
    except Exception:
        print(machine.execute("kubectl get suturaworkerpools -o yaml; kubectl get pods -A; kubectl describe pods; kubectl logs deployment/sutura-operator --tail=100; kubectl logs deployment/sutura --tail=100; kubectl logs -l app.kubernetes.io/name=sutura-worker --tail=100")[1])
        raise
  '';
}
