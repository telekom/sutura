//! Which capability each route is, and the layer that refuses one this caller was not granted.
//!
//! # The mapping is a table, and it is checked at ASSEMBLY rather than per request
//!
//! `sutura_app::Capability` is the tool set both transports render - see that module for why it is
//! not owned by either of them. This file is the HTTP side of the rendering: one row per route,
//! naming the capability it is.
//!
//! **A route missing from that table is a route this crate refuses to serve.**
//! [`crate::router::RouterNotBuilt::RouteNotGoverned`] is returned by `assemble`, which reads the
//! generated interface description - so the check is over the routes the router actually mounts
//! rather than over a second list somebody kept in step. That is the same shape as
//! `InboundIdentityNotAttached`: the failure is a process that does not start, not a request that
//! slips through.
//!
//! The alternative was a check inside each handler, and it was rejected for one reason: a handler can
//! forget. A layer over the whole subtree cannot, and the only place left to forget is a row in
//! [`governed`] - which is what the assembly refusal covers.
//!
//! # What this gates, and what it does not
//!
//! **It decides which OPERATIONS a caller may invoke. It decides nothing about which rows an answer
//! contains.** Both routes read the same pinned bundle and every question executes under the same
//! identity, because no source executes as the asking subject - `docs/adr/0014`'s leg 1 establishes
//! who is asking and leg 2 does not exist. A caller granted `sutura:metrics.ask` and not
//! `sutura:catalog.read` cannot list the catalog and gets exactly the same numbers from a question as
//! anybody else would.
//!
//! # Where the grant comes from, and the two holes a reader should check for
//!
//! [`permitted_for`] takes the [`sutura_app::Asked`] [`establish_asked`] already derived - not the
//! request - so there is no path left in this file where "no `Asked`" reads as "grant everything."
//! What `Asked` itself carries is still two cases, from whatever leg 1 established:
//!
//! * A [`crate::inbound::VerifiedCaller`] in the request extensions - which only
//!   `crate::inbound::gate::require_verified_caller` inserts, after a signature check - means the
//!   token's scopes decide, and **only** they do.
//! * No such extension means no caller identity was established, so there is no verified claim to
//!   narrow by and every capability is permitted. That is the single-player deployment this service
//!   ships as, and it is the correct answer rather than a fallback: a filter over an unverified claim
//!   looks like a control and is not one.
//!
//! Two different things could put a caller in the wrong case, and each is closed by its own
//! mechanism:
//!
//! * **A deployment that *meant* to establish an identity reaches the single-player answer above
//!   anyway.** Closed at ASSEMBLY: `crate::router::assemble` returns
//!   `RouterNotBuilt::InboundIdentityNotAttached` when the settings declare a mode and no gate was
//!   attached, so on a deployment that declares `security.inbound`, a request reaching a handler has
//!   been through the gate. `crate::inbound::tests::router` asserts the assembly half and the `401`.
//! * **[`establish_asked`] itself is skipped or reordered, so no `Asked` reaches this layer at
//!   all.** Nothing at assembly time proves a middleware stack's order, so this is closed by
//!   REFUSAL instead: [`require_capability`] answers `Failure::Internal` rather than falling back to
//!   [`Permitted::every_capability`], and the handler's own `axum::Extension<sutura_app::Asked>`
//!   parameter - a REQUIRED extractor, never `Option` - answers `500` on its own account if a request
//!   ever reached a handler without one. `crate::inbound::tests::router`'s
//!   `a_verified_caller_reaches_only_the_routes_its_scopes_name` and
//!   `a_verified_caller_whose_token_names_no_capability_scope_reaches_nothing` are the mechanism that
//!   proves the ORDER: a mutation moving `establish_asked` after this layer turns both red. Neither
//!   holds the ARM inside this function on its own, since both go through a route whose handler ALSO
//!   requires `Extension<sutura_app::Asked>` - `tests::a_skipped_establish_asked_is_refused_rather_than_answered_as_every_capability`
//!   is the cell that holds the ARM: it builds [`require_capability`] over a route with no
//!   `establish_asked` ahead of it at all, so the `500` it asserts can only be this function's own
//!   refusal.
//!
//! # It fails closed, and the refusal is what makes that survivable
//!
//! A verified caller whose token names no capability scope may do nothing - `sutura_app::Permitted`
//! carries that decision and its consequence. What keeps a deployment that forgot to author scopes
//! from being a mystery is the response: `403` with `code: insufficient_scope` and a sentence naming
//! the exact scope string, which is RFC 6750's own answer to this and is diagnosable without a log.

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use sutura_app::{Asked, Capability, Permitted};

use crate::constants::{API_V1_PREFIX, base_paths};
use crate::inbound::VerifiedCaller;
use crate::problem::Failure;

/// One row of the table: a method, the route template as `axum` matched it, and what it is.
///
/// **The route template and not the request's path**, which matters: `MatchedPath` is a value from
/// this process's own routing table, so nothing a caller sends can steer the lookup. The same reason
/// `crate::router::request_span` reads it rather than `uri().path()`.
///
/// A named type rather than a tuple because `crate::router` reads it too, and a three-tuple of
/// `(Method, String, Capability)` at two call sites is where an argument order gets swapped. Private
/// fields with accessors, which is the rule for a `pub struct` in a library crate here.
#[derive(Debug, Clone)]
pub struct GovernedRoute {
    method: Method,
    route: String,
    capability: Capability,
}

impl GovernedRoute {
    /// The route template, as it appears in the generated document and in `MatchedPath`.
    #[inline]
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// What invoking this route is.
    #[inline]
    #[must_use]
    pub const fn capability(&self) -> Capability {
        self.capability
    }
}

/// Every route this crate governs.
///
/// Built as a function rather than a `const` because the paths are composed from [`API_V1_PREFIX`]
/// and [`base_paths`], and composing them here is what keeps one owner for a path.
///
/// `pub` because `crate::router::assemble` reads it to refuse an ungoverned route, and because a test
/// in `crate::openapi` compares it against the generated document's operation identifiers.
#[must_use]
pub fn governed() -> [GovernedRoute; 3] {
    [
        GovernedRoute {
            method: Method::GET,
            route: format!("{API_V1_PREFIX}{}", base_paths::CATALOG),
            capability: Capability::DescribeCatalog,
        },
        GovernedRoute {
            method: Method::POST,
            route: format!("{API_V1_PREFIX}{}", base_paths::QUERY),
            capability: Capability::AskMetric,
        },
        GovernedRoute {
            method: Method::POST,
            route: format!("{API_V1_PREFIX}{}", base_paths::RUN_SQL),
            capability: Capability::RunSql,
        },
    ]
}

/// The capability a route is, or `None` if this crate does not govern it.
///
/// `None` is what `crate::router::assemble` refuses over. At request time it cannot happen - the
/// layer is installed on the versioned subtree only, and assembly proved every route in it has a row
/// - and the layer still refuses rather than passing, because "cannot happen" is not a control.
#[must_use]
pub fn capability_of(method: &Method, route: &str) -> Option<Capability> {
    governed()
        .into_iter()
        .find(|governed| governed.method == method && governed.route == route)
        .map(|governed| governed.capability)
}

/// What a caller `Asked` names may do.
///
/// See the module documentation for the two cases `Asked` itself carries, and for why the
/// single-player one is not a fallback.
///
/// **Takes the [`sutura_app::Asked`] [`establish_asked`] already derived, not the request - so an
/// absent `Asked` cannot read as every-capability here.** [`require_capability`] is the only caller;
/// it is the one place that reads one out of the extensions, and it refuses before this function is
/// ever reached when there is none.
///
/// `run_sql_enabled` narrows the result AFTER either case, and deliberately not inside them: a
/// deployment-level switch and a caller's own scope are two different reasons a capability is
/// absent, and [`Permitted::without`] is what applies the first without `Permitted` growing a
/// second notion of what a scope is. `docs/adr/0013`'s off-by-default raw SQL tool is the first
/// capability this applies to; a second one gains a parameter here rather than a widened boolean.
#[must_use]
pub fn permitted_for(asked: &Asked, run_sql_enabled: bool) -> Permitted {
    let permitted = asked.permitted().clone();
    if run_sql_enabled {
        permitted
    } else {
        permitted.without(Capability::RunSql)
    }
}

/// Derives [`sutura_app::Asked`] from whatever leg 1 established for this request, and inserts it.
///
/// **Total over both cases a deployment on this surface can be in, and refuses neither.** A verified
/// caller yields its scopes; no verified caller - because `security.inbound` is not declared -
/// yields [`Permitted::every_capability`], exactly as [`permitted_for`] answered before this existed.
/// So installing this layer changes nothing [`permitted_for`] returns.
///
/// **The only place either half of `Asked` is derived.** `crate::routes::v1::query::ask` and
/// `crate::routes::v1::run_sql::run_sql` read `asked.context()` from their own required
/// `axum::Extension<sutura_app::Asked>` parameter instead of re-deriving one through
/// `crate::principal::of_verified` - so `crate::principal::of_verified` has exactly one caller left,
/// this function. Before this, three call sites derived a context that happened to agree; now there
/// is one.
///
/// **What this does NOT do, named because a later reader will look for it:** it does not refuse a
/// request with no established caller. That is the correct single-player answer this surface has
/// always given on a deployment with no declared inbound identity, not an error - see the module
/// documentation. Where an absent value IS a refusal is the agent surface: a mount only happens
/// behind a declared inbound identity, and the outer `require_verified_caller` layer installed
/// there refuses an unverified caller before this ever runs.
pub(crate) async fn establish_asked(
    State(state): State<crate::state::ServiceState>,
    mut request: Request,
    next: Next,
) -> Response {
    let (context, permitted) = request.extensions().get::<VerifiedCaller>().map_or_else(
        || (crate::principal::established(), Permitted::every_capability()),
        |caller| {
            let granted = state
                .settings()
                .security()
                .audience_mapping()
                .granted_for(caller.groups().iter());
            (
                crate::principal::of_verified(caller).granting(sutura_domain::catalog::GrantedAudiences::of(granted)),
                Permitted::granted_by(caller.scopes().iter()),
            )
        },
    );
    drop(request.extensions_mut().insert(Asked::established(context, permitted)));
    next.run(request).await
}

/// Refuses a request for a capability this caller was not granted.
///
/// A layer over the versioned subtree rather than a check in each handler, so there is nothing for a
/// handler to forget. Installed inside both `require_verified_caller` and [`establish_asked`] - see
/// `crate::router` for the whole order - which is what makes `sutura_app::Asked` available here.
///
/// **Reads `Asked` itself, and refuses rather than falls back, when it is absent.** That case cannot
/// happen while `crate::router::assemble` installs [`establish_asked`] unconditionally ahead of this
/// layer - it is the same "cannot happen, refuse anyway" shape [`asked_for`]'s own ungoverned-route
/// arm already uses, not a second thing this file has to be tested for on its own.
///
/// Takes the state now, for one reading: `settings.tools().run_sql_enabled()`. `docs/adr/0013`'s tool
/// must be absent for every caller when a deployment never turned it on - see [`permitted_for`].
pub async fn require_capability(State(state): State<crate::state::ServiceState>, request: Request, next: Next) -> Response {
    let run_sql_enabled = state.settings().tools().run_sql_enabled();
    // Cannot happen from a correctly assembled router - see the doc comment above - and refused
    // rather than read as "no claim to narrow by" anyway: that reading is the every-capability
    // answer this whole file exists to prevent, and "cannot happen" is not a control.
    let Some(asked) = request.extensions().get::<Asked>() else {
        tracing::error!("no Asked reached the capability layer; establish_asked must run ahead of it");
        return Failure::Internal.into_response();
    };
    let permitted = permitted_for(asked, run_sql_enabled);
    match asked_for(&request) {
        // The grant was already read above, so a defect in it - the mutation that no longer
        // applies `Permitted::without` when off - shows up here as a `200` a router-level test
        // catches, not as a second check this arm could pass around.
        Some(capability) if permitted.includes(capability) => next.run(request).await,
        // Not included. WHICH of two reasons decides the BODY, never whether this proceeds -
        // `sutura:sql.run` would not help a caller told to go get it when the real reason is a
        // deployment switch (`#666`'s review, finding 2).
        Some(capability) if capability == Capability::RunSql && !run_sql_enabled => refused_tool_not_enabled(capability),
        Some(capability) => refused(capability),
        // The route is not one this crate governs. Assembly proved that cannot reach here, and it is
        // refused rather than passed anyway: a layer that fell open on a case its author thought
        // unreachable is the shape this file exists to avoid. The detail goes to the log, never to
        // the caller - see `crate::problem`.
        None => Failure::Internal.into_response(),
    }
}

/// What invoking this request would be, by the route it matched.
///
/// Its own function so [`require_capability`] stays a match rather than a body: the two unreachable
/// branches plus the grant check put it over the cognitive-complexity threshold in `clippy.toml`, and
/// the split puts "which capability is this" in one place.
fn asked_for(request: &Request) -> Option<Capability> {
    // `route_layer` runs only for a request that matched, so there is always a matched path - and the
    // absence is treated as an ungoverned route rather than passed, for the reason
    // `require_capability`'s `None` arm gives.
    let route = request.extensions().get::<axum::extract::MatchedPath>()?;
    let capability = capability_of(request.method(), route.as_str());
    if capability.is_none() {
        tracing::error!(
            route = route.as_str(),
            method = %request.method(),
            "a route under the version prefix names no capability; refusing"
        );
    }
    capability
}

/// The `403`, and the one place the missing scope is written down.
fn refused(capability: Capability) -> Response {
    tracing::warn!(
        capability = capability.id(),
        scope = capability.scope(),
        "refused: this caller was not granted the capability this route needs"
    );
    Failure::InsufficientScope {
        required: capability.scope(),
    }
    .into_response()
}

/// The `403` for a capability no caller may reach because this DEPLOYMENT never turned it on -
/// distinct from [`refused`], whose sentence sends a caller looking for a scope grant that would
/// not help here.
fn refused_tool_not_enabled(capability: Capability) -> Response {
    tracing::warn!(
        capability = capability.id(),
        "refused: this deployment has not enabled this capability"
    );
    Failure::ToolNotEnabled {
        capability: capability.scope(),
    }
    .into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use sutura_app::{Capability, Permitted};

    use super::{capability_of, governed, require_capability};
    use crate::constants::{API_V1_PREFIX, base_paths};

    /// **This crate's half of `both_transports_describe_the_same_tools`.**
    ///
    /// The two transports cannot compare notes - an adapter never calls another adapter - so what is
    /// asserted is that this transport governs **exactly** `sutura_app::Capability::every()`, once
    /// each. `sutura_mcp::tool`'s test of the same name asserts the same thing of its advertised tool
    /// list, against the same source. Two tests over one source is the only shape in which the two
    /// descriptions cannot disagree.
    #[test]
    fn both_transports_describe_the_same_tools() {
        let mut covered: Vec<&str> = governed().iter().map(|governed| governed.capability().id()).collect();
        covered.sort_unstable();
        let mut expected: Vec<&str> = Capability::every().map(Capability::id).collect();
        expected.sort_unstable();
        assert_eq!(covered, expected, "{covered:?}");
        // Once each: two rows for one capability would mean two routes doing the same job, and one
        // of them would drift.
        covered.dedup();
        assert_eq!(covered.len(), governed().len());
    }

    #[test]
    fn each_governed_route_resolves_to_its_capability_and_only_on_its_method() {
        for governed in governed() {
            let route = governed.route();
            let capability = governed.capability();
            // The method is part of the key. A `GET /v1/query` is not the question route, and a lookup
            // that ignored the method would gate it as though it were - which is how a route added
            // under a second method arrives ungoverned.
            //
            // Exactly one method resolves, and it resolves to this row's capability. Both halves: the
            // count alone would pass if the wrong capability came back.
            let resolved: Vec<Capability> = [Method::GET, Method::POST, Method::DELETE, Method::PUT]
                .iter()
                .filter_map(|method| capability_of(method, route))
                .collect();
            assert_eq!(resolved, vec![capability], "{route}");
        }
    }

    #[test]
    fn a_route_this_crate_does_not_govern_resolves_to_nothing() {
        // Liveness is outside the version prefix and deliberately ungoverned - a probe has no
        // credential to present. The layer is not installed for it; this asserts the table agrees.
        assert_eq!(capability_of(&Method::GET, crate::constants::HEALTH_PATH), None);
        assert_eq!(capability_of(&Method::GET, "/v1/anything"), None);
        // And the base path without the version prefix is not the route: a table keyed on the
        // unprefixed path would gate nothing, because `MatchedPath` carries the full template.
        assert_eq!(capability_of(&Method::GET, base_paths::CATALOG), None);
        assert!(
            governed().iter().all(|governed| governed.route().starts_with(API_V1_PREFIX)),
            "a governed route outside the version prefix would never be reached by the layer"
        );
    }

    /// The narrowing is the shared one, so this transport holds no second copy of the comparison.
    #[test]
    fn a_scope_decides_which_routes_are_reachable() {
        let catalog_only = Permitted::granted_by([Capability::DescribeCatalog.scope()]);
        assert!(catalog_only.includes(Capability::DescribeCatalog));
        assert!(!catalog_only.includes(Capability::AskMetric));
        // Fail closed.
        let nobody = Permitted::granted_by(["openid"]);
        for governed in governed() {
            assert!(!nobody.includes(governed.capability()), "{}", governed.route());
        }
    }

    /// `#129`'s own named test on this transport.
    ///
    /// A caller without `sutura:sql.run` is refused the raw route, and a caller who has every OTHER
    /// scope still does not reach it - narrowing is per capability, not an all-or-nothing gate.
    #[test]
    fn a_caller_without_the_sql_run_scope_cannot_reach_run_sql() {
        let without_it = Permitted::granted_by([Capability::DescribeCatalog.scope(), Capability::AskMetric.scope()]);
        assert!(!without_it.includes(Capability::RunSql));
        let with_it = Permitted::granted_by([Capability::RunSql.scope()]);
        assert!(with_it.includes(Capability::RunSql));
        assert_eq!(
            capability_of(&Method::POST, &format!("{API_V1_PREFIX}{}", base_paths::RUN_SQL)),
            Some(Capability::RunSql)
        );
    }

    /// The ARM half of the module doc's second hole - the two `crate::inbound::tests::router` cells
    /// hold the ORDER (`establish_asked` ahead of this layer), and this cell holds what happens
    /// INSIDE this function when that order is violated so completely there is no `Asked` at all.
    ///
    /// Built by hand rather than through `crate::router::assemble`, because assembly always installs
    /// `establish_asked` unconditionally - the only way to put [`require_capability`] on a route with
    /// NO `Asked` ahead of it is to not go through assembly at all. That is deliberately the shape a
    /// mis-ordered or dropped layer would produce.
    #[tokio::test]
    async fn a_skipped_establish_asked_is_refused_rather_than_answered_as_every_capability() {
        let settings = sutura_config::Settings::load(&sutura_config::Sources::defaults(sutura_config::Environment::Development))
            .expect("the default settings load");
        let service = crate::surface::LocalService::start(
            &crate::testing::catalog_of(crate::testing::bundle()),
            crate::testing::fake_warehouse(),
            crate::testing::sink(),
            crate::testing::broker(),
            1 << 30,
        )
        .expect("the test bundle validates");
        let state = crate::testing::state_over(std::sync::Arc::new(service), settings);
        let route = format!("{API_V1_PREFIX}{}", base_paths::CATALOG);
        // No `establish_asked` anywhere in this router - the point of the test.
        let app = axum::Router::new()
            .route(&route, axum::routing::get(|| async { axum::http::StatusCode::OK }))
            .route_layer(axum::middleware::from_fn_with_state(state, require_capability));
        let (status, body) =
            crate::testing::call(&app, crate::testing::request("GET", &route, None, axum::body::Body::empty())).await;
        assert_eq!(
            status,
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "an absent `Asked` must never read as every capability: {body}"
        );
    }
}
