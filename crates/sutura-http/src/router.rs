//! Assembling the router: three tiers, and what guards each.
//!
//! # The tiers
//!
//! | Tier | Reachable by | Rate limit | Token |
//! | --- | --- | --- | --- |
//! | liveness | anybody who can route a packet | public | no |
//! | documentation | anybody, when it is served at all | public | yes, when one is configured |
//! | `v1` | a caller with the token, when one is configured | general | yes, when one is configured |
//!
//! Liveness has no token because a probe has no credential to present, which is exactly why its
//! body carries nothing.
//!
//! # Layer order, and why it reads backwards
//!
//! `Router::layer` wraps what is already there, so **the last layer added is the outermost and runs
//! first**. Reading the assembly below from the bottom up gives the order a request travels in.
//! Getting this wrong is not a style problem: a token gate applied outside the tracing layer
//! produces a `401` with no span, and a body limit applied inside the JSON extractor is a limit that
//! never fires.
//!
//! # The limiter is OUTSIDE the token gate, and that was a bug fix
//!
//! It used to be inside it. `route_layer` for the gate was added last, so the gate was the
//! outermost layer of the versioned subtree and answered `401` **without calling `next.run`** - so a
//! wrong-token attempt never reached the limiter and never cost a cell. An unlimited burst of
//! authentication attempts against a 32-character shared secret is the one thing a rate limiter in
//! front of a bearer token is for.
//!
//! The order below is therefore: limiter, then gate, then the handler. Both subtrees that have a
//! gate - the versioned API and the documentation - are assembled the same way, because the
//! documentation router had the same inversion.
//!
//! **This is why the sweeper is started here and in the same change.** With the gate outermost, an
//! unauthenticated request was refused before it could create a bucket, so the only unauthenticated
//! path into a limiter was liveness. With the limiter outermost, every path an unauthenticated
//! caller can reach creates one - so fixing the order alone converts a narrow leak in
//! `governor`'s never-reaped keyed store into a surface-wide one. See
//! [`middleware::spawn_reaper`].
//!
//! `route_layer` rather than `layer` for the token gate, and that is load-bearing: `route_layer`
//! runs only for a request that matched a route in that subtree, so it applies to every real path
//! under the version prefix without also applying to the liveness probe merged in beside it. A
//! `layer` there would gate the probe as well, and a probe has no credential to present.
//!
//! **The consequence, stated rather than discovered later:** a path under the version prefix that
//! matches no route skips the gate and falls through to the top-level `404`. So an unauthenticated
//! caller can learn which paths exist, though not what is behind them - and the paths are in the
//! published interface description anyway. Every path that resolves to a handler does hold a
//! credential. There is a test on each half of that.
//!
//! # Why this returns a `Result`
//!
//! Because a limiter that will not build must stop the process rather than quietly become no
//! limiter, and so must a sweeper that will not start: a keyed store nothing sweeps grows for the
//! life of the process. Neither can happen from a loaded configuration - `Quota` refuses the values
//! that would cause the first - and the alternative to an error is either a panic for something the
//! types already ruled out or a fail-open fallback. See `middleware::LimiterNotBuilt`.

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use sutura_config::{Environment, Settings};
use utoipa_axum::router::OpenApiRouter;

use crate::client_address::ClientAddress;
use crate::constants::{API_V1_PREFIX, OPENAPI_JSON_PATH, SWAGGER_UI_PATH};
use crate::middleware::{self, LimiterHandle, LimiterNotBuilt};
use crate::routes;
use crate::state::ServiceState;

/// What the span calls the route of a request that matched none.
///
/// A constant and not the request's own path, which is the whole point: a path that matched nothing
/// is a caller-supplied string, and putting it on a span turns the log's own cardinality into
/// something a caller chooses. One bucket instead. See [`request_span`].
pub(crate) const UNMATCHED_ROUTE: &str = "unmatched";

/// The documentation subtree, and the limiter tier it installed if it installed one.
///
/// A named alias because the tuple is over the complexity threshold in `clippy.toml`, and naming it
/// says why the second half is optional: the subtree is empty when the description is not served,
/// and an empty subtree installs no tier.
type DocumentationRouter = Result<(Router, Option<LimiterHandle>), RouterNotBuilt>;

/// Why the router could not be assembled.
#[derive(Debug, thiserror::Error)]
pub enum RouterNotBuilt {
    #[error("a rate limit tier could not be built")]
    Limiter {
        #[source]
        cause: LimiterNotBuilt,
    },
    /// The housekeeping thread for the limiter's keyed state would not start.
    ///
    /// A refusal and not a warning, for the reason every refusal in this codebase is one: the
    /// alternative is a process that runs with a keyed store nothing ever sweeps, which is a slow
    /// leak that no request will ever reveal.
    #[error("the rate limit sweeper thread could not be started")]
    Reaper {
        #[source]
        cause: std::io::Error,
    },
    /// The settings declare an inbound identity and the state carries no gate to establish it.
    ///
    /// **The mechanism that makes attaching leg 1 unforgettable.** `crate::inbound::InboundGate` is
    /// built by the composition root, because building it reads a key set - so there is a state in
    /// which a deployment has declared `security.inbound` and nothing is verifying anything. Without
    /// this refusal that deployment would serve, answer every question as
    /// `sutura_domain::identity::Subject::TheDeploymentItself`, and log a startup line saying it
    /// establishes a caller identity. It fails to assemble instead.
    #[error(
        "security.inbound.mode is `{mode}` and no inbound identity gate was attached to the service \
         state, so nothing would verify a caller's token. The composition root builds one with \
         `InboundGate::from_declaration` and attaches it with `ServiceState::with_inbound_identity`"
    )]
    InboundIdentityNotAttached { mode: &'static str },
    /// A route under the version prefix that `crate::capability::governed` names no capability for.
    ///
    /// **The mechanism that makes the capability gate unforgettable**, and it is deliberately a
    /// refusal to assemble rather than a refusal at request time. The gate is a layer, so a handler
    /// cannot forget to call it; what is left to forget is a row in the table, and a route with no row
    /// would either be refused to every caller or - if the layer fell open - be reachable by every
    /// caller. Neither is something to discover from a request.
    ///
    /// It reads the generated interface description, which is generated from the handlers' own
    /// `#[utoipa::path]` attributes - so it is checked against the routes the router actually mounts
    /// and not against a second list somebody kept in step.
    #[error(
        "`{method} {route}` is mounted under the version prefix and `sutura_http::capability::governed` \
         names no capability for it, so nothing would decide whether a caller may invoke it. Add a row \
         naming its `sutura_app::Capability`"
    )]
    RouteNotGoverned { method: String, route: String },
}

/// The router, and the limiter state something has to keep sweeping.
///
/// **Two values because they have two owners.** The router goes to whatever serves it; the handles
/// go to the sweeper. [`router`] wires the second half up itself, which is what makes the
/// production path correct by default; [`assemble`] hands both back for a test that wants to
/// observe the keyed store directly.
pub struct Assembled {
    router: Router,
    limiters: Vec<LimiterHandle>,
}

impl Assembled {
    /// The router, for whatever will serve it.
    pub fn into_router(self) -> Router {
        self.router
    }

    /// The tiers that were built, for a sweeper or for an assertion.
    #[must_use]
    pub fn limiters(&self) -> &[LimiterHandle] {
        &self.limiters
    }
}

impl core::fmt::Debug for Assembled {
    /// Hand-written because `axum::Router` is not `Debug` in a useful way and the tiers are what a
    /// reader wants named.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Assembled")
            .field(
                "limiters",
                &self.limiters.iter().map(LimiterHandle::tier).collect::<Vec<&str>>(),
            )
            .finish_non_exhaustive()
    }
}

/// Builds the whole router for this state, and starts the sweeper for its keyed state.
///
/// Everything the posture decides is decided here, once, from settings that were already
/// refused-or-accepted at startup. A handler cannot re-decide any of it, which is the point: a
/// request never arrives at a branch that could turn a control off.
pub fn router(state: &ServiceState) -> Result<Router, RouterNotBuilt> {
    let assembled = assemble(state)?;
    middleware::spawn_reaper(assembled.limiters(), middleware::REAP_INTERVAL)
        .map_err(|cause| RouterNotBuilt::Reaper { cause })?;
    Ok(assembled.into_router())
}

/// The same assembly, with the limiter handles handed back instead of swept.
///
/// For a caller that wants to sweep on its own schedule, and for a test that wants to assert on the
/// keyed store. It starts no thread, so a test suite that assembles one router per test does not
/// accumulate one sweeper per test.
pub fn assemble(state: &ServiceState) -> Result<Assembled, RouterNotBuilt> {
    let settings = state.settings().clone();
    let limits = settings.rate_limit();
    let key = ClientAddress::from_settings(limits);
    announce_rate_limiting(settings.environment(), limits.enabled());
    announce_keying(limits);
    let mut limiters = Vec::new();

    // The versioned API. Nested before the layers are applied, so the token gate and the limiter
    // see the full path.
    let versioned: Router = Router::from(
        OpenApiRouter::new()
            .nest(API_V1_PREFIX, routes::v1::openapi_router())
            .with_state(state.clone()),
    )
    // Innermost of this subtree: the body bound. Inside the JSON extractor, which is what makes it
    // a limit on what is read rather than on what parses.
    .layer(DefaultBodyLimit::max(settings.server().max_body().bytes()));
    // Then the capability gate: which of this surface's operations this caller may invoke. INSIDE leg
    // 1, because `Router::layer` wraps what is already there - so by the time this runs the verified
    // caller is in the extensions, which is the whole reason it is here and not one line later. It is
    // a layer rather than a check per handler so that there is nothing for a handler to forget; what
    // is left to forget is a row in `crate::capability::governed`, and `governed_routes` below refuses
    // to assemble over one that is missing.
    governed_routes()?;
    let versioned = versioned.route_layer(axum::middleware::from_fn(crate::capability::require_capability));
    // Then leg 1, if this deployment has it: a verified caller, or a `401` with a challenge. INSIDE
    // the deployment token gate added below, because `Router::layer` wraps what is already there - so
    // the cheap comparison runs first and a signature verification is not work an unauthenticated
    // caller can spend. See `crate::inbound::gate`.
    let versioned = inbound_layered(versioned, state, settings.security().inbound())?;
    // Then the token gate, and only THEN the limiter - so the limiter is outside the gate and a
    // wrong-token attempt costs a cell. See the module documentation.
    let versioned = versioned.route_layer(axum::middleware::from_fn_with_state(state.clone(), middleware::require_token));
    let versioned = if limits.enabled() {
        let (layer, handle) = middleware::api_rate_limit_layer(limits.api(), key.clone()).map_err(limiter)?;
        limiters.push(handle);
        versioned.layer(layer)
    } else {
        versioned.layer(middleware::disabled_rate_limit_layer())
    };

    // Liveness. No token, and the tighter tier: nothing here is worth polling faster than that.
    let liveness: Router = Router::from(
        OpenApiRouter::new()
            .routes(utoipa_axum::routes!(routes::health::liveness))
            .with_state(state.clone()),
    );
    let liveness = if limits.enabled() {
        let (layer, handle) = middleware::probe_rate_limit_layer(limits.probe(), key.clone()).map_err(limiter)?;
        limiters.push(handle);
        liveness.layer(layer)
    } else {
        liveness.layer(middleware::disabled_rate_limit_layer())
    };

    let (documentation, documentation_limiter) = documentation(state, &settings, &key)?;
    limiters.extend(documentation_limiter);

    let router = Router::new()
        .merge(liveness)
        .merge(documentation)
        .merge(versioned)
        // The request bound, as a middleware of ours rather than `tower_http`'s: that one answers
        // the status with an EMPTY body, and every `408` this surface documents carries a
        // `ProblemBody`. See `middleware::enforce_timeout`.
        .layer(axum::middleware::from_fn_with_state(
            settings.server().request_timeout().duration(),
            middleware::enforce_timeout,
        ))
        // Outermost, so a request refused by any layer below still produces a span and a timing.
        // `TraceLayer` on the outside is the difference between a `401` you can find in a log and a
        // `401` that happened to somebody.
        //
        // CONFIGURED rather than defaulted, and that is a bug fix - see [`request_span`]. The
        // response line is raised with it: without a status at `info` the span says a request
        // happened and not how it ended, which is most of what "a `401` you can find" means.
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(request_span)
                .on_response(
                    tower_http::trace::DefaultOnResponse::new()
                        .level(tracing::Level::INFO)
                        .latency_unit(tower_http::LatencyUnit::Millis),
                ),
        );
    Ok(Assembled { router, limiters })
}

/// The span every request runs inside.
///
/// # Why this exists at all: the default span is switched off in the shipped default
///
/// `TraceLayer::new_for_http()` builds a `DefaultMakeSpan`, and pinned `tower-http` 0.7.0 seeds it
/// from `DEFAULT_MESSAGE_LEVEL`, which is `Level::DEBUG`. `telemetry.filter` defaults to `info`. So
/// the one span this service had was **disabled in every default deployment**: `JsonStorageLayer`
/// had no span to collect fields from, every machine-readable line carried an empty span context,
/// and `docs/serving.md` promised the opposite. An overstated claim is itself a defect, and this is
/// the line that made it one.
///
/// `info_span!` is the fix. The level is not a verbosity preference here - it is the difference
/// between the documented behaviour and no behaviour.
///
/// # The route comes from `MatchedPath`, and the raw path is deliberately never logged
///
/// **This is the important design point in this function.** A span carrying `uri().path()` needs a
/// query redactor to keep secrets out of the log, and a correct redactor is not cheap: it has to
/// percent-decode *before* it decides what is sensitive, because `to%6ben` is `token` and a naive
/// name check reads it as an unremarkable parameter. It also has to be an allowlist, since the next
/// sensitive parameter is one nobody has thought of yet.
///
/// This surface needs none of that: every question arrives in a JSON body, no route reads a query
/// parameter, and what an operator groups by is the ROUTE rather than the path. So the cheaper and
/// stronger answer is not to log the path at all. `MatchedPath` is the route template - `/v1/query`,
/// `/health` - which is a value from this process's own routing table and not from the request.
///
/// It is available here because `Router::layer` applies a layer **per route**, inside routing, so
/// the extension is already set by the time this runs. A request matching nothing reaches the
/// catch-all fallback, which the same layer wraps and where there is no extension - hence
/// [`UNMATCHED_ROUTE`], a constant, so that case is one bucket rather than one per probed path.
///
/// # The fields, and why these
///
/// `metric` and `grain` are declared `Empty` and filled in by the query handler once the body has
/// parsed. Declared here because `tracing` cannot record a field a span was not opened with, and
/// filled in there because that is where the value first exists. With `JsonStorageLayer` every line
/// of that request then carries them, which is what an operator filters on. Counts and outcomes
/// stay events: they are things that happened, not things the request *is*.
fn request_span(request: &axum::extract::Request) -> tracing::Span {
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or(UNMATCHED_ROUTE, axum::extract::MatchedPath::as_str);
    tracing::info_span!(
        "request",
        method = %request.method(),
        route,
        correlation = %crate::correlation::CorrelationId::from_headers(request.headers()),
        metric = tracing::field::Empty,
        grain = tracing::field::Empty,
        // `verified` or `deployment`, filled in by the query handler once the request context exists.
        // The LABEL and not the identifier: a subject id on a span would raise the log's cardinality
        // to the number of people who ever asked, and the audit record is where the id belongs.
        asker = tracing::field::Empty,
    )
}

const fn limiter(cause: LimiterNotBuilt) -> RouterNotBuilt {
    RouterNotBuilt::Limiter { cause }
}

/// The versioned subtree with leg 1 in front of it, or refused, or unchanged.
///
/// Three outcomes from one declaration, and the middle one is the point: a deployment that declared an
/// inbound identity and has no gate does not get a router. See
/// [`RouterNotBuilt::InboundIdentityNotAttached`].
///
/// `route_layer` rather than `layer`, for the reason the token gate uses it: it runs only for a request
/// that matched a route in this subtree, so a path under the version prefix that matches nothing falls
/// through to the top-level `404` without a token check - which `crate::router`'s own documentation
/// already states, along with why that is acceptable.
/// Every route under the version prefix names a capability, or this router does not assemble.
///
/// **Reads the generated document rather than a list**, which is what makes it a check on the routes
/// the router mounts: `utoipa_axum::routes!` derives the axum route and the documented path from one
/// attribute, so a handler registered without a capability row shows up here.
///
/// The prefix is applied by [`assemble`]'s `nest`, so it is applied here too - and to the same
/// fragment, from the same function - which is why the paths compared are the templates
/// `axum::extract::MatchedPath` will hand the layer.
fn governed_routes() -> Result<(), RouterNotBuilt> {
    let fragment = OpenApiRouter::<ServiceState>::new()
        .nest(API_V1_PREFIX, routes::v1::openapi_router())
        .into_openapi();
    every_route_governed(&fragment)
}

/// The check itself, over a document handed in.
///
/// Split from [`governed_routes`] for one reason: a test can hand it a document carrying a route
/// nobody mapped, which is the only way to show the check is not vacuous. Asserting that today's
/// document passes proves nothing about a document that should fail.
fn every_route_governed(fragment: &utoipa::openapi::OpenApi) -> Result<(), RouterNotBuilt> {
    for (route, item) in &fragment.paths.paths {
        // `utoipa::openapi::PathItem` carries one `Option<Operation>` per HTTP method rather than a
        // map, so this list is written out - all EIGHT it has, including the ones this service would
        // never mount. That is the fail-closed direction: a route mounted under a method nobody
        // expected still has to name a capability, and omitting a method here would be the way to
        // mount an ungoverned route without failing this check. There is no ninth: the type has no
        // `connect` field, so a CONNECT operation cannot be described and cannot reach this loop.
        let mounted = [
            (axum::http::Method::GET, item.get.is_some()),
            (axum::http::Method::PUT, item.put.is_some()),
            (axum::http::Method::POST, item.post.is_some()),
            (axum::http::Method::DELETE, item.delete.is_some()),
            (axum::http::Method::OPTIONS, item.options.is_some()),
            (axum::http::Method::HEAD, item.head.is_some()),
            (axum::http::Method::PATCH, item.patch.is_some()),
            (axum::http::Method::TRACE, item.trace.is_some()),
        ];
        for (method, present) in mounted {
            if present && crate::capability::capability_of(&method, route).is_none() {
                return Err(RouterNotBuilt::RouteNotGoverned {
                    method: String::from(method.as_str()),
                    route: route.clone(),
                });
            }
        }
    }
    Ok(())
}

fn inbound_layered(
    versioned: Router,
    state: &ServiceState,
    declared: Option<&sutura_config::InboundIdentity>,
) -> Result<Router, RouterNotBuilt> {
    let Some(declared) = declared else {
        // No declaration. Nothing is installed, `crate::principal::established` answers
        // `TheDeploymentItself`, and the startup log says so - which is every deployment that shipped
        // before leg 1.
        return Ok(versioned);
    };
    let Some(gate) = state.inbound_identity() else {
        return Err(RouterNotBuilt::InboundIdentityNotAttached { mode: declared.mode() });
    };
    tracing::info!(
        inbound_mode = declared.mode(),
        header = gate.header(),
        establishes = declared.who_authenticated(),
        limit = sutura_config::InboundIdentity::what_it_does_not_do(),
        "leg 1 is installed on the versioned surface"
    );
    Ok(versioned.route_layer(axum::middleware::from_fn_with_state(
        Arc::clone(gate),
        crate::inbound::require_verified_caller,
    )))
}

/// The generated document and the browser interface over it, or an empty router.
///
/// **Two routers, one of them possibly empty, always merged.** The alternative is an
/// `Option<Router>` and a branch at the merge site; this way there is one shape, the decision is
/// made here, and the assembly above does not know it happened.
///
/// Behind the token gate when a token is configured, and behind the limiter *outside* that gate for
/// the same reason the versioned API is: a document behind a secret is a secret worth guessing at.
fn documentation(state: &ServiceState, settings: &Settings, key: &ClientAddress) -> DocumentationRouter {
    if !settings.api().docs_enabled() {
        return Ok((Router::new(), None));
    }
    // Serialized once, at startup, and served from a clone. Serializing per request would put a few
    // hundred kilobytes of work behind a path a caller can poll.
    let json = match crate::openapi::document_json() {
        Ok(json) => json,
        Err(cause) => {
            // Not fatal, and deliberately not: this service's job is answering questions, and a
            // document that will not serialize is a bug in a description of it. Loud, then carry on
            // without it rather than refusing to serve anything.
            tracing::error!(error = %cause, "the interface description would not serialize; not serving it");
            return Ok((Router::new(), None));
        }
    };
    let served = Router::new()
        .route(
            OPENAPI_JSON_PATH,
            axum::routing::get(move || {
                let body = json.clone();
                async move { ([(axum::http::header::CONTENT_TYPE, "application/json")], body) }
            }),
        )
        .merge(
            utoipa_swagger_ui::SwaggerUi::new(SWAGGER_UI_PATH)
                // `config` and deliberately NOT `url`. `url` would have the browser interface
                // register its own handler for that path and serialize its own copy of the
                // document - a second route on the same path, which `axum` rejects at assembly
                // time, and a second serialization that could differ from the one above. This
                // points the interface at the route already registered, so there is one document.
                .config(
                    utoipa_swagger_ui::Config::new([OPENAPI_JSON_PATH])
                        // The interface otherwise offers to send the document to a third-party
                        // validator, which would disclose this deployment's surface to it.
                        .validator_url("none"),
                ),
        );
    let limits = settings.rate_limit();
    let served = served.route_layer(axum::middleware::from_fn_with_state(state.clone(), middleware::require_token));
    if limits.enabled() {
        let (layer, handle) = middleware::probe_rate_limit_layer(limits.probe(), key.clone()).map_err(limiter)?;
        Ok((served.layer(layer), Some(handle)))
    } else {
        Ok((served.layer(middleware::disabled_rate_limit_layer()), None))
    }
}

/// Says what the limiter is doing, in words that differ by environment.
///
/// Exhaustive over the product of the environment and the switch. The `(Production, false)` arm is
/// unreachable - `sutura-config` refuses to start there - and it is kept because the configuration
/// is what decides, and an `error` for a state that cannot happen costs nothing while a missing one
/// would cost the diagnosis.
fn announce_rate_limiting(environment: Environment, enabled: bool) {
    match (environment, enabled) {
        (Environment::Development | Environment::Test, true) => {
            tracing::info!(%environment, "rate limiting enabled");
        }
        (Environment::Development | Environment::Test, false) => {
            tracing::info!(%environment, "rate limiting disabled - a no-op layer is in its place");
        }
        (Environment::Production, true) => {
            tracing::info!("rate limiting enabled in production");
        }
        (Environment::Production, false) => {
            tracing::error!(
                "rate limiting is disabled in a PRODUCTION deployment. This should have been \
                 refused at startup; the configuration gate did not fire"
            );
        }
    }
}

/// Says what a bucket is counted against, because the two answers fail differently.
///
/// Worth a line of its own: peer keying behind an ingress controller is one bucket for every caller
/// there has ever been, which reads in a graph exactly like a limit that is working.
fn announce_keying(limits: &sutura_config::RateLimitSettings) {
    if limits.client_address().reads_a_header() {
        tracing::info!(
            client_address = %limits.client_address(),
            trusted_proxies = limits.trusted_proxies().len(),
            "rate limit buckets are keyed on the forwarded header, read only from a named hop"
        );
    } else {
        tracing::info!(
            client_address = %limits.client_address(),
            "rate limit buckets are keyed on the peer address - behind a proxy that is ONE bucket \
             for every caller, and rate_limit.client_address is what changes it"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{RouterNotBuilt, every_route_governed, governed_routes};

    /// The check passes for the routes this crate actually mounts.
    #[test]
    fn every_route_this_crate_mounts_names_a_capability() {
        assert!(governed_routes().is_ok(), "{:?}", governed_routes().err());
    }

    /// A document carrying one route under one method, for the two tests below.
    fn document_with(route: &str, method: utoipa::openapi::HttpMethod) -> utoipa::openapi::OpenApi {
        let mut document = utoipa::openapi::OpenApi::new(
            utoipa::openapi::InfoBuilder::new().title("test").version("0").build(),
            utoipa::openapi::Paths::new(),
        );
        drop(document.paths.paths.insert(
            String::from(route),
            utoipa::openapi::PathItem::new(method, utoipa::openapi::path::OperationBuilder::new().build()),
        ));
        document
    }

    /// **And the check is not vacuous.** A route nobody mapped is refused, and the error names it.
    ///
    /// This is the half that matters: the test above passes for a check that returns `Ok` without
    /// looking at anything. Here a document carrying a route the table does not name has to fail, and
    /// the failure has to name what was not governed - because the error is what tells whoever added
    /// the route what to do about it.
    #[test]
    fn a_route_that_names_no_capability_assembles_no_router() {
        let refused = every_route_governed(&document_with("/v1/rogue", utoipa::openapi::HttpMethod::Post))
            .expect_err("an ungoverned route is refused");
        let RouterNotBuilt::RouteNotGoverned { method, route } = refused else {
            panic!("expected RouteNotGoverned, got {refused:?}");
        };
        assert_eq!(method, "POST");
        assert_eq!(route, "/v1/rogue");
    }

    /// A method nobody expected on a route that IS governed is still refused.
    ///
    /// The fail-closed direction, and the reason the method list in [`every_route_governed`] is
    /// written out in full: `crate::capability::governed` keys on method as well as path, so a
    /// `DELETE` mounted by accident on the question route does not inherit its capability.
    #[test]
    fn a_governed_route_under_an_unexpected_method_is_refused_too() {
        let refused = every_route_governed(&document_with("/v1/query", utoipa::openapi::HttpMethod::Delete))
            .expect_err("an unexpected method is refused");
        assert!(
            matches!(refused, RouterNotBuilt::RouteNotGoverned { ref method, .. } if method == "DELETE"),
            "{refused:?}"
        );
    }
}
