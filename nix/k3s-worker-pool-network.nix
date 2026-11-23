# Exercise the chart's per-group egress policies with stand-in pods.
{ pkgs, chart, chartHelm }:
let
  # Two reading worker groups, each declaring the address of its own source host on the same port:
  # what the chart's policies and pod labels are rendered from, as release `rw`, so no real reader
  # starts.
  values =
    { dbHost, dbTwoHost }:
    let
      source = name: {
        kind = "files";
        data_dir = "/srv/sutura/${name}";
        posture = "shared-service-user";
        acknowledged_because = "a directory this deployment owns";
      };
      group = name: host: {
        sources = [ name ];
        egress = [
          {
            cidr = "${host}/32";
            port = 5432;
          }
        ];
        catalogs = [
          {
            name = "model";
            kind = "markdown";
            dir = "/srv/sutura/catalog";
            data_dir = "/srv/sutura/data";
            version = "k3s-worker-pool";
          }
        ];
      };
    in
    pkgs.writeText "k3s-worker-pool-reading-values.json" (
      builtins.toJSON {
        environment = "production";
        security = {
          metricsToken.secretName = "sutura-metrics-token";
          inbound.keySetSecretName = "sutura-inbound-keys";
        };
        workerPool = {
          enabled = true;
          issuer.name = "sutura-nodes";
        };
        networkPolicy.enabled = true;
        readingWorkers.groups = {
          a = group "a" dbHost;
          b = group "b" dbTwoHost;
        };
        config.base = {
          security = {
            identity = "multi-user";
            inbound = {
              mode = "direct";
              resource = "https://sutura.example.com";
              authorization_server = "https://issuer.example.com";
              algorithms = [ "RS256" ];
            };
          };
          sources = {
            a = source "a";
            b = source "b";
          };
        };
      }
    );
  # The pod labels of the serving, combining and reading Deployments as the chart renders them, so
  # a label the chart changes reaches the stand-in pods and the policies stop matching them.
  labels =
    hosts:
    pkgs.runCommand "k3s-worker-pool-reading-labels"
      {
        nativeBuildInputs = [
          chartHelm
          (pkgs.python3.withPackages (ps: [ ps.pyyaml ]))
        ];
      }
      ''
        export HOME="$TMPDIR"
        helm template rw ${chart} -f ${values hosts} \
          --show-only templates/deployment.yaml \
          --show-only templates/worker-pool.yaml \
          --show-only templates/reading-workers.yaml > rendered.yaml
        python - rendered.yaml > "$out" <<'PY'
        import json
        import sys

        import yaml

        found = {"readers": {}}
        for doc in yaml.safe_load_all(open(sys.argv[1])):
            if not doc or doc["kind"] != "Deployment":
                continue
            name = doc["metadata"]["name"]
            pod = doc["spec"]["template"]["metadata"]["labels"]
            if name == "rw":
                found["serving"] = pod
            elif name == "rw-workers":
                found["worker"] = pod
            elif name.startswith("rw-reading-"):
                found["readers"][name.removeprefix("rw-reading-")] = pod
        assert set(found) == {"serving", "worker", "readers"} and set(found["readers"]) == {"a", "b"}, found
        json.dump(found, sys.stdout)
        PY
      '';
in
{
  db2 = {
    virtualisation.memorySize = 512;
    networking = {
      firewall.allowedTCPPorts = [ 5432 ];
      enableIPv6 = false;
    };
    systemd.services.source-two = {
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${pkgs.python3}/bin/python -m http.server 5432 --bind 0.0.0.0";
    };
  };
  # The Python the test script splices in; it reads the script's own `machine`, `HOSTS`, `SOURCE`,
  # `get`, `apply`, `eventually`, `identity` and `evidence`.
  script = hosts: ''
    def reading_groups(image):
        # Stand-in pods carry the labels the chart renders for each role of a release `rw` - no
        # reading worker process starts - and the policies are the chart's own render for two
        # groups, each declaring a different source host on the same port. Nothing here probes
        # the internet, and no question runs through a reader.
        rendered = json.load(open("${labels hosts}"))
        marked = lambda labels: {**labels, "fixture-stand-in": "true"}
        roles = {
            "rw-serving": ("machine", marked(rendered["serving"])),
            "rw-worker": ("machine", marked(rendered["worker"])),
            "rw-reader-a": ("agent1", marked(rendered["readers"]["a"])),
            "rw-reader-b": ("agent2", marked(rendered["readers"]["b"])),
            "rw-outsider": ("agent1", {"fixture-stand-in": "true"}),
        }
        listeners = []
        try:
            for name, (node, labels) in roles.items():
                apply({"apiVersion": "v1", "kind": "Pod", "metadata": {"name": name, "labels": labels},
                       "spec": {"nodeName": node, "automountServiceAccountToken": False,
                                "containers": [{"name": "probe", "image": image, "imagePullPolicy": "Never"}]}})
            machine.succeed("kubectl wait --for=condition=Ready pod -l fixture-stand-in=true --timeout=120s")
            stand = {name: get("pod/" + name) for name in roles}
            address_of = {name: pod["status"]["podIP"] for name, pod in stand.items()}
            for name in ["rw-serving", "rw-worker", "rw-reader-a", "rw-reader-b"]:
                host, prefix = namespace(stand[name])
                pid = int(host.succeed(prefix + "sh -c 'nc -lk 8080 </dev/null >/dev/null 2>&1 & echo $!'"))
                listeners.append((host, pid))
                eventually(lambda: tcp(stand[name], address_of[name], 8080), 30)
            dns = get("service/kube-dns", "kube-system")["spec"]["clusterIP"]
            a, b = stand["rw-reader-a"], stand["rw-reader-b"]
            serving, worker, outsider = stand["rw-serving"], stand["rw-worker"], stand["rw-outsider"]
            # Control before any policy: every path below is open, so a later denial is a policy's.
            for origin, address, port in [
                (a, SOURCE, 5432), (a, "${hosts.dbTwoHost}", 5432), (b, SOURCE, 5432), (b, "${hosts.dbTwoHost}", 5432),
                (a, SOURCE, 5433),
                (outsider, address_of["rw-reader-a"], 8080),
                (worker, SOURCE, 5432), (worker, "${dbTwoHost}", 5432),
            ]:
                assert tcp(origin, address, port), (origin["metadata"]["name"], address, port)
            machine.succeed("helm template rw ${chart} -f ${values hosts} "
                            "--show-only templates/networkpolicy.yaml | kubectl apply -f -")
            assert {"rw-reading-a", "rw-reading-b", "rw-workers-egress"} <= {
                p["metadata"]["name"] for p in get("networkpolicies")["items"]}
            eventually(lambda: not tcp(a, "${hosts.dbTwoHost}", 5432), 30)
            # What stays open: the release's nodes reach one another on the service port, another
            # group's readers included, each group reaches its own source, and DNS answers.
            for origin, address, port in [
                (serving, address_of["rw-reader-a"], 8080), (serving, address_of["rw-reader-b"], 8080),
                (worker, address_of["rw-reader-a"], 8080), (worker, address_of["rw-reader-b"], 8080),
                (a, address_of["rw-reader-b"], 8080), (b, address_of["rw-reader-a"], 8080),
                (a, address_of["rw-serving"], 8080), (b, address_of["rw-worker"], 8080),
                (worker, address_of["rw-serving"], 8080),
                (a, SOURCE, 5432), (b, "${hosts.dbTwoHost}", 5432), (a, dns, 53), (b, dns, 53),
            ]:
                assert tcp(origin, address, port), (origin["metadata"]["name"], address, port)
            # Each group's source egress is its own: the other group's source is a different
            # address on the same port, and neither group reaches it.
            denied_with_control(a, "${hosts.dbTwoHost}", 5432, "rw-reading-a")
            denied_with_control(b, SOURCE, 5432, "rw-reading-b")
            # A group's policy bounds the port as well as the address: its own source's address at
            # a port it did not declare is denied too.
            denied_with_control(a, SOURCE, 5433, "rw-reading-a")
            # A pod outside the release is not admitted by a group's ingress.
            denied_with_control(outsider, address_of["rw-reader-a"], 8080, "rw-reading-a")
            # A combining worker holds no source, so the chart's egress denies it both.
            for address in [SOURCE, "${hosts.dbTwoHost}"]:
                denied_with_control(worker, address, 5432, "rw-workers-egress")
            evidence["reading-groups"] = {
                "policies": [get("networkpolicy/" + name)["spec"] for name in
                             ["rw-reading-a", "rw-reading-b", "rw-workers-egress"]],
                "limit": "stand-in pods with the chart's rendered labels and policies; no reading worker process runs a question, and no internet destination is probed"}
        finally:
            for host, pid in listeners:
                host.execute(f"kill {pid}")
            machine.succeed("kubectl delete pod -l fixture-stand-in=true --wait=true")
            machine.succeed("kubectl delete networkpolicy rw rw-workers rw-workers-egress rw-reading-a rw-reading-b rw-operator --ignore-not-found")
  '';
}
