# Two `sutura serve` processes and a `server.role: worker` in three NixOS VMs, over the network
# between them (`telekom/sutura#828`). `query` answers recurring revenue by region; `customers` lives
# on `source`, so `query` sends that leg over Flight to `source`, which runs it against ClickHouse
# signed in as the caller. `query` declares `source` and `spare` as its other nodes and a leg size of
# one byte, so it plans the combine's stages above a repartition across all three rather than on
# itself first.
#
# What is observed: the federated answer equals the one-source golden for a caller ClickHouse shows
# every customer, and for a caller its row policy shows only `north`, everything else falls to the
# outer join's null region - so the leg ran as the caller's own ClickHouse user, two hops from where
# they asked. Each hop's exchange is read out of the identity provider's journal, and an anonymous
# question is refused at `query`. After a question `source` holds a link it dialled to a node's
# listener and before any none: a node dials one only to read the input of a stage it was sent.
# `spare`, killed while a stage it runs reads its input, fails that question as `node_lost`, a 503;
# started again, the next question is answered across all three. The wall clock of twenty answers is
# printed after the conditions it was taken under (`telekom/sutura#140`).
# `source`'s worker service refuses a call with no client certificate, fails the handshake of one with
# a certificate another CA signed, and passes a call with `query`'s own certificate on to the engine.
#
# What this does not show: nobody signs in - `nix/multi-node-idp.py` is a fake identity provider, one
# per VM over one key, and the test mints the callers' tokens. Nothing here measures load. The lost
# node is killed, so its peers see its connections close: a node that vanishes without closing them,
# as a lost machine or a partition does, is not measured, and neither is a lost serving node. `spare`'s
# stage is caught running by its outbound traffic being held 1.5 s, so the link it dialled is up while
# the stage's input is still on its way. The wall clock is one VM's, under nested virtualisation on a
# shared CI runner, over a network of VMs on one host. The link shows that a stage ran on `source`, not
# how much of the combine did. One CA signs every node certificate and is also the node authority. A
# call with a node's certificate is any node's: nothing checks which task it may run.
{ pkgs, sutura, clickhouse, clickhouseDriver, example, golden }:
let
  python = pkgs.python3.withPackages (p: [ p.pyjwt p.cryptography ]);
  idp = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "multi-node";
  servicePassword = "multi-node-service-password";
  version = "multi-node";

  # A CA, each node's TLS leaf under it, a client leaf under another CA, the identity provider's
  # ES256 key and its key set, and the test's fixed secrets. No CA key is kept.
  material = pkgs.runCommand "multi-node-material" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir "$out" && cd "$out"
    printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=multi-node-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
    for node in query source spare; do
      openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout $node.key -out $node.csr -subj /CN=$node
      printf 'subjectAltName=DNS:%s\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\n' $node > $node.ext
      openssl x509 -req -in $node.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out $node.pem -days 3650 -extfile $node.ext
      rm $node.csr $node.ext
    done
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout other.key -out other.pem -days 3650 -config ca.cnf -subj /CN=other-ca
    openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout stranger.key -out stranger.csr -subj /CN=stranger
    printf 'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n' > stranger.ext
    openssl x509 -req -in stranger.csr -CA other.pem -CAkey other.key -CAcreateserial -out stranger.pem -days 3650 -extfile stranger.ext
    rm ca.key ca.srl ca.cnf other.key other.pem other.srl stranger.csr stranger.ext
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:prime256v1 -out idp.key
    openssl pkey -in idp.key -pubout -out idp.pub
    ${idp} jwks idp.pub ${kid} > jwks.json
    printf multi-node-client-secret > client-secret
    printf ${servicePassword} > service-password
  '';

  # The example catalog with `customers` moved onto `sales` - the rewrite
  # `crates/sutura-cli/tests/served/harness/two_source.rs` makes, for its reasons. A block that no
  # longer matches fails the build.
  chained = "  - name: sales_area\n    column: sales_area\n    via: [subscription_customer, customer_region]\n    values: [central, north_east, south_west]\n    description: >\n      Which sales area the customer's region rolls up into. The one dimension here\n      reached through a chain of two relationships rather than one.\n";
  catalog = pkgs.runCommand "multi-node-catalog" { } ''
    cp -r ${example}/catalog "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/models/customers.md" --replace-fail 'source: local' 'source: sales'
    substituteInPlace "$out/metrics/recurring_revenue.md" --replace-fail ${pkgs.lib.escapeShellArg chained} ""
    rm "$out/knowledge/caveats/sales-area-as-of-today.md"
  '';

  inbound = resource: {
    mode = "direct";
    inherit resource;
    authorization_server = "https://idp";
    key_set_file = "${material}/jwks.json";
    algorithms = [ "ES256" ];
  };
  delegation = audience: [{
    token_endpoint = "http://127.0.0.1:9100/token";
    client_id = "sutura";
    client_secret_file = "${material}/client-secret";
    inherit audience;
  }];
  settings = { server, security, sources, rest ? { } }: pkgs.writeTextDir "base.yaml" (builtins.toJSON ({
    inherit server sources;
    security = { identity = "multi-user"; } // security;
    telemetry.format = "bunyan";
    catalogs = [{ name = "model"; kind = "markdown"; dir = "${catalog}"; data_dir = "${example}/data"; inherit version; }];
  } // rest));
  # A node's listener over TLS, beside its node certificate and authority.
  pair = own: {
    host = "0.0.0.0";
    port = 8443;
    tls_certificate = "${material}/${own}.pem";
    tls_key = "${material}/${own}.key";
    tls_node_authority = "${material}/ca.pem";
  };
  # The same, declaring the other nodes.
  listener = own: others: pair own // {
    # The fixtures' legs are a few hundred bytes, which one node holds, so a combine reading both
    # data systems would stay on `query`; one byte sends every such combine across.
    nodes = { own = "https://${own}:8443"; others = map (other: "https://${other}:8443") others; across_above_bytes = 1; };
  };
  offHost = { tls_termination = "in-process"; metrics_token = "multi-node-metrics-token-0000000000"; };
  local = {
    kind = "files";
    data_dir = "${example}/data";
    posture = "shared-service-user";
    acknowledged_because = "the models off the other node read fixture files as one identity";
  };

  # Every user ClickHouse signs in through the checker, and the row policy each sees `dim_customer` under.
  callers = { analyst_a = "1"; analyst_b = "region = 'north'"; };
  clickhouseConfig = pkgs.writeText "clickhouse.yaml" (builtins.toJSON {
    logger = { level = "warning"; console = 1; };
    listen_host = "127.0.0.1";
    http_port = 8123;
    path = "/var/lib/clickhouse/";
    tmp_path = "/var/lib/clickhouse/scratch/";
    user_files_path = "/var/lib/clickhouse/user_files/";
    disable_internal_dns_cache = 1;
    users = {
      sutura = {
        password_sha256_hex = builtins.hashString "sha256" servicePassword;
        networks.ip = "127.0.0.1";
        profile = "default";
        quota = "default";
        grants.query = [ "GRANT ALL ON *.*" "REVOKE IMPERSONATE ON *" "REVOKE ACCESS MANAGEMENT ON *.*" ];
      };
    } // builtins.mapAttrs (_: filter: {
      http_authentication = { server = "checker"; scheme = "basic"; };
      networks.ip = "127.0.0.1";
      profile = "default";
      quota = "default";
      grants.query = [ "GRANT SELECT ON default.*" ];
      databases.default.dim_customer.filter = filter;
    }) callers;
    http_authentication_servers.checker.uri = "http://127.0.0.1:9101/";
    profiles.default = { };
    quotas.default = { };
  });

  daemon = command: {
    wantedBy = [ "multi-user.target" ];
    serviceConfig = { ExecStart = command; DynamicUser = true; };
  };
  # Started by the test script, once what it dials is up.
  serve = config: environment: {
    environment = { SUTURA_CONFIG_DIR = "${config}"; SUTURA_ENVIRONMENT = "development"; } // environment;
    serviceConfig = { ExecStart = "${sutura}/bin/sutura serve"; DynamicUser = true; };
  };
in
pkgs.testers.runNixOSTest {
  name = "multi-node";

  nodes.query = {
    virtualisation.memorySize = 2048;
    # One core plans one partition, so no repartition and no stage another node could run.
    virtualisation.cores = 2;
    environment.systemPackages = [ pkgs.curl ];
    networking.firewall.allowedTCPPorts = [ 8443 ];
    systemd.services.idp = daemon "${idp} idp ${material}/idp.key ${kid} ${material}/client-secret 9100";
    systemd.services.sutura = serve
      (settings {
        server = listener "query" [ "source" "spare" ];
        security = offHost // { inbound = inbound "https://query"; outbound.transport_anchors = "${material}/ca.pem"; };
        sources = {
          inherit local;
          sales = { kind = "node"; endpoint = "https://source:8443"; posture = "impersonation-at-source"; delegation = delegation "https://source"; };
        };
        rest.rate_limit.enabled = true;
      })
      { };
  };

  nodes.source = {
    virtualisation.memorySize = 3072;
    environment.systemPackages = [ pkgs.curl ];
    networking.firewall.allowedTCPPorts = [ 8443 ];
    systemd.services.idp = daemon "${idp} idp ${material}/idp.key ${kid} ${material}/client-secret 9100";
    systemd.services.checker = daemon "${idp} checker ${material}/idp.pub 9101";
    systemd.services.clickhouse = pkgs.lib.recursiveUpdate (daemon "${clickhouse}/bin/clickhouse server --config-file=${clickhouseConfig}") {
      environment.CLICKHOUSE_WATCHDOG_ENABLE = "0";
      serviceConfig.StateDirectory = "clickhouse";
    };
    systemd.services.sutura = serve
      (settings {
        server = listener "source" [ "query" ] // { flight_surface.enabled = true; };
        security = offHost // { inbound = inbound "https://source"; };
        sources = {
          inherit local;
          sales = {
            kind = "clickhouse";
            host = "127.0.0.1";
            port = 8123;
            user = "sutura";
            password_file = "${material}/service-password";
            transport_mode = "plaintext";
            posture = "impersonation-at-source";
            delegation = delegation "clickhouse";
          };
        };
        rest.rate_limit.enabled = true;
      })
      { SUTURA_CLICKHOUSE_ADBC_DRIVER = clickhouseDriver; };
  };

  # A pool worker: no catalog and no source, the worker transport its one surface.
  nodes.spare = {
    virtualisation.memorySize = 2048;
    networking.firewall.allowedTCPPorts = [ 8443 ];
    systemd.services.sutura = serve
      (pkgs.writeTextDir "base.yaml" (builtins.toJSON {
        server = pair "spare" // { role = "worker"; };
        security = offHost;
        telemetry.format = "bunyan";
        rate_limit.enabled = true;
      }))
      { };
  };

  testScript = ''
    import json
    import statistics

    QUESTION = '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
    SERVICE = "-H 'X-ClickHouse-User: sutura' -H 'X-ClickHouse-Key: ${servicePassword}'"


    def cell(raw: str) -> str:
        return "null" if raw == "Null" else raw.split("(", 1)[1].removesuffix(")")


    def golden() -> dict[str, int]:
        with open("${golden}") as handle:
            header, *body = handle.read().split("---\n")[2].strip().splitlines()
        rows = [dict(zip(header.split("\t"), map(cell, line.split("\t")))) for line in body]
        return {row["region"]: int(row["recurring_revenue"]) for row in rows}


    def asking(sub: str) -> str:
        token = query.succeed(f"${idp} mint ${material}/idp.key ${kid} {sub} https://query sutura:metrics.ask").strip()
        return f"curl -sS --cacert ${material}/ca.pem -H 'Authorization: Bearer {token}' -H 'Content-Type: application/json' --data '{QUESTION}' https://query:8443/v1/query"


    def ask(sub: str) -> dict[str, int]:
        reply = json.loads(query.succeed(asking(sub)))
        assert reply.get("outcome") == "answer", reply
        region, revenue = reply["columns"].index("region"), reply["columns"].index("recurring_revenue")
        return {row[region]: int(row[revenue]) for row in reply["rows"]}


    start_all()
    source.wait_for_unit("clickhouse.service")
    source.wait_until_succeeds("curl -fsS http://127.0.0.1:8123/ping")
    source.succeed(
        f"curl -fsS {SERVICE} --data-binary 'CREATE TABLE default.dim_customer (customer_key Int64, customer_id String, segment String, region String) ENGINE = MergeTree ORDER BY customer_key' http://127.0.0.1:8123/",
        f"curl -fsS {SERVICE} --data-binary @${example}/data/dim_customer.csv 'http://127.0.0.1:8123/?query=INSERT%20INTO%20default.dim_customer%20FORMAT%20CSVWithNames'",
    )
    source.wait_for_unit("checker.service")
    source.wait_for_unit("idp.service")
    source.succeed("systemctl start sutura")
    query.wait_until_succeeds("curl -fsS --cacert ${material}/ca.pem https://source:8443/health", timeout=120)
    spare.succeed("systemctl start sutura")
    query.wait_until_succeeds("curl -fsS --cacert ${material}/ca.pem https://spare:8443/health", timeout=120)
    query.wait_for_unit("idp.service")
    query.succeed("systemctl start sutura")
    query.wait_until_succeeds("curl -fsS --cacert ${material}/ca.pem https://query:8443/health", timeout=120)

    # A connection `source` dialled to a node's listener, its own or `query`'s.
    LINK = "ss -Htn state established dport = :8443 | grep -q ."
    source.fail(LINK)
    spare.fail(LINK)

    with subtest("a worker killed while a stage it runs reads its input fails the question as node_lost"):
        # Everything `spare` sends is held 1.5 s, so the stage is still running once its link is up.
        spare.succeed("tc qdisc add dev eth1 root netem delay 1500ms")
        query.succeed(f"({asking('analyst_a')} -o /tmp/lost.json -w '%{{http_code}}' > /tmp/lost.status 2> /dev/null < /dev/null &)")
        spare.wait_until_succeeds(LINK, timeout=60)
        spare.succeed("systemctl kill --signal=SIGKILL sutura")
        query.wait_until_succeeds("test -s /tmp/lost.status", timeout=60)
        status, body = query.succeed("cat /tmp/lost.status"), query.succeed("cat /tmp/lost.json")
        refused = json.loads(body).get("reason", {}).get("code") if body.startswith("{") else None
        assert (status, refused) == ("503", "node_lost"), (status, body, query.succeed("journalctl -u sutura -o cat | tail -n 30"))

    spare.succeed("tc qdisc del dev eth1 root", "systemctl start sutura")
    query.wait_until_succeeds("curl -fsS --cacert ${material}/ca.pem https://spare:8443/health", timeout=120)

    expected = golden()
    with subtest("a caller ClickHouse shows every customer gets the one-source answer, the worker started again"):
        assert ask("analyst_a") == expected, expected

    with subtest("source ran a stage of the combine, reading its input over a node link"):
        source.succeed(LINK)

    with subtest("a caller ClickHouse shows only north gets north, and the rest under the null region"):
        total = sum(expected.values())
        assert ask("analyst_b") == {"north": expected["north"], "null": total - expected["north"]}

    with subtest("each hop exchanged the caller's own subject, and ClickHouse admitted that user"):
        query.succeed("journalctl -u idp | grep -F 'exchange sub=analyst_b audience=https://source'")
        source.succeed("journalctl -u idp | grep -F 'exchange sub=analyst_b audience=clickhouse'")
        source.succeed("journalctl -u checker | grep -F 'admit analyst_b'")

    with subtest("twenty answers across the three nodes are timed, after the conditions they ran under"):
        conditions = query.succeed("echo \"$(systemd-detect-virt) $(nproc) cores $(free -m | awk '/^Mem:/ {print $2}') MiB load $(cut -d' ' -f1-3 /proc/loadavg)\"").strip()
        print(f"wall clock conditions: query {conditions}; three nodes, the leg over Flight to source and ClickHouse as the caller")
        seconds = sorted(float(query.succeed(f"{asking('analyst_a')} -o /dev/null -w '%{{time_total}}'")) for _ in range(20))
        print(f"wall clock of 20 federated answers (s): min {seconds[0]:.3f} median {statistics.median(seconds):.3f} p90 {seconds[17]:.3f} max {seconds[19]:.3f}")

    with subtest("an anonymous question is refused at the node it was asked"):
        status = query.succeed(f"curl -sS -o /dev/null -w '%{{http_code}}' --cacert ${material}/ca.pem -H 'Content-Type: application/json' --data '{QUESTION}' https://query:8443/v1/query")
        assert status == "401", status

    # A call to `source`'s worker service, as another node makes one, with an empty request.
    WORKER = "curl -sS -o /dev/null -w '%{http_code}' --http2 --cacert ${material}/ca.pem -X POST -H 'Content-Type: application/grpc' https://source:8443/worker.WorkerService/ExecuteTask"

    with subtest("a worker call without a client certificate is refused"):
        status = query.succeed(WORKER)
        assert status == "401", status

    with subtest("a worker call with a certificate another authority signed fails its handshake"):
        query.fail(f"{WORKER} --cert ${material}/stranger.pem --key ${material}/stranger.key")

    with subtest("a worker call with a node's certificate reaches the engine"):
        status = query.succeed(f"{WORKER} --cert ${material}/query.pem --key ${material}/query.key")
        assert status == "200", status
  '';
}
