//! The metric series the HTTP surface observes, and the one place their labels are closed.
//!
//! # It is a boundary, not a pass-through
//!
//! This module owns the transport's half of `docs/adr/0015`'s series table: the outcome counters,
//! the duration histogram, the admission series, the rate-limit visibility, the unauthorized
//! counter and the answer-rows histogram. The registry lives in `sutura-runtime`; this type holds
//! the handles and names the series. Every labeled registration and update accepts
//! `sutura_runtime::metrics::Label`; the values are chosen here from the transport's own fixed code
//! vocabulary and the complete set is registered at boot, so an unknown observation cannot mint a
//! series (Decision 5).
//!
//! # Registration is boot-only, and this type is the WHOLE of it here
//!
//! [`Metrics::install`] is the only place in this crate that registers a series, and it takes a
//! [`sutura_runtime::metrics::RegistryBuilder`] to do it - never a built
//! [`sutura_runtime::metrics::Registry`]. The two series whose value is the deployment rather than a
//! request - `sutura_engine_worker_threads` and `sutura_catalog_metrics` - are registered against
//! the same builder by [`crate::state::ServiceState::new`], which is the one place that has both the
//! settings and the served bundle. A built registry cannot be registered against, so the set is
//! closed once the builder is consumed.
//!
//! # What this does NOT hold
//!
//! The three engine-pool series `docs/adr/0015` specifies - reserved bytes, the limit and refusals -
//! are deliberately absent, following that record's own *absent rather than zero*: the pool bounds
//! the engine's own operators and nothing else, and a gauge an operator would alert on as process
//! memory is worse than no gauge. Nothing here closes that gap.

use std::time::{Duration, Instant};

use sutura_runtime::metrics::{Counter, Gauge, Histogram, Label, LabeledCounter, LabeledGauge, RegistryBuilder, label};

use crate::problem::Failure;

/// The bucket arrays the histograms are built with.
///
/// `DURATION` brackets the request timeout (default 30s) and the admission window; `ANSWER_ROWS`
/// brackets the row cap. Both are closed fixed arrays.
mod buckets {
    /// Seconds.
    pub(super) const DURATION: &[f64] = &[0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0];
    /// Seconds of admission wait.
    pub(super) const ADMISSION_WAIT: &[f64] = &[0.001, 0.01, 0.1, 1.0, 5.0, 30.0];
    /// Rows.
    pub(super) const ANSWER_ROWS: &[f64] = &[1.0, 10.0, 100.0, 1000.0, 10_000.0, 100_000.0];
}

/// The closed set of `code` labels for `sutura_questions_total`.
///
/// Every label is a literal here or exactly `Failure::code`'s vocabulary. The registry APIs accept
/// [`Label`] rather than text and pre-register this whole set, so an observation outside it cannot
/// create a series. `every_failure_code_is_a_label` has the exhaustive match that makes a new
/// `Failure` variant require an explicit decision here.
pub(crate) const QUESTION_CODES: &[Label] = &[
    ANSWER,
    label("at_capacity"),
    label("identity_unavailable"),
    label("insufficient_scope"),
    label("internal"),
    label("not_a_question"),
    label("rate_limited"),
    REFUSED,
    label("timeout"),
    label("too_large"),
    UNAUTHORIZED,
    label("unavailable"),
];

const ANSWER: Label = label("answer");
const REFUSED: Label = label("refused");
const UNAUTHORIZED: Label = label("unauthorized");

/// The closed set of `tier` labels for the rate-limit families.
///
/// One value per limiter tier `crate::router` builds: the probe/documentation surface, the versioned
/// API, and `/metrics`. A value here that no tier passes is a series that renders zero forever.
pub(crate) const PUBLIC_TIER: Label = label("public");
pub(crate) const GENERAL_TIER: Label = label("general");
pub(crate) const METRICS_TIER: Label = label("metrics");
pub(crate) const TIERS: &[Label] = &[PUBLIC_TIER, GENERAL_TIER, METRICS_TIER];

/// The outcome a terminal question response declares to the outer accounting layer.
///
/// A response extension, rather than a body inspection: failure details remain encapsulated in
/// `ProblemBody`, and the accounting layer cannot mistake an unrelated response with the same
/// status for a question outcome.
#[derive(Debug, Clone, Copy)]
pub(crate) struct QuestionOutcome {
    code: Label,
    rows: Option<usize>,
}

impl QuestionOutcome {
    pub(crate) const fn failed(failure: &Failure) -> Self {
        Self {
            code: label(failure.code()),
            rows: None,
        }
    }

    pub(crate) const fn answered(rows: usize) -> Self {
        Self {
            code: ANSWER,
            rows: Some(rows),
        }
    }

    pub(crate) const fn refused() -> Self {
        Self {
            code: REFUSED,
            rows: None,
        }
    }

    const fn internal() -> Self {
        Self {
            code: label("internal"),
            rows: None,
        }
    }

    pub(crate) fn is_unauthorized(self) -> bool {
        self.code == UNAUTHORIZED
    }
}

/// The transport's metrics handles, installed for one service state.
///
/// `Clone` is cheap and shares the same underlying atomics. The built [`sutura_runtime::metrics::Registry`]
/// is shared with the `/metrics` route separately, through the state, never through this type.
#[derive(Debug, Clone)]
pub struct Metrics {
    /// `sutura_questions_total{code}`.
    questions: LabeledCounter,
    /// `sutura_question_duration_seconds`.
    duration: Histogram,
    /// `sutura_execution_slots` - the capacity denominator.
    slots: Gauge,
    /// `sutura_execution_slots_in_use` - true in-flight work (a slot is held until the work ends).
    slots_in_use: Gauge,
    /// `sutura_admission_shed_total`.
    shed: Counter,
    /// `sutura_admission_wait_seconds`.
    wait: Histogram,
    /// `sutura_rate_limited_total{tier}`.
    rate_limited: LabeledCounter,
    /// `sutura_rate_limit_buckets{tier}` - how many keys a limiter tier's store is holding.
    rate_limit_buckets: LabeledGauge,
    /// `sutura_unauthorized_total`.
    unauthorized: Counter,
    /// `sutura_answer_rows`.
    answer_rows: Histogram,
}

impl Metrics {
    /// Registers every transport series against `builder`, and returns the handles to observe them.
    ///
    /// The only registration site in this crate. It takes a builder because registration is
    /// boot-only - the caller registers the deployment-wide series beside these, then builds the
    /// registry, and a built registry has no registration door.
    #[must_use]
    pub fn install(builder: &mut RegistryBuilder) -> Self {
        let questions = builder.labeled_counter("sutura_questions_total", "code", QUESTION_CODES);
        let duration = builder.histogram("sutura_question_duration_seconds", buckets::DURATION);
        let slots = builder.gauge("sutura_execution_slots");
        let slots_in_use = builder.gauge("sutura_execution_slots_in_use");
        let shed = builder.counter("sutura_admission_shed_total");
        let wait = builder.histogram("sutura_admission_wait_seconds", buckets::ADMISSION_WAIT);
        let rate_limited = builder.labeled_counter("sutura_rate_limited_total", "tier", TIERS);
        let rate_limit_buckets = builder.labeled_gauge("sutura_rate_limit_buckets", "tier", TIERS);
        let unauthorized = builder.counter("sutura_unauthorized_total");
        let answer_rows = builder.histogram("sutura_answer_rows", buckets::ANSWER_ROWS);
        Self {
            questions,
            duration,
            slots,
            slots_in_use,
            shed,
            wait,
            rate_limited,
            rate_limit_buckets,
            unauthorized,
            answer_rows,
        }
    }

    /// The capacity denominator, reported when the bound is known.
    pub fn set_capacity(&self, bound: usize) {
        self.slots.set(bound as u64);
    }

    /// Records a slot starting; the returned guard releases it when dropped.
    ///
    /// The guard is `#[must_use]` on its type precisely so a call site has to name what it does with
    /// it: the guard must be owned by the blocking work a slot was granted for, so a caller that
    /// times out and drops the request cannot release the in-use count before the work ends.
    pub fn slot_started(&self) -> SlotGuard {
        self.slots_in_use.adjust(1);
        SlotGuard(self.slots_in_use.clone())
    }
    /// Starts observing how long an admission attempt waits.
    ///
    /// The returned guard observes on drop, so success, shedding and cancellation by the outer
    /// request timeout all terminate one observation. It borrows the histogram and allocates
    /// nothing.
    pub(crate) fn admission_started(&self) -> AdmissionWait<'_> {
        AdmissionWait {
            histogram: &self.wait,
            started: Instant::now(),
        }
    }

    /// Records a shed: a slot not granted, distinct from a fault.
    ///
    /// Counts only the admission event. The outer completion layer records the question outcome and
    /// duration from the response extension, as it does for every other terminal response.
    pub fn shed(&self) {
        self.shed.inc();
    }

    /// Records one completed matched `POST /v1/query` response.
    ///
    /// Code and duration move together here, so no terminal path can update one without the other.
    /// The histogram deliberately carries no outcome label.
    pub(crate) fn completed_question(&self, outcome: Option<QuestionOutcome>, elapsed: Duration) {
        let outcome = outcome.unwrap_or_else(|| {
            tracing::error!("a matched question response declared no outcome; recording it as internal");
            QuestionOutcome::internal()
        });
        self.questions.inc(outcome.code);
        self.duration.observe(elapsed.as_secs_f64());
        if let Some(rows) = outcome.rows {
            // Answer rows are bounded by the domain contract. Metrics must remain non-panicking if
            // that contract is violated, so saturate before widening.
            self.answer_rows.observe(f64::from(u32::try_from(rows).unwrap_or(u32::MAX)));
        }
    }

    /// Records an unauthorized attempt. **Surface-wide, by design** - a `401` from the token gate or
    /// from `/metrics`'s own gate is a credential problem, not a question outcome, and it must not
    /// move the question family. The outer completion layer owns that accounting.
    pub fn unauthorized(&self) {
        self.unauthorized.inc();
    }

    /// Records a rate-limit refusal for a tier. **Surface-wide, by design** - the limiter stands
    /// outside the question handler and can honestly attribute only the tier it refused on. It must
    /// not move the question family itself; the route-aware completion layer does that only for a
    /// matched question response.
    pub fn rate_limited(&self, tier: Label) {
        self.rate_limited.inc(tier);
    }

    /// Records how many keys a limiter tier's store is holding.
    pub fn limiter_buckets(&self, tier: Label, count: usize) {
        self.rate_limit_buckets.set(tier, count as u64);
    }
}

/// A held execution slot being counted as in use; released when the work ends.
#[must_use]
pub struct SlotGuard(Gauge);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.adjust(-1);
    }
}

/// One admission attempt, observed however the awaiting future terminates.
#[must_use]
pub(crate) struct AdmissionWait<'metrics> {
    histogram: &'metrics Histogram,
    started: Instant,
}

impl Drop for AdmissionWait<'_> {
    fn drop(&mut self) {
        self.histogram.observe(self.started.elapsed().as_secs_f64());
    }
}

#[cfg(test)]
mod tests {
    use sutura_runtime::metrics::RegistryBuilder;

    use super::{Metrics, QuestionOutcome};
    use crate::problem::Failure;

    /// Every `Failure` variant is a label this module declares.
    ///
    /// The match is exhaustive with no wildcard arm, so a variant added to [`Failure`] does not
    /// compile until its code is either in `QUESTION_CODES` or explicitly argued away here. That is
    /// the mechanism the constant's own doc names, and the assertion then holds the two in step.
    #[test]
    fn every_failure_code_is_a_label() {
        let every = [
            Failure::Unauthorized,
            Failure::InsufficientScope {
                required: "catalog.read",
            },
            Failure::NotAQuestion {
                detail: String::from("a bad body"),
            },
            Failure::TooLarge,
            Failure::RateLimited,
            Failure::Timeout,
            Failure::Internal,
            Failure::Unavailable,
            Failure::IdentityUnavailable,
            Failure::AtCapacity { retry_after_seconds: 1 },
        ];
        for failure in &every {
            // No wildcard: a new failure cannot compile this test until its relationship to the
            // closed metric vocabulary is decided explicitly.
            match failure {
                Failure::Unauthorized
                | Failure::InsufficientScope { .. }
                | Failure::NotAQuestion { .. }
                | Failure::TooLarge
                | Failure::RateLimited
                | Failure::Timeout
                | Failure::Internal
                | Failure::Unavailable
                | Failure::IdentityUnavailable
                | Failure::AtCapacity { .. } => {}
            }
            assert!(
                super::QUESTION_CODES.contains(&sutura_runtime::metrics::label(failure.code())),
                "{} is a Failure code with no declared label",
                failure.code()
            );
        }
    }

    #[test]
    fn a_saturated_row_count_does_not_panic() {
        // The contract violation is impossible on the answer path, but a metric must not be the
        // thing that aborts the process under `panic = "abort"`. This is the boundary itself.
        let mut builder = RegistryBuilder::default();
        let metrics = Metrics::install(&mut builder);
        metrics.completed_question(Some(QuestionOutcome::answered(usize::MAX)), std::time::Duration::ZERO);
        let rendered = builder.build().render();
        assert!(rendered.contains("sutura_answer_rows_count 1"), "{rendered}");
    }
}
