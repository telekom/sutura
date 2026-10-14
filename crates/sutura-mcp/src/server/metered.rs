//! Where a question over this transport is counted: into the series `sutura_http`'s router carries
//! in with each `/mcp` request, which are the serving state's own - the ones `/v1` counts into.

use std::sync::Arc;
use std::time::Duration;

use rmcp::service::{RequestContext, RoleServer};
use sutura_app::question::{AdmissionWait, QuestionEnd, QuestionMeter};
use sutura_runtime::{Admission, AtCapacity, Slot};

/// The permit set a question waits on, and the series its wait, shed and end are counted into.
#[derive(Clone, Copy)]
pub(super) struct Turn<'a> {
    pub(super) admission: &'a Admission,
    pub(super) meter: &'a dyn QuestionMeter,
}

/// What a call that carries no meter counts into: nothing.
#[derive(Debug)]
struct Uncounted;

impl QuestionMeter for Uncounted {
    fn waited(&self, _waited: Duration) {}

    fn shed(&self) {}

    fn completed(&self, _end: QuestionEnd, _elapsed: Duration) {}
}

impl<'a> Turn<'a> {
    /// `admission`, counted into the meter this call's HTTP request carried. A call carrying none -
    /// standard input and output, or a request not built by `sutura_http`'s agent mount, which adds
    /// the meter where it establishes the caller - counts nothing. Admission alone decides a shed.
    pub(super) fn of(admission: &'a Admission, context: &'a RequestContext<RoleServer>) -> Self {
        let meter = context
            .extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Arc<dyn QuestionMeter>>())
            .map_or(&Uncounted as &dyn QuestionMeter, |meter| &**meter);
        Self { admission, meter }
    }

    /// A slot, its wait observed however this future ends and a shed counted: the one place
    /// questions and raw statements wait for a turn, so neither waits unseen.
    pub(super) async fn admit(self) -> Result<Slot, AtCapacity> {
        let _wait = AdmissionWait::start(self.meter);
        self.admission.admit().await.inspect_err(|_shed| self.meter.shed())
    }
}

#[cfg(test)]
impl<'a> From<&'a Admission> for Turn<'a> {
    fn from(admission: &'a Admission) -> Self {
        Self {
            admission,
            meter: &Uncounted,
        }
    }
}
