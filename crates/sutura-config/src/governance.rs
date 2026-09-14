//! What one replica will spend on one subject before it refuses them until a window resets.
//!
//! `docs/adr/0030-where-a-budget-lives.md` decides the shape this key carries and names its own
//! limit; this module is only the parsing. Read the ADR before changing either half.
//!
//! **The settings key says what this is, on purpose.** `governance.per_replica_spend_ceiling` and
//! not `governance.spend_budget`: the counter this key configures is per-replica, in-process, and
//! resets on every restart in addition to its own window - a deployment with N replicas gets N
//! times this ceiling before every replica has independently refused. Naming it `spend_budget`
//! unqualified would let an operator who read only the key mistake the limit for the goal.
//!
//! **Absent is a decision, not an omission.** `RawGovernance::per_replica_spend_ceiling` is an
//! `Option`, and `None` means this replica counts nothing and refuses nothing on this account -
//! today's behaviour before this key existed. A deployment that wants the counter writes both
//! `bytes` and `window_seconds` under one key, together: there is no state where only one of the
//! two is configured, because [`RawSpendCeiling`](crate::raw::RawSpendCeiling) is a nested object
//! and a YAML mapping either has it or does not.

use std::time::Duration;

use crate::server::InvalidBound;

/// A byte ceiling and the window it resets on.
///
/// **Two newtypes rather than a bare `(u64, Duration)`**, so an argument-order mistake at a call
/// site is a type error - the same reason a `SourceName` and a `SourceName` neighbour do not sit
/// as two bare `String`s elsewhere in this workspace. `Copy`, like every other bound in this crate:
/// it is read once at boot and carried by value from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendBudget {
    ceiling_bytes: SpendCeilingBytes,
    window: SpendWindow,
}

/// The byte ceiling half of [`SpendBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendCeilingBytes(u64);

/// The window half of [`SpendBudget`]: how long a subject's spend accumulates before it resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendWindow(Duration);

impl SpendBudget {
    /// Reads a ceiling and a window in whole seconds, refusing either at zero.
    ///
    /// **Zero is refused for both, and for the same reason every other bound in this crate refuses
    /// it**: a zero ceiling reads as "no limit" to somebody writing the file rather than "refuse
    /// every question", and a zero window never accumulates anything, which is a counter that
    /// never fires dressed as one that resets constantly. Neither ambiguity is one this type
    /// carries silently.
    pub const fn parse(bytes: u64, window_seconds: u64) -> Result<Self, InvalidBound> {
        if bytes == 0 {
            return Err(InvalidBound::Zero {
                name: "governance.per_replica_spend_ceiling.bytes",
            });
        }
        if window_seconds == 0 {
            return Err(InvalidBound::Zero {
                name: "governance.per_replica_spend_ceiling.window_seconds",
            });
        }
        Ok(Self {
            ceiling_bytes: SpendCeilingBytes(bytes),
            window: SpendWindow(Duration::from_secs(window_seconds)),
        })
    }

    /// The ceiling, in bytes.
    #[inline]
    #[must_use]
    pub const fn ceiling_bytes(self) -> u64 {
        self.ceiling_bytes.0
    }

    /// The window a subject's spend accumulates over before it resets.
    #[inline]
    #[must_use]
    pub const fn window(self) -> Duration {
        self.window.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::SpendBudget;
    use crate::server::InvalidBound;

    #[test]
    fn a_ceiling_and_a_window_round_trip() {
        let budget = SpendBudget::parse(1_000_000, 60).expect("a positive ceiling and window parse");
        assert_eq!(budget.ceiling_bytes(), 1_000_000);
        assert_eq!(budget.window(), Duration::from_secs(60));
    }

    #[test]
    fn a_zero_ceiling_is_refused_rather_than_read_as_no_limit() {
        assert_eq!(
            SpendBudget::parse(0, 60),
            Err(InvalidBound::Zero {
                name: "governance.per_replica_spend_ceiling.bytes"
            })
        );
    }

    #[test]
    fn a_zero_window_is_refused_rather_than_a_counter_that_never_fires() {
        assert_eq!(
            SpendBudget::parse(1_000_000, 0),
            Err(InvalidBound::Zero {
                name: "governance.per_replica_spend_ceiling.window_seconds"
            })
        );
    }
}
