//! A split PostgreSQL lookup leg granted to `server.role: reading-worker` peers, over up to three
//! SPAWNED `sutura serve` processes on loopback (`github.com/telekom/sutura#828`): a serving node
//! and its reading workers, each its own composition root.
//!
//! **What is observed.** `recurring_revenue` by `region` reads its fact leg on `local` and its
//! lookup leg on `geo`, whose root holds the example's customers and filler past one stream's
//! 1,000,000 rows, analysed so PostgreSQL estimates it. Declared readers of `geo` make the serving
//! node split that leg into two partitions and grant them to the caller; each reader re-certifies
//! the leg against its own catalog and reads its partitions. The answer is the example's six rows,
//! which no whole read of the leg returns: one stream refuses it past 1,000,000 rows. With no
//! reader resolved the question is refused `source_unavailable`.
//!
//! **What this does not show.** Every node runs on one host, presents one self-signed pair and
//! accepts it as its node authority, and reads the tier as its one fixture role, so nothing here
//! tells the serving node's statements from a reader's: that the serving node reads none of the
//! leg's rows is shown only by a refusal when no reader resolves. The serving node's port is
//! taken from a socket closed before it binds. The caller is a mock issuer's token; nobody signs
//! in.

#[cfg(test)]
mod tests {
    use std::io::{ErrorKind, Read as _, Write as _};
    use std::net::TcpStream;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use rustls::pki_types::{CertificateDer, ServerName};
    use sutura_dev::issuer::{MockIssuer, PublishedKeySet};
    use sutura_domain::model::SourceName;
    use sutura_exec_postgres::adbc::{Channel, FixtureAdmin, PostgresDriver};
    use sutura_exec_postgres::fixture::FixtureCredential;

    use crate::harness::{
        LOCAL_SOURCE, LOOKUP_SOURCE, SINGLE_USER, Served, accepted_by, an_issuer, bound_address, config_path, derived_beside,
        derived_catalog, discover_tier, example_root, load_into_tier, recurring_revenue_by_region, settings_over, source_entry,
        start_configured, v1,
    };

    /// The table `customers` reads here instead of the example's `dim_customer`.
    const SPLIT_TABLE: &str = "dim_customer_split";

    /// Filler customers no subscription names, past one stream's 1,000,000 rows: two partitions.
    const FILLER_ROWS: u32 = 1_200_000;

    /// Two peers declared as reading `geo` that resolve to no address.
    const UNRESOLVED: [&str; 2] = ["https://reader-a.invalid:8443", "https://reader-b.invalid:8443"];

    /// A serving node's own reason for each shared source it declares.
    const ACKNOWLEDGED: &str = "    acknowledged_because: \"the cells read fixture tables as one identity\"\n";

    /// The caller every cell asks as.
    const CALLER: &str = "user@example.com";

    /// The tier, loaded with the example and [`SPLIT_TABLE`], held for as long as `lock` lives.
    struct Tier<Lock> {
        _lock: Lock,
        port: u16,
        database: String,
        user: String,
        password_file: PathBuf,
        anchor: String,
    }

    impl<Lock> Tier<Lock> {
        /// A `postgres` source called `name` over the tier.
        fn source(&self, name: &str) -> String {
            source_entry(name, self.port, &self.database, &self.user, &self.password_file, &self.anchor)
        }
    }

    /// The serving node's catalog, the tier, the pair every node presents and the caller's issuer.
    struct Fixture<Lock> {
        case: &'static str,
        catalog: PathBuf,
        tier: Tier<Lock>,
        certificate: PathBuf,
        key: PathBuf,
        anchor: CertificateDer<'static>,
        issuer: MockIssuer,
        key_set: PublishedKeySet,
    }

    impl<Lock> Drop for Fixture<Lock> {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(derived_beside(&config_path(self.case))));
        }
    }

    /// The serving node, and the port it listens on.
    struct Serving {
        _served: Served,
        port: u16,
    }

    /// The example catalog with `customers` and `regions` on `geo` and `customers` reading
    /// [`SPLIT_TABLE`]. `regions` moves too, so `sales_area`'s chain crosses once and stays.
    fn split_catalog(case: &str) -> PathBuf {
        let catalog = derived_catalog(case, &example_root().join("catalog"), "customers.md", LOOKUP_SOURCE);
        let models = catalog.join("models");
        rewrite(
            &models.join("customers.md"),
            "table: dim_customer\n",
            &format!("table: {SPLIT_TABLE}\n"),
        );
        rewrite(
            &models.join("regions.md"),
            &format!("source: {LOCAL_SOURCE}\n"),
            &format!("source: {LOOKUP_SOURCE}\n"),
        );
        catalog
    }

    /// `path` with `from` replaced by `to`, which must have been there.
    fn rewrite(path: &Path, from: &str, to: &str) {
        let text = std::fs::read_to_string(path).expect("a derived catalog document is readable");
        let rewritten = text.replace(from, to);
        assert_ne!(rewritten, text, "{} no longer holds `{from}`", path.display());
        std::fs::write(path, rewritten).expect("a derived catalog document is writable");
    }

    /// [`SPLIT_TABLE`] as the example's customers and [`FILLER_ROWS`] more, analysed.
    fn grow_the_lookup_root() {
        let tier = discover_tier().expect("the tier was just loaded");
        let fixture = SourceName::parse("fixture").expect("the fixture's own source name parses");
        let conninfo = FixtureCredential::from_env()
            .unwrap_or_else(|unconfigured| panic!("{unconfigured}"))
            .conninfo(&fixture, &tier.host, tier.port, Channel::Plaintext)
            .expect("the tier's published endpoint builds a connection string");
        let driver = PostgresDriver::from_host().expect("the tier is up, so a driver is named");
        let mut admin = FixtureAdmin::open(&driver, &conninfo).expect("the tier accepts a fixture connection");
        admin
            .run(&format!(
                "DROP TABLE IF EXISTS {SPLIT_TABLE}; CREATE TABLE {SPLIT_TABLE} (LIKE dim_customer); \
                 INSERT INTO {SPLIT_TABLE} SELECT * FROM dim_customer; \
                 INSERT INTO {SPLIT_TABLE} SELECT 1000000 + n, 'F' || n, 'consumer', 'filler' \
                 FROM generate_series(1, {FILLER_ROWS}) AS n; ANALYZE {SPLIT_TABLE}"
            ))
            .expect("the lookup root grows past one stream's rows");
    }

    /// The fixture for `case`, or `None` where no PostgreSQL tier is provisioned.
    fn fixture(case: &'static str) -> Option<Fixture<impl Sized>> {
        discover_tier()?;
        // Before the load: the derivation clears the directory the load writes the password to.
        let catalog = split_catalog(case);
        let loaded = load_into_tier(case, LOOKUP_SOURCE)?;
        grow_the_lookup_root();
        let pair = rcgen::generate_simple_self_signed([String::from("127.0.0.1")]).expect("a node pair generates");
        let nodes = derived_beside(&config_path(case));
        let (certificate, key) = (nodes.join("node.pem"), nodes.join("node.key"));
        std::fs::write(&certificate, pair.cert.pem()).expect("the node certificate is writable");
        std::fs::write(&key, pair.signing_key.serialize_pem()).expect("the node key is writable");
        let issuer = an_issuer();
        let key_set = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
        Some(Fixture {
            case,
            catalog,
            tier: Tier {
                _lock: loaded.guard,
                port: loaded.port,
                database: loaded.database,
                user: loaded.user,
                password_file: loaded.password_file,
                anchor: loaded.anchor,
            },
            certificate,
            key,
            anchor: pair.cert.der().clone(),
            issuer,
            key_set,
        })
    }

    impl<Lock> Fixture<Lock> {
        /// The pair and the authority under `server:`.
        fn pair(&self) -> String {
            format!(
                "  tls_certificate: \"{}\"\n  tls_key: \"{}\"\n  tls_node_authority: \"{}\"\n",
                self.certificate.display(),
                self.key.display(),
                self.certificate.display(),
            )
        }

        /// A reading worker called `name` over `catalog`, declaring `sources`.
        fn reader(&self, name: &str, catalog: &Path, sources: &str) -> Served {
            let server = format!(
                "  role: \"reading-worker\"\n  host: \"127.0.0.1\"\n  port: 0\n{}",
                self.pair()
            );
            let security = format!("{SINGLE_USER}  tls_termination: \"in-process\"\n");
            let settings = settings_over(catalog, &example_root().join("data"), &server, &security, sources);
            start_configured(&format!("{}-{name}", self.case), &settings)
        }

        /// The serving node over `sources`, declaring a reading worker of `geo` at each of `readers`
        /// and planning every federated question across them.
        fn serving(&self, readers: &[String], sources: &str) -> Serving {
            let port = std::net::TcpListener::bind(("127.0.0.1", 0))
                .and_then(|socket| socket.local_addr())
                .expect("a loopback port is free")
                .port();
            let others = readers
                .iter()
                .map(|url| {
                    format!("      - url: \"{url}\"\n        role: \"reading-worker\"\n        sources: [\"{LOOKUP_SOURCE}\"]\n")
                })
                .collect::<Vec<_>>()
                .concat();
            let server = format!(
                "  host: \"127.0.0.1\"\n  port: {port}\n{}  nodes:\n    own: \"https://127.0.0.1:{port}\"\n    \
                 others:\n{others}    across_above_bytes: 1\n",
                self.pair(),
            );
            let security = format!(
                "  identity: \"multi-user\"\n  tls_termination: \"in-process\"\n  inbound:\n    mode: \"direct\"\n    \
                 resource: \"{}\"\n    authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n",
                self.issuer.audience(),
                self.issuer.issuer(),
                self.key_set.path().display(),
            );
            let settings = settings_over(&self.catalog, &example_root().join("data"), &server, &security, sources);
            Serving {
                _served: start_configured(self.case, &settings),
                port,
            }
        }

        /// The example's question asked of `serving` over TLS by [`CALLER`]: the status and the body.
        fn ask(&self, serving: &Serving) -> (u16, serde_json::Value) {
            let token = self.issuer.mint(&accepted_by(CALLER)).expect("the issuer signs a token");
            let mut roots = rustls::RootCertStore::empty();
            roots.add(self.anchor.clone()).expect("the node certificate is an anchor");
            let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("the default protocol versions are safe")
                .with_root_certificates(roots)
                .with_no_client_auth();
            let name = ServerName::try_from("127.0.0.1")
                .expect("an address is a server name")
                .to_owned();
            let connection = rustls::ClientConnection::new(Arc::new(config), name).expect("a client connection builds");
            let tcp = TcpStream::connect(("127.0.0.1", serving.port)).expect("the serving node accepts a connection");
            tcp.set_read_timeout(Some(Duration::from_secs(120)))
                .expect("a read timeout is settable");
            let mut tls = rustls::StreamOwned::new(connection, tcp);
            let question = recurring_revenue_by_region();
            let request = format!(
                "POST {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{question}",
                v1(sutura_http::constants::base_paths::QUERY),
                serving.port,
                question.len(),
            );
            tls.write_all(request.as_bytes()).expect("the question is writable");
            let mut raw = Vec::new();
            // A peer that closes without `close_notify` ends the reply as an unexpected end of file.
            if let Err(cause) = tls.read_to_end(&mut raw)
                && cause.kind() != ErrorKind::UnexpectedEof
            {
                panic!("the reply is not readable: {cause}");
            }
            let reply = String::from_utf8_lossy(&raw);
            let (head, body) = reply.split_once("\r\n\r\n").expect("a reply has a head and a body");
            let status = head
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse().ok())
                .unwrap_or_else(|| panic!("a reply opens with a status line: {head}"));
            let body =
                serde_json::from_str(body).unwrap_or_else(|cause| panic!("the {status} body is not JSON ({cause}): {body}"));
            (status, body)
        }
    }

    /// The address a reading worker reports listening on, as a peer of the serving node.
    fn url(reader: &Served) -> String {
        let bound = reader
            .startup
            .iter()
            .find_map(|line| bound_address(line))
            .expect("a started node reported its address");
        format!("https://{bound}")
    }

    /// `recurring-revenue-by-region__rows.snap`'s rows, as this transport serves them.
    fn whole_leg_rows() -> serde_json::Value {
        serde_json::json!([
            ["central", "2026-06-01", "51739"],
            ["east", "2026-06-01", "32598"],
            ["north", "2026-06-01", "42157"],
            ["south", "2026-06-01", "21203"],
            ["west", "2026-06-01", "49425"],
            ["null", "2026-06-01", "4999"],
        ])
    }

    fn unresolved() -> Vec<String> {
        UNRESOLVED.map(String::from).to_vec()
    }

    #[test]
    fn a_postgres_only_deployment_refuses_a_granted_lookup_no_reading_worker_resolves_for() {
        let Some(fixture) = fixture("split-pg-only-unresolved") else {
            return;
        };
        let sources = format!(
            "{}{ACKNOWLEDGED}{}{ACKNOWLEDGED}",
            fixture.tier.source(LOCAL_SOURCE),
            fixture.tier.source(LOOKUP_SOURCE)
        );
        let serving = fixture.serving(&unresolved(), &sources);

        let (status, body) = fixture.ask(&serving);
        assert_eq!(body["reason"]["code"], "source_unavailable", "{body}");
        assert_eq!(status, 503, "{body}");
        assert!(
            body["reason"]["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains(LOOKUP_SOURCE)),
            "the refusal must name the granted source: {body}"
        );
    }

    #[test]
    fn a_postgres_only_deployment_answers_a_lookup_split_across_its_two_reading_workers() {
        let Some(fixture) = fixture("split-pg-only-read") else {
            return;
        };
        let theirs = format!("{}{}", fixture.tier.source(LOCAL_SOURCE), fixture.tier.source(LOOKUP_SOURCE));
        let readers = [
            fixture.reader("a", &fixture.catalog, &theirs),
            fixture.reader("b", &fixture.catalog, &theirs),
        ];
        let sources = format!(
            "{}{ACKNOWLEDGED}{}{ACKNOWLEDGED}",
            fixture.tier.source(LOCAL_SOURCE),
            fixture.tier.source(LOOKUP_SOURCE)
        );
        let serving = fixture.serving(&readers.each_ref().map(url), &sources);

        let (status, body) = fixture.ask(&serving);
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["outcome"], "answer", "{body}");
        assert_eq!(body["rows"], whole_leg_rows(), "{body}");
        assert_eq!(
            body["executed_as"],
            serde_json::json!([
                { "source": LOOKUP_SOURCE, "posture": "shared-service-user" },
                { "source": LOCAL_SOURCE, "posture": "shared-service-user" },
            ]),
            "{body}"
        );
    }
}
