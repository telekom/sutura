//! A served Oracle lookup split across two reading workers, compared with the whole lookup.
//!
//! Forty actual fixture customers carry statistics declaring 1,200,000 rows, forcing two buckets
//! at the CLI's fixed row target. This measures split mechanics, exact result and order, and
//! fail-closed reader selection and re-certification; it proves no large-result capacity or row
//! bound. All processes share one host, node certificate and declared Oracle service user. The
//! caller's token comes from a fake issuer; nobody signs in and Oracle does not impersonate them.

use std::io::{ErrorKind, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet};

use crate::harness::{
    LOCAL_SOURCE, LOOKUP_SOURCE, SINGLE_USER, Served, accepted_by, an_issuer, bound_address, catalog_spanning_two_sources,
    example_root, files_source, recurring_revenue_by_region, settings_over, start_configured, v1,
};

const CASE: &str = "oracle-split-differential";
const TABLE: &str = "served_split_customers";
const ACKNOWLEDGED: &str = "    acknowledged_because: fixture tables are read as one declared identity\n";

struct Fixture {
    root: PathBuf,
    catalog: PathBuf,
    host: String,
    port: u16,
    user: String,
    authority: String,
    certificate: CertificateDer<'static>,
    issuer: MockIssuer,
    keys: PublishedKeySet,
}

impl Fixture {
    #[expect(
        clippy::disallowed_methods,
        reason = "a hosted test provisions its own Oracle schema through the real driver"
    )]
    fn here() -> Self {
        let provisioned = sutura_dev::provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle");
        let endpoint = provisioned
            .endpoint()
            .expect("the named Oracle acceptance task requires its tier");
        let required = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("the acceptance task sets {name}"));
        let password = required("SUTURA_DEV_PASSWORD");
        let authority = required("SUTURA_ORACLE_TIER_CA");
        let anchors = std::fs::read_to_string(&authority).expect("the Oracle authority reads");
        let dial = oracledb::Config::default()
            .set_connect_string(&format!("tcps://{}:{}/FREEPDB1", endpoint.host(), endpoint.port()))
            .expect("the tier endpoint is an Oracle connect string")
            .set_trust_anchors_pem(&anchors)
            .expect("the tier authority is an anchor");
        let admin =
            oracledb::connect(dial.clone().set_credentials("SYSTEM", &password)).expect("the fixture administrator opens");
        let user = format!("SPLIT_{}", std::process::id());
        if let Err(cause) = admin.execute(&format!("DROP USER \"{user}\" CASCADE"), &[]) {
            assert!(
                cause.to_string().contains("ORA-01918"),
                "the previous fixture did not drop: {cause}"
            );
        }
        assert!(!password.contains('"'), "the generated fixture password must be quotable");
        for statement in [
            format!("CREATE USER \"{user}\" IDENTIFIED BY \"{password}\""),
            format!("GRANT CREATE SESSION, CREATE TABLE, UNLIMITED TABLESPACE TO \"{user}\""),
        ] {
            admin.execute(&statement, &[]).expect("the private fixture schema creates");
        }
        let setup = oracledb::connect(dial.set_credentials(&user, &password)).expect("the fixture service user opens");
        setup
            .set_call_timeout(Some(Duration::from_secs(30)))
            .expect("the fixture timeout sets");
        setup.execute(
            &format!("CREATE TABLE \"{TABLE}\" (\"customer_key\" NUMBER(10,0), \"customer_id\" VARCHAR2(16), \"segment\" VARCHAR2(16), \"region\" VARCHAR2(16))"),
            &[],
        ).expect("the fixed-schema customer table creates");
        let csv = std::fs::read_to_string(example_root().join("data/dim_customer.csv")).expect("the fixture customers read");
        let mut customers = 0;
        for line in csv.lines().skip(1) {
            // ponytail: this committed fixture has four unquoted fields; fail if that shape changes.
            assert!(!line.contains('"'), "the fixture now needs a CSV parser");
            let fields: Vec<_> = line.split(',').collect();
            assert_eq!(fields.len(), 4, "a fixture customer has four fields");
            let key: i64 = fields[0].parse().expect("the customer key is an integer");
            setup
                .execute(
                    &format!("INSERT INTO \"{TABLE}\" VALUES (:1, :2, :3, :4)"),
                    &[&key, &fields[1], &fields[2], &fields[3]],
                )
                .expect("a fixture customer inserts");
            customers += 1;
        }
        assert_eq!(customers, 40, "the disclosed actual row count changed");
        setup.commit().expect("the fixture commits");
        setup
            .execute(
                &format!("BEGIN DBMS_STATS.SET_TABLE_STATS(USER, '\"{TABLE}\"', numrows => 1200000, avgrlen => 48); END;"),
                &[],
            )
            .expect("the fixture's synthetic statistics set");
        let buckets = setup.query(
            &format!("SELECT ORA_HASH(\"customer_key\", 1), COUNT(*) FROM \"{TABLE}\" GROUP BY ORA_HASH(\"customer_key\", 1) ORDER BY 1"),
            &[],
        ).expect("the actual bucket contents read").map(|row| {
            let row = row.expect("a bucket count reads");
            (row.get::<i64>(0).expect("the bucket is an integer"), row.get::<i64>(1).expect("the count is an integer"))
        }).collect::<Vec<_>>();
        assert_eq!(buckets.len(), 2, "both fixture buckets must be nonempty: {buckets:?}");
        assert_eq!(buckets.iter().map(|(_, count)| count).sum::<i64>(), 40, "{buckets:?}");
        assert!(buckets.iter().all(|(_, count)| *count > 0), "{buckets:?}");
        // TIMESTAMP_TO_SCN can lag commits by several seconds.
        std::thread::sleep(Duration::from_secs(8));

        let root = std::env::temp_dir().join(format!("sutura-{CASE}-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("the fixture material directory creates");
        std::fs::write(root.join("password"), password).expect("the fixture password file writes");
        let pair = rcgen::generate_simple_self_signed([String::from("127.0.0.1")]).expect("a node pair generates");
        std::fs::write(root.join("node.pem"), pair.cert.pem()).expect("the node certificate writes");
        std::fs::write(root.join("node.key"), pair.signing_key.serialize_pem()).expect("the node key writes");
        let catalog = catalog_spanning_two_sources(CASE, LOOKUP_SOURCE);
        let model = catalog.join("models/customers.md");
        rewrite(&model, "table: dim_customer\n", &format!("table: {TABLE}\n"));
        let issuer = an_issuer();
        let keys = PublishedKeySet::of(&issuer, CASE).expect("the fixture key set publishes");
        Self {
            root,
            catalog,
            host: String::from(endpoint.host()),
            port: endpoint.port(),
            user,
            authority,
            certificate: pair.cert.der().clone(),
            issuer,
            keys,
        }
    }

    fn source(&self, split: bool) -> String {
        format!(
            "  {LOOKUP_SOURCE}:\n    kind: oracle\n    host: {}\n    port: {}\n    service_name: FREEPDB1\n    user: {}\n    password_file: {}\n    transport_mode: verified\n    transport_anchors: {}\n    posture: shared-service-user\n{}",
            self.host,
            self.port,
            self.user,
            self.root.join("password").display(),
            self.authority,
            if split { "    split: flashback\n" } else { "" },
        )
    }

    fn pair(&self) -> String {
        format!(
            "  tls_certificate: {}\n  tls_key: {}\n  tls_node_authority: {}\n",
            self.root.join("node.pem").display(),
            self.root.join("node.key").display(),
            self.root.join("node.pem").display()
        )
    }

    fn reader(&self, name: &str, catalog: &Path) -> Served {
        let server = format!("  role: reading-worker\n  host: 127.0.0.1\n  port: 0\n{}", self.pair());
        let security = format!("{SINGLE_USER}  tls_termination: in-process\n");
        start_configured(
            &format!("{CASE}-{name}"),
            &settings_over(catalog, &example_root().join("data"), &server, &security, &self.source(true)),
        )
    }

    fn serving(&self, name: &str, readers: &[String], split: bool) -> Served {
        let port = TcpListener::bind(("127.0.0.1", 0))
            .and_then(|socket| socket.local_addr())
            .expect("a loopback port is free")
            .port();
        let nodes = if readers.is_empty() {
            String::new()
        } else {
            let others = readers
                .iter()
                .map(|url| format!("      - url: {url}\n        role: reading-worker\n        sources: [{LOOKUP_SOURCE}]\n"))
                .collect::<Vec<_>>()
                .concat();
            format!("  nodes:\n    own: https://127.0.0.1:{port}\n    others:\n{others}    across_above_bytes: 1\n")
        };
        let server = format!("  host: 127.0.0.1\n  port: {port}\n{}{nodes}", self.pair());
        let security = format!(
            "  identity: multi-user\n  tls_termination: in-process\n  inbound:\n    mode: direct\n    resource: {}\n    authorization_server: {}\n    key_set_file: {}\n    algorithms: [ES256]\n",
            self.issuer.audience(),
            self.issuer.issuer(),
            self.keys.path().display()
        );
        let sources = format!(
            "{}{ACKNOWLEDGED}{}{ACKNOWLEDGED}",
            files_source(LOCAL_SOURCE, &example_root().join("data")),
            self.source(split)
        );
        start_configured(
            &format!("{CASE}-{name}"),
            &settings_over(&self.catalog, &example_root().join("data"), &server, &security, &sources),
        )
    }

    fn ask(&self, served: &Served) -> (u16, serde_json::Value) {
        let address = address(served);
        let token = self
            .issuer
            .mint(&accepted_by("user@example.com"))
            .expect("the fake issuer signs a token");
        let mut roots = rustls::RootCertStore::empty();
        roots.add(self.certificate.clone()).expect("the node pair is an anchor");
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("the default TLS versions are safe")
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connection = rustls::ClientConnection::new(
            Arc::new(config),
            ServerName::try_from("127.0.0.1")
                .expect("an IP address is a server name")
                .to_owned(),
        )
        .expect("the client connection builds");
        let tcp = TcpStream::connect(&address).expect("the serving node accepts a connection");
        tcp.set_read_timeout(Some(Duration::from_secs(120)))
            .expect("the read timeout sets");
        let mut tls = rustls::StreamOwned::new(connection, tcp);
        let question = recurring_revenue_by_region();
        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{question}",
            v1(sutura_http::constants::base_paths::QUERY),
            question.len()
        );
        tls.write_all(request.as_bytes()).expect("the question writes");
        let mut raw = Vec::new();
        if let Err(cause) = tls.read_to_end(&mut raw)
            && cause.kind() != ErrorKind::UnexpectedEof
        {
            panic!("the reply is not readable: {cause}");
        }
        let raw = String::from_utf8_lossy(&raw);
        let (head, body) = raw.split_once("\r\n\r\n").expect("the HTTP reply has a head and body");
        assert!(
            !head.to_ascii_lowercase().contains("transfer-encoding: chunked"),
            "the fixture expects a length-delimited reply"
        );
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .expect("the HTTP status is an integer");
        (
            status,
            serde_json::from_str(body).unwrap_or_else(|cause| panic!("the reply is not JSON ({cause}): {body}")),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.root));
        drop(std::fs::remove_dir_all(&self.catalog));
    }
}

fn address(served: &Served) -> String {
    served
        .startup
        .iter()
        .find_map(|line| bound_address(line))
        .expect("the served binary reported its bound address")
}

fn rewrite(path: &Path, from: &str, to: &str) {
    let original = std::fs::read_to_string(path).expect("the derived catalog reads");
    let changed = original.replace(from, to);
    assert_ne!(changed, original, "the derived catalog no longer contains {from}");
    std::fs::write(path, changed).expect("the derived catalog writes");
}

#[test]
#[ignore = "requires just oracle-acceptance"]
fn an_oracle_lookup_split_across_readers_equals_the_whole_and_refuses_uncertified_readers() {
    let fixture = Fixture::here();
    let reference = fixture.serving("whole", &[], false);
    let (status, whole) = fixture.ask(&reference);
    assert_eq!(status, 200, "the whole Oracle lookup answers: {whole}");
    assert_eq!(whole["outcome"], "answer", "{whole}");
    let golden = serde_json::json!([
        ["central", "2026-06-01", "51739"],
        ["east", "2026-06-01", "32598"],
        ["north", "2026-06-01", "42157"],
        ["south", "2026-06-01", "21203"],
        ["west", "2026-06-01", "49425"],
        ["null", "2026-06-01", "4999"],
    ]);
    assert_eq!(
        whole["columns"],
        serde_json::json!(["region", "period", "recurring_revenue"]),
        "{whole}"
    );
    assert_eq!(whole["rows"], golden, "the whole lookup must match the hard golden in order");
    drop(reference);

    let unresolved = fixture.serving(
        "unresolved",
        &[
            String::from("https://reader-a.invalid:8443"),
            String::from("https://reader-b.invalid:8443"),
        ],
        true,
    );
    let (status, refused) = fixture.ask(&unresolved);
    assert_eq!(
        status, 503,
        "a granted Oracle lookup with no resolved reader must refuse: {refused}"
    );
    assert_eq!(refused["reason"]["code"], "source_unavailable", "{refused}");
    drop(unresolved);

    let readers = [
        fixture.reader("reader-a", &fixture.catalog),
        fixture.reader("reader-b", &fixture.catalog),
    ];
    let peers = readers.each_ref().map(|reader| format!("https://{}", address(reader)));
    let split = fixture.serving("split", &peers, true);
    let (status, answer) = fixture.ask(&split);
    assert_eq!(
        status, 200,
        "the granted Oracle lookup must answer through its reading workers: {answer}"
    );
    assert_eq!(answer["outcome"], "answer", "{answer}");
    assert_eq!(
        answer["columns"], whole["columns"],
        "the split columns and order match the whole"
    );
    assert_eq!(
        answer["rows"], whole["rows"],
        "every split row and its position match the whole"
    );
    assert_eq!(
        answer["executed_as"],
        serde_json::json!([
            {"source": LOOKUP_SOURCE, "posture": "shared-service-user"},
            {"source": LOCAL_SOURCE, "posture": "shared-service-user"},
        ]),
        "{answer}"
    );
    drop(split);
    drop(readers);

    let different = catalog_spanning_two_sources(&format!("{CASE}-different"), LOOKUP_SOURCE);
    rewrite(
        &different.join("models/customers.md"),
        "table: dim_customer\n",
        &format!("table: {TABLE}\n"),
    );
    rewrite(
        &different.join("metrics/recurring_revenue.md"),
        "description: Where the customer is.\n",
        "description: Where the customer was.\n",
    );
    let readers = [
        fixture.reader("different-a", &different),
        fixture.reader("different-b", &different),
    ];
    let peers = readers.each_ref().map(|reader| format!("https://{}", address(reader)));
    let split = fixture.serving("different", &peers, true);
    let (status, refused) = fixture.ask(&split);
    assert_eq!(status, 403, "a reader with another metric definition must refuse: {refused}");
    assert_eq!(refused["reason"]["code"], "remote_leg_not_certified", "{refused}");
    drop(split);
    drop(readers);
    drop(std::fs::remove_dir_all(different));
    eprintln!(
        "oracle-split: 40 actual rows, 1200000 declared rows, two nonempty buckets; exact whole/split columns and ordered rows; unresolved and uncertified readers refused"
    );
}
