# Seven immutable-fixture VMs: whole Distributed reads, then explicit shard-local bucket phases.
# No automatic cross-endpoint split routing, common snapshot, capacity or human sign-in claim.
{
  pkgs,
  sutura,
  clickhouse,
  clickhouseVersion,
  clickhouseDriver,
  example,
}:
let
  python = pkgs.python3.withPackages (p: [
    p.pyjwt
    p.cryptography
  ]);
  issuer = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "clickhouse-cluster";
  databaseNames = [
    "router"
    "shard1"
    "shard2"
  ];
  nodeNames = [
    "query"
    "reader1"
    "reader2"
  ]
  ++ databaseNames
  ++ [ "idp" ];
  material =
    pkgs.runCommand "clickhouse-cluster-material" { nativeBuildInputs = [ pkgs.openssl ]; }
      ''
        mkdir "$out" && cd "$out"
        printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=clickhouse-cluster-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
        openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
        for node in ${builtins.concatStringsSep " " nodeNames}; do
          openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout $node.key -out $node.csr -subj /CN=$node
          printf 'subjectAltName=DNS:%s\nbasicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth,clientAuth\n' $node > $node.ext
          openssl x509 -req -in $node.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out $node.pem -days 3650 -extfile $node.ext
          rm $node.csr $node.ext
        done
        openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout other.key -out other.pem -days 3650 -config ca.cnf -subj /CN=other-ca
        rm ca.key ca.srl ca.cnf other.key
        openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:prime256v1 -out issuer.key
        openssl pkey -in issuer.key -pubout -out issuer.pub
        ${issuer} jwks issuer.pub ${kid} > jwks.json
        openssl rand -hex 16 | tr -d '\n' > password
        openssl rand -hex 32 | tr -d '\n' > cluster-secret
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
        for user in ("sutura", "analyst_a", "analyst_b"):
            credential = password if user == "sutura" else f"{user}-{password}"
            hashes[user] = hashlib.sha256(credential.encode()).hexdigest()
            material.joinpath(f"curl-{user}.netrc").write_text(
                f"machine 127.0.0.1 login {user} password {credential}\n"
            )
        material.joinpath("password-hashes.json").write_text(json.dumps(hashes))
        PY
      '';
  publicKeys = pkgs.runCommand "clickhouse-cluster-jwks" { } ''
    mkdir "$out"
    cp ${material}/jwks.json "$out/jwks.json"
  '';
  phases = {
    ref = {
      host = "router";
      table = "dim_customer_ref";
    };
    all = {
      host = "router";
      table = "dim_customer_all";
    };
    shard1 = {
      host = "shard1";
      table = "dim_customer_local";
    };
    shard2 = {
      host = "shard2";
      table = "dim_customer_local";
    };
  };
  catalogs = builtins.mapAttrs (
    name: phase:
    pkgs.runCommand "clickhouse-cluster-catalog-${name}" { } ''
      cp -r ${example}/catalog "$out"
      chmod -R u+w "$out"
      substituteInPlace "$out/models/customers.md" --replace-fail 'source: local' 'source: geo' --replace-fail 'table: dim_customer' 'table: ${phase.table}'
      substituteInPlace "$out/models/regions.md" --replace-fail 'source: local' 'source: geo'
    ''
  ) phases;
  pair = own: {
    host = "0.0.0.0";
    port = 8443;
    request_timeout_seconds = 120;
    tls_certificate = "${material}/${own}.pem";
    tls_key = "${material}/${own}.key";
    tls_node_authority = "${material}/ca.pem";
  };
  offHost = {
    tls_termination = "in-process";
    metrics_token = "clickhouse-cluster-metrics-token-000";
  };
  sources = phase: split: {
    geo = {
      kind = "clickhouse";
      host = phases.${phase}.host;
      port = 8443;
      user = "sutura";
      password_file = "${material}/password";
      transport_mode = "verified";
      transport_anchors = "${material}/ca.pem";
      posture = "impersonation-at-source";
    }
    // pkgs.lib.optionalAttrs split {
      hash_buckets_acknowledged_because = "one fixed immutable shard per phase; buckets have no common snapshot";
    };
    local = {
      kind = "files";
      data_dir = "${example}/data";
      posture = "shared-service-user";
      acknowledged_because = "fixture files are read as one identity";
    };
  };
  serve =
    own: phase: split:
    pkgs.writeTextDir "base.yaml" (
      builtins.toJSON {
        server =
          pair own
          // (
            if own == "query" then
              pkgs.lib.optionalAttrs split {
                nodes = {
                  own = "https://query:8443";
                  others =
                    map
                      (peer: {
                        url = "https://${peer}:8443";
                        role = "reading-worker";
                        sources = [ "geo" ];
                      })
                      [
                        "reader1"
                        "reader2"
                      ];
                  across_above_bytes = 1;
                  address_family = "ipv4";
                };
              }
            else
              { role = "reading-worker"; }
          );
        security =
          offHost
          // (
            if own == "query" then
              {
                identity = "multi-user";
                outbound.transport_anchors = "${material}/ca.pem";
                inbound = {
                  mode = "direct";
                  resource = "https://query";
                  authorization_server = "https://idp";
                  key_set_url = "https://idp:9443/jwks.json";
                  algorithms = [ "ES256" ];
                };
              }
            else
              {
                identity = "single-user";
                single_user_because = "a reading worker admits only node-authenticated granted reads";
              }
          );
        sources = if own == "query" then sources phase split else { geo = (sources phase split).geo; };
        telemetry.format = "bunyan";
        rate_limit.enabled = true;
        catalogs = [
          {
            name = "model";
            kind = "markdown";
            dir = "${catalogs.${phase}}";
            data_dir = "${example}/data";
            version = "clickhouse-cluster-${phase}";
          }
        ];
      }
    );
  service = own: phase: split: {
    environment = {
      SUTURA_CONFIG_DIR = "${serve own phase split}";
      SUTURA_ENVIRONMENT = "development";
      SUTURA_CLICKHOUSE_ADBC_DRIVER = clickhouseDriver;
    };
    serviceConfig = {
      ExecStart = "${sutura}/bin/sutura serve";
      DynamicUser = true;
    };
  };
  ingress =
    nodes: port: origins:
    builtins.concatStringsSep "\n" (
      map (
        node:
        "iptables -A nixos-fw -p tcp --dport ${toString port} -s ${
          nodes.${node}.networking.primaryIPAddress
        } -j nixos-fw-accept"
      ) origins
    );
  reader = own: { nodes, ... }: {
    virtualisation.memorySize = 2048;
    networking.firewall.extraCommands = ingress nodes 8443 [ "query" ];
    systemd.services = builtins.listToAttrs (
      map
        (phase: {
          name = "sutura-${phase}";
          value = service own phase true;
        })
        [
          "shard1"
          "shard2"
        ]
    );
  };
  databaseConfig =
    own: broken:
    let
      template = pkgs.writeText "clickhouse-cluster-${own}-${broken}-template.json" (
        builtins.toJSON {
          logger = {
            level = "information";
            console = 1;
          };
          listen_host = "0.0.0.0";
          http_port = 8123;
          https_port = 8443;
          tcp_port = 9000;
          tcp_port_secure = 9440;
          path = "/var/lib/clickhouse/";
          tmp_path = "/var/lib/clickhouse/scratch/";
          user_files_path = "/var/lib/clickhouse/user_files/";
          disable_internal_dns_cache = 1;
          access_control_improvements.allow_impersonate_user = 1;
          openSSL = {
            server = {
              certificateFile = "${material}/${own}.pem";
              privateKeyFile = "${material}/${own}.key";
            };
            client = {
              caConfig = "${material}/${if broken == "ca" then "other" else "ca"}.pem";
              loadDefaultCAFile = false;
              verificationMode = "strict";
              extendedVerification = true;
              invalidCertificateHandler.name = "RejectCertificateHandler";
            };
          };
          remote_servers.fixture.shard =
            map
              (host: {
                replica = {
                  inherit host;
                  port = 9440;
                  secure = 1;
                };
              })
              [
                "shard1"
                "shard2"
              ];
          users = {
            sutura = {
              networks.ip = "0.0.0.0/0";
              profile = "default";
              quota = "default";
              grants.query = [
                "GRANT ALL ON *.*"
                "REVOKE IMPERSONATE ON *"
                "REVOKE ACCESS MANAGEMENT ON *.*"
                "GRANT IMPERSONATE ON analyst_a"
                "GRANT IMPERSONATE ON analyst_b"
              ];
            };
          }
          //
            builtins.mapAttrs
              (_: filter: {
                networks.ip = "0.0.0.0/0";
                profile = "default";
                quota = "default";
                grants.query = [
                  "GRANT SELECT ON default.*"
                  "GRANT SELECT(rows, bytes_on_disk, active, database, table) ON system.parts"
                ];
                # Never put a policy on Distributed: it could mask a shard running as the service.
                databases.default.${if own == "router" then "dim_customer_ref" else "dim_customer_local"}.filter =
                  filter;
              })
              {
                analyst_a = "1";
                analyst_b = "region = 'north'";
              };
          profiles.default = {
            log_queries = 1;
            log_query_settings = 1;
            skip_unavailable_shards = 0;
          };
          quotas.default = { };
          query_log = {
            database = "system";
            table = "query_log";
            flush_interval_milliseconds = 1000;
          };
        }
      );
    in
    pkgs.runCommand "clickhouse-cluster-${own}-${broken}.yaml" { nativeBuildInputs = [ pkgs.python3 ]; }
      ''
        python3 - ${template} ${material} "$out" <<'PY'
        import json
        import sys
        from pathlib import Path

        config = json.loads(Path(sys.argv[1]).read_text())
        material = Path(sys.argv[2])
        hashes = json.loads(material.joinpath("password-hashes.json").read_text())
        for user, settings in config["users"].items():
            settings["password_sha256_hex"] = hashes[user]
        secret = material.joinpath("cluster-secret").read_text()
        config["remote_servers"]["fixture"]["secret"] = secret + ${
          builtins.toJSON (if broken == "secret" then "-mismatch" else "")
        }
        Path(sys.argv[3]).write_text(json.dumps(config))
        PY
      '';
  database =
    own:
    { nodes, ... }:
    let
      run = broken: {
        environment.CLICKHOUSE_WATCHDOG_ENABLE = "0";
        serviceConfig = {
          ExecStart = "${clickhouse}/bin/clickhouse server --config-file=${databaseConfig own broken}";
          DynamicUser = true;
          StateDirectory = "clickhouse";
        };
      };
    in
    {
      virtualisation = {
        memorySize = 3072;
        cores = 2;
      };
      environment.systemPackages = [ clickhouse ];
      networking.firewall.extraCommands =
        ingress nodes 8443 (
          if own == "router" then
            [ "query" ]
          else
            [
              "query"
              "reader1"
              "reader2"
            ]
        )
        + "\n"
        + ingress nodes 9440 (if own == "router" then [ ] else [ "router" ]);
      systemd.services = {
        clickhouse = run "normal" // {
          wantedBy = [ "multi-user.target" ];
        };
      }
      // pkgs.lib.optionalAttrs (own == "router") { clickhouse-bad-ca = run "ca"; }
      // pkgs.lib.optionalAttrs (own == "shard2") { clickhouse-bad-secret = run "secret"; };
    };
in
pkgs.testers.runNixOSTest {
  name = "clickhouse-cluster";
  defaults = {
    networking.enableIPv6 = false;
    networking.firewall.enable = true;
    environment.systemPackages = [
      pkgs.curl
      pkgs.netcat-openbsd
    ];
  };
  nodes = {
    router = database "router";
    shard1 = database "shard1";
    shard2 = database "shard2";
    reader1 = reader "reader1";
    reader2 = reader "reader2";
    query = { nodes, ... }: {
      virtualisation = {
        memorySize = 2048;
        cores = 2;
      };
      networking.firewall.extraCommands = ingress nodes 8443 [ "idp" ];
      systemd.services = {
        sutura-ref = service "query" "ref" false;
        sutura-all = service "query" "all" false;
      }
      // builtins.listToAttrs (
        pkgs.lib.concatMap
          (
            phase:
            map
              (split: {
                name = "sutura-${phase}-${if split then "split" else "whole"}";
                value = service "query" phase split;
              })
              [
                false
                true
              ]
          )
          [
            "shard1"
            "shard2"
          ]
      );
    };
    idp = { nodes, ... }: {
      virtualisation.memorySize = 512;
      networking.firewall.extraCommands = ingress nodes 9443 [ "query" ];
      services.nginx = {
        enable = true;
        virtualHosts.idp = {
          onlySSL = true;
          listen = [
            {
              addr = "0.0.0.0";
              port = 9443;
              ssl = true;
            }
          ];
          sslCertificate = "${material}/idp.pem";
          sslCertificateKey = "${material}/idp.key";
          root = "${publicKeys}";
        };
      };
    };
  };
  testScript = { nodes, ... }: ''
    import runpy

    config = {
        "material": "${material}", "issuer": "${issuer}", "kid": "${kid}",
        "data": "${example}/data",
        "binary": "${sutura}/bin/sutura", "driver": "${clickhouseDriver}",
        "versions": {"clickhouse": "${clickhouseVersion}", "sutura": "${sutura.version}", "driver": "${(builtins.fromTOML (builtins.readFile ../vendor/adbc_clickhouse/Cargo.toml)).package.version}", "driver_lock_sha256": "${builtins.hashFile "sha256" ./clickhouse-adbc.Cargo.lock}"},
        "addresses": ${
          builtins.toJSON (
            builtins.listToAttrs (
              map (name: {
                inherit name;
                value = nodes.${name}.networking.primaryIPAddress;
              }) nodeNames
            )
          )
        },
    }
    harness = runpy.run_path("${./clickhouse-cluster.py}")
    harness["main"](config, {"query": query, "reader1": reader1, "reader2": reader2, "router": router, "shard1": shard1, "shard2": shard2, "idp": idp}, start_all, subtest, driver.out_dir)
  '';
}
