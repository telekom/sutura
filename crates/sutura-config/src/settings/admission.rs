//! Startup admission checks over the same declarations consumed by routing and brokers.

use super::NotFitToServe;
use crate::SourcePlacement;
use crate::inbound::{InboundIdentity, InboundVerifier};
use crate::sources::{ConfiguredSource, SourceRegistry};

pub(super) fn verifier_refusals(inbound: &[InboundVerifier]) -> Vec<NotFitToServe> {
    let mut refusals = Vec::new();
    for (index, later) in inbound.iter().enumerate() {
        let requirement = later.requirement();
        let direct = matches!(later.identity(), InboundIdentity::Direct { .. });
        if direct && requirement.audience().as_str() == requirement.issuer().as_str() {
            refusals.push(NotFitToServe::InboundAudienceIsTheIssuer);
        }
        if let Some(path) = later.metadata_path()
            && let Some(resource) = requirement.resource()
            && resource
                .as_str()
                .split_once("://")
                .and_then(|(_, authority)| authority.get(authority.find('/')?..))
                != Some(path.as_str())
        {
            refusals.push(NotFitToServe::InboundResourcePathMismatch {
                id: later.id().clone(),
                path: path.clone(),
            });
        }
        if [
            "/v1",
            "/health",
            "/metrics",
            "/openapi.json",
            "/docs",
            "/.well-known",
            "/arrow.flight.protocol.FlightService",
            "/worker.WorkerService",
        ]
        .iter()
        .any(|reserved| later.path().overlaps(reserved))
        {
            refusals.push(NotFitToServe::ReservedMcpPath {
                id: later.id().clone(),
                path: later.path().clone(),
            });
        }
        for earlier in inbound.iter().take(index) {
            let earlier_requirement = earlier.requirement();
            if later.id() == earlier.id() {
                refusals.push(NotFitToServe::DuplicateVerifierId { id: later.id().clone() });
            }
            if requirement.issuer() == earlier_requirement.issuer() {
                refusals.push(NotFitToServe::DuplicateVerifierIssuer {
                    earlier: earlier.id().clone(),
                    later: later.id().clone(),
                    issuer: requirement.issuer().as_str().to_owned(),
                });
            }
            if requirement.audience().as_str() == earlier_requirement.audience().as_str() {
                refusals.push(NotFitToServe::DuplicateVerifierAudience {
                    earlier: earlier.id().clone(),
                    later: later.id().clone(),
                    audience: requirement.audience().as_str().to_owned(),
                });
            }
            if later.path() == earlier.path() {
                if direct && matches!(earlier.identity(), InboundIdentity::Direct { .. }) {
                    refusals.push(NotFitToServe::DirectVerifiersSharePath {
                        earlier: earlier.id().clone(),
                        later: later.id().clone(),
                        path: later.path().clone(),
                    });
                }
            } else if later.path().overlaps(earlier.path().as_str()) {
                refusals.push(NotFitToServe::OverlappingMcpPaths {
                    earlier: earlier.path().clone(),
                    later: later.path().clone(),
                });
            }
        }
    }
    refusals
}

/// The declared delegation keys, across every source kind carrying such a list.
pub(super) fn delegation_key(source: &ConfiguredSource) -> Option<&'static str> {
    if source.workload_identity().is_some_and(|wif| !wif.delegation().is_empty()) {
        Some("workload_identity.delegation")
    } else if matches!(source.placement(), SourcePlacement::Oracle { delegation, .. } | SourcePlacement::Cube { delegation, .. } | SourcePlacement::Node { delegation, .. } | SourcePlacement::ClickHouse { delegation, .. } if !delegation.is_empty())
    {
        Some("delegation")
    } else {
        None
    }
}

pub(super) fn source_refusals(inbound: &[InboundVerifier], sources: &SourceRegistry) -> Vec<NotFitToServe> {
    let mut refusals = Vec::new();
    for (alias, source) in sources.each() {
        if inbound.len() > 1 && source.admitted_by().is_none() {
            refusals.push(NotFitToServe::SourceVerifiersUndeclared { alias: alias.clone() });
        }
        let delegation = delegation_key(source).is_some();
        let caller_run = matches!(
            source.posture(),
            Some(sutura_domain::source::SourcePosture::ImpersonationAtSource)
        );
        let mut direct = 0_usize;
        for id in source.admitted_by().into_iter().flatten() {
            match inbound.iter().find(|verifier| verifier.id() == id) {
                None => refusals.push(NotFitToServe::UnknownAdmittedVerifier {
                    alias: alias.clone(),
                    verifier: id.clone(),
                }),
                Some(verifier) if matches!(verifier.identity(), InboundIdentity::Direct { .. }) => {
                    direct = direct.saturating_add(1);
                }
                Some(_) if caller_run => refusals.push(NotFitToServe::CallerSourceAdmitsGateway {
                    alias: alias.clone(),
                    verifier: id.clone(),
                }),
                Some(_) => {}
            }
        }
        if delegation && !inbound.is_empty() && direct != 1 {
            refusals.push(NotFitToServe::DelegationVerifierCardinality {
                alias: alias.clone(),
                found: direct,
            });
        }
    }
    refusals
}

#[cfg(test)]
mod tests;
