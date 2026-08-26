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
//! limiter. It cannot happen from a loaded configuration - `Quota` refuses the values that would
//! cause it - and the alternative to an error is either a panic for something the types already
//! ruled out or a fail-open fallback. See `middleware::LimiterNotBuilt`.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use sutura_config::{Environment, Settings};
use utoipa_axum::router::OpenApiRouter;

use crate::constants::{API_V1_PREFIX, OPENAPI_JSON_PATH, SWAGGER_UI_PATH};
use crate::middleware::{self, LimiterNotBuilt};
use crate::routes;
use crate::state::ServiceState;

/// Why the router could not be assembled.
#[derive(Debug, thiserror::Error)]
pub enum RouterNotBuilt {
    #[error("a rate limit tier could not be built")]
    Limiter {
        #[source]
        cause: LimiterNotBuilt,
    },
}

/// Builds the whole router for this state.
///
/// Everything the posture decides is decided here, once, from settings that were already
/// refused-or-accepted at startup. A handler cannot re-decide any of it, which is the point: a
/// request never arrives at a branch that could turn a control off.
pub fn router(state: &ServiceState) -> Result<Router, RouterNotBuilt> {
    let settings = state.settings().clone();
    let limits = settings.rate_limit();
    announce_rate_limiting(settings.environment(), limits.enabled());

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
    let versioned = if limits.enabled() {
        versioned.layer(middleware::api_rate_limit_layer(limits.api()).map_err(limiter)?)
    } else {
        versioned.layer(middleware::disabled_rate_limit_layer())
    };
    let versioned = versioned.route_layer(axum::middleware::from_fn_with_state(state.clone(), middleware::require_token));

    // Liveness. No token, and the tighter tier: nothing here is worth polling faster than that.
    let liveness: Router = Router::from(
        OpenApiRouter::new()
            .routes(utoipa_axum::routes!(routes::health::liveness))
            .with_state(state.clone()),
    );
    let liveness = if limits.enabled() {
        liveness.layer(middleware::probe_rate_limit_layer(limits.probe()).map_err(limiter)?)
    } else {
        liveness.layer(middleware::disabled_rate_limit_layer())
    };

    let documentation = documentation(state, &settings)?;

    Ok(Router::new()
        .merge(liveness)
        .merge(documentation)
        .merge(versioned)
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            settings.server().request_timeout().duration(),
        ))
        // Outermost, so a request refused by any layer below still produces a span and a timing.
        // `TraceLayer` on the outside is the difference between a `401` you can find in a log and a
        // `401` that happened to somebody.
        .layer(tower_http::trace::TraceLayer::new_for_http()))
}

const fn limiter(cause: LimiterNotBuilt) -> RouterNotBuilt {
    RouterNotBuilt::Limiter { cause }
}

/// The generated document and the browser interface over it, or an empty router.
///
/// **Two routers, one of them possibly empty, always merged.** The alternative is an
/// `Option<Router>` and a branch at the merge site; this way there is one shape, the decision is
/// made here, and the assembly above does not know it happened.
///
/// Behind the token gate when a token is configured. An interface description is a map of the
/// surface, and handing one to an unauthenticated caller is the same disclosure as handing them the
/// catalog.
fn documentation(state: &ServiceState, settings: &Settings) -> Result<Router, RouterNotBuilt> {
    if !settings.api().docs_enabled() {
        return Ok(Router::new());
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
            return Ok(Router::new());
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
    let served = if limits.enabled() {
        served.layer(middleware::probe_rate_limit_layer(limits.probe()).map_err(limiter)?)
    } else {
        served.layer(middleware::disabled_rate_limit_layer())
    };
    Ok(served.route_layer(axum::middleware::from_fn_with_state(state.clone(), middleware::require_token)))
}

/// Says what the limiter is doing, in words that differ by environment.
///
/// Exhaustive over the product of the environment and the switch. The `(Production, false)` arm is
/// unreachable - `sutura-config` refuses to start there - and it is kept because the configuration
/// is what decides, and an `error` for a state that cannot happen costs nothing while a missing one
/// would cost the diagnosis.
#[expect(
    clippy::cognitive_complexity,
    reason = "every arm is a tracing macro expanding into branches; the control flow is one match"
)]
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
