//! The federation provider one source and one subject give the plan cutter.
//!
//! `datafusion-federation` fuses two scans into one cut node when their providers are equal, and
//! two providers are equal when their [`name`](datafusion_federation::FederationProvider::name)
//! and [`compute_context`](datafusion_federation::FederationProvider::compute_context) are. So the
//! context is [`ComputeContext::of`](sutura_domain::identity::ComputeContext::of) the source and
//! the subject: one caller's scans of one source may fuse, two callers' never do.

use std::sync::Arc;

use datafusion::optimizer::Optimizer;
use datafusion_federation::FederationProvider;
use sutura_domain::identity::{ComputeContext, Subject};
use sutura_domain::model::SourceName;

/// The provider for `subject`'s scans of `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProvider {
    source: SourceName,
    context: ComputeContext,
}

impl SourceProvider {
    /// The provider for `subject` asking of `source`.
    #[must_use]
    pub fn of(source: &SourceName, subject: &Subject) -> Self {
        Self {
            source: source.clone(),
            context: ComputeContext::of(source, subject),
        }
    }
}

impl FederationProvider for SourceProvider {
    fn name(&self) -> &str {
        self.source.as_str()
    }

    fn compute_context(&self) -> Option<String> {
        Some(self.context.as_str().to_owned())
    }

    /// No optimizer of its own: the cut is the workspace's rule, not a provider's.
    fn optimizer(&self) -> Option<Arc<Optimizer>> {
        None
    }
}
