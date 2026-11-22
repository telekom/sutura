# The real worker-pool operator, one k3s server, two agents and a PostgreSQL source (#828).
# All bounded processes share one engine reservation ceiling, with separate cgroup headroom.
# The differential compares exact ordered answers with the same image/catalog/data on one roomy
# node. IPv4 only, one KVM host, shared fixture identity; neither RSS nor human identity is proved.
# Deletion cost is a preference with a read-to-delete race, even when this controlled run observes
# the idle UID removed. Venue-only source policies know addresses the generic chart cannot know.
{ pkgs
, chartHelm
, ociImage
, operatorImage
, chart
, ...
}:
let
  # File names ending in `.tar`, which is what k3s imports from its images directory.
  image = pkgs.runCommand "sutura-image.tar" { } "${ociImage} > $out";
  operator = pkgs.runCommand "sutura-operator-image.tar" { } "${operatorImage} > $out";
  jetstack =
    name: tag: imageDigest: hash:
    pkgs.dockerTools.pullImage {
      imageName = "quay.io/jetstack/${name}";
      inherit imageDigest hash;
      finalImageTag = tag;
    };
  certManagerImages = [
    (jetstack "cert-manager-controller" "v1.21.3"
      "sha256:9f4f96a33c7774cef24466fcd85031997531a03a8d117f151126baa8d1290dcc"
      "sha256-P2gawOAOZPAICFXzRKhdlWvsk+LX52tOzQOSsCMbmhQ="
    )
    (jetstack "cert-manager-webhook" "v1.21.3"
      "sha256:055ac8e9de6a0ba6763794838fe38944acc650f68ebe1b01dd8be079e580f458"
      "sha256-uYEkHcw+YLilkRkvuoyjFTiWY4SBwh+p/Tv+VC5Tq38="
    )
    (jetstack "cert-manager-cainjector" "v1.21.3"
      "sha256:083d0e906f8eb44e255fe4ff817ae54e49f9ed82e22f1011bb4e9e4f4b8a641f"
      "sha256-t1nsNeypTkr/Y1yYo5I2NOH/m4WaAOYiJBdL7ujROzw="
    )
    (jetstack "cert-manager-csi-driver" "v0.16.1"
      "sha256:00493ba0324911cd93db80e9012076931053100b4d53bedd5fb574fc4c6aca8b"
      "sha256-bioKyXaN/4bcAqL/J5BlHVYkYF41QsxwWymHPpsOUco="
    )
    (jetstack "csi-node-driver-registrar" "v2.18.0"
      "sha256:b7fefd08651f00ac4df1a196ed4c621c2451fbb899297fe9fad9e36c667af0c0"
      "sha256-PhJvFFrOU3rruolaWvLTiPa43j/OQq88MNvkVw0QWC0="
    )
    (jetstack "livenessprobe" "v2.20.0"
      "sha256:19f2cf2f40e1987c7943945deca5f65cd4e4bf5cc0657fc78a5df131861578e8"
      "sha256-hx9cvHJ9w76H2uhoY0PQob+J9xCFhCl2oY1deR3Sob4="
    )
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
  issuer = pkgs.writeText "k3s-worker-pool-issuer.json" (
    builtins.toJSON {
      apiVersion = "v1";
      kind = "List";
      items = [
        {
          apiVersion = "cert-manager.io/v1";
          kind = "Issuer";
          metadata.name = "bootstrap";
          spec.selfSigned = { };
        }
        {
          apiVersion = "cert-manager.io/v1";
          kind = "Certificate";
          metadata.name = "sutura-nodes-ca";
          spec = {
            isCA = true;
            commonName = "sutura-nodes";
            secretName = "sutura-nodes-ca";
            privateKey = {
              algorithm = "ECDSA";
              size = 256;
            };
            issuerRef = {
              name = "bootstrap";
              kind = "Issuer";
            };
          };
        }
        {
          apiVersion = "cert-manager.io/v1";
          kind = "Issuer";
          metadata.name = "sutura-nodes";
          spec.ca.secretName = "sutura-nodes-ca";
        }
      ];
    }
  );

  sourceTls =
    dbHost:
    pkgs.runCommand "k3s-worker-pool-source-tls" { nativeBuildInputs = [ pkgs.openssl ]; } ''
      mkdir "$out" && cd "$out"
      openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
        -keyout ca.key -out ca.pem -days 3650 -subj /CN=k3s-worker-pool-source-ca \
        -addext 'basicConstraints=critical,CA:TRUE' -addext 'keyUsage=critical,keyCertSign'
      openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
        -keyout db.key -out db.csr -subj /CN=db
      printf 'subjectAltName=IP:%s\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n' \
        ${dbHost} > db.ext
      openssl x509 -req -in db.csr -CA ca.pem -CAkey ca.key -CAcreateserial \
        -out db.pem -days 3650 -extfile db.ext
      rm ca.key ca.srl db.csr db.ext
    '';
  fixture = dbHost: pkgs.runCommand "k3s-worker-pool-fixture" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    python - <<'PY'
    import csv
    import json
    import os
    from pathlib import Path

    root = Path(os.environ["out"])
    (root / "data").mkdir(parents=True)
    (root / "catalog").mkdir()
    (root / "password").write_text("fixture-password")

    def document(name, value):
        (root / "catalog" / (name + ".md")).write_text(
            "---\n" + json.dumps(value) + "\n---\nDeterministic capacity fixture.\n"
        )

    document("declaration", dict(kind="declaration",
        definitions=["structure", "descriptions", "relationships", "cardinality",
                     "metrics", "grains", "allowed_values", "anchors", "column_types"], knowledge=[]))

    for size, fact_count in [("large", 32768), ("small", 8)]:
        fact, lookup, link = "fact_" + size, "lookup_" + size, "link_" + size
        lookup_segment, link_segment = "lookup_" + size + "_segment", "link_" + size + "_segment"
        lookup_segment_two, link_segment_two = "lookup_" + size + "_segment_two", "link_" + size + "_segment_two"
        with (root / "data" / (fact + ".csv")).open("w") as handle:
            rows = csv.writer(handle)
            rows.writerow(["day", "link", "amount"])
            rows.writerows(("2026-06-01", "c" + str(n), 100) for n in range(fact_count))
        document(fact, dict(kind="model", name=fact, source="local", table=fact,
                            columns=[dict(name="day", type="date"),
                                     dict(name="link", type="text"),
                                     dict(name="amount", type="bigint")]))
        document(lookup, dict(kind="model", name=lookup, source="geo", table=lookup,
                              columns=[dict(name="link", type="text"),
                                       dict(name="region", type="text")]))
        document(link, dict(kind="relationship", name=link,
                            origin=dict(model=fact, column="link"),
                            target=dict(model=lookup, column="link"), join_type="many_to_one"))
        document(lookup_segment, dict(kind="model", name=lookup_segment, source="geo_aux", table=lookup_segment,
                            columns=[dict(name="link", type="text"),
                                     dict(name="segment", type="text")]))
        document(link_segment, dict(kind="relationship", name=link_segment,
                            origin=dict(model=fact, column="link"),
                            target=dict(model=lookup_segment, column="link"), join_type="many_to_one"))
        document(lookup_segment_two, dict(kind="model", name=lookup_segment_two, source="geo_aux_2", table=lookup_segment_two,
                            columns=[dict(name="link", type="text"),
                                     dict(name="segment_two", type="text")]))
        document(link_segment_two, dict(kind="relationship", name=link_segment_two,
                            origin=dict(model=fact, column="link"),
                            target=dict(model=lookup_segment_two, column="link"), join_type="many_to_one"))
        document("total_" + size, dict(kind="metric", name="total_" + size, model=fact,
                 measure=dict(simple=dict(aggregate="sum", column="amount")),
                 time_column="day", grains=["month"], audience="open",
                 dimensions=[dict(name="region", column="region", via=link,
                                  values=["north", "south"]),
                             dict(name="segment", column="segment", via=link_segment,
                                  values=["segment-a", "segment-b"]),
                             dict(name="segment_two", column="segment_two", via=link_segment_two,
                                  values=["segment-a", "segment-b"])],
                 anchor=dict(range=dict(start="2026-06-01", end="2026-07-01"), value=fact_count * 100)))
    PY
    cp ${sourceTls dbHost}/ca.pem "$out/postgres-ca.pem"
  '';
  files = {
    kind = "files";
    data_dir = "/srv/sutura/data";
    posture = "shared-service-user";
  };
  postgres = dbHost: {
    kind = "postgres"; host = dbHost; port = 5432; database = "sutura"; user = "sutura";
    password_file = "/srv/sutura/password"; transport_mode = "verified";
    transport_anchors = "/srv/sutura/postgres-ca.pem"; posture = "shared-service-user";
  };
  values =
    dbHost:
    pkgs.writeText "k3s-worker-pool-values.json" (
      builtins.toJSON {
        environment = "development";
        image = {
          repository = "sutura";
          tag = "latest";
          pullPolicy = "Never";
        };
        networkPolicy.enabled = true;
        nodeSelector."kubernetes.io/hostname" = "machine";
        resources = {
          requests = {
            cpu = "250m";
            memory = "256Mi";
          };
          limits = {
            cpu = "2";
            memory = "2Gi";
          };
        };
        runtime = {
          engineWorkerThreads = 2;
          workingSetMaxBytes = "536870912";
        };
        telemetry.filter = "info,worker_pool_fixture=debug";
        security = {
          accessToken.secretName = "sutura-access-token";
          metricsToken.secretName = "sutura-metrics-token";
        };
        workerPool = {
          enabled = true;
          # Request two peers for the large fixture while the small control stays local.
          acrossAboveBytes = 4 * 1024 * 1024;
          issuer.name = "sutura-nodes";
          min = 0;
          max = 2;
          idleAfterSeconds = 60;
          addressFamily = "ipv4";
          resources = {
            requests = {
              cpu = "250m";
              memory = "256Mi";
            };
            limits = {
              cpu = "2";
              memory = "2Gi";
            };
          };
          # Long enough for the operator's next pass and a worker's start.
          waitForWorkersSeconds = 90;
          operator.image.tag = "operator";
        };
        config.base = {
          server.request_timeout_seconds = 300;
          catalogs = [
            {
              name = "model";
              kind = "markdown";
              dir = "/srv/sutura/catalog";
              data_dir = "/srv/sutura/data";
              version = "k3s-worker-pool";
            }
          ];
          sources = {
            local = files;
            geo = postgres dbHost;
            geo_aux = postgres dbHost;
            geo_aux_2 = postgres dbHost;
          };
          security = {
            identity = "single-user";
            single_user_because = "one fixture identity reads all declared sources";
          };
        };
        extraVolumes = [
          {
            name = "fixture";
            hostPath = {
              path = "${fixture dbHost}";
              type = "Directory";
            };
          }
        ];
        extraVolumeMounts = [
          {
            name = "fixture";
            mountPath = "/srv/sutura";
            readOnly = true;
          }
        ];
      }
    );
  k3sNodeAddresses = { config, ... }: {
    services.k3s.extraFlags = [
      "--node-ip=${config.networking.primaryIPAddress}"
      "--flannel-iface=eth1"
    ]
    ++ pkgs.lib.optionals (config.services.k3s.role == "server") [
      "--advertise-address=${config.networking.primaryIPAddress}"
    ];
  };
  agent = {
    imports = [ k3sNodeAddresses ];
    virtualisation = {
      memorySize = 4096;
      cores = 2;
      diskSize = 8192;
    };
    networking = {
      firewall.enable = false;
      enableIPv6 = false;
    };
    environment.systemPackages = [
      pkgs.curl
      pkgs.netcat-openbsd
      pkgs.iproute2
      pkgs.iptables
      pkgs.bind.dnsutils
    ];
    services.k3s = {
      enable = true;
      role = "agent";
      serverAddr = "https://machine:6443";
      token = "fixture-cluster-token";
      images = [
        pkgs.k3s.airgap-images
        image
        operator
      ]
      ++ certManagerImages;
    };
  };
in
pkgs.testers.runNixOSTest {
  name = "k3s-worker-pool";

  nodes.agent1 = agent;
  nodes.agent2 = agent;
  nodes.db = { config, ... }: {
    virtualisation.memorySize = 1024;
    networking = {
      firewall.allowedTCPPorts = [
        5432
        5433
      ];
      enableIPv6 = false;
    };
    systemd.services.postgres-tls = {
      before = [ "postgresql.service" ];
      requiredBy = [ "postgresql.service" ];
      serviceConfig.Type = "oneshot";
      script = "install -d -m 0700 -o postgres -g postgres /run/postgres-tls && install -m 0600 -o postgres -g postgres ${sourceTls config.networking.primaryIPAddress}/db.key /run/postgres-tls/db.key";
    };
    services.postgresql = {
      enable = true;
      package = pkgs.postgresql_18;
      enableTCPIP = true;
      authentication = pkgs.lib.mkForce ''
        local all postgres peer
        hostssl sutura sutura all scram-sha-256
      '';
      settings = {
        ssl = true;
        ssl_cert_file = "${sourceTls config.networking.primaryIPAddress}/db.pem";
        ssl_key_file = "/run/postgres-tls/db.key";
      };
      initialScript = pkgs.writeText "worker-pool-init.sql" ''
        CREATE ROLE sutura LOGIN PASSWORD 'fixture-password';
        CREATE DATABASE sutura OWNER sutura;
        \connect sutura
        CREATE TABLE lookup_large AS SELECT 'c' || n AS link,
          CASE WHEN n % 2 = 0 THEN 'north' ELSE 'south' END AS region
          FROM generate_series(0, 131071) AS n;
        CREATE TABLE lookup_large_segment AS SELECT 'c' || n AS link,
          CASE WHEN n % 4 < 2 THEN 'segment-a' ELSE 'segment-b' END AS segment
          FROM generate_series(0, 131071) AS n;
        CREATE TABLE lookup_large_segment_two AS SELECT 'c' || n AS link,
          CASE WHEN n % 8 < 4 THEN 'segment-a' ELSE 'segment-b' END AS segment_two
          FROM generate_series(0, 131071) AS n;
        CREATE TABLE lookup_small AS SELECT * FROM lookup_large WHERE link IN
          ('c0', 'c1', 'c2', 'c3', 'c4', 'c5', 'c6', 'c7');
        CREATE TABLE lookup_small_segment AS SELECT * FROM lookup_large_segment WHERE link IN
          ('c0', 'c1', 'c2', 'c3', 'c4', 'c5', 'c6', 'c7');
        CREATE TABLE lookup_small_segment_two AS SELECT * FROM lookup_large_segment_two WHERE link IN
          ('c0', 'c1', 'c2', 'c3', 'c4', 'c5', 'c6', 'c7');
        ALTER TABLE lookup_large OWNER TO sutura;
        ALTER TABLE lookup_large_segment OWNER TO sutura;
        ALTER TABLE lookup_large_segment_two OWNER TO sutura;
        ALTER TABLE lookup_small OWNER TO sutura;
        ALTER TABLE lookup_small_segment OWNER TO sutura;
        ALTER TABLE lookup_small_segment_two OWNER TO sutura;
        ANALYZE;
      '';
    };
    # A second live port distinguishes a policy denial from an absent listener.
    systemd.services.source-control = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.python3}/bin/python -m http.server 5433 --bind 0.0.0.0";
    };
  };
  nodes.machine = {
    imports = [ k3sNodeAddresses ];
    virtualisation = {
      memorySize = 6144;
      cores = 4;
      diskSize = 12288;
    };
    networking = {
      firewall.enable = false;
      enableIPv6 = false;
    };
    environment.systemPackages = [
      pkgs.kubectl
      chartHelm
      pkgs.curl
      pkgs.netcat-openbsd
      pkgs.iproute2
      pkgs.iptables
      pkgs.bind.dnsutils
    ];
    environment.sessionVariables.KUBECONFIG = "/etc/rancher/k3s/k3s.yaml";
    services.k3s = {
      enable = true;
      role = "server";
      token = "fixture-cluster-token";
      extraFlags = [
        "--cluster-cidr=10.42.0.0/16"
        "--service-cidr=10.43.0.0/16"
      ];
      disable = [
        "traefik"
        "servicelb"
        "local-storage"
        "metrics-server"
      ];
      images = [
        pkgs.k3s.airgap-images
        image
        operator
      ]
      ++ certManagerImages;
    };
  };

  testScript = { nodes, ... }: ''
    import copy
    import json
    import secrets
    import shlex
    import time
    from pathlib import Path

    HOSTS = {"machine": machine, "agent1": agent1, "agent2": agent2}
    SOURCE = "${nodes.db.networking.primaryIPAddress}"
    CEILING_KEY = "SUTURA__RUNTIME__WORKING_SET_MAX_BYTES"
    WORKERS = "app.kubernetes.io/name=sutura-worker,app.kubernetes.io/instance=sutura"
    SERVING = "app.kubernetes.io/name=sutura,app.kubernetes.io/instance=sutura"
    OPERATOR = "app.kubernetes.io/name=sutura-operator,app.kubernetes.io/instance=sutura"
    q = shlex.quote
    evidence = {"mode": "discovery-only; no accepted fixed ceiling"}
    denied_observations = []
    startup_refusals = {}
    resource_refusals = {}

    def get(resource, namespace="default"):
        return json.loads(machine.succeed(f"kubectl -n {q(namespace)} get {resource} -o json"))

    def apply(value):
        machine.succeed("printf %s " + q(json.dumps(value)) + " | kubectl apply -f -")

    def patch(resource, value):
        machine.succeed(f"kubectl patch {resource} --type=merge -p {q(json.dumps(value))}")

    def eventually(predicate, timeout=120):
        end = time.monotonic() + timeout
        while True:
            result = predicate()
            if result:
                return result
            assert time.monotonic() < end, f"condition did not hold: {predicate}"
            time.sleep(0.6)

    def ready(pod):
        return (not pod["metadata"].get("deletionTimestamp") and
                any(c["type"] == "Ready" and c["status"] == "True"
                    for c in pod.get("status", {}).get("conditions", [])))

    def pods(selector):
        return get("pods -l " + q(selector))["items"]

    def worker_pods(count):
        def observed():
            found = pods(WORKERS)
            if len(found) == count and all(ready(p) for p in found):
                return found or True
            return False
        result = eventually(observed, 180)
        return [] if result is True else result

    def one(selector):
        def observed():
            found = pods(selector)
            return found[0] if len(found) == 1 and ready(found[0]) else False
        return eventually(observed, 180)

    def identity(pod):
        return {"uid": pod["metadata"]["uid"], "node": pod["spec"]["nodeName"],
                "ip": pod["status"]["podIP"], "ready": ready(pod),
                "priority": pod["spec"].get("priority", 0),
                "restarts": [s["restartCount"] for s in pod["status"]["containerStatuses"]],
                "images": [s["imageID"] for s in pod["status"]["containerStatuses"]]}

    def namespace(pod):
        host = HOSTS[pod["spec"]["nodeName"]]
        sandboxes = json.loads(host.succeed("k3s crictl pods -o json"))["items"]
        found = [s for s in sandboxes if s["metadata"]["uid"] == pod["metadata"]["uid"]
                 and s["state"] == "SANDBOX_READY"]
        assert len(found) == 1, found
        pid = json.loads(host.succeed("k3s crictl inspectp " + q(found[0]["id"])))["info"]["pid"]
        return host, f"nsenter -t {int(pid)} -n "

    def tcp(pod, address, port):
        host, prefix = namespace(pod)
        return host.execute(prefix + f"nc -z -w 2 {q(address)} {port}")[0] == 0

    def metrics(pod, origin=None):
        name = "sutura-workers" if pod["metadata"]["labels"].get("app.kubernetes.io/name") == "sutura-worker" else "sutura"
        if origin is None and name == "sutura-workers":
            origin = operator_pod
        host, prefix = (machine, "") if origin is None else namespace(origin)
        raw = host.succeed(prefix + f"curl -fsS --max-time 5 --cacert /tmp/ca.pem "
                           f"--resolve {name}.default.svc:8080:{pod['status']['podIP']} "
                           f"-H {q('authorization: Bearer ' + metrics_token)} https://{name}.default.svc:8080/metrics")
        return {key: float(value) for line in raw.splitlines() if line and not line.startswith("#")
                for key, value in [line.split()] if "{" not in key}

    def gauge(pod, name):
        values = metrics(pod)
        assert name in values, values
        return values[name]

    def pool(**fields):
        patch("suturaworkerpool/sutura", {"spec": fields})

    def settled(count):
        worker_pods(count)
        eventually(lambda: get("suturaworkerpool/sutura").get("status", {}).get("replicas") == count)
        assert get("deployment/sutura-workers")["spec"]["replicas"] == count

    def question(size="large", federated=True):
        return {"metrics": ["total_" + size], "grain": "month",
                "range": {"start": "2026-06-01", "end": "2026-07-01"},
                "dimensions": ["region", "segment", "segment_two"] if federated else []}

    def start_query(label, question, address, correlation=None):
        base = "/tmp/" + label
        correlation_header = "" if correlation is None else f"-H {q('x-correlation-id: ' + correlation)} "
        command = (f"curl -sS --max-time 310 --cacert /tmp/ca.pem {correlation_header}"
                   f"--resolve sutura.default.svc:8080:{address} "
                   f"-H {q('authorization: Bearer ' + access_token)} -H 'content-type: application/json' "
                   f"--data {q(json.dumps(question))} -o {base}.json -w '%{{http_code}}' "
                   f"https://sutura.default.svc:8080/v1/query > {base}.status 2> {base}.stderr; "
                   f"echo $? > {base}.exit")
        machine.succeed(f"rm -f {base}.exit; ({command}) </dev/null >/dev/null 2>&1 & echo $! > {base}.pid")
        return base, time.monotonic()

    def finish_query(request):
        base, started = request
        machine.wait_until_succeeds(f"test -f {base}.exit", timeout=320)
        assert machine.succeed(f"cat {base}.exit").strip() == "0", machine.succeed(f"cat {base}.stderr")
        return (int(machine.succeed(f"cat {base}.status")),
                json.loads(machine.succeed(f"cat {base}.json")), time.monotonic() - started)

    def exact(reply, size="large", federated=True):
        assert reply.get("outcome") == "answer", reply
        count = 32768 if size == "large" else 8
        expected = ([{"period": "2026-06-01", "region": region, "segment": segment, "segment_two": segment_two,
                      "total_" + size: str(count * 100 // 8)}
                     for region in ["north", "south"]
                     for segment in ["segment-a", "segment-b"]
                     for segment_two in ["segment-a", "segment-b"]] if federated else
                    [{"period": "2026-06-01", "total_" + size: str(count * 100)}])
        assert set(reply["columns"]) == set(expected[0]), reply
        assert reply["rows"] == [[row[c] for c in reply["columns"]] for row in expected], reply
        return {"columns": reply["columns"], "rows": reply["rows"]}

    def rollout(name):
        machine.succeed(f"kubectl rollout status deployment/{name} --timeout=180s")

    def set_ceiling(name, ceiling):
        machine.succeed(f"kubectl set env deployment/{name} {CEILING_KEY}={ceiling}")
        rollout(name)

    def reference(ceiling):
        machine.succeed("kubectl delete pod reference --ignore-not-found --wait=true")
        template = copy.deepcopy(get("deployment/sutura")["spec"]["template"])
        template["metadata"] = {"name": "reference", "labels": {"fixture-reader": "true"}}
        spec = template["spec"]
        spec["restartPolicy"] = "Never"
        container = spec["containers"][0]
        container["env"] = [e for e in container["env"] if not e["name"].startswith("SUTURA__SERVER__NODES") and e["name"] != CEILING_KEY]
        container["env"].append({"name": CEILING_KEY, "value": str(ceiling)})
        for volume in spec["volumes"]:
            if "configMap" in volume:
                assert volume["configMap"]["name"] == "sutura-config", volume
                volume["configMap"]["name"] = "reference-config"
        apply({"apiVersion": "v1", "kind": "Pod", **template})
        machine.succeed("kubectl wait --for=condition=Ready pod/reference --timeout=180s")
        return get("pod/reference")

    def endpoints(expected):
        def observed():
            slices = get("endpointslices -l kubernetes.io/service-name=sutura-workers")["items"]
            addresses = {a for s in slices for ep in s.get("endpoints", [])
                         if ep.get("conditions", {}).get("ready") for a in ep["addresses"]}
            return addresses == {p["status"]["podIP"] for p in expected}
        eventually(observed)
        dns = get("service/kube-dns", "kube-system")["spec"]["clusterIP"]
        host, prefix = namespace(one(SERVING))
        raw = host.succeed(prefix + f"${pkgs.bind.dnsutils}/bin/dig +short @{dns} sutura-workers.default.svc.cluster.local A")
        assert set(raw.split()) == {p["status"]["podIP"] for p in expected}, raw

    def denied_with_control(pod, address, port, policy):
        saved = get("networkpolicy/" + policy)
        saved["metadata"] = {key: value for key, value in saved["metadata"].items()
                             if key in {"name", "namespace", "labels", "annotations"}}
        machine.succeed("kubectl delete networkpolicy " + q(policy))
        try:
            eventually(lambda: tcp(pod, address, port), 30)
        finally:
            apply(saved)
        assert get("networkpolicy/" + policy)["spec"] == saved["spec"]
        eventually(lambda: not tcp(pod, address, port), 30)
        for _ in range(3):
            assert not tcp(pod, address, port), (identity(pod), address, port)
        denied_observations.append([identity(pod), address, port, policy])
        evidence["denied"] = denied_observations

    def failed_process(label, deployment, removed, added, config, sentence):
        spec = copy.deepcopy(get("deployment/" + deployment)["spec"]["template"]["spec"])
        spec["restartPolicy"] = "Never"
        spec["nodeSelector"] = {"kubernetes.io/hostname": "machine"}
        spec.pop("affinity", None)
        container = spec["containers"][0]
        container.pop("startupProbe", None)
        container.pop("livenessProbe", None)
        container["env"] = [e for e in container["env"] if e["name"] not in set(removed) | set(added)]
        container["env"].extend({"name": name, "value": value} for name, value in added.items())
        if config is not None:
            apply({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": label},
                   "data": {"base.yaml": json.dumps(config)}})
            container["env"] = [e for e in container["env"] if e["name"] != "SUTURA_CONFIG_DIR"]
            container["env"].append({"name": "SUTURA_CONFIG_DIR", "value": "/etc/refusal"})
            container.setdefault("volumeMounts", []).append({"name": "refusal", "mountPath": "/etc/refusal", "readOnly": True})
            spec.setdefault("volumes", []).append({"name": "refusal", "configMap": {"name": label}})
        apply({"apiVersion": "v1", "kind": "Pod", "metadata": {"name": label}, "spec": spec})
        eventually(lambda: get("pod/" + label).get("status", {}).get("phase") in {"Failed", "Succeeded"})
        pod = get("pod/" + label)
        ended = pod["status"]["containerStatuses"][0]["state"]["terminated"]
        log = machine.succeed("kubectl logs " + q(label))
        assert ended["exitCode"] != 0 and ended["reason"] != "OOMKilled", ended
        assert sentence in log, log
        startup_refusals[label] = {"termination": ended, "log": log}
        evidence["startup-refusals"] = startup_refusals
        machine.succeed("kubectl delete pod " + q(label) + " --wait=true")

    def watch_records(raw):
        decoder, records = json.JSONDecoder(), []
        while raw.strip():
            record, end = decoder.raw_decode(raw.lstrip())
            records.append(record)
            raw = raw.lstrip()[end:]
        return records

    start_all()
    db.wait_for_unit("postgresql")
    db.wait_for_unit("source-control")
    machine.wait_for_unit("k3s")
    machine.wait_until_succeeds("kubectl get serviceaccount default", timeout=300)
    for host in HOSTS.values():
        host.wait_for_unit("k3s")
        host.wait_until_succeeds("k3s ctr -n k8s.io images tag docker.io/library/sutura-operator:latest docker.io/library/sutura:operator", timeout=180)
    eventually(lambda: {n["metadata"]["name"] for n in get("nodes")["items"]} == set(HOSTS), 180)
    machine.succeed("kubectl wait --for=condition=Ready nodes --all --timeout=180s")
    node_addresses = {
        node["metadata"]["name"]: next(a["address"] for a in node["status"]["addresses"]
                                      if a["type"] == "InternalIP")
        for node in get("nodes")["items"]
    }
    assert node_addresses == {
        "machine": "${nodes.machine.networking.primaryIPAddress}",
        "agent1": "${nodes.agent1.networking.primaryIPAddress}",
        "agent2": "${nodes.agent2.networking.primaryIPAddress}",
    }, node_addresses
    evidence["k3s-node-addresses"] = node_addresses
    try:
        machine.succeed("kubectl apply --server-side -f ${certManager}")
        machine.succeed("kubectl -n cert-manager wait --for=condition=Available deployment --all --timeout=300s")
        machine.succeed("helm install csi ${csiDriver} -n cert-manager --wait --timeout 5m")
        machine.wait_until_succeeds("kubectl apply -f ${issuer}", timeout=120)
        machine.succeed("kubectl wait --for=condition=Ready issuer/sutura-nodes --timeout=120s")
        machine.succeed("kubectl get secret sutura-nodes-ca -o jsonpath='{.data.ca\\.crt}' | base64 -d > /tmp/ca.pem")
        ca = machine.succeed("cat /tmp/ca.pem")
        for host in HOSTS.values():
            host.succeed("printf %s " + q(ca) + " > /tmp/ca.pem")
        access_token, metrics_token = secrets.token_hex(32), secrets.token_hex(32)
        machine.succeed(f"kubectl create secret generic sutura-access-token --from-literal=token={access_token}")
        machine.succeed(f"kubectl create secret generic sutura-metrics-token --from-literal=token={metrics_token}")
        machine.succeed("helm install sutura ${chart} -f ${values nodes.db.networking.primaryIPAddress} --wait --timeout 5m")
        address = get("service/sutura")["spec"]["clusterIP"]
        serving = one(SERVING)
        operator_pod = one(OPERATOR)
        eventually(lambda: tcp(operator_pod, serving["status"]["podIP"], 8080), 30)
        assert metrics(serving, operator_pod), "operator cannot scrape authenticated metrics during initial startup"
        settled(0)

        # A venue overlay, not a new chart setting. Combining workers have no fixture mounts.
        patch("deployment/sutura-workers", {"spec": {"template": {"spec": {
            "nodeSelector": {"kubernetes.io/hostname": None, "fixture-worker": "true"},
            "affinity": {"podAntiAffinity": {"requiredDuringSchedulingIgnoredDuringExecution": [{
                "labelSelector": {"matchLabels": {"app.kubernetes.io/name": "sutura-worker"}},
                "topologyKey": "kubernetes.io/hostname"}]}}}}}})
        machine.succeed("kubectl label nodes agent1 agent2 fixture-worker=true")
        patch("deployment/sutura", {"spec": {"template": {"metadata": {"labels": {"fixture-reader": "true"}}}}})
        rollout("sutura")
        serving = one(SERVING)
        operator_pod = one(OPERATOR)
        with subtest("chart metric paths and IPv4 policy dataplane"):
            pool(min=2)
            # Set up listeners independently of the operator path this cell is checking.
            machine.succeed("kubectl scale deployment sutura-workers --replicas=2")
            workers = worker_pods(2)
            assert {p["spec"]["nodeName"] for p in workers} == {"agent1", "agent2"}
            for worker in workers:
                spec = worker["spec"]
                assert all(not ({"hostPath", "configMap"} & v.keys()) for v in spec.get("volumes", [])), spec
                for container in spec["containers"] + spec.get("initContainers", []):
                    assert not container.get("envFrom"), container
                    assert all(e["name"] != "SUTURA_CONFIG_DIR" and not e["name"].startswith(
                        ("SUTURA__SOURCES", "SUTURA__CATALOGS")) for e in container.get("env", [])), container
                    assert all(m["mountPath"] in {"/etc/sutura/node", "/var/run/secrets/kubernetes.io/serviceaccount"}
                               for m in container.get("volumeMounts", [])), container
            evidence["worker-specs"] = [p["spec"] for p in workers]
            for target in [serving, *workers]:
                assert tcp(serving, target["status"]["podIP"], 8080)
                assert tcp(operator_pod, target["status"]["podIP"], 8080), "operator metric TCP path is blocked"
                assert metrics(target, operator_pod), "operator cannot scrape authenticated metrics"
            for origin, target in [(workers[0], workers[1]), (workers[1], workers[0]),
                                   (workers[0], serving), (workers[1], serving)]:
                assert tcp(origin, target["status"]["podIP"], 8080)

            pause = [i for i in machine.succeed("k3s ctr -n k8s.io images ls -q").splitlines() if "/mirrored-pause:" in i]
            assert len(pause) == 1, pause
            apply({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": "outside"}})
            for ns in ["default", "outside"]:
                apply({"apiVersion": "v1", "kind": "Pod", "metadata": {"name": "probe", "namespace": ns},
                       "spec": {"nodeName": "machine", "automountServiceAccountToken": False,
                                "containers": [{"name": "probe", "image": pause[0], "imagePullPolicy": "Never"}]}})
                machine.succeed(f"kubectl -n {ns} wait --for=condition=Ready pod/probe --timeout=120s")
                probe = get("pod/probe", ns)
                # Serving's ingress is deliberately public. It is a live listener control.
                assert tcp(probe, serving["status"]["podIP"], 8080)
                denied_with_control(probe, workers[0]["status"]["podIP"], 8080, "sutura-workers")

            dns_rules = {"ports": [{"port": 53, "protocol": p} for p in ["UDP", "TCP"]]}
            peers = [{"podSelector": {"matchLabels": {"app.kubernetes.io/name": name,
                                                      "app.kubernetes.io/instance": "sutura"}}}
                     for name in ["sutura", "sutura-worker"]]
            links = {"to": peers, "ports": [{"port": 8080, "protocol": "TCP"}]}
            for name, selector, rules in [
                ("fixture-readers", {"fixture-reader": "true"}, [dns_rules, links,
                    {"to": [{"ipBlock": {"cidr": SOURCE + "/32"}}], "ports": [{"port": 5432, "protocol": "TCP"}]}]),
                ("fixture-workers", {"app.kubernetes.io/name": "sutura-worker", "app.kubernetes.io/instance": "sutura"}, [dns_rules, links]),
            ]:
                apply({"apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicy", "metadata": {"name": name},
                       "spec": {"podSelector": {"matchLabels": selector}, "policyTypes": ["Egress"], "egress": rules}})
            assert tcp(serving, SOURCE, 5432)
            denied_with_control(serving, SOURCE, 5433, "fixture-readers")
            for port in [5432, 5433]:
                denied_with_control(workers[0], SOURCE, port, "fixture-workers")
                denied_with_control(operator_pod, SOURCE, port, "sutura-operator")
            host, prefix = namespace(operator_pod)
            api = get("service/kubernetes")["spec"]["clusterIP"]
            assert tcp(operator_pod, api, 443)
            dns = get("service/kube-dns", "kube-system")["spec"]["clusterIP"]
            host.succeed(prefix + f"${pkgs.bind.dnsutils}/bin/dig +short @{dns} kubernetes.default.svc.cluster.local A | grep -Fx {api}")
            endpoints(workers)
            evidence["network"] = {"family": "IPv4", "ipv6": "disabled, unmeasured",
                                   "workers": [identity(p) for p in workers],
                                   "policies": get("networkpolicies")["items"]}
            pool(min=0, warm=0, idleAfterSeconds=0)
            settled(0)
        exec(${builtins.toJSON (import ./k3s-worker-pool-calibration.nix)}, globals())
        roomy_record = evidence["roomy"]
        assert isinstance(roomy_record, dict), roomy_record
        expected = roomy_record["answer"]
        ceiling = evidence["ceiling"]
        roomy_pod = roomy_record["pod"]
        assert isinstance(roomy_pod, dict), roomy_pod
        runtime_images = roomy_pod["images"]
        with subtest("small one-source and genuinely small federated plans stay local"):
            for size, federated in [("large", False), ("small", False), ("small", True)]:
                before = gauge(serving, "sutura_worker_slots_wanted")
                request = start_query("small", question(size, federated), address)
                observed = [before]
                while machine.execute("test -f /tmp/small.exit")[0] != 0:
                    assert time.monotonic() - request[1] < 320, "local control did not finish"
                    observed.append(gauge(serving, "sutura_worker_slots_wanted"))
                    assert not pods(WORKERS)
                    time.sleep(0.6)
                status, reply, _ = finish_query(request)
                assert status == 200, reply
                exact(reply, size, federated)
                assert observed and not any(observed), observed
                settled(0)
                evidence["local-" + size + "-" + str(federated)] = {"reply": reply, "demand": observed}
        with subtest("actual demand acquires bounded capacity on other machines"):
            pool(idleAfterSeconds=60)
            settled(0)
            request = start_query("cold", question(), address)
            demand = eventually(lambda: gauge(serving, "sutura_worker_slots_wanted"))
            status, reply, took = finish_query(request)
            workers = pods(WORKERS)
            evidence["cold"] = {"demand": demand, "status": status, "reply": reply}
            assert demand == len(workers) == 2 and all(ready(p) for p in workers), (demand, workers)
            assert {p["spec"]["nodeName"] for p in workers} == {"agent1", "agent2"}, workers
            assert status == 200 and exact(reply) == expected, evidence["cold"]
            for pod in [serving, *workers]:
                env = {e["name"]: e.get("value") for e in pod["spec"]["containers"][0]["env"]}
                assert env[CEILING_KEY] == str(ceiling), env
                assert identity(pod)["restarts"] == [0], identity(pod)
                assert identity(pod)["images"] == runtime_images, identity(pod)
            endpoints(workers)
            evidence["cold"].update(seconds=took, answer=exact(reply), pods=[identity(p) for p in [serving, *workers]])
            evidence["bounded-manifests"] = {name: get("deployment/" + name)["spec"] for name in ["sutura", "sutura-workers"]}
            settled(0)
        with subtest("ready warm UIDs are reused without a rollout"):
            pool(warm=2)
            settled(2)
            warm = worker_pods(2)
            endpoints(warm)
            before = [identity(p) for p in warm]
            revision = get("deployment/sutura-workers")["metadata"]["generation"]
            status, reply, took = finish_query(start_query("warm", question(), address))
            assert status == 200 and exact(reply) == expected, reply
            after = [identity(p) for p in worker_pods(2)]
            assert sorted(before, key=lambda p: p["uid"]) == sorted(after, key=lambda p: p["uid"])
            assert get("deployment/sutura-workers")["metadata"]["generation"] == revision
            evidence["warm"] = {"before": before, "after": after, "seconds": took, "answer": exact(reply)}
        with subtest("warm pods stay through sampled deployed idle window then reach zero"):
            pool(min=2, warm=0, idleAfterSeconds=60)
            settled(2)
            eventually(lambda: gauge(serving, "sutura_worker_slots_wanted") == 0)
            wanted = {p["uid"] for p in after}
            started = time.monotonic()
            pool(min=0)
            samples = []
            evidence["idle-window"] = {"seconds": 60, "uids": sorted(wanted), "samples": samples}
            while time.monotonic() < started + 60:
                found = pods(WORKERS)
                replicas = get("deployment/sutura-workers")["spec"]["replicas"]
                status = get("suturaworkerpool/sutura").get("status", {}).get("replicas")
                demand = gauge(serving, "sutura_worker_slots_wanted")
                elapsed = time.monotonic() - started
                # Completed samples only: a read crossing the deadline cannot prove early shrink.
                if elapsed < 60:
                    sample = {"seconds": elapsed, "uids": [p["metadata"]["uid"] for p in found],
                              "replicas": replicas, "status": status, "demand": demand}
                    samples.append(sample)
                    assert len(found) == 2 and all(ready(p) for p in found), sample
                    assert set(sample["uids"]) == wanted and replicas == status == 2 and demand == 0, sample
                time.sleep(0.6)
            assert samples, "no idle-window observation completed before the deadline"
            settled(0)
            evidence["idle-window"]["zero-after-seconds"] = time.monotonic() - started
        exec(${builtins.toJSON (import ./k3s-worker-pool-idle-first.nix)}, globals())
        with subtest("actual bad processes refuse startup at their own checks"):
            failed_process("worker-source", "sutura-workers", [], {},
                {"sources": {"local": {"kind": "files", "data_dir": "/unused", "posture": "shared-service-user"}},
                 "security": {"identity": "single-user", "single_user_because": "a refusal fixture declares one identity"}},
                "server.role is `worker` and 1 source(s) are declared")
            failed_process("worker-catalog", "sutura-workers", [], {},
                {"catalogs": [{"name": "model", "kind": "markdown", "dir": "/unused", "data_dir": "/unused", "version": "fixture"}]},
                "server.role is `worker` and `catalogs` is declared")
            failed_process("worker-zero", "sutura-workers", [], {CEILING_KEY: "0"}, None,
                "runtime.working_set_max_bytes may not be zero")
            failed_process("worker-too-large", "sutura-workers", [], {CEILING_KEY: "1099511627777"}, None,
                "runtime.working_set_max_bytes is 1099511627777 and the maximum is 1099511627776")
            for suffix, variable, sentence in [
                ("authority", "SUTURA__SERVER__TLS_NODE_AUTHORITY", "`server.tls_node_authority` is not set"),
                ("workers", "SUTURA__OPERATOR__WORKERS", "`operator.workers` is not set"),
                ("serving", "SUTURA__OPERATOR__SERVING", "`operator.serving` is not set"),
            ]:
                failed_process("operator-" + suffix, "sutura-operator", [variable], {}, None, sentence)
            pool(min=1, max=2, warm=0)
            settled(1)
            assert gauge(worker_pods(1)[0], "sutura_worker_requests_in_flight") == 0
        with subtest("a refused replacement preserves four ready workers and reports its refusal"):
            # Four replicas distinguish maxUnavailable=0 from Kubernetes' default 25 percent.
            original_affinity = copy.deepcopy(get("deployment/sutura-workers")["spec"]["template"]["spec"]["affinity"])
            patch("deployment/sutura-workers", {"spec": {"template": {"spec": {"affinity": None}}}})
            rollout("sutura-workers")
            pool(min=4, max=4, warm=0)
            settled(4)
            old = worker_pods(4)
            old_uids = {p["metadata"]["uid"] for p in old}
            endpoints(old)
            valid = copy.deepcopy(get("deployment/sutura-workers")["spec"]["template"])
            broken = copy.deepcopy(valid)
            broken["spec"]["volumes"].append({"name": "rollout-refusal", "configMap": {"name": "rollout-refusal"}})
            container = broken["spec"]["containers"][0]
            container["env"].append({"name": "SUTURA_CONFIG_DIR", "value": "/etc/refusal"})
            container["volumeMounts"].append({"name": "rollout-refusal", "mountPath": "/etc/refusal", "readOnly": True})
            config = {"sources": {"local": {"kind": "files", "data_dir": "/unused", "posture": "shared-service-user"}},
                      "security": {"identity": "single-user", "single_user_because": "a refused replacement declares one identity"}}
            apply({"apiVersion": "v1", "kind": "ConfigMap", "metadata": {"name": "rollout-refusal"},
                   "data": {"base.yaml": json.dumps(config)}})
            sentence = "server.role is `worker` and 1 source(s) are declared"
            samples = []
            try:
                patch("deployment/sutura-workers", {"spec": {"template": broken}})
                def refused_replacement():
                    found = pods(WORKERS)
                    healthy = {p["metadata"]["uid"] for p in found if ready(p)}
                    assert healthy == old_uids, ("old_ready_workers_lost", healthy, old_uids, found)
                    for pod in found:
                        if pod["metadata"]["uid"] in old_uids:
                            continue
                        assert not ready(pod), ("refused_replacement_ready", pod)
                        for status in pod.get("status", {}).get("containerStatuses", []):
                            ended = status.get("state", {}).get("terminated") or status.get("lastState", {}).get("terminated")
                            if ended:
                                assert ended["exitCode"] != 0 and ended["reason"] != "OOMKilled", ended
                                assert sentence in ended.get("message", ""), ("termination_message", ended)
                                return {"pod": identity(pod), "termination": ended}
                    return False
                refusal = eventually(refused_replacement, 120)
                # Keep sampling after the failure so a delayed default-25-percent shrink cannot pass.
                started = time.monotonic()
                while time.monotonic() - started < 30:
                    samples.append(refused_replacement())
                    time.sleep(0.6)
                assert samples and all(samples), samples
                status, reply, _ = finish_query(start_query("bad-rollout", question(), address))
                assert status == 200 and exact(reply) == expected, reply
                assert refused_replacement(), "refused replacement disappeared before restoration"
                evidence["bad-rollout"] = {"old": [identity(p) for p in old], "refusal": refusal,
                                           "samples": samples, "answer": exact(reply)}
            finally:
                # Replace the pod template, including removal of the forbidden mount and environment.
                valid["metadata"].setdefault("annotations", {})["fixture-valid-generation"] = "restored"
                machine.succeed("kubectl patch deployment/sutura-workers --type=json -p " + q(json.dumps([
                    {"op": "replace", "path": "/spec/template", "value": valid}])))
                rollout("sutura-workers")
            recovered = worker_pods(4)
            assert not old_uids & {p["metadata"]["uid"] for p in recovered}, ("fresh_valid_workers", recovered)
            endpoints(recovered)
            status, reply, _ = finish_query(start_query("rollout-restored", question(), address))
            assert status == 200 and exact(reply) == expected, reply
            evidence["bad-rollout"]["recovered"] = {"pods": [identity(p) for p in recovered], "answer": exact(reply)}
            pool(min=1, max=2)
            settled(1)
            patch("deployment/sutura-workers", {"spec": {"template": {"spec": {"affinity": original_affinity}}}})
            rollout("sutura-workers")
            worker_pods(1)
        with subtest("foreign names and inverted bounds are resource status refusals"):
            foreign = copy.deepcopy(get("deployment/sutura-workers"))
            foreign["metadata"] = {"name": "foreign"}
            foreign["spec"]["replicas"] = 0
            foreign["spec"]["selector"] = {"matchLabels": {"fixture": "foreign"}}
            foreign["spec"]["template"]["metadata"] = {"labels": {"fixture": "foreign"}}
            foreign.pop("status", None)
            apply(foreign)
            legitimate = get("suturaworkerpool/sutura")
            sizes = {name: get("deployment/" + name)["spec"]["replicas"] for name in ["sutura", "sutura-workers", "foreign"]}
            for label, stimulus, sentence in [
                ("foreign-workers", {"workers": "foreign"}, "not the Deployment this operator sizes"),
                ("foreign-serving", {"serving": "foreign"}, "not the Deployment this operator reads demand from"),
                ("inverted", {"min": 2, "max": 1}, "a pool's floor cannot be above its ceiling"),
            ]:
                invalid = {"apiVersion": legitimate["apiVersion"], "kind": legitimate["kind"],
                           "metadata": {"name": label}, "spec": {**legitimate["spec"], **stimulus}}
                apply(invalid)
                try:
                    eventually(lambda: sentence in (get("suturaworkerpool/" + label).get("status", {}).get("refusal") or ""))
                    status = get("suturaworkerpool/" + label)["status"]
                    assert {name: get("deployment/" + name)["spec"]["replicas"] for name in sizes} == sizes
                    if label == "inverted":
                        assert status["replicas"] == sizes["sutura-workers"], status
                    resource_refusals[label] = {"resource": invalid, "status": status, "sizes": sizes}
                    evidence["resource-refusals"] = resource_refusals
                finally:
                    machine.succeed("kubectl delete suturaworkerpool " + q(label))
            pool(min=0, max=2, warm=2)
            settled(2)
            eventually(lambda: not get("suturaworkerpool/sutura").get("status", {}).get("refusal"))
            status, reply, _ = finish_query(start_query("restored", question(), address))
            assert status == 200 and exact(reply) == expected, reply
            pool(min=0, warm=0, idleAfterSeconds=0)
            settled(0)
    except Exception:
        for command in ["kubectl get suturaworkerpools -o yaml", "kubectl get pods -A -o wide",
                        "kubectl describe pods", "kubectl logs deployment/sutura-operator --tail=100",
                        "kubectl logs deployment/sutura --tail=-1", "kubectl logs -l " + q(WORKERS) + " --tail=100",
                        "for file in /tmp/cold.exit /tmp/cold.status /tmp/cold.json /tmp/cold.stderr; do echo \"$file\"; cat \"$file\"; done"]:
            print(machine.execute(command)[1])
        raise
    finally:
        Path(driver.out_dir, "elastic-evidence.json").write_text(json.dumps(evidence, indent=2))
  '';
}
