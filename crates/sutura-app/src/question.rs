//! What each transport says about a question it takes, the same way on every surface.
//!
//! Here and not in a transport because `sutura-http` and `sutura-mcp` cannot see each other: a line
//! or a series spelled in one is one the other has to copy, and the two surfaces then drift on what an
//! operator reads about the same question. [`received`] is the log line; [`QuestionMeter`] is the
//! port the series are counted through, implemented by `sutura_http::Metrics`.
//!
//! The port reports what happened and never a value the caller wrote: a refusal arrives as its code,
//! an answer as its row count.

use std::time::{Duration, Instant};

use sutura_domain::identity::RequestContext;
use sutura_domain::query::{Query, ToolOutcome};

use crate::surface::SurfaceFailure;

/// Says which question was asked, before it is answered or turned away.
///
/// `metric` and `grain` go on the request span, so every later line of the request carries them
/// (they are what the request IS, and what an operator filters by); `asker` goes with them as the
/// established label, never the subject. The counts stay on the event. Deliberately no filter value:
/// a value reflected into a log outlives the request.
///
/// `request` is the span the transport declared `metric`, `grain` and `asker` on, and it is passed
/// rather than read from `Span::current()` because the current span is not always that one: `tracing`
/// cannot record a field a span was not opened with and drops the write silently, and the `/mcp`
/// tool call runs inside a span of the MCP library, a child of the request's. The line is written
/// inside `request` for the same reason: the log formatter copies a parent's fields into a child when
/// the child opens, so a span opened before these records - that MCP span among them - never carries
/// them, and neither does a line written inside it.
pub fn received(request: &tracing::Span, query: &Query, asker: &RequestContext) {
    request.record("metric", tracing::field::display(query.metric()));
    request.record("grain", tracing::field::display(query.grain()));
    request.record("asker", asker.chain().subject().established());
    request.in_scope(|| {
        tracing::info!(
            dimensions = query.dimensions().len(),
            filters = query.filters().len(),
            "question received"
        );
    });
}

/// How one question ended, as the series count it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionEnd {
    /// Answered with this many rows.
    Answered {
        /// Rows in the answer.
        rows: usize,
    },
    /// Refused, under `RefusalReason::code`.
    Refused {
        /// The refusal's own code.
        code: &'static str,
    },
    /// No execution slot came free inside the admission window.
    AtCapacity,
    /// The caller's whole wait ran out first.
    TimedOut,
    /// The data system did not answer.
    DataSystemUnavailable,
    /// The credential broker did not answer.
    IdentityUnavailable,
    /// This deployment's own fault.
    Internal,
}

impl From<&ToolOutcome> for QuestionEnd {
    fn from(outcome: &ToolOutcome) -> Self {
        match *outcome {
            ToolOutcome::Answer { ref rows, .. } => Self::Answered { rows: rows.rows().len() },
            ToolOutcome::Refusal { ref reason } => Self::Refused { code: reason.code() },
        }
    }
}

impl From<&SurfaceFailure> for QuestionEnd {
    fn from(failure: &SurfaceFailure) -> Self {
        match *failure {
            SurfaceFailure::Warehouse { .. } => Self::DataSystemUnavailable,
            SurfaceFailure::Broker { .. } => Self::IdentityUnavailable,
            SurfaceFailure::Compile { .. } | SurfaceFailure::Miswired { .. } => Self::Internal,
        }
    }
}

/// Where a transport reports its questions. Counting is the implementor's business; it must not
/// block, and it cannot fail a question.
pub trait QuestionMeter: Send + Sync + std::fmt::Debug {
    /// One question waited this long for an execution slot, granted or not.
    fn waited(&self, waited: Duration);

    /// One question was turned away for want of a slot.
    fn shed(&self);

    /// One question ended, `elapsed` after it arrived.
    fn completed(&self, end: QuestionEnd, elapsed: Duration);
}

/// Observes one wait for an execution slot when dropped.
///
/// Drop and not a call, so a grant, a shed and a caller that stops waiting each end the observation
/// once.
#[derive(Debug)]
#[must_use = "the wait is observed when this is dropped, so binding it to `_` observes nothing"]
pub struct AdmissionWait<'a> {
    meter: &'a dyn QuestionMeter,
    started: Instant,
}

impl<'a> AdmissionWait<'a> {
    /// Starts timing a wait for a slot.
    pub fn start(meter: &'a dyn QuestionMeter) -> Self {
        Self {
            meter,
            started: Instant::now(),
        }
    }
}

impl Drop for AdmissionWait<'_> {
    fn drop(&mut self) {
        self.meter.waited(self.started.elapsed());
    }
}
