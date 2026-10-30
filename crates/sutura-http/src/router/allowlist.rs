//! The ungoverned-route allowlist: every route mounted outside the versioned subtree, and why.

use super::RouterNotBuilt;

/// One route mounted outside the versioned/governed subtree, and why that is deliberate.
///
/// The parallel of `crate::capability::GovernedRoute` for the routes `governed()`'s table cannot
/// see. Where a governed route names its `Capability`, an ungoverned route names its REASON - the
/// review that decided it does not need a per-route `Capability` row. See [`ungoverned_routes`].
pub(crate) struct UngovernedRoute {
    pub(super) path: &'static str,
    pub(super) reason: &'static str,
}

/// Every route this crate mounts outside the versioned subtree, with the reason each one is.
///
/// One of four fixed-size arrays, picked by which of `agent`/`flight` are compiled, rather than a
/// `Vec` - the same explicit-size style [`super::Ungoverned`] itself favours, so an added route is a
/// new element in a literal array rather than a push nothing checks the shape of. `/health`,
/// `/metrics` and the documentation paths are merged by `assemble` without an allowlist row and are
/// deliberately not listed here - see [`check_ungoverned`] for what that means this table does and
/// does not guard.
#[cfg(all(feature = "agent", feature = "flight"))]
#[must_use]
pub(crate) const fn ungoverned_routes() -> [UngovernedRoute; 2] {
    [AGENT_ROUTE, FLIGHT_ROUTE]
}

/// See [`ungoverned_routes`].
#[cfg(all(feature = "agent", not(feature = "flight")))]
#[must_use]
pub(crate) const fn ungoverned_routes() -> [UngovernedRoute; 1] {
    [AGENT_ROUTE]
}

/// See [`ungoverned_routes`].
#[cfg(all(feature = "flight", not(feature = "agent")))]
#[must_use]
pub(crate) const fn ungoverned_routes() -> [UngovernedRoute; 1] {
    [FLIGHT_ROUTE]
}

/// See [`ungoverned_routes`].
#[cfg(not(any(feature = "agent", feature = "flight")))]
#[must_use]
pub(crate) const fn ungoverned_routes() -> [UngovernedRoute; 0] {
    []
}

#[cfg(feature = "agent")]
const AGENT_ROUTE: UngovernedRoute = UngovernedRoute {
    path: crate::constants::AGENT_MOUNT_PATH,
    reason: "one route offers many tools, scoped per-tool inside `AgentSurface` via `Asked::permitted()` \
             rather than per-route the way `governed()` {path, capability} does",
};

#[cfg(feature = "flight")]
const FLIGHT_ROUTE: UngovernedRoute = UngovernedRoute {
    path: crate::constants::FLIGHT_MOUNT_PATH,
    reason: "a gRPC service dispatches its own methods by the tonic-generated path, which `governed()` \
             {path, capability} cannot name one row for",
};

/// Refuses a RECORDED ungoverned route that [`ungoverned_routes`] does not name.
///
/// The fail-closed half of the allowlist, over what [`super::Ungoverned`]'s type has already bound to the
/// merge: every ungoverned mount rides into `assemble` as an [`super::Ungoverned`] value, so the routes it
/// reports ARE the routes `assemble` merges - a recorded path with no row here is refused by name.
/// The reverse direction - a table row with nothing merged - is NOT held and deliberately so,
/// because `/mcp` is an OPTIONAL route (off unless `server.agent_surface.enabled` is set), and an
/// empty mounted set is a legal and common deployment shape. What this guards, WITH [`super::Ungoverned`]
/// and the `check-boundaries`' `ungoverned` gate, is that a merged ungoverned route always carries a row:
/// the type fuses a mount with its own path, and the gate refuses a mount primitive anywhere but
/// [`super::Ungoverned::mount`], so an unrecorded merge is not expressible. The limit of THIS function
/// alone is that it only sees what the type hands it - stating the mechanism whole is
/// [`super::Ungoverned`]'s job.
pub(crate) fn check_ungoverned(recorded: &[&str]) -> Result<(), RouterNotBuilt> {
    for path in recorded {
        if !ungoverned_routes().iter().any(|row| row.path == *path) {
            return Err(RouterNotBuilt::UngovernedRouteNotAllowlisted {
                path: String::from(*path),
            });
        }
    }
    Ok(())
}
