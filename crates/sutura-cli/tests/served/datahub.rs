//! `catalog.kind: datahub`, served for real: a loopback `DataHub` fake (issue #202's own, reused
//! from `sutura_catalog_datahub::test_support` rather than rebuilt here) backs a composed deployment
//! that boots and answers a certified question over HTTP - the wave-one E2E plan's lane 2 cell,
//! "a served binary built with the new feature boots against a `DataHub` catalog and answers a
//! certified question."
//!
//! # Why this is a NEW file rather than a case added to `served.rs`
//!
//! Every existing case in this suite serves a `markdown` catalog; this one serves a `datahub`
//! catalog, behind this crate's `datahub` feature, over an HTTP fake instead of a files-backed
//! prose tree. Splitting it keeps `served.rs` markdown-only and this file's own `#[cfg(feature =
//! "datahub")]` gate legible at the module boundary rather than smeared across one `#[cfg]` per
//! case in a shared file - the same reason `harness/postgres.rs` is its own file behind `postgres`.
//!
//! # The source mapping this test exercises, and why the data is `files`-backed
//!
//! `open_one_datahub_catalog` (`sutura-serve/src/catalog.rs`) fixes the `dataPlatform` -> source
//! alias mapping to the single literal `"bigquery"`, answered by the CATALOG's OWN declared name -
//! the same convention `sutura_catalog_datahub::fixture::over_fixture_source` uses. So this
//! deployment's `sources:` entry is named IDENTICALLY to its `catalogs:` entry (`"metrics"`, below),
//! and its `kind` is `files`: nothing here needs a real `BigQuery`, because the mapping only cares that
//! the NAME matches, not that the engine behind it is the platform `DataHub`'s dataset aspects claim.
//! `crates/sutura-exec-datafusion` reads one CSV per table, named after the model's `table`, which is
//! why the two files below are `orders.csv`/`customers.csv` - the datasets
//! `sutura_catalog_datahub::test_support::dataset_page` declares sit on tables `orders`/`customers`
//! (the URN's middle segment), so the engine resolves each to its own `<table>.csv`.
//!
//! # The fixture reused, and the anchor deliberately reasoned about
//!
//! `test_support::happy_path_answers()` serves the SAME three pages
//! `crates/sutura-catalog-datahub/tests/http_reader.rs` certifies against - unmodified, so this test
//! cannot drift from that crate's own proof of the wire shape. That corpus's certified `revenue`
//! metric carries an ANCHOR: `{"range":{"start":"2026-06-01","end":"2026-07-01"},"value":"412345"}` -
//! `sutura_app::verify_anchors` RE-EXECUTES it against the attached engine at BOOT, before this
//! process ever reports `listening` (`served/harness.rs`'s `START_BUDGET` doc says so). So the CSV
//! rows below are not arbitrary: `SUM(amount_cents) WHERE status = 'active' AND order_date IN
//! [2026-06-01, 2026-07-01)` is engineered to equal exactly `412345`, split across three active
//! orders (200000 + 150000 + 62345) plus one cancelled order and one out-of-range order that must
//! NOT be counted - the anchor check is itself a second, boot-time proof that this test's data is
//! correct, ahead of anything this file asserts by asking. A CSV total this file got wrong would
//! fail the DEPLOYMENT'S OWN boot, not a later assertion.
//!
//! # What is asserted after boot
//!
//! One POST of `{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}`
//! (no dimension, so the relationship to `customers` is exercised only structurally - harvested,
//! assembled, never joined for this question). The assertion is against `answer["rows"]`, mirroring
//! `served.rs`'s own `recurring_revenue_june` shape byte for byte (`columns`, `rows`,
//! `executed_as`).
//!
//! # RED/GREEN
//!
//! The mutation this cell is meant to catch: revert `sutura-serve/src/catalog.rs`'s `Datahub` arm to
//! the unconditional refusal it replaced - RED, the deployment never boots, `refused_to_start`'s
//! shape rather than this file's. GREEN is this file as written. Verified by this branch's lane and
//! independently by the PR review's mutations (M1/M4/M6 red, base green).

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use sutura_catalog_datahub::test_support::{DEPLOYMENT_PROPERTY, FakeServer, happy_path_answers};

    use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside, start_configured, v1};

    /// The catalog's declared name, reused verbatim as the `sources:` entry's own name - see the
    /// module header on why that identity is what the fixed `"bigquery"` alias mapping needs.
    const CATALOG: &str = "metrics";

    /// `orders`'s rows: three active orders inside the certified anchor's June window (summing to
    /// its declared `412345`), one cancelled order inside the window (excluded by the metric's own
    /// `required_filters`), and one active order outside the window (excluded by the question's
    /// range) - so a filter or a range mistake would move the total rather than leave it looking
    /// right by accident. The file is named `<table>.csv`: the datasets `test_support::dataset_page`
    /// declares are on tables `orders`/`customers` (the URN's middle segment), and the `files`
    /// engine reads one CSV per model, named after the model's `table`.
    const ORDERS_CSV: &str = "order_id,customer_id,amount_cents,order_date,status\n\
         O1,C1,200000,2026-06-01,active\n\
         O2,C2,150000,2026-06-01,active\n\
         O3,C1,62345,2026-06-01,active\n\
         O4,C2,999999,2026-06-01,cancelled\n\
         O5,C1,111111,2026-05-01,active\n";

    /// `customers`'s rows - the `segment` dimension `test_support::relationship_page`'s harvested
    /// relationship reaches, unused by the question this file asks but required for the bundle to
    /// assemble (a dimension's `via` names a relationship whose target model must exist and load).
    const CUSTOMERS_CSV: &str = "customer_id,segment\n\
         C1,retail\n\
         C2,wholesale\n";

    /// A directory this test owns and removes on every path out, including a panicking assertion -
    /// the same promise `served/harness.rs`'s own `Served`/`Spawned` guards make for the settings
    /// directory, made here for the sibling one this case writes its CSVs into. A sibling of the
    /// settings directory rather than inside it: `written()` (via `start_configured`) clears and
    /// recreates the settings directory, which would delete these files if they lived under it.
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the data directory is creatable");
            std::fs::write(path.join("orders.csv"), ORDERS_CSV).expect("orders.csv is writable");
            std::fs::write(path.join("customers.csv"), CUSTOMERS_CSV).expect("customers.csv is writable");
            std::fs::write(path.join("token"), "pat-under-test\n").expect("the token file is writable");
            Self(path)
        }

        fn token_file(&self) -> PathBuf {
            self.0.join("token")
        }

        /// Writes a declared `security.outbound.transport_anchors` PEM bundle into this data
        /// directory (a sibling of the settings directory, which `written()` clears), returning its
        /// absolute path - the path a deployment's `security.outbound` names.
        fn bundle_file(&self, name: &str, certificate: &rcgen::Certificate) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, certificate.pem()).expect("the anchor bundle is writable");
            path
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings a `datahub`-kind deployment needs: `server`/`security`/`telemetry` mirror every
    /// other case in this suite (see `served/harness.rs::settings_over`); `catalogs`/`sources` are
    /// this file's own, since no existing helper writes a non-markdown catalog. `dir`/`data_dir` on
    /// the `catalogs` entry are `CatalogSettings::parse`'s two path fields, required non-empty for
    /// EVERY kind including `datahub` (a stated limit of this PR's settings surface, also carried by
    /// `docs/serving.md`'s `catalogs[].dir`/`data_dir` rows), and unread by `open_one_datahub_catalog`
    /// - hence the obviously-unused placeholder value rather than a real directory.
    fn settings(server: &FakeServer, data: &DataDir) -> String {
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"datahub\"\n    \
                 dir: \"/unused-for-datahub\"\n    \
                 data_dir: \"/unused-for-datahub\"\n    \
                 version: \"{VERSION}\"\n    \
                 endpoint: \"{endpoint}\"\n    \
                 token_file: \"{token_file}\"\n    \
                 metric_property: \"{DEPLOYMENT_PROPERTY}\"\n\
             sources:\n  \
               {CATALOG}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{data_dir}\"\n    \
                 posture: \"shared-service-user\"\n",
            endpoint = server.endpoint(),
            token_file = data.token_file().display(),
            data_dir = data.0.display(),
        )
    }

    /// The one question this file asks: `revenue`, over exactly the certified anchor's own range -
    /// see the module header for why that range is not incidental.
    const QUESTION: &str = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    #[test]
    fn a_datahub_catalog_answers_a_certified_question_from_the_served_binary() {
        let case = "datahub-answer";
        let data = DataDir::prepared(case);
        // The composition root loads `DataHubCatalog` TWICE at boot - once here for `catalog::load`
        // (so the engine can be opened for the sources the catalog names) and once inside
        // `LocalService::start_composed`, which loads the catalog it serves rather than trusting the
        // bundle it was handed (`crates/sutura-cli/src/serve.rs` says so). Each `load()` reads the
        // three pages, so the fake must answer SIX connections: `happy_path_answers()` twice. Because
        // every response carries `Connection: close`, the reader opens a fresh connection per page,
        // so after boot the fake has served exactly those six and `finish()` (below) joins a thread
        // that has already exited - no waiting on a seventh. Once boot is done no question reads the
        // fake again (`DataHubCatalog` read its snapshot at load).
        let mut answers = happy_path_answers();
        answers.extend(happy_path_answers());
        let server = FakeServer::start(answers);
        let deployment = start_configured(case, &settings(&server, &data));

        let reply = deployment.post(&v1(sutura_http::constants::base_paths::QUERY), Some(TOKEN), QUESTION);
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["columns"], serde_json::json!(["period", "revenue"]));
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "412345"]]));
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": CATALOG, "posture": "shared-service-user" }]),
            "{}",
            reply.body
        );
        // **`token_file` actually reaches the wire.** The answer body alone would not prove the
        // declared token file was read and sent: a composition root that ignored it (or read the
        // wrong key) would still answer the question. So the served binary's captured requests must
        // ALL carry the bearer, one per page - `finish()` joins the (already-exited) fake thread and
        // returns every request's `authorization` header.
        let authorizations = server.finish();
        assert!(
            !authorizations.is_empty(),
            "the served binary sent no DataHub page request at all"
        );
        assert!(
            authorizations
                .iter()
                .all(|seen| seen.as_deref() == Some("Bearer pat-under-test")),
            "every DataHub page request must carry the token_file's bearer, got: {authorizations:?}"
        );
    }

    /// **The boot-line cell that holds the CATALOG seam of #742's review-held limit**: a
    /// `catalog.kind: datahub` deployment whose endpoint dials an HTTPS loopback fake (leaf issued
    /// by a CA the deployment declares as `security.outbound.transport_anchors`) BOOTS and answers -
    /// proving the served binary threads its ONE boot-time `outbound` value into the catalog reader,
    /// whose handshake against the declared CA is what lets the catalog load at all. A reader that
    /// ignored the anchors (or a composition root that never handed them over) would refuse the
    /// self-signed leaf at CATALOG LOAD and the process would never report listening. This closes
    /// only the CATALOG half of #742's limit; the STS/job seam (`broker.rs:101`) stays
    /// review-held - handing the STS agent `None` still passes the whole suite.
    ///
    /// **RED/GREEN.** Removing the `outbound` threading from `sutura-serve/src/catalog.rs` (or
    /// dropping the `.tls_config(..)` arm from the reader) makes the boot handshake refuse the
    /// leaf, `refused_to_start` fires, and this test goes red. GREEN is boot + the same certified
    /// answer the plaintext sibling asserts.
    ///
    /// **The fake and the corpus.** A loopback TLS server presenting an `rcgen`-issued IP-SAN leaf
    /// and answering the SAME `happy_path_answers()` (twice - `catalog::load` plus
    /// `LocalService::start_composed` read the three pages each, exactly as the plaintext sibling
    /// reasons), so a deployment declaring that leaf's own certificate as its anchor stores
    /// verifies it. `security.outbound` is anchors only, so no client identity is involved.
    #[test]
    fn a_declared_anchor_bundle_lets_the_served_datahub_catalog_boot_and_answer() {
        let case = "datahub-answer-anchored";
        let data = DataDir::prepared(case);
        let issued = issue();
        let bundle = data.bundle_file("declared-root.pem", &issued.certificate);
        let mut answers = happy_path_answers();
        answers.extend(happy_path_answers());
        let tls = TlsDataHub::start(&issued, answers);
        let deployment = start_configured(case, &settings_with_anchors(&tls, &data, &bundle));

        let reply = deployment.post(&v1(sutura_http::constants::base_paths::QUERY), Some(TOKEN), QUESTION);
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "412345"]]));
    }

    /// A self-signed leaf whose SAN names the IP literal the endpoint dials (`127.0.0.1`): server
    /// name verification is by dial, so the SAN must match the literal in the declared `endpoint`.
    struct Issued {
        certificate: rcgen::Certificate,
        key: rcgen::KeyPair,
    }

    fn issue() -> Issued {
        // `CertificateParams::new` turns a SAN string that parses as an IP into
        // `SanType::IpAddress` - the SAN rustls checks an IP dial against.
        let params =
            rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP subject alternative name parameterizes");
        let key = rcgen::KeyPair::generate().expect("a key pair generates");
        let certificate = params.self_signed(&key).expect("a self-signed leaf signs");
        Issued { certificate, key }
    }

    fn server_config(issued: &Issued) -> Arc<rustls::ServerConfig> {
        let chain: Vec<CertificateDer<'static>> = vec![issued.certificate.der().clone()];
        let key = PrivateKeyDer::try_from(issued.key.serialize_der()).expect("a generated key is a usable private key");
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        Arc::new(
            rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("the default protocol versions are safe")
                .with_no_client_auth()
                .with_single_cert(chain, key)
                .expect("a freshly generated chain and its own key are a usable pair"),
        )
    }

    /// A loopback TLS `DataHub` fake answering the given scripted pages, presenting one leaf. The
    /// deployment's catalog is read twice (load plus serve), so the corpus below carries six pages.
    struct TlsDataHub {
        addr: std::net::SocketAddr,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl TlsDataHub {
        fn start(issued: &Issued, answers: Vec<sutura_catalog_datahub::test_support::Scripted>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
            let addr = listener.local_addr().expect("a bound listener has a local address");
            let config = server_config(issued);
            let handle = thread::spawn(move || serve_until(&listener, &config, &answers));
            Self {
                addr,
                handle: Some(handle),
            }
        }
    }

    impl Drop for TlsDataHub {
        fn drop(&mut self) {
            let _ignored = self.handle.take();
        }
    }

    fn serve_until(
        listener: &TcpListener,
        config: &Arc<rustls::ServerConfig>,
        answers: &[sutura_catalog_datahub::test_support::Scripted],
    ) {
        let _ignored = listener.set_nonblocking(true);
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut next = 0;
        while next < answers.len() && Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    // On Darwin, an accepted socket inherits the LISTENER's non-blocking flag -
                    // undone here so the read/write calls below block normally rather than
                    // racing a `WouldBlock` mid-handshake or mid-response.
                    let _ignored = stream.set_nonblocking(false);
                    serve_connection(stream, config, &answers[next]);
                    next += 1;
                }
                Err(cause) if cause.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    }

    fn serve_connection(
        stream: TcpStream,
        config: &Arc<rustls::ServerConfig>,
        answer: &sutura_catalog_datahub::test_support::Scripted,
    ) {
        let mut tcp: TcpStream = stream;
        let mut connection = rustls::ServerConnection::new(Arc::clone(config)).expect("a server connection builds");
        {
            let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
            let mut request = [0_u8; 2048];
            let _ignored = tls.read(&mut request);
            let head = format!(
                "HTTP/1.1 {} OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                answer.status_code(),
                answer.body().len()
            );
            let _ignored = tls.write_all(head.as_bytes());
            let _ignored = tls.write_all(answer.body());
        }
        // A graceful `close_notify` before the socket drops - see `http_reader.rs`'s own
        // `serve_connection` for why an abrupt drop reads as `UnexpectedEof` on the client side.
        connection.send_close_notify();
        let _ignored = connection.complete_io(&mut tcp);
    }

    /// The settings for the anchored boot-line cell - the plaintext sibling's [`settings`], with the
    /// endpoint dialed over `https://` and a `security.outbound.transport_anchors` naming the CA
    /// that signed the fake's leaf. Absolute bundle path (a relative one is refused).
    fn settings_with_anchors(tls: &TlsDataHub, data: &DataDir, bundle: &Path) -> String {
        let anchors = format!("  outbound:\n    transport_anchors: \"{}\"\n", bundle.display());
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             {ANCHORS}telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"datahub\"\n    \
                 dir: \"/unused-for-datahub\"\n    \
                 data_dir: \"/unused-for-datahub\"\n    \
                 version: \"{VERSION}\"\n    \
                 endpoint: \"https://{addr}\"\n    \
                 token_file: \"{token_file}\"\n    \
                 metric_property: \"{DEPLOYMENT_PROPERTY}\"\n\
             sources:\n  \
               {CATALOG}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{data_dir}\"\n    \
                 posture: \"shared-service-user\"\n",
            addr = tls.addr,
            token_file = data.token_file().display(),
            data_dir = data.0.display(),
            ANCHORS = anchors,
        )
    }
}
