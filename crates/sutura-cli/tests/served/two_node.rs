//! Two sutura nodes over the wire: a SPAWNED `sutura serve` (A) whose `node` source sends a leg of a
//! federated answer to a second node (B), served in this process over TLS on loopback.
//!
//! **What is observed.** A exchanges the caller's own verified token at the identity provider for one
//! with B's audience - the `node` source's `delegation` hop - and dials B with it. B's leg-1 gate
//! verifies that exchanged token, B's Flight mount reads its `sutura:legs.run` scope, and the subject
//! B's surface is asked as is the caller A verified. B's refusal reaches A's caller as A's own typed
//! refusal, naming A's source. An exchanged token without the leg scope reaches no surface at B, and
//! an anonymous caller at A reaches nothing.
//!
//! **What this does not show.** B's surface is a recording fake: it executes no leg and certifies
//! none, so the furthest any cell gets is a refusal, and no row crosses back. B is the gate, router,
//! Flight mount and TLS listener `sutura serve` assembles, composed here rather than by B's own
//! composition root. The identity provider is a loopback fake and both issuers are mocks, so nobody
//! signs in. An anonymous caller's refusal is A's status alone: with no scripted answer the fake
//! identity provider records no dial.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, Sender, channel};

    use sutura_app::Capability;
    use sutura_app::surface::{Adopted, NotAdopted, Surface, SurfaceFailure};
    use sutura_catalog_local::LocalCatalog;
    use sutura_config::{Environment, Settings, Sources, TlsMaterial};
    use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
    use sutura_domain::identity::{RequestContext, Subject};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, PinnedDefinitions, SemanticCatalog as _};
    use sutura_domain::plan::remote::{LegWire, RemoteLegOutcome};
    use sutura_domain::query::RefusalReason;
    use sutura_domain::warehouse::ParamValue;
    use sutura_domain::warehouse::deadline::Deadline;
    use sutura_http::{InboundGate, ServiceState, Termination, TlsListener};
    use sutura_http_client::test_support::{FakeServer, Scripted};

    use crate::harness::{
        DELEGATED, LOCAL_SOURCE, LOOPBACK, Reply, Served, VERSION, accepted_by, catalog_spanning_two_sources, example_root,
        files_source, recurring_revenue_by_region, settings_over, start_configured, v1,
    };

    /// A's source for `customers`, which B serves.
    const NODE_SOURCE: &str = "sales";

    /// B's own resource, and the audience A's delegation hop asks the identity provider for.
    const NODE_RESOURCE: &str = "https://node-b.example.com";

    /// Records the subject each leg is asked as and refuses the leg, which is all B's surface does.
    struct RecordingSurface {
        definitions: Arc<PinnedDefinitions>,
        source: SourceName,
        asked_as: Sender<Subject>,
    }

    impl RecordingSurface {
        fn over_the_example(asked_as: Sender<Subject>) -> Self {
            let source = SourceName::parse(LOCAL_SOURCE).expect("a source name is a name");
            let version = DefinitionVersion::parse(VERSION).expect("the fixed version is a version");
            let definitions = LocalCatalog::new(source.clone(), example_root().join("catalog"), version)
                .load()
                .unwrap_or_else(|cause| panic!("the example catalog does not load: {cause}"));
            Self {
                definitions: Arc::new(definitions),
                source,
                asked_as,
            }
        }
    }

    impl Surface for RecordingSurface {
        fn definitions(&self) -> Arc<PinnedDefinitions> {
            Arc::clone(&self.definitions)
        }

        #[expect(clippy::panic_in_result_fn, reason = "never called: a remote leg never adopts a bundle")]
        fn adopt(&self, _next: PinnedDefinitions) -> Result<Adopted, NotAdopted> {
            panic!("a remote leg never adopts a bundle")
        }

        #[expect(clippy::panic_in_result_fn, reason = "never called: a remote leg never asks a question")]
        fn answer(
            &self,
            _context: &RequestContext,
            _query: &sutura_domain::query::Query,
            _deadline: Deadline,
        ) -> Result<sutura_domain::query::ToolOutcome, SurfaceFailure> {
            panic!("a remote leg never asks a question")
        }

        #[expect(clippy::panic_in_result_fn, reason = "never called: a remote leg never runs raw SQL")]
        fn run_sql(
            &self,
            _context: &RequestContext,
            _statement: &sutura_domain::raw::RawStatement,
            _deadline: Deadline,
        ) -> Result<sutura_domain::raw::RawOutcome, SurfaceFailure> {
            panic!("a remote leg never runs raw SQL")
        }

        fn run_remote_leg(
            &self,
            context: &RequestContext,
            _leg: LegWire,
            _params: Vec<ParamValue>,
            _deadline: Deadline,
        ) -> Result<RemoteLegOutcome, SurfaceFailure> {
            // The receiver lives as long as the node, so a failed send is a node already dropped.
            drop(self.asked_as.send(context.chain().subject().clone()));
            Ok(RemoteLegOutcome::Refusal {
                reason: RefusalReason::RemoteLegNotCertified {
                    source: self.source.clone(),
                },
            })
        }

        fn spend_headroom_bytes(&self) -> Option<u64> {
            None
        }

        fn spent_bytes_total(&self) -> Option<u64> {
            None
        }
    }

    /// A per-test directory for the files both nodes read, removed with it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn create(case: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("sutura-{case}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("a per-test directory is creatable");
            Self(dir)
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, contents).expect("a scratch file is writable");
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// Node B, serving on loopback until its runtime is dropped.
    struct NodeB {
        _runtime: tokio::runtime::Runtime,
        port: u16,
        anchor: PathBuf,
        asked_as: Receiver<Subject>,
    }

    impl NodeB {
        /// The subjects B's surface was asked as since the last call, in order.
        fn asked_as(&self) -> Vec<Subject> {
            self.asked_as.try_iter().collect()
        }
    }

    /// B over a self-signed `localhost` certificate, trusting `idp` for leg 1 at [`NODE_RESOURCE`].
    fn node_b(scratch: &Scratch, idp: &MockIssuer, key_set: &Path) -> NodeB {
        let issued = rcgen::generate_simple_self_signed([String::from("localhost")]).expect("a self-signed pair generates");
        let anchor = scratch.write("node-b.pem", &issued.cert.pem());
        let key = scratch.write("node-b.key", &issued.signing_key.serialize_pem());
        let material = TlsMaterial::parse(anchor.to_str(), key.to_str())
            .expect("a generated pair is valid material")
            .expect("both halves are given");
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!(
            "server:\n{LOOPBACK}  flight_surface:\n    enabled: true\nsecurity:\n  inbound:\n    mode: \"direct\"\n    \
             resource: \"{}\"\n    authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n",
            idp.audience(),
            idp.issuer(),
            key_set.display(),
        )))
        .expect("node B's settings load");
        let (recorded, asked_as) = channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("a runtime builds");
        let served = Arc::new(RecordingSurface::over_the_example(recorded)) as Arc<dyn Surface>;
        let port = runtime.block_on(async move {
            let gate = InboundGate::from_declaration(settings.security().inbound().expect("node B declares leg 1"))
                .expect("node B's gate reads its key set");
            let admission = sutura_runtime::Admission::from_settings(settings.runtime());
            let state = ServiceState::new(served, Arc::new(settings), admission).with_inbound_identity(Arc::new(gate));
            let mount = sutura_http::flight::mount(&state);
            let router = sutura_http::router(&state.with_flight_surface(mount)).expect("node B's router assembles");
            let termination = Termination::prepare(&material).expect("a generated pair terminates TLS");
            let tcp = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .expect("a loopback port is bindable");
            let port = tcp.local_addr().expect("a bound socket has an address").port();
            let listener = TlsListener::wrap(tcp, termination.config()).expect("the listener wraps");
            drop(tokio::spawn(async move {
                let _outcome = axum::serve(
                    axum::serve::ListenerExt::tap_io(listener, |_io| ()),
                    router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .await;
            }));
            port
        });
        NodeB {
            _runtime: runtime,
            port,
            anchor,
            asked_as,
        }
    }

    /// Both nodes, A's leg-1 issuer, and the identity provider A exchanges at.
    struct Deployed {
        served: Served,
        issuer: MockIssuer,
        idp: FakeServer,
        node: NodeB,
        _published: [PublishedKeySet; 2],
        _scratch: Scratch,
    }

    /// A over a `files` source and a `node` source for B; the identity provider answers `answers`
    /// exchanges with tokens for [`DELEGATED`] with B's audience, each granting `scope`.
    fn deployed(case: &str, answers: usize, scope: &str) -> Deployed {
        let scratch = Scratch::create(case);
        let identity_provider = MockIssuer::generating("https://idp.example.com", NODE_RESOURCE, "the-idp-key")
            .expect("a mock issuer generates a key pair");
        let trusted_at_b = PublishedKeySet::of(&identity_provider, &format!("{case}-node-b")).expect("the key set publishes");
        let node = node_b(&scratch, &identity_provider, trusted_at_b.path());
        let exchanged = identity_provider
            .mint(&Token::for_subject(DELEGATED).granting(scope))
            .expect("the identity provider signs a token");
        let idp = FakeServer::start(
            std::iter::repeat_with(|| Scripted::ok(&issued(&exchanged)))
                .take(answers)
                .collect(),
        );

        let issuer = MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "the-current-key")
            .expect("a mock issuer generates a key pair");
        let trusted_at_a = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
        let secret = scratch.write("client-secret", "idp-client-secret\n");
        let data = example_root().join("data");
        let catalog = catalog_spanning_two_sources(case, NODE_SOURCE);
        let security = format!(
            "  identity: \"multi-user\"\n  \
             inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
             authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n  \
             outbound:\n    transport_anchors: \"{}\"\n",
            issuer.audience(),
            issuer.issuer(),
            trusted_at_a.path().display(),
            node.anchor.display(),
        );
        let sources = format!(
            "{}    acknowledged_because: \"the models off the node read fixture files as one identity\"\n  \
             {NODE_SOURCE}:\n    kind: \"node\"\n    endpoint: \"https://localhost:{}\"\n    \
             posture: \"impersonation-at-source\"\n    delegation:\n      - token_endpoint: \"{}/token\"\n        \
             client_id: \"sutura\"\n        client_secret_file: \"{}\"\n        audience: \"{NODE_RESOURCE}\"\n",
            files_source(LOCAL_SOURCE, &data),
            node.port,
            idp.endpoint(),
            secret.display(),
        );
        let served = start_configured(case, &settings_over(&catalog, &data, LOOPBACK, &security, &sources));
        Deployed {
            served,
            issuer,
            idp,
            node,
            _published: [trusted_at_b, trusted_at_a],
            _scratch: scratch,
        }
    }

    fn issued(token: &str) -> serde_json::Value {
        serde_json::json!({
            "access_token": token,
            "issued_token_type": "urn:ietf:params:oauth:token-type:access_token",
            "token_type": "Bearer",
            "expires_in": 300,
        })
    }

    fn ask(deployed: &Deployed, bearer: Option<&str>) -> Reply {
        deployed.served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            bearer,
            &recurring_revenue_by_region(),
        )
    }

    fn the_caller() -> Subject {
        Subject::verified(DELEGATED).expect("a test subject is a subject")
    }

    #[test]
    fn a_leg_reaches_the_other_node_as_the_callers_own_subject_and_its_refusal_comes_back() {
        let mut deployed = deployed("two-node-asked-as", 1, Capability::RunRemoteLeg.scope());
        let token = deployed
            .issuer
            .mint(&accepted_by(DELEGATED))
            .expect("the issuer signs a token");

        let reply = ask(&deployed, Some(&token));
        assert_eq!(reply.status, 403, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "refusal", "{}", reply.body);
        assert_eq!(body["reason"]["code"], "remote_leg_not_certified", "{}", reply.body);
        assert!(
            body["reason"]["detail"]
                .as_str()
                .is_some_and(|detail| detail.contains(&format!("`{NODE_SOURCE}`"))),
            "the refusal must name this node's source, never one the other node wrote: {}",
            reply.body
        );
        assert_eq!(
            deployed.node.asked_as(),
            vec![the_caller()],
            "the other node must be asked once, as the caller A verified"
        );
        deployed.served.terminate();

        let offered = deployed.idp.finish();
        let [offer] = offered.as_slice() else {
            panic!("one leg to the other node is one exchange: {offered:?}");
        };
        let offer = offer.body();
        assert!(
            offer.contains(&format!("subject_token={token}")),
            "the exchange must offer the caller's own verified token: {offer}"
        );
        let audience = NODE_RESOURCE.replace(':', "%3A").replace('/', "%2F");
        assert!(
            offer.contains(&format!("audience={audience}")),
            "the exchange must ask for the other node's audience: {offer}"
        );
    }

    #[test]
    fn an_exchanged_token_without_the_leg_scope_reaches_no_surface_at_the_other_node() {
        let mut deployed = deployed("two-node-unscoped", 1, Capability::AskMetric.scope());
        let token = deployed
            .issuer
            .mint(&accepted_by(DELEGATED))
            .expect("the issuer signs a token");

        let reply = ask(&deployed, Some(&token));
        assert_eq!(reply.status, 503, "{}", reply.body);
        assert!(
            !reply.body.contains(&token),
            "the refusal must not carry the caller's token: {}",
            reply.body
        );
        assert_eq!(
            deployed.node.asked_as(),
            Vec::<Subject>::new(),
            "a token without the leg scope must reach no surface at the other node"
        );
        deployed.served.terminate();
        let offered = deployed.idp.finish();
        assert_eq!(offered.len(), 1, "the exchange did run: {offered:?}");
    }

    #[test]
    fn an_anonymous_caller_is_refused_at_the_first_node() {
        let mut deployed = deployed("two-node-anonymous", 0, Capability::RunRemoteLeg.scope());

        let reply = ask(&deployed, None);
        assert_eq!(reply.status, 401, "{}", reply.body);
        assert_eq!(
            deployed.node.asked_as(),
            Vec::<Subject>::new(),
            "an anonymous caller must reach nothing at the other node"
        );
        deployed.served.terminate();
    }
}
