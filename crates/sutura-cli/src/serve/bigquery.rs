//! The `BigQuery` half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **Split out of `main.rs` when a second adapter's composition pushed that file past the
//! unexemptable 1000-line cap**, and it takes the shape `sutura-cli`'s `sources/` directory already
//! has: the dispatcher stays in the composition root, and each kind's open-and-build pair lives in a
//! file of its own. Both `open_bigquery` definitions are here - the one that opens the adapter and
//! the refusal for a build that linked none - because the compiler picks between them at the one
//! call site, and keeping them apart is what let that refusal live in the composition root at all.
//!
//! What is NOT here: the ADBC driver it loads and the broker that resolves a subject to a principal
//! are `sutura-exec-bigquery`'s and this root's `broker` module's, reached through `sutura_config`'s
//! parsed placement and this root's own helpers by their `super::` paths.

/// Opens one `BigQuery` adapter per declared source, over the ADBC driver.
///
/// **Nothing is attached and nothing is registered, which is the difference from `open_files` that
/// matters:** the tables live in the dataset. What this function does instead is everything that can
/// fail before a listener is bound - the posture cross-check, the driver path, and PARSING the
/// declared pool audience, each of which would otherwise fail on a question rather than at boot.
#[cfg(feature = "bigquery")]
pub(crate) fn open_bigquery(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
    request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::BigQuerySource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let engine = build_bigquery(source, configured, request_timeout, outbound)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for the
    // reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::BigQuery)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the `BigQuery` adapter.
///
/// **Two definitions of one signature rather than a `cfg` inside one body**, so every caller -
/// `open_engine`'s single-kind arm and `kind::bigquery_group` - makes one call and the compiler
/// decides which of these it reaches. The parameters this body does not read are named for it,
/// which is what lets both signatures stay identical under `dead_code = "deny"`.
///
/// The message names the FEATURE and not just the kind, because the two things an operator can do are
/// in two different files: change the `kind:`, or build with `--features bigquery`. A message that
/// only named the kind sent them to the first when they wanted the
/// second - which was this refusal's shape before the adapter was registered at all.
#[cfg(not(feature = "bigquery"))]
pub(crate) fn open_bigquery(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
    _request_timeout: sutura_config::RequestTimeout,
    _outbound: Option<&sutura_tls::Declared>,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: bigquery`, and this binary was built without the `bigquery` \
         feature - so it links no BigQuery adapter and composes the in-process engine only. Build \
         `sutura-cli` with `--features bigquery`, or declare a `files` source"
    ))
}

/// Builds one `BigQuery` adapter, after checking this build can deliver the source's posture.
///
/// **Every value it needs is declared on the source entry.** Who its jobs are billed to, the
/// bytes-billed ceiling and - where the source impersonates - the audience of the pool the caller's
/// assertion is exchanged against. The request timeout is passed here and not read: a job that a
/// question runs carries what is left of that question's deadline as its `jobTimeoutMs`, and a
/// boot-path job (anchor verification, the identity read) carries none. The ceiling is parsed here
/// and sent on every statement as `bigquery.query.max_bytes_billed`.
///
/// The audience is parsed HERE as well as in `sutura-config`, and that is the single-owner rule
/// rather than laziness: the crate that puts a value into a request is the one whose parse decides
/// whether it can be sent. What the settings tree owns is that the key was written. There is no
/// `scope` setting.
#[cfg(feature = "bigquery")]
fn build_bigquery(
    source: &sutura_domain::model::SourceName,
    configured: &sutura_config::ConfiguredSource,
    _request_timeout: sutura_config::RequestTimeout,
    outbound: Option<&sutura_tls::Declared>,
) -> Result<super::BigQuerySource, String> {
    crate::bigquery_driver::refuse_undeliverable_outbound(source, outbound)?;
    let sutura_config::SourcePlacement::BigQuery {
        ref billing,
        max_bytes_billed,
    } = *configured.placement()
    else {
        return Err(format!(
            "`sources.{source}` reached the BigQuery attach step with a placement no BigQuery adapter \
             reads, which `one_kind` should have dispatched elsewhere"
        ));
    };
    let identity = configured
        .identity()
        .ok_or_else(|| format!("`sources.{source}` declares no identity a query could run under"))?;
    // The same cross-check `open_files` makes and against a DIFFERENT constant, which is the point of
    // it being per adapter rather than per deployment: this adapter declares `PerSubjectCredential`,
    // so a `shared-service-user` entry is deliverable and an `impersonation-at-source` entry passes
    // the adapter's capability half. The composition's half - the broker that resolves a subject to
    // the principal a source declared - is `crate::serve::broker`, attached in `run()`'s `bigquery`
    // and `mixed` arms rather than here. **The claim this comment used to make, that the attaching
    // half was unchanged by the wire removal, was wrong**: the broker it named was deleted with the
    // wire, so between that commit and this one an `impersonation-at-source` entry booted clean and
    // was refused `credential_unavailable` on every question.
    identity
        .posture()
        .deliverable_by(
            <super::BigQuerySource as sutura_domain::warehouse::Warehouse>::IMPERSONATION,
            source,
        )
        .map_err(super::flatten)?;
    // **The ADBC driver authenticates itself, so there is no credential to read and no token
    // rotation to drive - the removed `wire` half.** The driver is opened at BOOT rather than on the
    // first question, which is the same argument the inbound key set is read before the listener
    // opens: a driver this process cannot open has to stop it, not become a deployment that answers
    // every question with a failure while its startup log says it opened a dataset.
    //
    // **And it opens the DRIVER and not only a declaration.** `AdbcBigQuery::probe` initialises it,
    // so a release artefact whose linked-in driver does not start, and a mounted path naming a
    // missing or wrong-ABI `.so`, both stop the process here. It is also the only place in a normal
    // boot that runs the driver's Go runtime beside tokio and the release allocator - a coexistence
    // a link check cannot see - which is what makes `just bigquery-driver-check` a real verification
    // rather than a compile. What it still does not establish is that a question can be ANSWERED:
    // the probe opens no connection and looks for no credential.
    //
    // WHICH driver is `crate::bigquery_driver`'s decision, shared with `sources::bigquery` and with
    // `doctor` - `nix/shipped.nix` links one into every published artefact now, which is what
    // `docs/adr/0018`'s sixth amendment records and its fifth left open.
    // **The money bound, parsed before anything expensive is opened.** `sources.<alias>.max_bytes_billed`
    // is required at boot and its RANGE belongs to the transport that sends it, so a `0` - BigQuery's
    // own spelling of *no ceiling* - and a value above what the transport will send both stop the
    // process here rather than reaching the driver as no bound at all. For one review round this key
    // was required, unparsed and read by nothing.
    //
    // **Ahead of the driver deliberately**, and it is the one ordering here a cell depends on: a
    // number the deployment wrote is the operator's own typo, and resolving the driver starts a Go
    // runtime in this process, so a refusal about the ceiling must not be preceded by one about an
    // artefact. `a_bigquery_ceiling_the_adapter_will_not_send_is_a_startup_refusal_naming_the_key`
    // asserts the refusal names the key and the source and does NOT name the driver.
    let max_bytes_billed = sutura_exec_bigquery::adbc::BytesBilledCeiling::parse(max_bytes_billed)
        .map_err(|cause| format!("`sources.{source}.max_bytes_billed` is not a ceiling this transport will send: {cause}"))?;
    let driver = crate::bigquery_driver::resolve(&format!("`sources.{source}`"))?;
    sutura_exec_bigquery::adbc::AdbcBigQuery::probe(&driver)
        .map_err(|cause| format!("the BigQuery ADBC driver ({driver}) did not initialise for {source}: {cause}"))?;
    let billing = crate::bigquery_driver::job_billing(source, billing)?;
    // **Which of the two modes this source is, decided here and never per request.** They are XOR
    // rather than a ladder: a `shared-service-user` source runs on the deployment's own application
    // default credentials and impersonates nothing, and an `impersonation-at-source` source
    // federates the asking subject's own assertion against the pool it declares. `sutura_config`
    // refuses a `workload_identity` block on a shared entry and refuses its absence on an
    // impersonating one, so the presence of the block IS the posture and there is no third state.
    //
    // **`audience` is load-bearing again.** It was read by nothing for three rounds, while the
    // shipped mechanism was a principal switch on the deployment's own credential; the pool's
    // exchange needs it, and `externalaccount::Options::validate` refuses an empty one - so an
    // unusable declaration fails here, before a listener is bound, rather than on the first
    // impersonated question.
    //
    // **There is no `scope`.** The pinned driver has nowhere to put one - `WorkloadPool` carries the
    // measurement - so `sutura_config` refuses the key by name rather than carry a value that
    // reaches nothing. A value carried here and dropped later would be the shape this whole change
    // exists to remove.
    let impersonation = impersonation_of(source, configured)?;
    Ok(sutura_exec_bigquery::BigQueryWarehouse::over_adbc(
        source.clone(),
        identity.posture().clone(),
        billing,
        driver,
        impersonation,
        max_bytes_billed,
    ))
}

/// Which mode `configured` impersonates in, decided once at composition and never per request.
#[cfg(feature = "bigquery")]
pub(super) fn impersonation_of(
    source: &sutura_domain::model::SourceName,
    configured: &sutura_config::ConfiguredSource,
) -> Result<sutura_exec_bigquery::adbc::Impersonation, String> {
    Ok(match configured.caller_identity() {
        None => sutura_exec_bigquery::adbc::Impersonation::Disabled,
        Some(sutura_config::sources::caller_identity::CallerIdentity::WorkloadIdentity(workload)) => {
            sutura_exec_bigquery::adbc::Impersonation::ThroughPool(
                sutura_exec_bigquery::adbc::WorkloadPool::parse(workload.audience().as_str()).map_err(|cause| {
                    format!("`sources.{source}.workload_identity` names a pool this transport cannot exchange against: {cause}")
                })?,
            )
        }
        Some(sutura_config::sources::caller_identity::CallerIdentity::GoogleLogin(_)) => {
            sutura_exec_bigquery::adbc::Impersonation::GoogleLogin
        }
        Some(sutura_config::sources::caller_identity::CallerIdentity::DomainDelegation(delegation)) => {
            sutura_exec_bigquery::adbc::Impersonation::DomainDelegation(
                sutura_exec_bigquery::adbc::DelegatingAccount::parse(delegation.delegating_account().as_str())
                    .map_err(|cause| format!("`sources.{source}.domain_delegation.delegating_account` is not usable: {cause}"))?,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(feature = "bigquery")]
    fn a_google_login_source_is_opened_in_the_google_login_mode() {
        let overlay = "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n  inbound:\n    \
                       mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
                       authorization_server: \"https://idp.example.com\"\n    key_set_file: \"/etc/sutura/jwks.json\"\n    \
                       algorithms: [\"RS256\"]\nsources:\n  warehouse:\n    kind: \"bigquery\"\n    \
                       max_bytes_billed: 1073741824\n    \
                       posture: \"impersonation-at-source\"\n    google_login:\n      \
                       token_endpoint: \"https://idp.example.com/realms/r/broker/google/token\"\n      \
                       client_id: \"sutura\"\n      client_secret_file: \"/run/secrets/idp-client-secret\"\n";
        let settings = sutura_config::Settings::load(
            &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
        )
        .expect("a direct deployment admits a google_login source");
        let warehouse = sutura_domain::model::SourceName::parse("warehouse").expect("a test source is a source");
        let configured = settings.sources().get(&warehouse).expect("the source is declared");
        assert_eq!(
            super::impersonation_of(&warehouse, configured),
            Ok(sutura_exec_bigquery::adbc::Impersonation::GoogleLogin)
        );
    }

    /// A `domain_delegation` source served end to end: the real leg-1 gate reads the caller's
    /// address, the composition root's broker and `impersonation_of` decide the mode, and a
    /// recording transport behind the real `BigQueryWarehouse` sees what a job runs as.
    #[cfg(feature = "bigquery")]
    mod domain_delegation {
        use std::sync::Arc;
        use std::sync::mpsc::{Receiver, Sender, channel};

        use sutura_dev::issuer::{MockIssuer, PublishedKeySet, Token};
        use sutura_domain::model::SourceName;
        use sutura_domain::pinned::PinnedDefinitions;
        use sutura_exec_bigquery::adbc::{AdbcBigQuery, BytesBilledCeiling, DriverLocation};
        use sutura_exec_bigquery::transport::{
            DatasetAddress, DryRunEstimate, HeldTables, JobIdentity, JobRequest, JobTransport,
        };

        const ADDRESS: &str = "Analyst.One@Example.com";

        /// One billed-revenue metric over `ledger`, unanchored so the service asks nothing at boot.
        fn ledger_bundle() -> PinnedDefinitions {
            use sutura_domain::catalog::{Audience, Definitions, Description, Metric, Model};
            use sutura_domain::measure::{AggregatedColumn, Measure, Term};
            use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, QualifiedTable};
            use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion};

            let invoice = ColumnName::parse("invoice_key").expect("a column");
            let billed_on = ColumnName::parse("billed_on").expect("a column");
            let ledger = ModelName::parse("ledger").expect("a model");
            let model = Model::new(
                ledger.clone(),
                SourceName::parse("ledger").expect("a source"),
                QualifiedTable::parse("acme-analytics.ledger.fct_invoice").expect("a table"),
                std::collections::BTreeSet::from([invoice.clone(), billed_on.clone()]),
                Description::default(),
            );
            let measure = Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Count, invoice)));
            let metric = Metric::new(
                MetricName::parse("invoices").expect("a metric"),
                ledger,
                measure,
                Vec::new(),
                billed_on,
                std::collections::BTreeSet::from([Grain::Month]),
                Vec::new(),
                None,
                Description::default(),
                Audience::Open,
            )
            .expect("no dimensions");
            let definitions = Definitions::assemble(vec![model], vec![], vec![metric]).expect("a consistent bundle");
            let produced = sutura_domain::capabilities::MetadataCapabilities::produced(
                &definitions,
                &sutura_domain::knowledge::Knowledge::none(),
            );
            PinnedDefinitions::pin(
                DefinitionVersion::parse("ledger-1").expect("a version"),
                definitions,
                sutura_domain::knowledge::Knowledge::none(),
                ContributionManifest::single(SourceName::parse("local").expect("a source"), Contribution::of(produced)),
            )
            .expect("the bundle hashes")
        }

        struct Ledger(PinnedDefinitions);

        impl sutura_domain::pinned::SemanticCatalog for Ledger {
            type Error = core::convert::Infallible;

            const KIND: sutura_domain::pinned::CatalogKind = sutura_domain::pinned::CatalogKind::Golden;

            fn capabilities() -> sutura_domain::capabilities::MetadataCapabilities {
                sutura_domain::capabilities::MetadataCapabilities::everything()
            }

            fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
                Ok(self.0.clone())
            }
        }

        #[derive(Debug, thiserror::Error)]
        #[error("the recording transport answers no job")]
        struct Declined;

        /// Sends each job's address - `None` for any other identity - and answers none. Whether it
        /// delegates by address is the real transport's answer for the mode the source declared.
        struct Recording {
            seen: Sender<Option<String>>,
            adbc: AdbcBigQuery,
        }

        impl Recording {
            fn record(&self, request: &JobRequest<'_>) {
                let address = match request.identity() {
                    JobIdentity::AsAddress { address } => Some(String::from(address.as_str())),
                    JobIdentity::Transport | JobIdentity::AsSubject { .. } => None,
                };
                self.seen.send(address).expect("the cell holds the receiving end");
            }
        }

        impl JobTransport for Recording {
            type Error = Declined;

            fn run(&self, request: &JobRequest<'_>) -> Result<sutura_domain::warehouse::ResultBatches, Self::Error> {
                self.record(request);
                Err(Declined)
            }

            fn validate(&self, request: &JobRequest<'_>) -> Result<DryRunEstimate, Self::Error> {
                self.record(request);
                Ok(None)
            }

            fn list_tables(&self, _at: &DatasetAddress) -> Result<HeldTables, Self::Error> {
                Err(Declined)
            }

            fn apply(&self, _request: &JobRequest<'_>) -> Result<(), Self::Error> {
                Err(Declined)
            }

            fn delegates_by_address(&self) -> bool {
                self.adbc.delegates_by_address()
            }
        }

        /// The router over one `domain_delegation` source, its issuer, and what reached its transport.
        struct Served {
            app: axum::Router,
            issuer: MockIssuer,
            jobs: Receiver<Option<String>>,
            _published: PublishedKeySet,
        }

        fn served(case: &str) -> Served {
            served_with(case, "", "")
        }

        /// [`served`], with `inbound` appended to the `security.inbound` block and `delegation` to
        /// the `domain_delegation` block.
        fn served_with(case: &str, inbound: &str, delegation: &str) -> Served {
            let issuer =
                MockIssuer::generating("https://issuer.example.com", "https://sutura.example.com", "k1").expect("a key pair");
            let published = PublishedKeySet::of(&issuer, case).expect("the key set publishes");
            let overlay = format!(
                "security:\n  identity: \"multi-user\"\n  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
                 authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"ES256\"]\n{inbound}\
                 sources:\n  ledger:\n    kind: \"bigquery\"\n    \
                 max_bytes_billed: 1073741824\n    posture: \"impersonation-at-source\"\n    \
                 domain_delegation:\n      delegating_account: \"dwd@acme-analytics.iam.gserviceaccount.com\"\n{delegation}",
                issuer.audience(),
                issuer.issuer(),
                published.path().display(),
            );
            let settings = sutura_config::Settings::load(
                &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(&overlay),
            )
            .expect("a direct deployment serves a domain_delegation source");
            let source = SourceName::parse("ledger").expect("a source");
            let configured = settings.sources().get(&source).expect("the source is declared");
            let adbc = AdbcBigQuery::new(
                DriverLocation::parse("/nonexistent/libadbc_driver_bigquery.so").expect("an absolute path"),
                super::super::impersonation_of(&source, configured).expect("the declared mode"),
                BytesBilledCeiling::parse(1 << 30).expect("a ceiling"),
            );
            let (seen, jobs) = channel();
            let warehouse = sutura_exec_bigquery::BigQueryWarehouse::new(
                source,
                sutura_domain::source::SourcePosture::ImpersonationAtSource,
                sutura_exec_bigquery::JobBilling::CallerRun { caller_may_name: false },
                Recording { seen, adbc },
            );
            let broker = super::super::super::broker::build_broker(settings.sources(), None).expect("a broker");
            let service: Arc<dyn sutura_app::surface::Surface> = Arc::new(
                sutura_app::surface::LocalService::start(
                    &Ledger(ledger_bundle()),
                    sutura_app::Warehouses::of(warehouse),
                    sutura_runtime::TracingAuditSink::new(),
                    broker,
                    sutura_domain::plan::RefusingCombiner,
                    1 << 30,
                )
                .expect("the bundle validates"),
            );
            let gate = sutura_http::InboundGate::from_declaration(settings.security().inbound().expect("a direct deployment"))
                .expect("a gate");
            let admission = sutura_runtime::Admission::from_settings(settings.runtime());
            let state =
                sutura_http::ServiceState::new(service, Arc::new(settings), admission).with_inbound_identity(Arc::new(gate));
            let app = sutura_http::router(&state).expect("the router assembles");
            Served {
                app,
                issuer,
                jobs,
                _published: published,
            }
        }

        /// `/v1/query` for the one metric under `token`: the status and the body.
        async fn query(app: axum::Router, token: &str) -> (axum::http::StatusCode, String) {
            use tower::ServiceExt as _;

            let request = axum::http::Request::post("/v1/query")
                .header(axum::http::header::HOST, "localhost")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
                .body(axum::body::Body::from(
                    r#"{"metrics":["invoices"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#,
                ))
                .expect("a request");
            let response = app.oneshot(request).await.expect("infallible");
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("the body reads");
            (status, String::from_utf8_lossy(&body).into_owned())
        }

        fn caller(email_verified: Option<bool>) -> Token {
            let scopes = sutura_app::Capability::every()
                .map(sutura_app::Capability::scope)
                .collect::<Vec<_>>()
                .join(" ");
            let token = Token::for_subject("analyst-one")
                .granting(&scopes)
                .claiming("email", serde_json::json!(ADDRESS));
            match email_verified {
                Some(verified) => token.claiming("email_verified", serde_json::json!(verified)),
                None => token,
            }
        }

        #[tokio::test]
        async fn a_verified_email_runs_the_job_as_that_address_as_written() {
            let Served { app, issuer, jobs, .. } = served("domain-delegation-verified");
            let token = issuer.mint(&caller(Some(true))).expect("a signed token");
            let (status, body) = query(app, &token).await;
            let seen: Vec<Option<String>> = jobs.try_iter().collect();
            assert!(!seen.is_empty(), "the question must reach the source ({status}: {body})");
            assert!(
                seen.iter().all(|job| job.as_deref() == Some(ADDRESS)),
                "every job runs as the verified address, as written: {seen:?}"
            );
        }

        #[tokio::test]
        async fn an_email_not_marked_verified_is_refused_before_the_source() {
            let Served { app, issuer, jobs, .. } = served("domain-delegation-unverified");
            for verified in [None, Some(false)] {
                let token = issuer.mint(&caller(verified)).expect("a signed token");
                let (status, body) = query(app.clone(), &token).await;
                assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{verified:?}: {body}");
                assert!(body.contains("credential_unavailable"), "{verified:?}: {body}");
                assert_eq!(jobs.try_iter().count(), 0, "{verified:?}: no job reaches the source ({body})");
            }
        }

        /// An Entra ID access token: the user in `upn`, no `email_verified`.
        fn entra_caller() -> Token {
            let scopes = sutura_app::Capability::every()
                .map(sutura_app::Capability::scope)
                .collect::<Vec<_>>()
                .join(" ");
            Token::for_subject("AAAAAAAAAAAAAAAAAAAAAIkzqFVrSaSaFHy782bbtaQ")
                .granting(&scopes)
                .claiming("upn", serde_json::json!(ADDRESS))
        }

        #[tokio::test]
        async fn an_entra_shaped_token_runs_as_its_upn_only_where_the_claim_is_declared() {
            let declared = served_with(
                "domain-delegation-upn",
                "    address_claim: \"upn\"\n    accept_address_claim: true\n",
                "",
            );
            let token = declared.issuer.mint(&entra_caller()).expect("a signed token");
            let (status, body) = query(declared.app, &token).await;
            let seen: Vec<Option<String>> = declared.jobs.try_iter().collect();
            assert!(!seen.is_empty(), "the question must reach the source ({status}: {body})");
            assert!(seen.iter().all(|job| job.as_deref() == Some(ADDRESS)), "{seen:?}");
            let Served { app, issuer, jobs, .. } = served("domain-delegation-upn-undeclared");
            let token = issuer.mint(&entra_caller()).expect("a signed token");
            let (status, body) = query(app, &token).await;
            assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{body}");
            assert!(body.contains("credential_unavailable"), "{body}");
            assert_eq!(jobs.try_iter().count(), 0, "no job reaches the source ({body})");
        }

        #[tokio::test]
        async fn an_address_outside_the_delegated_domains_is_refused_before_the_source() {
            let outside = served_with("domain-delegation-outside", "", "      address_domains: [\"example.org\"]\n");
            let token = outside.issuer.mint(&caller(Some(true))).expect("a signed token");
            let (status, body) = query(outside.app, &token).await;
            assert_eq!(status, axum::http::StatusCode::FORBIDDEN, "{body}");
            assert!(body.contains("address_outside_domains"), "{body}");
            assert_eq!(outside.jobs.try_iter().count(), 0, "no job reaches the source ({body})");
            let inside = served_with("domain-delegation-inside", "", "      address_domains: [\"example.com\"]\n");
            let token = inside.issuer.mint(&caller(Some(true))).expect("a signed token");
            let (status, body) = query(inside.app, &token).await;
            let seen: Vec<Option<String>> = inside.jobs.try_iter().collect();
            assert!(!seen.is_empty(), "a listed domain reaches the source ({status}: {body})");
            assert!(seen.iter().all(|job| job.as_deref() == Some(ADDRESS)), "{seen:?}");
        }
    }
}
