//! The pool an `impersonation-at-source` source federates its callers at.
//!
//! The type is the domain's, beside the [`PoolExchange`](sutura_domain::identity::PoolExchange)
//! port that consumes it; this module keeps the transport's path to it and the cell on its
//! screening. How the caller's assertion becomes an access token is `super::identity`'s.

pub use sutura_domain::identity::{UnusablePool, WorkloadPool};

#[cfg(test)]
mod tests;
