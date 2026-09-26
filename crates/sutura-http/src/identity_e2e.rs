//! The credential half of the identity path, end to end through the assembled router.
//!
//! [`crate::inbound`] establishes **who is asking**. This module is about what happens next: the
//! subject leg 1 verified reaches a `sutura_domain::identity::CredentialBroker`, what the broker mints
//! reaches the adapter, and a subject the broker has nothing for is **refused rather than answered as
//! the process**. All of it through `crate::router` with `tower`'s `oneshot`, so what is exercised is
//! the composed transport rather than a call to `sutura_app::answer`.
//!
//! # The two things this closes, and they were both holes rather than gaps
//!
//! 1. **`403 credential_unavailable` had never been produced by a request.** It was asserted in
//!    `crate::wire::refusal`'s own unit test - a `RefusalReason` handed to the mapper - and by nothing
//!    that went through a router. The refusal exists because a subject with no grant at a data system
//!    must not be answered under the deployment's identity, which is the single most consequential
//!    thing in the product; a mapping test cannot say the request path reaches it.
//! 2. **Nothing showed two subjects driving two different credentials through the transport.** The
//!    property was asserted one layer below a request: what was missing is that the *verified
//!    caller's* own subject and its own assertion are what the broker is asked about, per request,
//!    and that what comes back is what the adapter is handed.
//!
//! **A third pair of cells lived here and is deleted** (`docs/adr/0018`, eighth amendment). They
//! joined leg 1's verified bytes to `sutura_exec_bigquery`'s exchanging broker over a fake
//! `StsExchange`; that broker had no reachable implementor and is gone with its tree, so the cells
//! proved a property of a fake. The same byte join on the path that SHIPS is held inside the
//! adapter, where the asker's assertion is served to the driver verbatim
//! (`crates/sutura-exec-bigquery/src/adbc/subject.rs`).
//!
//! # What none of it is
//!
//! **This is not leg 2 delivered, and nothing here may be cited as it.** Both tests mint from a fake
//! broker and every one of them is consumed by a fake adapter - so what is proved is that the
//! transport carries a per-subject credential from the broker to the port without mixing two callers
//! up. It says nothing about whether a real authorization server accepts anything, and nothing
//! whatever about two subjects reading two row sets - that needs a data system with row-level
//! security and two real grants. `AGENTS.md` keeps the shipped position: no source a deployment
//! SERVES executes as the asking subject.

use std::sync::Arc;

use axum::http::StatusCode;
use sutura_config::{Environment, Settings, Sources};
use sutura_dev::issuer::{MockIssuer, PublishedKeySet};
use sutura_domain::identity::{CredentialBroker, Expiry, LegCredentials, Minted, Presented, RequestContext, Secret, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, RowSet, Value, Warehouse};

use crate::inbound::gate::InboundGate;
use crate::surface::LocalService;
use crate::testing::{
    Answered, accepted_by, an_issuer, asked, bundle, call, catalog_of, declared_inbound, direct_overlay, every_scope, request,
    serving, sink, source,
};

// ------------------------------------------------------------------- the fakes ----

/// A broker that has nothing for whoever is asking.
///
/// **The honest instrument for `credential_unavailable`, and it is a real implementor of the port
/// rather than a flag on the fixture broker.** `sutura_config::StaticCredentialBroker` refuses exactly
/// this way - it holds an entry only for a source declared `shared-service-user`, so a source declared
/// `impersonation-at-source` gets nothing - and what that shipped broker cannot do is be reached
/// through a router on this build, because the composition root will not boot such a deployment. So the
/// shape of the refusal is the shipped one and the deployment producing it is this one.
struct NothingForThisSubject {
    at: SourceName,
}

/// The fixture broker's own defect, which nothing here can provoke.
///
/// It exists because the port's error and its refusal are different things: `Minted::Refused` is the
/// governance answer this module asserts on, and a broker that could not be *reached* is a `503`. A
/// fake with one error type for both would let the `403` pass for the wrong reason.
#[derive(Debug, thiserror::Error)]
#[error("the fixture broker in `identity_e2e` cannot fail, and did")]
struct NoFixtureFailure;

impl CredentialBroker for NothingForThisSubject {
    type Error = NoFixtureFailure;

    fn mint(&self, _context: &RequestContext, _sources: &SourceSet) -> Result<Minted, Self::Error> {
        Ok(Minted::Refused { source: self.at.clone() })
    }
}

/// A broker that mints the asking subject's **own** credential for the one source.
///
/// It derives the credential from two things the request carries and nothing else: the subject leg 1
/// verified, and that caller's own assertion. Both, deliberately - a broker keyed on the subject NAME
/// alone would pass a test that two subjects get two credentials while proving nothing about the
/// material travelling, and an exchanging broker's whole job is to turn the caller's own document into
/// something a data system accepts.
struct ExchangesForTheAsker;

impl CredentialBroker for ExchangesForTheAsker {
    type Error = NoFixtureFailure;

    #[expect(
        clippy::disallowed_methods,
        reason = "a fake broker that EXCHANGES has to read what it was handed, which is how a test \
                  shows the verified caller's own assertion is what reached it"
    )]
    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let subject = context
            .chain()
            .subject()
            .id()
            .map_or_else(|| String::from("<the deployment itself>"), |id| String::from(id.as_str()));
        // The assertion is `Option`, and its absence is recorded rather than defaulted away: a broker
        // that quietly exchanged nothing is exactly the shape this test is meant to be able to see.
        let assertion = context
            .assertion()
            .map_or_else(|| String::from("<no assertion>"), |held| exchanged_from(held.expose_secret()));
        let mut presented = std::collections::BTreeMap::new();
        for name in sources.iter() {
            drop(presented.insert(
                name.clone(),
                Presented::SubjectToken {
                    material: Secret::new(format!("for:{subject}/from:{assertion}")),
                    // This fake exchanges rather than federating, so there is no second hop for it
                    // to name - see `Presented::SubjectToken`'s own doc for why an absence is an
                    // `Option` and not a fourth posture.
                    impersonate: None,
                },
            ));
        }
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|_unreachable| NoFixtureFailure)
    }
}

/// What this fake "exchange" does to a caller's assertion: names its last eight characters.
///
/// A digest of the input rather than the input, because the assertion is a signed token and a test
/// asserting on a whole one would be asserting on `jsonwebtoken`'s output. Eight characters of a
/// signature is enough to tell two callers' tokens apart and is not a credential anybody could use.
fn exchanged_from(assertion: &str) -> String {
    let tail = assertion.len().saturating_sub(8);
    assertion.get(tail..).unwrap_or_default().to_owned()
}

/// A data system that can carry a per-subject credential, and reports which one it was handed.
///
/// **`PerSubjectCredential` and `ImpersonationAtSource`, which is what makes it the right fake here.**
/// Every other adapter in `crate::testing` declares `NoPlaceForASubject` over a shared posture, which
/// is honest for a fake over no data system - and it means none of them can be handed a subject's
/// credential at all. This one can, so it is the only fixture in this crate that can observe what a
/// broker minted per request.
struct RecordsWhatItWasHanded {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
    handed: tokio::sync::mpsc::UnboundedSender<String>,
}

/// The one way this fake fails: it was handed a credential that disagrees with its own posture.
///
/// Its own variant rather than a panic, because that is the check a real adapter makes at exactly this
/// point - `Presented::agrees_with` - and a fake that skipped it would accept a shared leg and report
/// the answer as impersonated.
#[derive(Debug, thiserror::Error)]
#[error("this fake was handed a credential that does not agree with its posture")]
struct Disagreed {
    #[source]
    cause: sutura_domain::identity::PresentedDisagreesWithPosture,
}

impl Warehouse for RecordsWhatItWasHanded {
    type Error = Disagreed;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::PerSubjectCredential;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the recording fake exists to report WHICH credential reached the adapter, which is \
                  the property two subjects driving two credentials is measured on"
    )]
    fn execute(
        &self,
        _executable: Executable<'_>,
        presented: &Presented,
        _deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| Disagreed { cause })?;
        // Recorded through a channel and not a lock, because `clippy.toml` bans `std::sync::Mutex` and
        // `tokio::sync::Mutex` cannot be taken from a synchronous port method. An unbounded sender is
        // `Send + Sync`, its `send` is not async, and what a test needs is the sequence rather than
        // shared mutable state.
        let material = match *presented {
            Presented::SubjectToken { ref material, .. } => String::from(material.expose_secret()),
            Presented::SubjectPrincipal { ref name } => format!("principal:{name}"),
            Presented::SharedServiceUser { .. } => String::from("the deployment's own identity"),
        };
        drop(self.handed.send(material));
        Ok(crate::testing::canned(&self.result))
    }
}

/// The registry holding the impersonating fake, paired with the receiving end of what it was handed.
///
/// A named pair rather than a tuple in a signature, because `clippy::type_complexity` is right about
/// what two generic types in a return position read like.
struct Recording {
    warehouses: sutura_app::Warehouses<RecordsWhatItWasHanded>,
    handed: tokio::sync::mpsc::UnboundedReceiver<String>,
}

/// The impersonating fake, and the receiving end of what it was handed.
fn recording_warehouse() -> Recording {
    let (handed, received) = tokio::sync::mpsc::unbounded_channel();
    // The number the bundle's anchor certifies, so `LocalService::start` re-executing it validates.
    let result = RowSet::new(vec![String::from("revenue")], vec![vec![Value::Integer(197_122)]])
        .expect("a one-cell result is a result set");
    Recording {
        warehouses: sutura_app::Warehouses::of(RecordsWhatItWasHanded {
            source: source(),
            posture: SourcePosture::ImpersonationAtSource,
            result,
            handed,
        }),
        handed: received,
    }
}

// ------------------------------------------------------------ the two routers ----

/// A router with no inbound identity, over a broker that refuses.
///
/// No inbound identity, so the subject is the deployment itself - which is the right shape for this
/// assertion: the refusal is about a credential at a data system and not about who was asking, and a
/// deployment answering `403 credential_unavailable` to a caller it never identified is exactly the
/// case an operator has to be able to tell apart from a `401`.
fn app_over_a_refusing_broker() -> axum::Router {
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the default settings load");
    let recording = recording_warehouse();
    let service = LocalService::start(
        &catalog_of(bundle()),
        recording.warehouses,
        sink(),
        NothingForThisSubject { at: source() },
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates: the anchor path takes no credential");
    crate::router(&crate::testing::state_over(Arc::new(service), settings)).expect("the test router assembles")
}

/// A router that verifies its callers and exchanges for each of them.
fn app_that_exchanges(
    issuer: &MockIssuer,
    published: &PublishedKeySet,
) -> (axum::Router, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let overlay = direct_overlay(issuer, &published.path().to_string_lossy());
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
        .expect("the exchanging overlay loads");
    let recording = recording_warehouse();
    let service = LocalService::start(
        &catalog_of(bundle()),
        recording.warehouses,
        sink(),
        ExchangesForTheAsker,
        sutura_domain::plan::RefusingCombiner,
        1 << 30,
    )
    .expect("the test bundle validates");
    let declaration = settings
        .security()
        .inbound()
        .expect("this overlay declares an inbound identity")
        .clone();
    let gate = InboundGate::from_declaration(&declaration).expect("a published key set builds a gate");
    let state = crate::testing::state_over(Arc::new(service), settings).with_inbound_identity(Arc::new(gate));
    (crate::router(&state).expect("the test router assembles"), recording.handed)
}

/// One question through the real router, carrying `token` if there is one.
async fn ask(app: &axum::Router, token: Option<&str>) -> Answered {
    asked(app, "POST", "/v1/query", token).await
}

/// A token this deployment accepts, granting every capability the surface has.
fn accepted(issuer: &MockIssuer, subject: &str) -> String {
    issuer.mint(&accepted_by(subject)).expect("the issuer signs a token")
}

// ----------------------------------------------------------------- the tests ----

#[tokio::test]
async fn credential_unavailable_is_reachable_end_to_end() {
    // `AGENTS.md` records this refusal as unreachable end to end on the SHIPPED binary, and the reason
    // is composition rather than code: the only configuration the shipped broker refuses is a source
    // declared `impersonation-at-source`, which the composition root will not boot. That limit is
    // unchanged by this test - what changes is that the REQUEST PATH is now shown to reach the refusal
    // rather than only the mapper in `crate::wire::refusal` being shown to name it.
    let app = app_over_a_refusing_broker();
    let refused = ask(&app, None).await;
    let body = refused.body;

    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{body}");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("a refusal is JSON");
    assert_eq!(parsed["outcome"], "refusal", "{body}");
    assert_eq!(parsed["reason"]["code"], "credential_unavailable", "{body}");
    let detail = parsed["reason"]["detail"].as_str().expect("a refusal carries a detail");

    // **A `403` and not a `503`**, and the sentence has to match: what is missing is a grant at the data
    // system, so a caller must not be sent back to authenticate against this service. `docs/adr/0005`
    // is amended by exactly this, and a broker that could not be REACHED is the other outcome - a `503`
    // with its own code, which this fake cannot produce.
    assert!(detail.contains(source().as_str()), "the refusal names the source: {detail}");
    assert!(
        !detail.to_lowercase().contains("try again") && !detail.to_lowercase().contains("retry"),
        "a governance refusal must not read as a hiccup: {detail}"
    );
}

#[tokio::test]
async fn two_subjects_drive_two_different_exchanged_credentials() {
    // Through the TRANSPORT rather than at the broker, which is the whole point: what is asserted is
    // that the subject leg 1 verified for THIS request, and that caller's own assertion, are what the
    // broker was asked about - and that what it minted is what the adapter was handed. A broker keyed
    // on anything process-wide passes at the broker and fails here.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "exchange").expect("the key set publishes");
    let (app, mut handed) = app_that_exchanges(&issuer, &published);

    let first = accepted(&issuer, "ada@example.com");
    let second = accepted(&issuer, "grace@example.com");
    assert_ne!(first, second, "two subjects' tokens differ");

    for token in [&first, &second] {
        let answered = ask(&app, Some(token)).await;
        assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    }

    let ada = handed
        .try_recv()
        .expect("the adapter was handed a credential for the first caller");
    let grace = handed
        .try_recv()
        .expect("the adapter was handed a credential for the second caller");

    // The subject half: each leg named the person who asked for it (as the masked form the type
    // holds - the raw is consumed at parse), and neither named the other.
    assert!(ada.contains("for:a***@e***.c***"), "{ada}");
    assert!(grace.contains("for:g***@e***.c***"), "{grace}");
    // A SUBSTRING over a haystack whose tail is random, and it is safe by ARITHMETIC rather than by
    // alphabet: `exchanged_from` appends eight base64url characters of a signature, `grace` is five
    // characters of that same 64-character alphabet, so there are four offsets to land in and the
    // odds are about 4 x 64^-5. That is small enough to leave alone and NOT a property of the
    // needle - it is a property of the two lengths. Shorten the needle or lengthen that window and
    // this becomes the assertion `sutura-http`'s key-set cell already had to stop being.
    assert!(!ada.contains("grace"), "the first caller's leg named the second: {ada}");

    // **The material half, and it is the one that would be missed.** The credential is derived from
    // that caller's OWN assertion, so two different credentials proves the assertion travelled rather
    // than only the name - a broker handed a name and no document would produce two distinct strings
    // here as well.
    assert_ne!(ada, grace, "two subjects were handed one credential");
    assert!(
        !ada.contains("<no assertion>"),
        "the caller's own assertion did not reach the broker: {ada}"
    );
    assert!(!grace.contains("<no assertion>"), "{grace}");
}

#[tokio::test]
async fn an_answer_records_the_posture_the_adapter_declared_and_not_the_one_a_file_says() {
    // The recording half, at the transport. `Provenance` reads the posture off the ADAPTER the plan
    // selected, which is what stops a leg being reported as impersonated on the strength of a settings
    // file - so an answer from the fake above has to say `impersonation-at-source` because that is what
    // the fake declares, and no overlay anywhere in this file says so.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "provenance").expect("the key set publishes");
    let (app, _handed) = app_that_exchanges(&issuer, &published);

    let answered = ask(&app, Some(&accepted(&issuer, "ada@example.com"))).await;
    assert_eq!(answered.status, StatusCode::OK, "{}", answered.body);
    assert!(
        answered.body.contains(SourcePosture::ImpersonationAtSource.as_str()),
        "an answer says which identity produced each of its legs: {}",
        answered.body
    );
}

// -------------------------------------------------------- metric visibility ----

/// A metric only a caller granted `finance` may see - `docs/adr/0028`.
fn finance_metric_name() -> sutura_domain::model::MetricName {
    sutura_domain::model::MetricName::parse("finance_only").expect("a test metric is a metric")
}

/// [`crate::testing::bundle`], plus a second metric restricted to the `finance` audience.
///
/// One bundle, two verified callers, two different catalogs: `revenue` stays open so every caller
/// can still ask the ordinary question, and `finance_only` is what tells the two callers apart.
/// Inlined here rather than in `crate::testing` - only this module's tests need it, and a fixture
/// with no test of its own is a file `just causality` cannot hold at HEAD against a reverted base.
fn bundle_with_a_restricted_metric() -> sutura_domain::pinned::PinnedDefinitions {
    use sutura_domain::catalog::{Audience, AudienceGrant, Definitions, Metric, Model};
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, AudienceId, Grain, ModelName, TableName};
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

    let column = |raw: &str| sutura_domain::model::ColumnName::parse(raw).expect("a test column is a column");
    let description = |raw: &str| sutura_domain::catalog::Description::parse(raw).expect("a test description");
    let model = Model::new(
        ModelName::parse("orders").expect("a test model is a model"),
        source(),
        TableName::parse("orders").expect("a test table is a table"),
        std::collections::BTreeSet::from([column("amount_cents"), column("order_date"), column("region")]),
        description("Orders, one row per order."),
    );
    let revenue = Metric::new(
        crate::testing::metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        std::collections::BTreeSet::from([Grain::Day, Grain::Month]),
        Vec::new(),
        Some(sutura_domain::catalog::Anchor::new(
            sutura_domain::calendar::TimeRange::new(
                sutura_domain::calendar::Date::parse("2026-06-01").expect("a test date"),
                sutura_domain::calendar::Date::parse("2026-07-01").expect("a test date"),
            )
            .expect("a test range"),
            sutura_domain::catalog::AnchorValue::parse(crate::testing::ANCHORED_VALUE).expect("a test anchor value"),
        )),
        description("Revenue, in minor units."),
        Audience::Open,
    )
    .expect("no dimensions to duplicate");
    let restricted_to_finance = Audience::Restricted(
        AudienceGrant::parse(std::collections::BTreeSet::from([
            AudienceId::parse("finance").expect("a test audience id is one")
        ]))
        .expect("one id grants"),
    );
    let finance_only = Metric::new(
        finance_metric_name(),
        ModelName::parse("orders").expect("a test model is a model"),
        Measure::Simple(Term::Aggregate(AggregatedColumn::new(Aggregate::Sum, column("amount_cents")))),
        Vec::new(),
        column("order_date"),
        std::collections::BTreeSet::from([Grain::Month]),
        Vec::new(),
        None,
        description("Only a finance-granted caller may see this."),
        restricted_to_finance,
    )
    .expect("no dimensions to duplicate");
    let definitions =
        Definitions::assemble(vec![model], vec![], vec![revenue, finance_only]).expect("the test bundle is consistent");
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        definitions.clone(),
        Knowledge::none(),
        ContributionManifest::single(
            source(),
            Contribution::of(sutura_domain::capabilities::MetadataCapabilities::produced(
                &definitions,
                &Knowledge::none(),
            )),
        ),
    )
    .expect("the test definitions hash")
}

/// [`crate::testing::direct_overlay`], with `docs/adr/0028`'s deployment mapping added under the
/// same `security:` block - a `groups` claim value of `finance-team` grants the `finance` audience.
fn direct_overlay_granting_finance(issuer: &MockIssuer, key_set_path: &str) -> String {
    format!(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
         authorization_server: \"{}\"\n    key_set_file: \"{key_set_path}\"\n    algorithms: [\"ES256\"]\n  \
         audience_mapping:\n    finance-team: [\"finance\"]\n",
        issuer.audience(),
        issuer.issuer(),
    )
}

/// A router serving [`bundle_with_a_restricted_metric`], behind a real leg-1 gate that maps a
/// `groups` claim of `finance-team` onto `docs/adr/0028`'s `finance` audience.
fn app_serving_two_metrics(issuer: &MockIssuer, published: &PublishedKeySet) -> axum::Router {
    let overlay = direct_overlay_granting_finance(issuer, &published.path().to_string_lossy());
    let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(&overlay))
        .expect("the audience-mapping overlay loads");
    let declaration = declared_inbound(&settings);
    let gate = InboundGate::from_declaration(&declaration).expect("a published key set builds a gate");
    let recording = recording_warehouse();
    serving(
        bundle_with_a_restricted_metric(),
        recording.warehouses,
        NothingForThisSubject { at: source() },
        settings,
        Some(gate),
    )
}

/// `GET /v1/catalog`, carrying `token`. Returns the status and the whole body.
async fn catalog(app: &axum::Router, token: &str) -> (StatusCode, String) {
    call(app, request("GET", "/v1/catalog", Some(token), axum::body::Body::empty())).await
}

#[tokio::test]
async fn two_verified_callers_get_two_different_catalogs_from_one_bundle() {
    // `docs/adr/0028`: the catalog handler now reads `Asked::context`'s mapped audiences, so two
    // callers this ONE issuer verified against this ONE bundle see two different catalogs -
    // through the assembled router, not through the domain type directly.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "audience-mapping").expect("the key set publishes");
    let app = app_serving_two_metrics(&issuer, &published);

    let outsider = issuer
        .mint(&accepted_by("outsider@example.com"))
        .expect("the issuer signs a token");
    let (_, seen_by_outsider) = catalog(&app, &outsider).await;
    assert!(seen_by_outsider.contains("revenue"), "{seen_by_outsider}");
    assert!(
        !seen_by_outsider.contains(finance_metric_name().as_str()),
        "an unmapped caller must not see the restricted metric: {seen_by_outsider}"
    );

    let finance_token = issuer
        .mint(
            &sutura_dev::issuer::Token::for_subject("finance-caller@example.com")
                .granting(&every_scope())
                .claiming("groups", serde_json::json!(["finance-team"])),
        )
        .expect("the issuer signs a token");
    let (_, seen_by_finance) = catalog(&app, &finance_token).await;
    assert!(seen_by_finance.contains("revenue"), "{seen_by_finance}");
    assert!(
        seen_by_finance.contains(finance_metric_name().as_str()),
        "a caller mapped to `finance` must see the restricted metric: {seen_by_finance}"
    );
}

#[tokio::test]
async fn an_invisible_metric_is_refused_byte_identically_to_an_unknown_one() {
    // `docs/adr/0028`'s "invisible means absent at both doors": resolving a metric this caller was
    // not granted has to reach the SAME refusal, with the SAME detail, as a metric this catalog
    // never declared at all - otherwise a caller could tell the two apart, which is the
    // metadata-enumeration oracle the record declines to open.
    let issuer = an_issuer();
    let published = PublishedKeySet::of(&issuer, "byte-identical").expect("the key set publishes");
    let app = app_serving_two_metrics(&issuer, &published);
    let outsider = issuer
        .mint(&accepted_by("outsider@example.com"))
        .expect("the issuer signs a token");

    let (invisible_status, invisible_body) = ask_about(&app, &outsider, finance_metric_name().as_str()).await;
    let (unknown_status, unknown_body) = ask_about(&app, &outsider, "no_such_metric_at_all").await;

    assert_eq!(invisible_status, unknown_status);
    let invisible_json: serde_json::Value = serde_json::from_str(&invisible_body).expect("a refusal is JSON");
    let unknown_json: serde_json::Value = serde_json::from_str(&unknown_body).expect("a refusal is JSON");
    assert_eq!(invisible_json["reason"]["code"], "metric_unknown", "{invisible_body}");
    // Only the echoed metric name may differ between the two bodies - everything else, including
    // the sentence's shape, is identical.
    let invisible_detail = invisible_json["reason"]["detail"]
        .as_str()
        .expect("a refusal carries a detail")
        .replace(finance_metric_name().as_str(), "no_such_metric_at_all");
    assert_eq!(
        invisible_detail,
        unknown_json["reason"]["detail"].as_str().expect("a refusal carries a detail"),
        "an invisible metric and an unknown one must read as the same refusal"
    );
}

/// One question about `metric`, through the real router.
async fn ask_about(app: &axum::Router, token: &str, metric: &str) -> (StatusCode, String) {
    let body = format!(r#"{{"metrics":["{metric}"],"grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}}}}"#);
    call(app, request("POST", "/v1/query", Some(token), axum::body::Body::from(body))).await
}
