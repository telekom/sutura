# TPC-H across nodes against one node, in four NixOS VMs (`telekom/sutura#828`).
#
# `query` holds the generated star (`nix/tpch-data.nix`) as a DuckDB file and serves three
# `sutura serve` topologies over it, one running at a time:
#
#   REF   every model on the DuckDB source: one statement, no federation. The answer.
#   ONE   the federated catalog (facts in DuckDB, dimensions in PostgreSQL), no `server.nodes`.
#   MANY  the same catalog with `reader1` and `reader2` as `server.role: reading-worker` peers of the
#         PostgreSQL source, which `db` holds. A lookup leg over a table past one stream's rows is
#         split by `ctid` range and granted to the readers.
#
# `nix/tpch/acceptance.json` names the questions MANY must answer and agree with REF on, and
# `nix/tpch-across-nodes.py` is what asks them and judges. At SF10 `customer` (1,500,000 rows) and
# `part` (2,000,000) are past the 1,000,000 a stream may carry, so ONE refuses an unfiltered lookup
# over either as `result_too_large` and MANY reads it in two partitions.
#
# What is observed: each listed answer on MANY equals REF's, by `sutura-dev agree`. PostgreSQL's
# statement log, prefixed with each client's address, holds exactly the leg's partitions as distinct
# `ctid` ranges read by the readers, each reader reading at least one, none by `query`, which
# exported the snapshot. A question that splits no leg shows no range at all.
#
# What this does not show: nobody signs in - the test mints the caller's token over one key. Every
# node reads PostgreSQL as one role, so the log tells nodes apart by address alone. The fact leg is
# read whole on `query`, so only lookup legs are distributed, and nothing here measures load. REF
# and MANY share the semantic compiler: a defect in the plan they share agrees with itself. A split
# is cut by pages, so a table of exactly two streams' rows has no slack and a partition may pass the
# bound; such a question is only observed. One CA signs every node certificate and PostgreSQL's.
# The VMs run under KVM, so the times are this host's and not a deployment's.
#
# `reltuples` decides the partition count, and `ANALYZE` only estimates it, so the tables are
# vacuumed instead - which counts them - with autovacuum off so nothing re-estimates mid-run. The
# harness refuses a table whose estimate is not its count.
#
# `scale` is a parameter, and only a scale whose `customer` and `part` are both past one stream's
# rows is accepted: below that nothing splits and the test would prove nothing. SF10 is the first
# scale `nix/tpch-data.nix` publishes a row count for that does.
{
  pkgs,
  scale,
  sutura,
  postgresDriver,
  duckdbDriver,
  agree,
  data,
  dimensions,
  corpus,
}:
let
  inherit (pkgs) lib;

  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
  idp = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "tpch-across-nodes";

  # A CA, each node's TLS leaf under it, the identity provider's ES256 key and its key set, and the
  # role's password. No CA key is kept.
  material =
    pkgs.runCommand "tpch-across-nodes-material" { nativeBuildInputs = [ pkgs.openssl ]; }
      ''
        mkdir "$out" && cd "$out"
        printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=tpch-across-nodes-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
        openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
        for node in query reader1 reader2 db; do
          openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout $node.key -out $node.csr -subj /CN=$node
          printf 'subjectAltName=DNS:%s\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\n' $node > $node.ext
          openssl x509 -req -in $node.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out $node.pem -days 3650 -extfile $node.ext
          rm $node.csr $node.ext
        done
        rm ca.key ca.srl ca.cnf
        openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:prime256v1 -out idp.key
        openssl pkey -in idp.key -pubout -out idp.pub
        ${idp} jwks idp.pub ${kid} > jwks.json
        openssl rand -hex 24 | tr -d '\n' > postgres-password
      '';

  # REF's catalog: the federated one with every dimension model on the DuckDB source, whose file
  # holds all seven tables. A rewrite that no longer matches fails the build.
  whole = pkgs.runCommand "tpch-across-nodes-whole-star" { } ''
    cp -r ${corpus}/catalog "$out"
    chmod -R u+w "$out"
    for model in customer nation part region supplier; do
      substituteInPlace "$out/models/$model.md" --replace-fail 'source: crm' 'source: lake'
    done
  '';

  # The five dimension tables, keyed and typed as `nix/tpch-refusal-rate.py` does: a name ending in
  # `key` is BIGINT, every other column VARCHAR. Owned by the role every node reads as, and vacuumed
  # to an exact `reltuples`.
  load = pkgs.writeText "tpch-across-nodes-load.sql" (
    lib.concatStrings (
      lib.mapAttrsToList (table: columns: ''
        CREATE TABLE ${table} (${
          lib.concatStringsSep ", " (
            lib.imap0 (
              i: column:
              "${column} ${if lib.hasSuffix "key" column then "BIGINT" else "VARCHAR"} NOT NULL${
                lib.optionalString (i == 0) " PRIMARY KEY"
              }"
            ) columns
          )
        });
        \copy ${table} FROM '${data}/dimensions/${table}.csv' WITH (FORMAT csv, HEADER true)
        ALTER TABLE ${table} OWNER TO sutura;
        VACUUM ${table};
      '') dimensions
    )
  );

  # The role's password, set from the file the nodes read it from and kept out of the statement log.
  role = pkgs.writeText "tpch-across-nodes-role.sql" ''
    \set secret `cat ${material}/postgres-password`
    SET log_statement = 'none';
    SELECT format('ALTER ROLE sutura PASSWORD %L', :'secret') \gexec
  '';

  settings =
    {
      dir,
      server,
      security,
      sources,
      runtime ? { },
    }:
    pkgs.writeTextDir "base.yaml" (
      builtins.toJSON (
        {
          server = server // {
            request_timeout_seconds = 300;
          };
          inherit security sources;
          # A node reachable from other hosts is refused without it (`RateLimitingDisabled`).
          rate_limit.enabled = true;
          telemetry.format = "bunyan";
          governance.federated_row_ceiling = 1000000;
          catalogs = [
            {
              name = "model";
              kind = "markdown";
              inherit dir;
              data_dir = "${corpus}";
              version = "tpch";
            }
          ];
        }
        // lib.optionalAttrs (runtime != { }) { inherit runtime; }
      )
    );

  # A listener over TLS, and a node's own, which also names the node authority.
  listener = own: port: {
    host = "0.0.0.0";
    inherit port;
    tls_certificate = "${material}/${own}.pem";
    tls_key = "${material}/${own}.key";
  };
  node = own: listener own 8443 // { tls_node_authority = "${material}/ca.pem"; };

  offHost = {
    tls_termination = "in-process";
    metrics_token = "tpch-across-nodes-metrics-token-000";
  };
  multiUser = {
    identity = "multi-user";
    inbound = {
      mode = "direct";
      resource = "https://query";
      authorization_server = "https://idp";
      key_set_file = "${material}/jwks.json";
      algorithms = [ "ES256" ];
    };
  };
  # The working set `nix/tpch-refusal-rate.py`'s large point raises; its row ceiling is in `settings`.
  large.working_set_max_bytes = 8589934592;

  lake = {
    kind = "duckdb";
    database_file = "/var/lib/tpch/star.duckdb";
    posture = "shared-service-user";
  };
  crm = {
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
  acknowledged.acknowledged_because = "every node reads generated tables as one identity";

  readerConfig =
    own:
    settings {
      dir = "${corpus}/catalog";
      server = node own // {
        role = "reading-worker";
      };
      security = offHost // {
        identity = "single-user";
        single_user_because = "a reading worker reads one source as its declared role";
      };
      sources = {
        inherit crm;
      };
    };
  refConfig = settings {
    dir = "${whole}";
    server = listener "query" 8444;
    security = offHost // multiUser;
    sources.lake = lake // acknowledged;
    runtime = large;
  };
  oneConfig = settings {
    dir = "${corpus}/catalog";
    server = listener "query" 8445;
    security = offHost // multiUser;
    sources = {
      lake = lake // acknowledged;
      crm = crm // acknowledged;
    };
    runtime = large;
  };
  manyConfig = settings {
    dir = "${corpus}/catalog";
    server = node "query" // {
      # One byte, so every combine reading both sources goes across: a granted leg does anyway.
      nodes = {
        own = "https://query:8443";
        others = map (other: {
          url = "https://${other}:8443";
          role = "reading-worker";
          sources = [ "crm" ];
        }) [ "reader1" "reader2" ];
        across_above_bytes = 1;
      };
    };
    security = offHost // multiUser;
    sources = {
      lake = lake // acknowledged;
      crm = crm // acknowledged;
    };
    runtime = large;
  };

  reader = own: {
    virtualisation = {
      memorySize = 4096;
      cores = 2;
    };
    networking.firewall.allowedTCPPorts = [ 8443 ];
    systemd.services.sutura = service (readerConfig own);
  };

  # Started by the test script, once what it dials is up.
  service = config: {
    environment = {
      SUTURA_CONFIG_DIR = "${config}";
      SUTURA_ENVIRONMENT = "development";
      SUTURA_POSTGRES_ADBC_DRIVER = postgresDriver;
      SUTURA_DUCKDB_ADBC_DRIVER = duckdbDriver;
    };
    serviceConfig = {
      ExecStart = "${sutura}/bin/sutura serve";
      DynamicUser = true;
    };
  };

  plan = pkgs.writeText "tpch-across-nodes-plan.json" (
    builtins.toJSON {
      inherit scale;
      data = "${data}";
      corpus = "${corpus}";
      material = "${material}";
      mint = "${idp} mint ${material}/idp.key ${kid} user@example.com https://query sutura:metrics.ask";
      agree = "${agree}/bin/sutura-dev";
      ports = {
        ref = 8444;
        one = 8445;
        many = 8443;
      };
      sutura = "${sutura}/bin/sutura";
      # What `sutura prompt` loads on each node before anything boots: the settings and the catalog
      # each service reads, which refuses what `sutura serve` would refuse of them at startup.
      preflight = [
        {
          machine = "query";
          name = "ref";
          config = "${refConfig}";
          catalog = "${whole}";
        }
        {
          machine = "query";
          name = "one";
          config = "${oneConfig}";
          catalog = "${corpus}/catalog";
        }
        {
          machine = "query";
          name = "many";
          config = "${manyConfig}";
          catalog = "${corpus}/catalog";
        }
        {
          machine = "reader1";
          name = "reader1";
          config = "${readerConfig "reader1"}";
          catalog = "${corpus}/catalog";
        }
        {
          machine = "reader2";
          name = "reader2";
          config = "${readerConfig "reader2"}";
          catalog = "${corpus}/catalog";
        }
      ];
      load = "${load}";
      role = "${role}";
      tables = builtins.attrNames dimensions;
    }
  );
in
assert lib.assertMsg (150000 * scale > 1000000 && 200000 * scale > 1000000)
  "tpch-across-nodes: SF${toString scale} does not put customer and part past one stream's 1,000,000 rows, so nothing splits";
pkgs.testers.runNixOSTest {
  name = "tpch-across-nodes-sf${toString scale}";
  globalTimeout = 5400;

  nodes.db = {
    virtualisation = {
      memorySize = 4096;
      diskSize = 8192;
      cores = 2;
    };
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
      initialScript = pkgs.writeText "tpch-across-nodes-init.sql" ''
        CREATE ROLE sutura LOGIN;
        CREATE DATABASE sutura OWNER sutura;
      '';
      settings = {
        ssl = true;
        ssl_cert_file = "${material}/db.pem";
        ssl_key_file = "/run/postgres-tls/db.key";
        password_encryption = "scram-sha-256";
        log_statement = "all";
        log_line_prefix = "%h ";
        autovacuum = false;
        shared_buffers = "1GB";
      };
    };
  };

  nodes.reader1 = reader "reader1";
  nodes.reader2 = reader "reader2";

  nodes.query = {
    virtualisation = {
      memorySize = 16384;
      diskSize = 16384;
      cores = 8;
    };
    environment.systemPackages = [
      pkgs.curl
      pkgs.jq
    ];
    networking.firewall.allowedTCPPorts = [
      8443
      8444
      8445
    ];
    systemd.services.sutura-ref = service refConfig;
    systemd.services.sutura-one = service oneConfig;
    systemd.services.sutura-many = service manyConfig;
  };

  testScript =
    { nodes, ... }:
    ''
      import json
      import runpy

      plan = json.load(open("${plan}"))
      # Both families: PostgreSQL logs whichever one the client connected over.
      plan["addresses"] = {
          "query": ["${nodes.query.networking.primaryIPAddress}", "${nodes.query.networking.primaryIPv6Address}"],
          "reader1": ["${nodes.reader1.networking.primaryIPAddress}", "${nodes.reader1.networking.primaryIPv6Address}"],
          "reader2": ["${nodes.reader2.networking.primaryIPAddress}", "${nodes.reader2.networking.primaryIPv6Address}"],
      }
      harness = runpy.run_path("${./tpch-across-nodes.py}")
      harness["main"](plan, query, db, [reader1, reader2], start_all, subtest, driver.out_dir)
    '';
}
