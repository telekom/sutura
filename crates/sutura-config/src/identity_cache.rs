//! The exchanged-credential cache's own settings - `docs/adr/0031`, `security.credential_cache`.
//!
//! **Off by default**, and the reason is stated rather than assumed: nobody has run a token
//! exchange against a real authorization server yet (`github.com/telekom/sutura#376`), so the
//! round-trip cost this cache would save is unmeasured. Shipping it off lets an operator turn it on
//! once they have a measurement of their own `IdP`'s behaviour, rather than sutura asserting the
//! trade on their behalf. Modelled on [`crate::tools::ToolsSettings`]: infallible once parsed, one
//! key per capability, no group-wide switch.

use std::num::NonZeroUsize;
use std::time::Duration;

/// `security.credential_cache.{capacity,window_seconds}` is not usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCredentialCacheSettings {
    /// A capacity of zero caches nothing - a slower way to spell `enabled: false`.
    #[error(
        "`security.credential_cache.capacity` is zero, which caches nothing - remove the key or write a positive number of entries"
    )]
    EmptyCapacity,
    /// A window of zero seconds serves nothing - the same shape of mistake as a zero capacity.
    #[error(
        "`security.credential_cache.window_seconds` is zero, which serves nothing - remove the key or write a positive number of seconds"
    )]
    NoWindow,
}

/// How long an operator lets the cache serve an entry, on top of whatever the credential's own
/// life and the broker's floor already bound it to.
///
/// **A ceiling only, never a grant.** `sutura_exec_bigquery`'s cache folds this window together
/// with the credential's own expiry (minus the broker's floor) through `min`, so a large window
/// here cannot make a served credential outlive what was actually minted - it can only make the
/// cache stop serving an entry SOONER than the credential's own life would allow. That asymmetry is
/// the whole reason this is a distinct type rather than a bare `u64`: the field name alone invites
/// reading it as "how long the cache keeps something alive", and the truth is narrower - it is one
/// of three numbers a `min` is taken over, and the other two are never influenced by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheWindow(Duration);

impl CacheWindow {
    /// Parses a configured number of seconds. Refuses zero, the same shape
    /// `sutura_config::server::RequestTimeout::parse` refuses one - a zero-second window is a
    /// cache that never serves anything, which is a slower way to spell `enabled: false`.
    pub const fn parse(seconds: u64) -> Result<Self, InvalidCredentialCacheSettings> {
        if seconds == 0 {
            Err(InvalidCredentialCacheSettings::NoWindow)
        } else {
            Ok(Self(Duration::from_secs(seconds)))
        }
    }

    /// The window, as a duration the broker's cache can add to an instant it read.
    #[inline]
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }
}

/// The default entry count when `security.credential_cache` declares no `capacity`.
const DEFAULT_CAPACITY: u64 = 1024;

/// The default window, in seconds, when `security.credential_cache` declares no `window_seconds`.
const DEFAULT_WINDOW_SECONDS: u64 = 300;

/// The exchanged-credential cache's own settings.
///
/// **`Copy`, like [`crate::tools::ToolsSettings`]**: every field is a small owned value, and this
/// is held in [`crate::Settings`] the same way. There is deliberately no credential-shaped field
/// here - this type is a bound and a switch, never a place a secret could arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CredentialCacheSettings {
    enabled: bool,
    capacity: NonZeroUsize,
    window: CacheWindow,
}

impl Default for CredentialCacheSettings {
    /// Off, with the shipped defaults for capacity and window - the same values calling
    /// [`Self::parse`] with `false, None, None` would produce. Spelled directly rather than through
    /// that `Result`, because both literals are provably non-zero and a `Default` impl has nowhere
    /// to return an error if they somehow were not - `unwrap_or` rather than `unwrap`/`expect`,
    /// which `Cargo.toml`'s workspace lints deny outside test code, so the fallback arm is a second
    /// non-zero literal rather than a panic that can never fire.
    fn default() -> Self {
        Self {
            enabled: false,
            capacity: NonZeroUsize::new(usize::try_from(DEFAULT_CAPACITY).unwrap_or(usize::MAX)).unwrap_or(NonZeroUsize::MIN),
            window: CacheWindow(Duration::from_secs(DEFAULT_WINDOW_SECONDS)),
        }
    }
}

impl CredentialCacheSettings {
    /// Assembles the group from parts that have each already been parsed.
    #[inline]
    #[must_use]
    pub const fn new(enabled: bool, capacity: NonZeroUsize, window: CacheWindow) -> Self {
        Self {
            enabled,
            capacity,
            window,
        }
    }

    /// Reads the raw, optional configuration and applies the defaults above - a capacity or a
    /// window an operator did not write, never a capacity or a window of zero: those are refused
    /// by name rather than silently rounded up, the same way an unset `security.credential_cache`
    /// block is silently `enabled: false` rather than a refusal.
    pub fn parse(
        enabled: bool,
        capacity_entries: Option<u64>,
        window_seconds: Option<u64>,
    ) -> Result<Self, InvalidCredentialCacheSettings> {
        let capacity_entries = capacity_entries.unwrap_or(DEFAULT_CAPACITY);
        // `unwrap_or(usize::MAX)` rather than a refusal on a 32-bit target where the configured
        // number does not fit `usize`: a capacity that wide is not one this process could fill
        // either way, so it is treated as "no practical bound" rather than as a startup refusal
        // over a platform width nobody configuring this key is thinking about.
        let capacity = NonZeroUsize::new(usize::try_from(capacity_entries).unwrap_or(usize::MAX))
            .ok_or(InvalidCredentialCacheSettings::EmptyCapacity)?;
        let window = CacheWindow::parse(window_seconds.unwrap_or(DEFAULT_WINDOW_SECONDS))?;
        Ok(Self {
            enabled,
            capacity,
            window,
        })
    }

    /// Whether an operator turned this on. Off unless `security.credential_cache.enabled: true`.
    #[inline]
    #[must_use]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    /// How many live entries the cache may hold at once.
    #[inline]
    #[must_use]
    pub const fn capacity(self) -> NonZeroUsize {
        self.capacity
    }

    /// The operator's own ceiling on how long an entry is served.
    #[inline]
    #[must_use]
    pub const fn window(self) -> CacheWindow {
        self.window
    }
}

#[cfg(test)]
mod tests {
    use super::{CacheWindow, CredentialCacheSettings, InvalidCredentialCacheSettings};

    #[test]
    fn off_unless_an_operator_turns_it_on() {
        let settings = CredentialCacheSettings::parse(false, None, None).expect("defaults parse");
        assert!(!settings.enabled());
    }

    #[test]
    fn absent_capacity_and_window_take_the_documented_defaults() {
        let settings = CredentialCacheSettings::parse(true, None, None).expect("defaults parse");
        assert_eq!(settings.capacity().get(), 1024);
        assert_eq!(settings.window().duration().as_secs(), 300);
    }

    #[test]
    fn a_zero_capacity_is_refused() {
        assert_eq!(
            CredentialCacheSettings::parse(true, Some(0), None).unwrap_err(),
            InvalidCredentialCacheSettings::EmptyCapacity
        );
    }

    #[test]
    fn a_zero_window_is_refused() {
        assert_eq!(CacheWindow::parse(0).unwrap_err(), InvalidCredentialCacheSettings::NoWindow);
        assert_eq!(
            CredentialCacheSettings::parse(true, None, Some(0)).unwrap_err(),
            InvalidCredentialCacheSettings::NoWindow
        );
    }
}
