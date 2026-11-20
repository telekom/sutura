# The network probes of `k3s-worker-pool.nix` that read a chart policy: a connection test from inside
# a pod's network namespace, the delete-observe-restore control that attributes a denial to one
# policy, and the cell that checks the chart's per-group reading worker policies over stand-in pods.
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
  # The Python the test script splices in; it reads the script's own `machine`, `HOSTS`, `SOURCE`,
  # `get`, `apply`, `eventually`, `identity` and `evidence`.
  script = hosts: ''
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


    def saved_policy(name):
        saved = get("networkpolicy/" + name)
        saved["metadata"] = {key: value for key, value in saved["metadata"].items()
                             if key in {"name", "namespace", "labels", "annotations"}}
        return saved


    def denied_with_control(pod, address, port, policy):
        names = [policy] if isinstance(policy, str) else list(policy)
        saved = [saved_policy(name) for name in names]
        machine.succeed("kubectl delete networkpolicy " + " ".join(q(name) for name in names))
        try:
            eventually(lambda: tcp(pod, address, port), 30)
        finally:
            for policy_object in saved:
                apply(policy_object)
        for policy_object in saved:
            assert get("networkpolicy/" + policy_object["metadata"]["name"])["spec"] == policy_object["spec"]
        eventually(lambda: not tcp(pod, address, port), 30)
        for _ in range(3):
            assert not tcp(pod, address, port), (identity(pod), address, port)
        denied_observations.append([identity(pod), address, port, names])
        evidence["denied"] = denied_observations


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
                host.succeed(prefix + "sh -c '(nc -lk 8080 </dev/null >/dev/null 2>&1 &)'")
                listeners.append(host)
            dns = get("service/kube-dns", "kube-system")["spec"]["clusterIP"]
            a, b = stand["rw-reader-a"], stand["rw-reader-b"]
            serving, worker, outsider = stand["rw-serving"], stand["rw-worker"], stand["rw-outsider"]
            # Control before any policy: every path below is open, so a later denial is a policy's.
            for origin, address, port in [
                (a, SOURCE, 5432), (a, SOURCE_TWO, 5432), (b, SOURCE, 5432), (b, SOURCE_TWO, 5432),
                (outsider, address_of["rw-reader-a"], 8080),
                (worker, SOURCE, 5432), (worker, SOURCE_TWO, 5432),
            ]:
                assert tcp(origin, address, port), (origin["metadata"]["name"], address, port)
            machine.succeed("helm template rw ${chart} -f ${values hosts} "
                            "--show-only templates/networkpolicy.yaml | kubectl apply -f -")
            assert {"rw-reading-a", "rw-reading-b", "rw-workers-egress"} <= {
                p["metadata"]["name"] for p in get("networkpolicies")["items"]}
            eventually(lambda: not tcp(a, SOURCE_TWO, 5432), 30)
            # What stays open: the release's nodes reach one another on the service port, another
            # group's readers included, each group reaches its own source, and DNS answers.
            for origin, address, port in [
                (serving, address_of["rw-reader-a"], 8080), (serving, address_of["rw-reader-b"], 8080),
                (worker, address_of["rw-reader-a"], 8080), (worker, address_of["rw-reader-b"], 8080),
                (a, address_of["rw-reader-b"], 8080), (b, address_of["rw-reader-a"], 8080),
                (a, address_of["rw-serving"], 8080), (b, address_of["rw-worker"], 8080),
                (worker, address_of["rw-serving"], 8080),
                (a, SOURCE, 5432), (b, SOURCE_TWO, 5432), (a, dns, 53), (b, dns, 53),
            ]:
                assert tcp(origin, address, port), (origin["metadata"]["name"], address, port)
            # Each group's source egress is its own: the other group's source is a different
            # address on the same port, and neither group reaches it.
            denied_with_control(a, SOURCE_TWO, 5432, "rw-reading-a")
            denied_with_control(b, SOURCE, 5432, "rw-reading-b")
            # A pod outside the release is not admitted by a group's ingress.
            denied_with_control(outsider, address_of["rw-reader-a"], 8080, "rw-reading-a")
            # A combining worker holds no source, so the chart's egress denies it both.
            for address in [SOURCE, SOURCE_TWO]:
                denied_with_control(worker, address, 5432, "rw-workers-egress")
            evidence["reading-groups"] = {
                "policies": [get("networkpolicy/" + name)["spec"] for name in
                             ["rw-reading-a", "rw-reading-b", "rw-workers-egress"]],
                "limit": "stand-in pods with the chart's rendered labels and policies; no reading worker process runs a question, and no internet destination is probed"}
        finally:
            for host in listeners:
                host.execute("pkill -f 'nc -lk 8080'")
            machine.succeed("kubectl delete pod -l fixture-stand-in=true --wait=true")
            machine.succeed("kubectl delete networkpolicy rw rw-workers rw-workers-egress rw-reading-a rw-reading-b rw-operator --ignore-not-found")
  '';
}
