//! The per-replica spend counter: in-process, windowed, keyed by the asking subject.
//!
//! `docs/adr/0030-where-a-budget-lives.md` decides every shape here; this module is the mechanism.
//! Three decisions worth restating because a reader of the code alone could miss them:
//!
//! **Fixed windows, not sliding ones.** A subject's spend resets to zero the first time this
//! ledger is consulted after the window has elapsed, rather than decaying continuously. Simpler to
//! reason about - "how much has this subject spent since their window started" needs one `Instant`
//! and one running total, not a queue of timestamped charges to prune - and the cost a sliding
//! window would avoid (a subject who spends right at a boundary can spend up to twice the ceiling
//! across the seam) is not a cost `docs/adr/0030` asked this record to close: the record's own
//! scope is a per-replica counter that resets on restart in addition to its own window, so a seam
//! effect inside one window is not the precision this shape is buying.
//!
//! **Keyed on [`Subject`], never the whole [`PrincipalChain`](sutura_domain::identity::PrincipalChain).**
//! An agent acting for a subject spends that subject's own budget - see the ADR for the argument and
//! its cost.
//!
//! **`None` configured is unlimited, not zero.** [`SpendLedger::no_budget`] is what every deployment
//! ran before this existed, and it is a real state a caller may still choose rather than a value
//! nothing constructs.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use sutura_domain::identity::Subject;

/// A byte ceiling and the window it resets on, already validated.
///
/// A plain pair rather than a re-export of `sutura_config::SpendBudget`: this crate depends on
/// nothing outside `sutura-domain`, `sutura-semantic` and `thiserror` - see this crate's own module
/// documentation - and a composition root reads the validated ceiling and window out of its
/// settings and hands the two primitives here, the same shape `working_set_bytes: u64` already
/// uses for `RuntimeSettings::working_set`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpendBudget {
    ceiling_bytes: u64,
    window: Duration,
}

impl SpendBudget {
    #[must_use]
    #[inline]
    pub const fn new(ceiling_bytes: u64, window: Duration) -> Self {
        Self { ceiling_bytes, window }
    }
}

/// One subject's running total for the window currently open.
struct Window {
    started_at: Instant,
    spent_bytes: u64,
}

/// What one charge against the ledger decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    /// Under the ceiling, or no ceiling is configured at all.
    Admitted,
    /// Spending this would put the subject over the ceiling for the window still open.
    Refused {
        /// How long until this subject's window resets, from the instant charged.
        reset_after: Duration,
    },
}

/// The counter: one instance per process, consulted by every question this replica answers.
///
/// **`&self`, not `&mut self`** - one ledger is shared by every request without a lock in this
/// type's own signature, the same shape `sutura_domain::audit::AuditSink::record` uses for the same
/// reason. The mutable state is inside a `Mutex` guarding the per-subject map.
pub struct SpendLedger {
    budget: Option<SpendBudget>,
    #[expect(
        clippy::disallowed_types,
        reason = "charge() is a synchronous method, never held across an await point, so it cannot deadlock an executor - the same argument `sutura_runtime::testing::Capture` already makes for its own std Mutex"
    )]
    windows: std::sync::Mutex<HashMap<Subject, Window>>,
}

impl SpendLedger {
    /// A ledger bounded by `budget`, or unbounded if `None`.
    #[must_use]
    #[expect(clippy::disallowed_types, reason = "see the field's own note")]
    pub fn new(budget: Option<SpendBudget>) -> Self {
        Self {
            budget,
            windows: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// No ceiling configured. Every question is admitted and nothing is counted - `docs/adr/0030`'s
    /// "absent means no budget, which is today's behaviour" read back as a constructor.
    #[must_use]
    pub fn no_budget() -> Self {
        Self::new(None)
    }

    /// Checks whether `estimated_bytes` may be spent by `subject` right now, and charges it against
    /// the subject's open window if it may.
    ///
    /// **Charging and checking are one call, not two**, for the reason a check-then-act pair always
    /// is: two calls would let two concurrent questions from the same subject both read "under the
    /// ceiling" before either recorded its own spend, which is exactly the race a lock inside one
    /// method closes and a lock taken twice at two call sites does not.
    ///
    /// `now` is a parameter rather than read inside, so a test can assert a fixed-window reset
    /// without sleeping - the same shape `Deadline::remaining_at(Instant::now())` already takes at
    /// its call sites.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the guard has to live for the whole function: `window`, borrowed from it, is read \
                  and written across every branch below, and a compiled attempt to inline the lock \
                  into one chained expression does not borrow-check - E0716, temporary dropped while \
                  still borrowed"
    )]
    pub fn charge(&self, subject: &Subject, estimated_bytes: u64, now: Instant) -> Charge {
        let Some(budget) = self.budget else {
            return Charge::Admitted;
        };
        let mut windows = self.windows.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let window = windows.entry(subject.clone()).or_insert_with(|| Window {
            started_at: now,
            spent_bytes: 0,
        });
        // Lazy reset: a window that has elapsed is not swept on a timer, it is noticed the next
        // time this subject is charged - which is every window this ledger needs to track, because
        // a subject that never asks again never needs one.
        if now.saturating_duration_since(window.started_at) >= budget.window {
            window.started_at = now;
            window.spent_bytes = 0;
        }
        let projected = window.spent_bytes.saturating_add(estimated_bytes);
        if projected > budget.ceiling_bytes {
            let elapsed = now.saturating_duration_since(window.started_at);
            return Charge::Refused {
                reset_after: budget.window.saturating_sub(elapsed),
            };
        }
        window.spent_bytes = projected;
        Charge::Admitted
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use sutura_domain::identity::{Subject, SubjectId};

    use super::{Charge, SpendBudget, SpendLedger};

    fn subject(id: &str) -> Subject {
        Subject::Verified {
            id: SubjectId::parse(id).expect("a test subject id parses"),
        }
    }

    #[test]
    fn no_budget_configured_admits_everything_and_charges_nothing() {
        let ledger = SpendLedger::no_budget();
        let now = Instant::now();
        // A charge far beyond any ceiling anybody would configure - proving "unlimited" rather than
        // "a very large default".
        assert_eq!(ledger.charge(&subject("alice"), u64::MAX, now), Charge::Admitted);
        assert_eq!(ledger.charge(&subject("alice"), u64::MAX, now), Charge::Admitted);
    }

    #[test]
    fn spending_past_the_ceiling_is_refused() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 600, now), Charge::Admitted);
        // 600 + 500 = 1100, over the 1000-byte ceiling.
        let refused = ledger.charge(&subject("alice"), 500, now);
        assert!(matches!(refused, Charge::Refused { .. }), "{refused:?}");
    }

    #[test]
    fn spending_exactly_the_ceiling_is_admitted() {
        // The row-cap discipline this crate already uses elsewhere: AT the ceiling is not OVER it.
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 1_000, now), Charge::Admitted);
    }

    #[test]
    fn the_window_resets_after_it_elapses() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 1_000, now), Charge::Admitted);
        // Still inside the window: no room left.
        assert!(matches!(ledger.charge(&subject("alice"), 1, now), Charge::Refused { .. }));
        // Past the window: the same subject, the same amount, admitted again.
        let after_window = now + Duration::from_secs(61);
        assert_eq!(ledger.charge(&subject("alice"), 1_000, after_window), Charge::Admitted);
    }

    #[test]
    fn a_refusal_names_how_long_until_the_window_resets() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 1_000, now), Charge::Admitted);
        let ten_seconds_in = now + Duration::from_secs(10);
        let Charge::Refused { reset_after } = ledger.charge(&subject("alice"), 1, ten_seconds_in) else {
            panic!("spending over the ceiling must be refused");
        };
        // 60s window, 10s already elapsed: 50s left.
        assert_eq!(reset_after, Duration::from_secs(50));
    }

    #[test]
    fn two_subjects_are_isolated_from_each_other() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 1_000, now), Charge::Admitted);
        // Alice is now fully spent for the window; bob has never been charged and is unaffected.
        assert!(matches!(ledger.charge(&subject("alice"), 1, now), Charge::Refused { .. }));
        assert_eq!(ledger.charge(&subject("bob"), 1_000, now), Charge::Admitted);
    }
}
