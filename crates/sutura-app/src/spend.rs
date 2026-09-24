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
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// **A raw constructor, not a parse - the zero fence lives one crate over.** This type takes
    /// whatever `ceiling_bytes` and `window` it is given, `Duration::ZERO` included: under a zero
    /// window every charge resets immediately, and a priced question over the ceiling mints
    /// `reset_after == Duration::ZERO` again. The only production caller is
    /// `sutura_config::SpendBudget::parse` (`crates/sutura-config/src/governance.rs`), which
    /// refuses a zero window (and a zero ceiling) before this constructor ever sees one - this
    /// type's own doc comment above states why this crate cannot depend on that one, so the fence
    /// sits there rather than here.
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
///
/// `pub(crate)`, not `pub`: nothing outside this crate calls [`SpendLedger::charge`] - only
/// `crate::charge_subject` does, which is also the one place `federated::answer_federated`'s
/// summed charge reaches the ledger through - so exporting this widened `docs/api/sutura-app.md`
/// for nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Charge {
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
    /// Every byte admitted since this ledger was built, across every subject and every window
    /// that has since reset - **never reset itself**, unlike the per-subject windows above.
    ///
    /// A plain atomic rather than a field inside the same `Mutex`: nothing here re-reads it
    /// alongside a window, so a lock shared with `windows` would only widen the critical section
    /// `charge`'s own `#[expect]` already argues for keeping tight. See
    /// [`Self::spent_bytes_total`] for why monotonicity is the whole point of this field existing
    /// beside a gauge that already reports headroom.
    total_spent_bytes: AtomicU64,
}

impl SpendLedger {
    /// A ledger bounded by `budget`, or unbounded if `None`.
    #[must_use]
    #[expect(clippy::disallowed_types, reason = "see the field's own note")]
    pub fn new(budget: Option<SpendBudget>) -> Self {
        Self {
            budget,
            windows: std::sync::Mutex::new(HashMap::new()),
            total_spent_bytes: AtomicU64::new(0),
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
    pub(crate) fn charge(&self, subject: &Subject, estimated_bytes: u64, now: Instant) -> Charge {
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
        // Cumulative, and never rolled back by the window reset above: `spent_bytes_total` is the
        // running total THIS PROCESS has admitted, which is exactly what a Prometheus counter needs
        // to survive a rolling deploy's changing replica count - see that method's own doc.
        self.total_spent_bytes.fetch_add(estimated_bytes, Ordering::Relaxed);
        Charge::Admitted
    }

    /// The tightest remaining headroom across every subject this ledger is currently tracking, or
    /// `None` where no ceiling is configured (`docs/adr/0030`'s "absent means no budget").
    ///
    /// **Deployment-wide, never per-subject** - ADR-0015 Decision 5 types every metric label
    /// parameter as `&'static str` precisely so request-owned text (a [`Subject`]'s own identifier
    /// included) cannot become one, so this reports the worst case across every subject rather than
    /// naming which one it is. A subject not yet in the map, or whose window has elapsed, has its
    /// full ceiling as headroom - so an empty or fully-expired map reports the ceiling itself, never
    /// `None` and never zero, the same "absent rather than zero" discipline `sutura-http`'s own
    /// metrics module already applies to a gauge with no meaningful value.
    #[must_use]
    pub fn headroom_bytes(&self, now: Instant) -> Option<u64> {
        let budget = self.budget?;
        // The guard's construction and its one use merged into one expression, per
        // `clippy::significant_drop_tightening` - unlike `charge`, nothing here re-borrows across
        // branches, so there is no reason to hold it a statement longer than the read.
        let tightest = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .filter(|window| now.saturating_duration_since(window.started_at) < budget.window)
            .map(|window| budget.ceiling_bytes.saturating_sub(window.spent_bytes))
            .min();
        Some(tightest.unwrap_or(budget.ceiling_bytes))
    }

    /// Every byte this replica has admitted since it started, or `None` where no ceiling is
    /// configured - the same "absent rather than zero" [`Self::headroom_bytes`] already applies.
    ///
    /// **Monotonic, and that is the whole reason this exists beside a gauge that already reports
    /// headroom.** `sutura_spend_headroom_bytes` (`#884`) cannot be summed across replicas: it
    /// resets to the full ceiling on every restart, and `sum()` over N replicas is `N * ceiling -
    /// total_spend`, a number that moves whenever N does. This total only grows, so
    /// `sum(rate(sutura_spend_bytes_total[5m]))` is correct across a restart (a monitoring
    /// system's counter-reset handling) and across a changing replica count - the owner decision
    /// `docs/adr/0030`'s amendment records, 2026-09-18: aggregation belongs to the monitoring
    /// system, never to enforcement, which stays per-replica either way.
    ///
    /// **Exported as `sutura_spend_bytes_total` by both transports, by different routes.**
    /// `sutura_http::ServiceState::new` (`sutura-http`) registers the counter beside the
    /// `sutura_spend_headroom_bytes` gauge. `POST /v1/query`'s post-answer poll pushes it through
    /// the state's own `record_spend_headroom`; the agent surface raises it through the
    /// `SpendHeadroomPush` handle the composition root hands its `Serving` wrapper - the HTTP
    /// route never touches that handle. The `/metrics` scrape renders it.
    ///
    /// **Registered only where a ceiling is configured:** `Some` here is the registration
    /// condition, so an unconfigured deployment exports neither series - absent rather than zero,
    /// the same discipline the gauge applies.
    ///
    /// **Limit: on every deployment that can boot with a ceiling today, this series reads 0
    /// forever.** A ledger charges a `None` estimate nothing, and the adapters that can boot
    /// under `governance.per_replica_spend_ceiling` cannot price a dry run: a `bigquery` source
    /// beside the ceiling is refused at boot
    /// (`NotFitToServe::UnpricedSourceUnderSpendCeiling`), so no
    /// priced adapter can be served with one; `DuckDB` and Postgres answer
    /// `PreFlight::Accepted { estimated_bytes: None }`, and `ClickHouse` and Oracle take the port's
    /// own default `PreFlight::NotAsked`. A dashboard reading `sum(rate(...))` of zero here
    /// therefore cannot distinguish "nothing was spent" from "no call was ever priced" - see
    /// `docs/adr/0030`'s amendment for the same sentence.
    #[must_use]
    pub fn spent_bytes_total(&self) -> Option<u64> {
        self.budget?;
        Some(self.total_spent_bytes.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use sutura_domain::identity::Subject;

    use super::{Charge, SpendBudget, SpendLedger};

    fn subject(id: &str) -> Subject {
        Subject::verified(id).expect("a test subject id parses")
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

    #[test]
    fn no_budget_configured_reports_no_headroom_value_at_all() {
        // Unlimited is `None`, never a very large number - the same "absent means unlimited"
        // reading `SpendBudget::new`'s own doc gives the ceiling.
        let ledger = SpendLedger::no_budget();
        assert_eq!(ledger.headroom_bytes(Instant::now()), None);
    }

    #[test]
    fn an_untouched_ledger_reports_the_full_ceiling_as_headroom() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        assert_eq!(ledger.headroom_bytes(Instant::now()), Some(1_000));
    }

    #[test]
    fn headroom_is_the_tightest_subject_not_the_average() {
        // Deterministic against `.min()` versus a mean, and NOT against `.next()`/the first entry
        // a `HashMap` iterator yields - that order is unspecified and randomised per-process, so a
        // mutation to `.next()` survives some fraction of runs here rather than every one. The name
        // says only what this cell can actually hold.
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 100, now), Charge::Admitted);
        assert_eq!(ledger.charge(&subject("bob"), 900, now), Charge::Admitted);
        // Alice has 900 left, bob has 100 left - the gauge must read bob's, the worse case.
        assert_eq!(ledger.headroom_bytes(now), Some(100));
    }

    #[test]
    fn a_charge_past_the_window_no_longer_narrows_the_reported_headroom() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 900, now), Charge::Admitted);
        assert_eq!(ledger.headroom_bytes(now), Some(100));
        let after_window = now + Duration::from_secs(61);
        // Alice's window has elapsed; nothing has re-charged her yet, so she is back at full
        // headroom rather than still reading as the tightest subject.
        assert_eq!(ledger.headroom_bytes(after_window), Some(1_000));
    }

    #[test]
    fn no_budget_configured_reports_no_spend_total_at_all() {
        let ledger = SpendLedger::no_budget();
        assert_eq!(ledger.spent_bytes_total(), None);
    }

    #[test]
    fn an_untouched_ledger_reports_a_zero_spend_total() {
        // Zero is a real reading here, unlike headroom's own untouched value - nothing has been
        // admitted yet, so the running total genuinely is zero rather than absent.
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        assert_eq!(ledger.spent_bytes_total(), Some(0));
    }

    #[test]
    fn spend_total_grows_with_every_admitted_charge_and_ignores_a_refusal() {
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 600, now), Charge::Admitted);
        assert_eq!(ledger.spent_bytes_total(), Some(600));
        // Refused: puts alice over the ceiling, admits nothing, adds nothing to the total.
        assert!(matches!(ledger.charge(&subject("alice"), 500, now), Charge::Refused { .. }));
        assert_eq!(ledger.spent_bytes_total(), Some(600));
    }

    #[test]
    fn spend_total_survives_a_window_reset_that_zeroes_the_per_subject_headroom() {
        // The property `#139` exists for: headroom bounces back to the full ceiling once a
        // subject's window elapses, but the cumulative total must not - a restarted window is not
        // spend being refunded, and summing this series across a rolling deploy depends on it only
        // ever growing.
        let ledger = SpendLedger::new(Some(SpendBudget::new(1_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 900, now), Charge::Admitted);
        assert_eq!(ledger.spent_bytes_total(), Some(900));
        let after_window = now + Duration::from_secs(61);
        assert_eq!(ledger.headroom_bytes(after_window), Some(1_000), "headroom bounces back");
        assert_eq!(ledger.spent_bytes_total(), Some(900), "the total does not");
        assert_eq!(ledger.charge(&subject("alice"), 100, after_window), Charge::Admitted);
        assert_eq!(
            ledger.spent_bytes_total(),
            Some(1_000),
            "and keeps accumulating across the reset"
        );
    }

    #[test]
    fn two_subjects_add_to_one_shared_spend_total() {
        // Unlike headroom, which reports the tightest SUBJECT, the total is deployment-wide by
        // construction - it has no per-subject shape to report at all.
        let ledger = SpendLedger::new(Some(SpendBudget::new(10_000, Duration::from_secs(60))));
        let now = Instant::now();
        assert_eq!(ledger.charge(&subject("alice"), 100, now), Charge::Admitted);
        assert_eq!(ledger.charge(&subject("bob"), 900, now), Charge::Admitted);
        assert_eq!(ledger.spent_bytes_total(), Some(1_000));
    }
}
