# A serving VM, two Oracle readers, a database VM and a fixture issuer (`telekom/sutura#828`).
# Forty customer rows have synthetic 1.2M-row statistics: this proves routing, not capacity.
# Source-side SQL traces attribute fetched bucket rows to the two reader addresses. A metadata
# parse/describe may name a bucket on the serving node; it must fetch no bucket rows there.
# Every source session is the same declared user. The fixture issuer is not human sign-in.
# IPv4 ingress is default-deny with explicit role paths; IPv6 is disabled in this venue.
{ pkgs, sutura, oracleImage, walletImage, example }:
let
  python = pkgs.python3.withPackages (p: [ p.pyjwt p.cryptography ]);
  idp = "${python}/bin/python3 ${./multi-node-idp.py}";
  kid = "multi-node-oracle";
  version = "multi-node-oracle";
  material = pkgs.runCommand "multi-node-oracle-material" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir "$out" && cd "$out"
    printf '[req]\ndistinguished_name=dn\nx509_extensions=ca\nprompt=no\n[dn]\nCN=multi-node-oracle-ca\n[ca]\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\nsubjectKeyIdentifier=hash\n' > ca.cnf
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ca.key -out ca.pem -days 3650 -config ca.cnf
    for node in query reader1 reader2 idp; do
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
    ${idp} jwks issuer.pub ${kid} > jwks.json
    openssl rand -hex 16 > password
  '';
  publicKeys = pkgs.runCommand "multi-node-oracle-jwks" { } ''
    mkdir "$out"
    cp ${material}/jwks.json "$out/jwks.json"
  '';
  catalog = pkgs.runCommand "multi-node-oracle-catalog" { } ''
    cp -r ${example}/catalog "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/models/customers.md" --replace-fail 'source: local' 'source: geo' --replace-fail 'table: dim_customer' 'table: dim_customer_split'
    substituteInPlace "$out/models/regions.md" --replace-fail 'source: local' 'source: geo'
  '';
  changedCatalog = pkgs.runCommand "multi-node-oracle-changed-catalog" { } ''
    cp -r ${catalog} "$out"
    chmod -R u+w "$out"
    substituteInPlace "$out/metrics/recurring_revenue.md" --replace-fail 'description: Where the customer is.' 'description: Where the customer was.'
  '';
  seed = pkgs.runCommand "multi-node-oracle-load.sql" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    python3 ${./multi-node-oracle-seed.py} ${example}/data ${material}/password > "$out"
  '';
  walletScript = pkgs.runCommand "multi-node-oracle-wallet.sh" { } ''
    cp ${./oracle-tier-wallet.sh} "$out"
    substituteInPlace "$out" --replace-fail 'subjectAltName=DNS:localhost,IP:127.0.0.1' 'subjectAltName=DNS:db,DNS:localhost,IP:127.0.0.1'
  '';
  sql = pkgs.writeShellScriptBin "oracle-sql" ''
    set -euo pipefail
    {
      printf '%s\n' 'WHENEVER OSERROR EXIT FAILURE' 'WHENEVER SQLERROR EXIT FAILURE' 'SET HEADING OFF FEEDBACK OFF PAGESIZE 0 LINESIZE 32767 TRIMSPOOL ON' 'ALTER SESSION SET CONTAINER=FREEPDB1;'
      cat
      printf '%s\n' 'EXIT'
    } | docker exec -i oracle bash -c 'sqlplus -L -s / as sysdba'
  '';
  pair = own: {
    host = "0.0.0.0";
    port = 8443;
    tls_certificate = "${material}/${own}.pem";
    tls_key = "${material}/${own}.key";
    tls_node_authority = "${material}/ca.pem";
  };
  offHost = { tls_termination = "in-process"; metrics_token = "multi-node-oracle-metrics-token-000"; };
  geo = {
    kind = "oracle";
    host = "db";
    port = 2484;
    service_name = "FREEPDB1";
    user = "SUTURA";
    password_file = "${material}/password";
    transport_mode = "verified";
    transport_anchors = "/run/oracle-ca.pem";
    posture = "shared-service-user";
  };
  acknowledged = { acknowledged_because = "every node reads fixture tables as one declared identity"; };
  settings = { server, security, sources, model ? catalog }: pkgs.writeTextDir "base.yaml" (builtins.toJSON {
    inherit server security sources;
    telemetry.format = "bunyan";
    rate_limit.enabled = true;
    catalogs = [{ name = "model"; kind = "markdown"; dir = "${model}"; data_dir = "${example}/data"; inherit version; }];
  });
  serve = config: {
    environment = { SUTURA_CONFIG_DIR = "${config}"; SUTURA_ENVIRONMENT = "development"; };
    serviceConfig = { ExecStart = "${sutura}/bin/sutura serve"; DynamicUser = true; };
  };
  serving = peers: split: serve (settings {
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
      local = { kind = "files"; data_dir = "${example}/data"; posture = "shared-service-user"; } // acknowledged;
      geo = geo // acknowledged // pkgs.lib.optionalAttrs split { split = "flashback"; };
    };
  });
  reader = own: { nodes, ... }: {
    virtualisation.memorySize = 2048;
    networking.firewall.extraCommands = "iptables -A nixos-fw -p tcp --dport 8443 -s ${nodes.query.networking.primaryIPAddress} -j nixos-fw-accept";
    systemd.services.sutura = serve (settings {
      server = pair own // { role = "reading-worker"; };
      security = offHost // { identity = "single-user"; single_user_because = "a reading worker reads one source as its declared role"; };
      sources.geo = geo // { split = "flashback"; };
    });
    systemd.services.sutura-changed = serve (settings {
      server = pair own // { role = "reading-worker"; };
      security = offHost // { identity = "single-user"; single_user_because = "a reading worker reads one source as its declared role"; };
      sources.geo = geo // { split = "flashback"; };
      model = changedCatalog;
    });
  };
in
pkgs.testers.runNixOSTest {
  name = "multi-node-oracle";
  defaults = {
    networking.enableIPv6 = false;
    networking.firewall.enable = true;
    environment.systemPackages = [ pkgs.curl pkgs.netcat-openbsd ];
  };
  nodes.db = { nodes, ... }: {
    virtualisation = { memorySize = 6144; cores = 2; diskSize = 32768; };
    virtualisation.docker.enable = true;
    environment.systemPackages = [ sql ];
    networking.firewall.extraCommands = builtins.concatStringsSep "\n" (map (node:
      "iptables -A nixos-fw -p tcp --dport 2484 -s ${nodes.${node}.networking.primaryIPAddress} -j nixos-fw-accept"
    ) [ "query" "reader1" "reader2" ]);
    systemd.services.oracle = {
      after = [ "docker.service" ];
      requires = [ "docker.service" ];
      path = [ pkgs.docker pkgs.coreutils ];
      serviceConfig = { Type = "oneshot"; RemainAfterExit = true; TimeoutStartSec = 1800; };
      script = ''
        docker load -i ${oracleImage}
        docker load -i ${walletImage}
        docker tag gvenzl/oracle-free:23.26.3-faststart sutura-oracle-server:fixture
        docker tag container-registry.oracle.com/database/free:23.26.3.0 sutura-oracle-wallet:fixture
        install -d -m 0700 /run/oracle-tls /run/oracle-tls/wallet
        docker run --rm --user 0 -v /run/oracle-tls:/out --entrypoint chown sutura-oracle-wallet:fixture oracle /out /out/wallet
        docker run --rm --user oracle -e HOME=/out -v /run/oracle-tls:/out -v ${walletScript}:/wallet.sh:ro --entrypoint bash sutura-oracle-wallet:fixture -c 'test -O /out/wallet && test -w /out/wallet && echo "wallet output is owned and writable" && exec bash /wallet.sh'
        docker run -d --name oracle --network host -e ORACLE_PASSWORD="$(cat ${material}/password)" -v /run/oracle-tls/wallet:/opt/oracle/tls:ro -v ${./oracle-tier-tcps.sh}:/container-entrypoint-startdb.d/tcps.sh:ro sutura-oracle-server:fixture
      '';
    };
  };
  nodes.reader1 = reader "reader1";
  nodes.reader2 = reader "reader2";
  nodes.query = { nodes, ... }: {
    virtualisation.memorySize = 2048;
    networking.firewall.extraCommands = "iptables -A nixos-fw -p tcp --dport 8443 -s ${nodes.idp.networking.primaryIPAddress} -j nixos-fw-accept";
    systemd.services.sutura = serving [ "reader1" "reader2" ] true;
    systemd.services.sutura-whole = serving [ ] false;
    systemd.services.sutura-unresolved = serving [ "reader-a.invalid" "reader-b.invalid" ] true;
  };
  nodes.idp = { nodes, ... }: {
    virtualisation.memorySize = 512;
    networking.firewall.extraCommands = "iptables -A nixos-fw -p tcp --dport 9443 -s ${nodes.query.networking.primaryIPAddress} -j nixos-fw-accept";
    services.nginx = {
      enable = true;
      virtualHosts.idp = {
        onlySSL = true;
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
        "material": "${material}",
        "issuer": "${idp}",
        "kid": "${kid}",
        "seed": "${seed}",
        "customers": "${example}/data/dim_customer.csv",
        "addresses": {
            "query": "${nodes.query.networking.primaryIPAddress}",
            "reader1": "${nodes.reader1.networking.primaryIPAddress}",
            "reader2": "${nodes.reader2.networking.primaryIPAddress}",
        },
    }
    harness = runpy.run_path("${./multi-node-oracle.py}")
    harness["main"](config, query, reader1, reader2, db, idp, start_all, subtest, driver.out_dir)
  '';
}
