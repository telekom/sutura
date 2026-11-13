# Three `sutura serve` processes and a PostgreSQL server in four NixOS VMs (`telekom/sutura#828`).
# `query` serves; `reader1` and `reader2` are `server.role: reading-worker` peers that read `geo`;
# `db` holds `geo`'s tables. `query` answers recurring revenue by region. Its fact leg reads fixture
# files on `query`, and its lookup leg reads `geo`, whose root holds the example's customers and
# 1,200,000 filler rows that no subscription names, analysed, so PostgreSQL estimates it past one
# stream's 1,000,000 rows. `query` splits that leg into two partitions and grants them to the caller.
#
# What is observed: the answer equals the one-source golden. PostgreSQL's statement log, prefixed
# with each client's address, holds a `ctid` range read from each reader and none from `query`, which
# exports the snapshot. With both readers stopped the question is refused `503 node_lost` and no
# range is read anywhere; started again, the question is answered. Each reader declares `geo` alone,
# beside a catalog that also reads the fixture files. A worker call with a certificate that another
# authority signed fails its handshake on a reader, and an anonymous question is refused at `query`.
#
# What this does not show: nobody signs in - the test mints the caller's token over one key. Every
# node reads `geo` as one PostgreSQL role, so the log tells the nodes apart by address alone. One CA
# signs every node certificate and PostgreSQL's, and is also the node authority. The current combiner
# reads each partition once, with its link check in the joining plan; the native log measures
# actual calls rather than inferring them from partition count. That `query` read no range rests on PostgreSQL logging a statement only when it is executed: `query` describes the
# first partition's statement without running it. A stopped reader still resolves, so it is lost
# rather than unresolved: `source_unavailable` for a reader that resolves to nothing is the
# in-process cell's (`crates/sutura-cli/tests/served/split_grant.rs`). Nothing here measures load.
{ pkgs, sutura, postgresDriver, example, golden }:
let
  python = pkgs.python3.withPackages (p: [ p.pyjwt p.cryptography ]);
  idp = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "multi-node-postgres";
  version = "multi-node-postgres";
  password = "multi-node-postgres-password";

  # A CA, each node's TLS leaf under it, a client leaf under another CA, the identity provider's
  # ES256 key and its key set, and the role's password. No CA key is kept.
  material = pkgs.runCommand "multi-node-postgres-material" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir "$out" && cd "$out"
    printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=multi-node-postgres-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
    for node in query reader1 reader2 db; do
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
    printf ${password} > postgres-password
  '';

  # The example catalog with `customers` and `regions` on `geo` and `customers` reading the split
  # table - the derivation `crates/sutura-cli/tests/served/split_grant.rs` makes, for its reasons.
  # A rewrite that no longer matches fails the build.
  catalog = pkgs.runCommand "multi-node-postgres-catalog" { } ''
    cp -r ${example}/catalog "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/models/customers.md" --replace-fail 'source: local' 'source: geo' --replace-fail 'table: dim_customer' 'table: dim_customer_split'
    substituteInPlace "$out/models/regions.md" --replace-fail 'source: local' 'source: geo'
  '';

  # `geo`'s two tables, owned by the role every node reads as, and analysed.
  load = pkgs.writeText "multi-node-postgres-load.sql" ''
    CREATE TABLE dim_customer_split (customer_key BIGINT, customer_id TEXT, segment TEXT, region TEXT);
    \copy dim_customer_split FROM '${example}/data/dim_customer.csv' WITH (FORMAT csv, HEADER true)
    INSERT INTO dim_customer_split SELECT 1000000 + n, 'F' || n, 'consumer', 'filler' FROM generate_series(1, 1200000) AS n;
    CREATE TABLE dim_region (region TEXT, sales_area TEXT);
    \copy dim_region FROM '${example}/data/dim_region.csv' WITH (FORMAT csv, HEADER true)
    ALTER TABLE dim_customer_split OWNER TO sutura;
    ALTER TABLE dim_region OWNER TO sutura;
    ANALYZE dim_customer_split;
    ANALYZE dim_region;
  '';

  settings = { server, security, sources }: pkgs.writeTextDir "base.yaml" (builtins.toJSON {
    inherit server security sources;
    telemetry.format = "bunyan";
    rate_limit.enabled = true;
    catalogs = [{ name = "model"; kind = "markdown"; dir = "${catalog}"; data_dir = "${example}/data"; inherit version; }];
  });
  # A node's listener over TLS, beside its node certificate and authority.
  pair = own: {
    host = "0.0.0.0";
    port = 8443;
    tls_certificate = "${material}/${own}.pem";
    tls_key = "${material}/${own}.key";
    tls_node_authority = "${material}/ca.pem";
  };
  offHost = { tls_termination = "in-process"; metrics_token = "multi-node-postgres-metrics-token-000"; };
  geo = {
    kind = "postgres";
    host = "db";
    port = 5432;
    database = "sutura";
    user = "sutura";
    password_file = "${material}/postgres-password";
    transport_mode = "verified";
    transport_anchors = "${material}/ca.pem";
    posture = "shared-service-user";
  };
  acknowledged = { acknowledged_because = "every node reads fixture tables as one identity"; };
  inbound = {
    mode = "direct";
    resource = "https://query";
    authorization_server = "https://idp";
    key_set_file = "${material}/jwks.json";
    algorithms = [ "ES256" ];
  };

  reader = own: {
    virtualisation.memorySize = 2048;
    networking.firewall.allowedTCPPorts = [ 8443 ];
    systemd.services.sutura = serve
      (settings {
        server = pair own // { role = "reading-worker"; };
        security = offHost // { inherit inbound; identity = "single-user"; single_user_because = "a reading worker reads one source as its declared role"; };
        sources = { inherit geo; };
      });
  };

  # Started by the test script, once what it dials is up.
  serve = config: {
    environment = { SUTURA_CONFIG_DIR = "${config}"; SUTURA_ENVIRONMENT = "development"; SUTURA_POSTGRES_ADBC_DRIVER = postgresDriver; };
    serviceConfig = { ExecStart = "${sutura}/bin/sutura serve"; DynamicUser = true; };
  };
in
pkgs.testers.runNixOSTest {
  name = "multi-node-postgres";

  nodes.db = {
    virtualisation.memorySize = 2048;
    networking.firewall.allowedTCPPorts = [ 5432 ];
    # PostgreSQL refuses a key file other users can read, and a store path is world-readable.
    systemd.services.postgres-tls = {
      wantedBy = [ "postgresql.service" ];
      before = [ "postgresql.service" ];
      serviceConfig.Type = "oneshot";
      script = "install -d -m 0700 -o postgres -g postgres /run/postgres-tls && install -m 0600 -o postgres -g postgres ${material}/db.key /run/postgres-tls/db.key";
    };
    services.postgresql = {
      enable = true;
      package = pkgs.postgresql_18;
      enableTCPIP = true;
      authentication = pkgs.lib.mkForce ''
        local all postgres peer
        hostssl sutura sutura all scram-sha-256
      '';
      initialScript = pkgs.writeText "multi-node-postgres-init.sql" ''
        CREATE ROLE sutura LOGIN PASSWORD '${password}';
        CREATE DATABASE sutura OWNER sutura;
      '';
      settings = {
        ssl = true;
        ssl_cert_file = "${material}/db.pem";
        ssl_key_file = "/run/postgres-tls/db.key";
        password_encryption = "scram-sha-256";
        log_statement = "all";
        logging_collector = true;
        log_destination = pkgs.lib.mkForce "jsonlog";
        log_filename = "worker-receipts.log";
        log_rotation_age = 0;
        log_rotation_size = 0;
        log_line_prefix = "%h ";
      };
    };
  };

  nodes.reader1 = reader "reader1";
  nodes.reader2 = reader "reader2";

  nodes.query = {
    virtualisation.memorySize = 2048;
    environment.systemPackages = [ pkgs.curl ];
    networking.firewall.allowedTCPPorts = [ 8443 ];
    systemd.services.sutura = serve
      (settings {
        server = pair "query" // {
          # One byte, so every combine reading both sources goes across: a granted leg does anyway.
          nodes = {
            own = "https://query:8443";
            others = map (other: { url = "https://${other}:8443"; role = "reading-worker"; sources = [ "geo" ]; }) [ "reader1" "reader2" ];
            across_above_bytes = 1;
          };
        };
        security = offHost // {
          identity = "multi-user";
          inherit inbound;
        };
        sources = {
          local = {
            kind = "files";
            data_dir = "${example}/data";
            posture = "shared-service-user";
          } // acknowledged;
          geo = geo // acknowledged;
        };
      });
  };

  testScript = { nodes, ... }: ''
    import json
    import shlex
    import time
    from collections import Counter

    QUESTION = '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
    # Both families: PostgreSQL logs whichever one the client connected over.
    ADDRESSES = {
        "query": {"${nodes.query.networking.primaryIPAddress}", "${nodes.query.networking.primaryIPv6Address}"},
        "reader1": {"${nodes.reader1.networking.primaryIPAddress}", "${nodes.reader1.networking.primaryIPv6Address}"},
        "reader2": {"${nodes.reader2.networking.primaryIPAddress}", "${nodes.reader2.networking.primaryIPv6Address}"},
    }
    NODE = {address: node for node, addresses in ADDRESSES.items() for address in addresses}


    def cell(raw: str) -> str:
        return "null" if raw == "Null" else raw.split("(", 1)[1].removesuffix(")")


    def golden() -> dict[str, int]:
        with open("${golden}") as handle:
            header, *body = handle.read().split("---\n")[2].strip().splitlines()
        rows = [dict(zip(header.split("\t"), map(cell, line.split("\t")))) for line in body]
        return {row["region"]: int(row["recurring_revenue"]) for row in rows}


    def asking() -> str:
        token = query.succeed("${idp} mint ${material}/idp.key ${kid} user@example.com https://query sutura:metrics.ask").strip()
        return f"curl -sS --cacert ${material}/ca.pem -H 'Authorization: Bearer {token}' -H 'Content-Type: application/json' --data '{QUESTION}' https://query:8443/v1/query"


    def answered() -> dict:
        reply = json.loads(query.succeed(asking()))
        assert reply.get("outcome") == "answer", reply
        return reply


    def ask() -> dict[str, int]:
        reply = answered()
        assert reply.get("outcome") == "answer", reply
        region, revenue = reply["columns"].index("region"), reply["columns"].index("recurring_revenue")
        return {row[region]: int(row[revenue]) for row in reply["rows"]}


    def statements() -> list[dict]:
        # One bounded structured log; setup and observer records never enter the client set.
        path = db.succeed("runuser -u postgres -- psql -Atc \"SELECT current_setting('data_directory') || '/' || pg_current_logfile('jsonlog')\"").strip()
        assert path, "PostgreSQL did not open its JSON log"
        raw = db.succeed(f"head -c 4194305 -- {shlex.quote(path)}")
        assert len(raw.encode()) <= 4194304, "native query log exceeded the fixture bound"
        records = [json.loads(line) for line in raw.splitlines()]
        return [record for record in records if record.get("remote_host") in NODE]


    def by_client(records: list[dict]) -> dict[str, list[str]]:
        seen: dict[str, list[str]] = {}
        for record in records:
            seen.setdefault(NODE[record["remote_host"]], []).append(record["message"])
        return seen


    def submitted(message: str) -> str | None:
        if message.startswith("statement: "):
            return message.removeprefix("statement: ")
        if message.startswith("execute "):
            return message.partition(": ")[2]
        return None


    def range_sql(records: list[dict], node: str) -> list[str]:
        return [sql for message in by_client(records).get(node, [])
                if (sql := submitted(message)) is not None and "ctid" in sql]


    def ranges(records: list[dict], node: str) -> int:
        return len(range_sql(records, node))


    def readers_up() -> None:
        for reader in ("reader1", "reader2"):
            query.wait_until_succeeds(f"curl -fsS --cacert ${material}/ca.pem https://{reader}:8443/health", timeout=120)


    start_all()
    db.wait_for_unit("postgresql.service")
    db.wait_for_unit("postgresql-setup.service")
    db.succeed("runuser -u postgres -- psql -v ON_ERROR_STOP=1 -d sutura -f ${load}")
    reader1.succeed("systemctl start sutura")
    reader2.succeed("systemctl start sutura")
    readers_up()
    query.succeed("systemctl start sutura")
    query.wait_until_succeeds("curl -fsS --cacert ${material}/ca.pem https://query:8443/health", timeout=120)

    expected = golden()
    before = len(statements())
    with subtest("a lookup split across two reading workers answers what the whole leg answers"):
        reply = answered()
        region, revenue = reply["columns"].index("region"), reply["columns"].index("recurring_revenue")
        assert {row[region]: int(row[revenue]) for row in reply["rows"]} == expected, reply

    with subtest("native worker receipts match the per-query PostgreSQL submission multiset"):
        lines = statements()[before:]
        receipts = [record for record in reply["executions"]
                    if record["kind"] == "sql" and record["source"] == "geo"]
        assert receipts, receipts
        assert all(record["dialect"] == "postgres" and record["parameters"] == [] for record in receipts), receipts
        # A Counter preserves repeated native reads; a set would hide a lost receipt.
        for _ in range(100):
            native = range_sql(lines, "reader1") + range_sql(lines, "reader2")
            if Counter(record["sql"] for record in receipts) == Counter(native):
                break
            time.sleep(0.1)
            lines = statements()[before:]
        assert native and Counter(record["sql"] for record in receipts) == Counter(native), (receipts, native)

    with subtest("each reader read a ctid range of the lookup root, and the serving node read none"):
        lines = statements()[before:]
        assert ranges(lines, "reader1") > 0 and ranges(lines, "reader2") > 0, lines
        assert ranges(lines, "query") == 0, lines
        assert any("pg_export_snapshot" in line for line in by_client(lines).get("query", [])), lines

    with subtest("with both readers stopped the question is refused as node_lost and nothing reads a range"):
        reader1.succeed("systemctl stop sutura")
        reader2.succeed("systemctl stop sutura")
        before = len(statements())
        status = query.succeed(f"{asking()} -o /tmp/lost.json -w '%{{http_code}}'")
        body = query.succeed("cat /tmp/lost.json")
        assert status == "503" and json.loads(body)["reason"]["code"] == "node_lost", (status, body)
        lines = statements()[before:]
        assert sum(ranges(lines, node) for node in ADDRESSES) == 0, lines

    with subtest("started again, the readers answer the question"):
        reader1.succeed("systemctl start sutura")
        reader2.succeed("systemctl start sutura")
        readers_up()
        assert ask() == expected, expected

    with subtest("an anonymous question is refused at the node it was asked"):
        status = query.succeed(f"curl -sS -o /dev/null -w '%{{http_code}}' --cacert ${material}/ca.pem -H 'Content-Type: application/json' --data '{QUESTION}' https://query:8443/v1/query")
        assert status == "401", status

    with subtest("a worker call to a reader with a certificate another authority signed fails its handshake"):
        query.fail("curl -sS -o /dev/null --http2 --cacert ${material}/ca.pem -X POST -H 'Content-Type: application/grpc' --cert ${material}/stranger.pem --key ${material}/stranger.key https://reader1:8443/worker.WorkerService/ExecuteTask")
  '';
}
