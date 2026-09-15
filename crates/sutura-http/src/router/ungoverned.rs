//! [`Ungoverned`], in its own module so its fields are private to more than one `impl` block.
//!
//! Module privacy in Rust is scoped to the defining module and its descendants, not to the `impl`
//! block a field's methods live in - a private field is reachable from ANY item in the same module,
//! including a sibling function that never went through an accessor. Round 3 of `#758` measured
//! exactly that: `assemble` lives in `router.rs` beside this type, so `router.clone().merge(mount.router)`
//! (a bare field read, not a method call) compiled, passed `just lint` and the full suite. Nesting the
//! type here, with `super::assemble` outside this module, turns that same expression into `E0616`
//! ("field `router` of struct `Ungoverned` is private") - the type's own claim that "no method hands
//! back a bare `Router`" now also covers the field, because the field is unreachable from anywhere
//! that is not this module.

use axum::Router;

/// An ungoverned subtree, fused with the path it was mounted at.
///
/// The only way anything outside the governed subtree is mounted, and therefore the ALLOWLIST'S
/// MECHANISM rather than the recorder's memory. [`super::agent_subtree`] produces one of these and
/// `assemble` consumes it via [`Self::merge_into`] - the merge and the path [`super::check_ungoverned`]
/// must see are ONE call, so a subtree that is merged into the router is, by construction, recorded.
///
/// **What holds that, precisely:** no method on this type hands back a bare [`Router`] - [`Self::mount`]
/// builds the value, [`Self::layered`]/[`Self::try_layered`] transform the fused router IN PLACE while
/// `path` travels with it unread and unrewritten, and [`Self::merge_into`] is the only way to extract
/// the router at all, and it extracts by merging and recording in the same statement. So a caller
/// holding an `Ungoverned` cannot merge it without recording it, and cannot re-fuse its router under a
/// different literal path than the one [`Self::mount`] was given - the two mutation-table cells
/// `sutura/gates` names (an unrecorded `.merge`, and a mount recorded under a path other than the one
/// it serves) are both refused by this shape rather than by a caller's discipline.
///
/// **Nor by field access**: the fields below are private to THIS module, not to the `impl` block, and
/// `assemble` lives one module up - so `mount.router` does not compile there either. See the module
/// doc for the round-3 finding this closes.
///
/// The structural half lives in `xtask::boundaries::ungoverned`, which refuses a
/// `.nest`/`.nest_service`/`.route_service`/`.fallback_service` and a wildcard `.route` anywhere in
/// `sutura-http` or `sutura-serve` except inside [`Self::mount`] - a backstop for a mount primitive
/// written with no `Ungoverned` in sight at all, not for what this type already holds.
#[cfg(feature = "agent")]
#[derive(Clone, Debug)]
pub(crate) struct Ungoverned {
    router: Router,
    path: &'static str,
}

#[cfg(feature = "agent")]
impl Ungoverned {
    /// Nests a transport service at `path` - the one place anything outside the governed subtree
    /// is mounted - and fuses it with the path.
    ///
    /// Making this the SOLE `nest_service` call site is what `xtask::boundaries::ungoverned` holds, so a
    /// future ungoverned route has to mount here to exist, and mounting here records it.
    pub(crate) fn mount<S>(path: &'static str, service: S) -> Self
    where
        S: tower::Service<axum::http::Request<axum::body::Body>, Error = std::convert::Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Response: axum::response::IntoResponse,
        S::Future: Send + 'static,
    {
        Self {
            router: Router::new().nest_service(path, service),
            path,
        }
    }

    /// Applies `f` to the fused router while `path` travels with the value unread - a layer added
    /// here cannot relabel what it is layering.
    pub(crate) fn layered(self, f: impl FnOnce(Router) -> Router) -> Self {
        Self {
            router: f(self.router),
            path: self.path,
        }
    }

    /// The fallible form, for a layer that can itself refuse to attach (leg 1's own gate check).
    pub(crate) fn try_layered<E>(self, f: impl FnOnce(Router) -> Result<Router, E>) -> Result<Self, E> {
        Ok(Self {
            router: f(self.router)?,
            path: self.path,
        })
    }

    /// Merges this ungoverned subtree into `router` AND records its path, in one call.
    ///
    /// This is the atomicity that makes the allowlist structural rather than recorder's recall: a
    /// bare `merge` has no route to the fused router except this method's own merge, so a subtree
    /// that is merged is, by construction, handed to [`super::check_ungoverned`]. An unrecorded merge
    /// cannot be expressed, because nothing else on this type returns a bare `Router`, and no other
    /// module can reach the field to fuse one by hand.
    pub(crate) fn merge_into(self, router: &mut Router, recorded: &mut Vec<&'static str>) {
        *router = router.clone().merge(self.router);
        recorded.push(self.path);
    }
}
