# The network probes of `k3s-worker-pool.nix` that read a chart policy: a connection test from inside
# a pod's network namespace, the delete-observe-restore control that attributes a denial to one
# policy, and the cell that checks the chart's per-group reading worker policies over stand-in pods.
{ pkgs, chart }:
let
  # Two reading worker groups over the one source host, each declaring its own port: what the
  # chart's per-group policy is rendered from, as release `rw`, so no real reader starts.
  values =
    dbHost:
    let
      source = name: {
        kind = "files";
        data_dir = "/srv/sutura/${name}";
        posture = "shared-service-user";
        acknowledged_because = "a directory this deployment owns";
      };
      group = name: port: {
        sources = [ name ];
        egress = [
          {
            cidr = "${dbHost}/32";
            inherit port;
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
          a = group "a" 5432;
          b = group "b" 5433;
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
in
{
  # The Python the test script splices in; it reads the script's own `machine`, `HOSTS`, `SOURCE`,
  # `get`, `apply`, `eventually`, `identity` and `evidence`.
  script = dbHost: ''
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


    def reading_groups(pause):
        # Stand-in pods carry the labels of each role of a release `rw` - no reading worker
        # process starts - and the policies are the chart's own render for two groups, each
        # declaring one port of the source host. The source's other port stands in for another
        # group's source; nothing here probes the internet.
        release = {"app.kubernetes.io/instance": "rw", "fixture-stand-in": "true"}
        reading = {"app.kubernetes.io/name": "sutura-reading-worker", **release}
        roles = {
            "rw-serving": ("machine", {"app.kubernetes.io/name": "sutura", **release}),
            "rw-worker": ("machine", {"app.kubernetes.io/name": "sutura-worker", **release}),
            "rw-reader-a": ("agent1", {**reading, "app.kubernetes.io/component": "a"}),
            "rw-reader-b": ("agent2", {**reading, "app.kubernetes.io/component": "b"}),
            "rw-outsider": ("agent1", {"fixture-stand-in": "true"}),
        }
        listeners = []
        try:
            for name, (node, labels) in roles.items():
                apply({"apiVersion": "v1", "kind": "Pod", "metadata": {"name": name, "labels": labels},
                       "spec": {"nodeName": node, "automountServiceAccountToken": False,
                                "containers": [{"name": "probe", "image": pause[0], "imagePullPolicy": "Never"}]}})
            machine.succeed("kubectl wait --for=condition=Ready pod -l fixture-stand-in=true --timeout=120s")
            stand = {name: get("pod/" + name) for name in roles}
            address_of = {name: pod["status"]["podIP"] for name, pod in stand.items()}
            for name in ["rw-serving", "rw-worker", "rw-reader-a", "rw-reader-b"]:
                host, prefix = namespace(stand[name])
                host.succeed(prefix + "sh -c '(nc -lk 8080 </dev/null >/dev/null 2>&1 &)'")
                listeners.append(host)
            dns = get("service/kube-dns", "kube-system")["spec"]["clusterIP"]
            a, b = stand["rw-reader-a"], stand["rw-reader-b"]
            # Control before any policy: every path below is open, so a later denial is a policy's.
            for origin, address, port in [
                (a, SOURCE, 5432), (a, SOURCE, 5433), (b, SOURCE, 5432), (b, SOURCE, 5433),
                (stand["rw-outsider"], address_of["rw-reader-a"], 8080),
                (b, address_of["rw-reader-a"], 8080), (a, address_of["rw-reader-b"], 8080),
                (stand["rw-worker"], SOURCE, 5432),
            ]:
                assert tcp(origin, address, port), (origin["metadata"]["name"], address, port)
            machine.succeed("helm template rw ${chart} -f ${values dbHost} "
                            "--show-only templates/networkpolicy.yaml | kubectl apply -f -")
            assert {"rw-reading-a", "rw-reading-b", "rw-workers-egress"} <= {
                p["metadata"]["name"] for p in get("networkpolicies")["items"]}
            eventually(lambda: not tcp(a, SOURCE, 5433), 30)
            # What each group may still do.
            for origin, address, port in [
                (stand["rw-serving"], address_of["rw-reader-a"], 8080),
                (stand["rw-worker"], address_of["rw-reader-a"], 8080),
                (stand["rw-serving"], address_of["rw-reader-b"], 8080),
                (stand["rw-worker"], address_of["rw-reader-b"], 8080),
                (a, SOURCE, 5432), (b, SOURCE, 5433), (a, dns, 53), (b, dns, 53),
                (stand["rw-worker"], address_of["rw-serving"], 8080),
            ]:
                assert tcp(origin, address, port), (origin["metadata"]["name"], address, port)
            # Each group's egress is its own: neither reaches the other's source.
            denied_with_control(a, SOURCE, 5433, "rw-reading-a")
            denied_with_control(b, SOURCE, 5432, "rw-reading-b")
            # A pod outside the release is not admitted by a group's ingress.
            denied_with_control(stand["rw-outsider"], address_of["rw-reader-a"], 8080, "rw-reading-a")
            # Another group's reader is not admitted either. With the other group's policy gone,
            # its pod is unbounded, so the denial is group a's ingress alone.
            other = saved_policy("rw-reading-b")
            machine.succeed("kubectl delete networkpolicy rw-reading-b")
            try:
                eventually(lambda: tcp(b, SOURCE, 5432), 30)
                denied_with_control(b, address_of["rw-reader-a"], 8080, "rw-reading-a")
            finally:
                apply(other)
            eventually(lambda: not tcp(b, SOURCE, 5432), 30)
            # A combining worker holds no source, so the chart's egress denies it both ports.
            for port in [5432, 5433]:
                denied_with_control(stand["rw-worker"], SOURCE, port, "rw-workers-egress")
            evidence["reading-groups"] = {
                "policies": [get("networkpolicy/" + name)["spec"] for name in
                             ["rw-reading-a", "rw-reading-b", "rw-workers-egress"]],
                "limit": "stand-in pods and the chart's rendered policies; no reading worker process, no internet destination"}
        finally:
            for host in listeners:
                host.execute("pkill -f 'nc -lk 8080'")
            machine.succeed("kubectl delete pod -l fixture-stand-in=true --wait=true")
            machine.succeed("kubectl delete networkpolicy rw rw-workers rw-workers-egress rw-reading-a rw-reading-b rw-operator --ignore-not-found")
  '';
}
