//! Resolved source admission used by the concrete brokers and reading-worker composition.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::identity::{RequestContext, SourceSet, Subject, VerifierId};
use sutura_domain::model::SourceName;

use crate::sources::SourceRegistry;

/// A snapshot of configured source membership. Missing sources admit nobody.
#[derive(Debug, Clone, Default)]
pub struct SourceAdmissions(Admissions);

type Admissions = BTreeMap<SourceName, Option<BTreeSet<VerifierId>>>;

impl SourceAdmissions {
    #[must_use]
    pub fn from_registry(registry: &SourceRegistry) -> Self {
        Self(
            registry
                .each()
                .map(|(name, source)| (name.clone(), source.admitted_by().cloned()))
                .collect(),
        )
    }

    /// A source declared in code, with explicit verifier membership or deployment-local admission.
    #[must_use]
    pub fn and(mut self, source: SourceName, admitted_by: Option<BTreeSet<VerifierId>>) -> Self {
        drop(self.0.insert(source, admitted_by));
        self
    }

    /// A worker trusts the authenticated granting node's verifier id, not a fresh human assertion.
    #[must_use]
    pub fn admits_verifier(&self, source: &SourceName, verifier: &VerifierId) -> bool {
        self.0
            .get(source)
            .and_then(Option::as_ref)
            .is_some_and(|ids| ids.contains(verifier))
    }

    pub(super) fn refused(&self, context: &RequestContext, sources: &SourceSet) -> Option<SourceName> {
        let subject = context.chain().subject();
        let consistent = subject.verifier_id() == context.verifier().map(sutura_domain::identity::AdmittingVerifier::id);
        sources
            .iter()
            .find(|source| {
                !consistent
                    || match subject {
                        Subject::TheDeploymentItself => !matches!(self.0.get(*source), Some(None)),
                        Subject::Verified(_) => subject.verifier_id().is_none_or(|id| !self.admits_verifier(source, id)),
                    }
            })
            .cloned()
    }
}
