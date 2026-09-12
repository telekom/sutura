//! The route tree.
//!
//! Three groups, and they differ in what guards them rather than in what they do:
//!
//! * [`health`] is unversioned and unauthenticated. An orchestrator's probe has no credential to
//!   present, so this is the one path that is always reachable - which is why its body carries
//!   nothing.
//! * [`protected_resource`] is unversioned and unauthenticated when direct inbound identity is
//!   configured. It tells a client which authorization server governs that exact resource.
//! * [`v1`] is the versioned API. It is behind the token gate when a token is configured, and
//!   behind the wider of the two rate-limit tiers.
//!
//! The prefixes and the guards are applied in [`crate::router`](fn@crate::router), not here. A
//! module that mounted itself would have to know what it is mounted under, which is the coupling
//! that makes a second version a rewrite.

pub(crate) mod health;
pub(crate) mod protected_resource;
pub(crate) mod v1;
