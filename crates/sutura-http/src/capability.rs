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
//! # Where the grant comes from, and the one honest hole in it
//!
//! [`permitted_for`] is the whole derivation, and it is two cases:
//!
//! * A [`crate::inbound::VerifiedCaller`] in the request extensions - which only
//!   `crate::inbound::gate::require_verified_caller` inserts, after a signature check - means the
//!   token's scopes decide, and **only** they do.
//! * No such extension means no caller identity was established, so there is no verified claim to
//!   narrow by and every capability is permitted. That is the single-player deployment this service
//!   ships as, and it is the correct answer rather than a fallback: a filter over an unverified claim
//!   looks like a control and is not one.
//!
//! **The hole a reader should check for, and why it is closed:** if the second case could be reached
//! by a deployment that *meant* to establish an identity, this layer would be a control that silently
//! turned itself off. It cannot be. `crate::router::assemble` returns
//! `RouterNotBuilt::InboundIdentityNotAttached` when the settings declare a mode and no gate was
//! attached, and the gate either refuses the request with a `401` or inserts the extension. So on a
//! deployment that declares `security.inbound`, a request reaching a handler has been through the
//! gate. `crate::inbound::tests::router` already asserts the assembly half and the `401`.
//!
//! # It fails closed, and the refusal is what makes that survivable
//!
//! A verified caller whose token names no capability scope may do nothing - `sutura_app::Permitted`
//! carries that decision and its consequence. What keeps a deployment that forgot to author scopes
//! from being a mystery is the response: `403` with `code: insufficient_scope` and a sentence naming
//! the exact scope string, which is RFC 6750's own answer to this and is diagnosable without a log.

use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use sutura_app::{Capability, Permitted};

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
pub fn governed() -> [GovernedRoute; 2] {
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

/// What this request's caller may do.
///
/// See the module documentation for the two cases and for why the second is not a fallback.
#[must_use]
pub fn permitted_for(request: &Request) -> Permitted {
    request.extensions().get::<VerifiedCaller>().map_or_else(
        // No verified caller: nothing established an identity, so there is no claim to narrow by.
        Permitted::every_capability,
        // The scopes, and nothing else. `Scopes::iter` yields what the token carried, parsed and
        // bounded by `crate::inbound::caller`; the comparison against the capability's own scope
        // literal happens once, in `sutura_app`, so this transport holds no copy of it.
        |caller| Permitted::granted_by(caller.scopes().iter()),
    )
}

/// Refuses a request for a capability this caller was not granted.
///
/// A layer over the versioned subtree rather than a check in each handler, so there is nothing for a
/// handler to forget. Installed INSIDE `crate::inbound::gate::require_verified_caller`, which is what
/// makes the extension available here - see `crate::router` for the whole order.
pub async fn require_capability(request: Request, next: Next) -> Response {
    match asked_for(&request) {
        Some(capability) if permitted_for(&request).includes(capability) => next.run(request).await,
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

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use sutura_app::{Capability, Permitted};

    use super::{capability_of, governed};
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
        let mut covered: Vec<&str> = governed()
            .iter()
            .map(|governed| governed.capability().id())
            .collect();
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
            governed()
                .iter()
                .all(|governed| governed.route().starts_with(API_V1_PREFIX)),
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
}
