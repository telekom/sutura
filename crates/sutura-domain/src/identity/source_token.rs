//! A token for the asking caller's own principal at a source, which one node exchanged and another
//! reads a partition with (`docs/adr/20261102082308-workers-read-sources.md`, W2).

use core::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::identity::{AssertionDigest, Delegated, Secret, WorkloadPool};

/// An access token of the caller's pool principal, exchanged from the credential the caller
/// presented, for the node that reads a partition of that caller's split leg.
///
/// It keeps the fingerprint of the credential it was exchanged from, never that credential, so a
/// grant can refuse a token exchanged for someone other than its caller. Its `Debug` prints neither
/// the token nor the fingerprint.
///
/// **The limit:** the constructor is `pub`, so the fingerprint states what the exchanging adapter
/// says it exchanged. Nothing here can tell that an exchange took place.
pub struct SourceToken {
    secret: Secret,
    not_after: SystemTime,
    pool: WorkloadPool,
    exchanged_from: AssertionDigest,
}

impl SourceToken {
    /// `forwarded`, which the token service issued at `pool` in exchange for `from`. An expiry past
    /// what [`SystemTime`] can hold reads as the epoch: already ended, so no grant outlives it.
    #[must_use]
    pub fn exchanged(from: &Secret, pool: &WorkloadPool, forwarded: Delegated) -> Self {
        let not_after = UNIX_EPOCH
            .checked_add(Duration::from_secs(forwarded.not_after_unix_seconds()))
            .unwrap_or(UNIX_EPOCH);
        Self {
            secret: forwarded.into_token(),
            not_after,
            pool: pool.clone(),
            exchanged_from: AssertionDigest::of(from),
        }
    }

    /// The token.
    #[must_use]
    pub const fn secret(&self) -> &Secret {
        &self.secret
    }

    /// When the token service said the token stops being one.
    #[must_use]
    pub const fn not_after(&self) -> SystemTime {
        self.not_after
    }

    /// The pool whose principal the token is.
    #[must_use]
    pub const fn pool(&self) -> &WorkloadPool {
        &self.pool
    }

    /// The fingerprint of the credential the token was exchanged from.
    #[must_use]
    pub const fn exchanged_from(&self) -> &AssertionDigest {
        &self.exchanged_from
    }
}

impl core::fmt::Debug for SourceToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SourceToken")
            .field("not_after", &self.not_after)
            .field("pool", &self.pool)
            .finish_non_exhaustive()
    }
}
