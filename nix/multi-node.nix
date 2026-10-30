# Two `sutura serve` processes in two NixOS VMs, over the network between them (`telekom/sutura#828`).
# `query` answers recurring revenue by region; `customers` lives on `source`, so `query` sends that
# leg over Flight to `source`, which runs it against ClickHouse signed in as the caller.
#
# What is observed: the federated answer equals the one-source golden for a caller ClickHouse shows
# every customer, and for a caller its row policy shows only `north`, everything else falls to the
# outer join's null region - so the leg ran as the caller's own ClickHouse user, two hops from where
# they asked. Each hop's exchange is read out of the identity provider's journal, and an anonymous
# question is refused at `query`.
#
# What this does not show: nobody signs in - `nix/multi-node-idp.py` is a fake identity provider, one
# per VM over one key, and the test mints the callers' tokens. One question per caller: nothing here
# measures load, a slow peer or a node going away.
{ pkgs, sutura, clickhouse, clickhouseDriver, example, golden }:
let
  python = pkgs.python3.withPackages (p: [ p.pyjwt p.cryptography ]);
  idp = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "multi-node";
  servicePassword = "multi-node-service-password";
  version = "multi-node";

  # A CA, `source`'s TLS leaf under it, the identity provider's ES256 key and its key set, and the
  # test's fixed secrets. The CA key is not kept.
  material = pkgs.runCommand "multi-node-material" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir "$out" && cd "$out"
    printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=multi-node-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
    openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout source.key -out source.csr -subj /CN=source
    printf 'subjectAltName=DNS:source\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\n' > source.ext
    openssl x509 -req -in source.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out source.pem -days 3650 -extfile source.ext
    rm ca.key ca.srl ca.cnf source.csr source.ext
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
    environment.systemPackages = [ pkgs.curl ];
    systemd.services.idp = daemon "${idp} idp ${material}/idp.key ${kid} ${material}/client-secret 9100";
    systemd.services.sutura = serve
      (settings {
        server = { host = "127.0.0.1"; port = 8080; };
        security = { inbound = inbound "https://query"; outbound.transport_anchors = "${material}/ca.pem"; };
        sources = {
          inherit local;
          sales = { kind = "node"; endpoint = "https://source:8443"; posture = "impersonation-at-source"; delegation = delegation "https://source"; };
        };
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
        server = {
          host = "0.0.0.0";
          port = 8443;
          tls_certificate = "${material}/source.pem";
          tls_key = "${material}/source.key";
          flight_surface.enabled = true;
        };
        security = { tls_termination = "in-process"; metrics_token = "multi-node-metrics-token-0000000000"; inbound = inbound "https://source"; };
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

  testScript = ''
    import json

    QUESTION = '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
    SERVICE = "-H 'X-ClickHouse-User: sutura' -H 'X-ClickHouse-Key: ${servicePassword}'"


    def cell(raw: str) -> str:
        return "null" if raw == "Null" else raw.split("(", 1)[1].removesuffix(")")


    def golden() -> dict[str, int]:
        with open("${golden}") as handle:
            header, *body = handle.read().split("---\n")[2].strip().splitlines()
        rows = [dict(zip(header.split("\t"), map(cell, line.split("\t")))) for line in body]
        return {row["region"]: int(row["recurring_revenue"]) for row in rows}


    def ask(sub: str) -> dict[str, int]:
        token = query.succeed(f"${idp} mint ${material}/idp.key ${kid} {sub} https://query sutura:metrics.ask").strip()
        reply = json.loads(query.succeed(
            f"curl -sS -H 'Authorization: Bearer {token}' -H 'Content-Type: application/json' --data '{QUESTION}' http://127.0.0.1:8080/v1/query"
        ))
        assert reply["outcome"] == "answer", reply
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
    query.wait_for_unit("idp.service")
    query.succeed("systemctl start sutura")
    query.wait_until_succeeds("curl -fsS http://127.0.0.1:8080/health", timeout=120)

    expected = golden()
    with subtest("a caller ClickHouse shows every customer gets the one-source answer"):
        assert ask("analyst_a") == expected, expected

    with subtest("a caller ClickHouse shows only north gets north, and the rest under the null region"):
        total = sum(expected.values())
        assert ask("analyst_b") == {"north": expected["north"], "null": total - expected["north"]}

    with subtest("each hop exchanged the caller's own subject, and ClickHouse admitted that user"):
        query.succeed("journalctl -u idp | grep -F 'exchange sub=analyst_b audience=https://source'")
        source.succeed("journalctl -u idp | grep -F 'exchange sub=analyst_b audience=clickhouse'")
        source.succeed("journalctl -u checker | grep -F 'admit analyst_b'")

    with subtest("an anonymous question is refused at the node it was asked"):
        status = query.succeed(f"curl -sS -o /dev/null -w '%{{http_code}}' -H 'Content-Type: application/json' --data '{QUESTION}' http://127.0.0.1:8080/v1/query")
        assert status == "401", status
  '';
}
