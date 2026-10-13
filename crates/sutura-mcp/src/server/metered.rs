//! Where a question over this transport is counted: into the series `sutura_http`'s router carries
//! in with each `/mcp` request, which are the serving state's own - the ones `/v1` counts into.

use std::sync::Arc;
use std::time::Duration;

use rmcp::ErrorData;
use rmcp::service::{RequestContext, RoleServer};
use sutura_app::question::{AdmissionWait, QuestionEnd, QuestionMeter};
use sutura_runtime::{Admission, AtCapacity, Slot};

/// The permit set a question waits on, and the series its wait, shed and end are counted into.
#[derive(Clone, Copy)]
pub(super) struct Turn<'a> {
    pub(super) admission: &'a Admission,
    pub(super) meter: &'a dyn QuestionMeter,
}

/// Counts nothing: standard input and output has no request and no scrape endpoint.
#[derive(Debug)]
struct NoScrapeEndpoint;

impl QuestionMeter for NoScrapeEndpoint {
    fn waited(&self, _waited: Duration) {}

    fn shed(&self) {}

    fn completed(&self, _end: QuestionEnd, _elapsed: Duration) {}
}

impl<'a> Turn<'a> {
    /// `admission`, counted into the series this call's HTTP request carried. An HTTP request that
    /// carried none is refused rather than counted nowhere; a call with no request counts nothing.
    pub(super) fn of(admission: &'a Admission, context: &'a RequestContext<RoleServer>) -> Result<Self, ErrorData> {
        let meter: &dyn QuestionMeter = match context.extensions.get::<http::request::Parts>() {
            None => &NoScrapeEndpoint,
            Some(parts) => &**parts
                .extensions
                .get::<Arc<dyn QuestionMeter>>()
                .ok_or_else(|| ErrorData::internal_error("this request carries no question meter", None))?,
        };
        Ok(Self { admission, meter })
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
            meter: &NoScrapeEndpoint,
        }
    }
}
