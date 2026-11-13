# Five VMs, one fixed local MergeTree shard and fixture-issued access tokens. Repeated physical
# rows trigger the production split; forty returned groups prove routing, not capacity or clusters.
# query_log separates LIMIT 0 metadata from bucket data and records caller and reader addresses.
# IPv4 ingress is default-deny; IPv6 is disabled and unmeasured. No human sign-in is exercised.
{ pkgs, sutura, clickhouse, clickhouseDriver, example }:
let
  python = pkgs.python3.withPackages (p: [ p.pyjwt p.cryptography ]);
  issuer = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "multi-node-clickhouse";
  stimuli = [ "missing" "mismatched" "expired" ];
  stimulated = stimulus: sutura.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ (./. + "/multi-node-clickhouse-${stimulus}.patch") ];
  });
  material = pkgs.runCommand "multi-node-clickhouse-material" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir "$out" && cd "$out"
    printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=multi-node-clickhouse-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
    for node in query reader1 reader2 db idp; do
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
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:prime256v1 -out issuer.key
    openssl pkey -in issuer.key -pubout -out issuer.pub
    ${issuer} jwks issuer.pub ${kid} > jwks.json
    openssl rand -hex 16 | tr -d '\n' > password
    ${python}/bin/python3 - "$out" <<'PY'
    import hashlib
    import json
    import re
    import sys
    from pathlib import Path

    material = Path(sys.argv[1])
    password = material.joinpath("password").read_text()
    if re.fullmatch(r"[0-9a-f]{32}", password) is None:
        raise ValueError("fixture password must be 32 lowercase hexadecimal characters")
    hashes = {}
    for user in ("sutura", "analyst_a", "analyst_b", "analyst_ungranted", "analyst_readonly"):
        credential = password if user == "sutura" else f"{user}-{password}"
        hashes[user] = hashlib.sha256(credential.encode()).hexdigest()
        material.joinpath(f"curl-{user}.netrc").write_text(
            f"machine 127.0.0.1 login {user} password {credential}\n"
        )
    material.joinpath("password-hashes.json").write_text(json.dumps(hashes))
    PY
  '';
  publicKeys = pkgs.runCommand "multi-node-clickhouse-jwks" { } ''
    mkdir "$out"
    cp ${material}/jwks.json "$out/jwks.json"
  '';
  catalog = pkgs.runCommand "multi-node-clickhouse-catalog" { } ''
    cp -r ${example}/catalog "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/models/customers.md" --replace-fail 'source: local' 'source: geo' --replace-fail 'table: dim_customer' 'table: dim_customer_split'
    substituteInPlace "$out/models/regions.md" --replace-fail 'source: local' 'source: geo'
  '';
  changedCatalog = pkgs.runCommand "multi-node-clickhouse-changed-catalog" { } ''
    cp -r ${catalog} "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/metrics/recurring_revenue.md" --replace-fail 'description: Where the customer is.' 'description: Where the customer was.'
  '';
  pair = own: {
    host = "0.0.0.0";
    port = 8443;
    request_timeout_seconds = 120;
    tls_certificate = "${material}/${own}.pem";
    tls_key = "${material}/${own}.key";
    tls_node_authority = "${material}/ca.pem";
  };
  offHost = { tls_termination = "in-process"; metrics_token = "multi-node-clickhouse-metrics-token-000"; };
  geo = {
    kind = "clickhouse";
    host = "db";
    port = 8443;
    user = "sutura";
    password_file = "${material}/password";
    transport_mode = "verified";
    transport_anchors = "${material}/ca.pem";
    posture = "impersonation-at-source";
  };
  buckets = { hash_buckets_acknowledged_because = "this fixed local shard is immutable during the venue; buckets have no common snapshot"; };
  settings = { server, security, sources, model ? catalog }: pkgs.writeTextDir "base.yaml" (builtins.toJSON {
    inherit server security sources;
    telemetry.format = "bunyan";
    rate_limit.enabled = true;
    catalogs = [{ name = "model"; kind = "markdown"; dir = "${model}"; data_dir = "${example}/data"; version = "multi-node-clickhouse"; }];
  });
  serve = binary: config: {
    environment = {
      SUTURA_CONFIG_DIR = "${config}";
      SUTURA_ENVIRONMENT = "development";
      SUTURA_CLICKHOUSE_ADBC_DRIVER = clickhouseDriver;
    };
    serviceConfig = { ExecStart = "${binary}/bin/sutura serve"; DynamicUser = true; };
  };
  serving = binary: peers: split: serve binary (settings {
    server = pair "query" // pkgs.lib.optionalAttrs (peers != [ ]) {
      nodes = {
        own = "https://query:8443";
        others = map (peer: { url = "https://${peer}:8443"; role = "reading-worker"; sources = [ "geo" ]; }) peers;
        across_above_bytes = 1;
        address_family = "ipv4";
      };
    };
    security = offHost // {
      identity = "multi-user";
      outbound.transport_anchors = "${material}/ca.pem";
      inbound = {
        mode = "direct";
        resource = "https://query";
        authorization_server = "https://idp";
        key_set_url = "https://idp:9443/jwks.json";
        algorithms = [ "ES256" ];
      };
    };
    sources = {
      local = { kind = "files"; data_dir = "${example}/data"; posture = "shared-service-user"; acknowledged_because = "fixture files are read as one identity"; };
      geo = geo // pkgs.lib.optionalAttrs split buckets;
    };
  });
  reader = own: { nodes, ... }: {
    virtualisation.memorySize = 2048;
    networking.firewall.extraCommands = "iptables -A nixos-fw -p tcp --dport 8443 -s ${nodes.query.networking.primaryIPAddress} -j nixos-fw-accept";
    systemd.services = builtins.listToAttrs (map (changed: {
      name = if changed then "sutura-changed" else "sutura";
      value = serve sutura (settings {
        server = pair own // { role = "reading-worker"; };
        security = offHost // { identity = "single-user"; single_user_because = "a reading worker admits only node-authenticated granted reads"; };
        sources.geo = geo // buckets;
        model = if changed then changedCatalog else catalog;
      });
    }) [ false true ]);
  };
  clickhouseConfigTemplate = pkgs.writeText "multi-node-clickhouse-template.json" (builtins.toJSON {
    logger = { level = "warning"; console = 1; };
    listen_host = "0.0.0.0";
    http_port = 8123;
    https_port = 8443;
    path = "/var/lib/clickhouse/";
    tmp_path = "/var/lib/clickhouse/scratch/";
    user_files_path = "/var/lib/clickhouse/user_files/";
    disable_internal_dns_cache = 1;
    access_control_improvements.allow_impersonate_user = 1;
    openSSL.server = { certificateFile = "${material}/db.pem"; privateKeyFile = "${material}/db.key"; };
    users = {
      sutura = {
        networks.ip = "0.0.0.0/0";
        profile = "default";
        quota = "default";
        grants.query = [ "GRANT ALL ON *.*" "REVOKE IMPERSONATE ON *" "REVOKE ACCESS MANAGEMENT ON *.*" "GRANT IMPERSONATE ON analyst_a" "GRANT IMPERSONATE ON analyst_b" "GRANT IMPERSONATE ON analyst_readonly" ];
      };
    } // builtins.mapAttrs (name: filter: {
      networks.ip = "0.0.0.0/0";
      profile = if name == "analyst_readonly" then "read_only_caller" else "default";
      quota = "default";
      grants.query = [ "GRANT SELECT ON default.*" ];
      databases.default.dim_customer_split.filter = filter;
    }) { analyst_a = "1"; analyst_b = "region = 'north'"; analyst_ungranted = "1"; analyst_readonly = "1"; };
    profiles = { default = { log_queries = 1; log_query_settings = 1; }; read_only_caller = { readonly = 1; }; };
    quotas.default = { };
    query_log = { database = "system"; table = "query_log"; flush_interval_milliseconds = 1000; };
  });
  clickhouseConfig = pkgs.runCommand "multi-node-clickhouse.yaml" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    python3 - ${clickhouseConfigTemplate} ${material}/password-hashes.json "$out" <<'PY'
    import json
    import sys
    from pathlib import Path

    config = json.loads(Path(sys.argv[1]).read_text())
    hashes = json.loads(Path(sys.argv[2]).read_text())
    for user, settings in config["users"].items():
        settings["password_sha256_hex"] = hashes[user]
    Path(sys.argv[3]).write_text(json.dumps(config))
    PY
  '';
in
pkgs.testers.runNixOSTest {
  name = "multi-node-clickhouse";
  defaults = {
    networking.enableIPv6 = false;
    networking.firewall.enable = true;
    environment.systemPackages = [ pkgs.curl pkgs.netcat-openbsd ];
  };
  nodes.db = { nodes, ... }: {
    virtualisation.memorySize = 3072;
    networking.firewall.extraCommands = builtins.concatStringsSep "\n" (map (node:
      "iptables -A nixos-fw -p tcp --dport 8443 -s ${nodes.${node}.networking.primaryIPAddress} -j nixos-fw-accept"
    ) [ "query" "reader1" "reader2" ]);
    systemd.services.clickhouse = {
      wantedBy = [ "multi-user.target" ];
      environment.CLICKHOUSE_WATCHDOG_ENABLE = "0";
      serviceConfig = { ExecStart = "${clickhouse}/bin/clickhouse server --config-file=${clickhouseConfig}"; DynamicUser = true; StateDirectory = "clickhouse"; };
    };
  };
  nodes.reader1 = reader "reader1";
  nodes.reader2 = reader "reader2";
  nodes.query = { nodes, ... }: {
    virtualisation = { memorySize = 2048; cores = 2; };
    networking.firewall.extraCommands = "iptables -A nixos-fw -p tcp --dport 8443 -s ${nodes.idp.networking.primaryIPAddress} -j nixos-fw-accept";
    systemd.services = {
      sutura = serving sutura [ "reader1" "reader2" ] true;
      sutura-whole = serving sutura [ ] false;
      sutura-unresolved = serving sutura [ "reader-a.invalid" "reader-b.invalid" ] true;
    } // builtins.listToAttrs (map (stimulus: {
      name = "sutura-${stimulus}";
      value = serving (stimulated stimulus) [ "reader1" "reader2" ] true;
    }) stimuli);
  };
  nodes.idp = { nodes, ... }: {
    virtualisation.memorySize = 512;
    networking.firewall.extraCommands = "iptables -A nixos-fw -p tcp --dport 9443 -s ${nodes.query.networking.primaryIPAddress} -j nixos-fw-accept";
    services.nginx = {
      enable = true;
      virtualHosts.idp = {
        listen = [{ addr = "0.0.0.0"; port = 9443; ssl = true; }];
        sslCertificate = "${material}/idp.pem";
        sslCertificateKey = "${material}/idp.key";
        root = "${publicKeys}";
      };
    };
  };
  testScript = { nodes, ... }: ''
    import runpy

    config = {
        "material": "${material}", "issuer": "${issuer}", "kid": "${kid}",
        "data": "${example}/data",
        "addresses": {
            "query": "${nodes.query.networking.primaryIPAddress}",
            "reader1": "${nodes.reader1.networking.primaryIPAddress}",
            "reader2": "${nodes.reader2.networking.primaryIPAddress}",
        },
    }
    harness = runpy.run_path("${./multi-node-clickhouse.py}")
    harness["main"](config, query, reader1, reader2, db, idp, start_all, subtest, driver.out_dir)
  '';
}
