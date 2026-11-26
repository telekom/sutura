//! Startup admission checks over the same declarations consumed by routing and brokers.

use super::NotFitToServe;
use crate::SourcePlacement;
use crate::inbound::InboundVerifier;
use crate::sources::{ConfiguredSource, SourceRegistry};

pub(super) fn verifier_refusals(inbound: &[InboundVerifier]) -> Vec<NotFitToServe> {
    let mut refusals = Vec::new();
    for (index, later) in inbound.iter().enumerate() {
        let requirement = later;
        let direct = later.is_direct();
        if direct
            && requirement
                .audience()
                .is_some_and(|audience| audience.as_str() == requirement.issuer().as_str())
        {
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
            let earlier_requirement = earlier;
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
            if let (Some(audience), Some(earlier_audience)) = (requirement.audience(), earlier_requirement.audience())
                && audience.as_str() == earlier_audience.as_str()
            {
                refusals.push(NotFitToServe::DuplicateVerifierAudience {
                    earlier: earlier.id().clone(),
                    later: later.id().clone(),
                    audience: audience.as_str().to_owned(),
                });
            }
            if later.path() == earlier.path() {
                if direct && earlier.is_direct() {
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
        if let Some(target_id) = later.requires_gateway() {
            if let Some(header) = later.token_header()
                && header.is_authorization()
            {
                refusals.push(NotFitToServe::RequiresGatewayHeaderCollision { id: later.id().clone() });
            }
            if let Some(header) = later.token_header() {
                for other in inbound.iter().filter(|candidate| candidate.id() != later.id()) {
                    let collides = other
                        .requirement()
                        .is_some_and(|requirement| matches!(requirement.location(), crate::inbound::TokenLocation::Header { name } if name == header));
                    if collides {
                        refusals.push(NotFitToServe::RequiresGatewayTokenHeaderCollidesWithVerifier {
                            id: later.id().clone(),
                            other: other.id().clone(),
                        });
                    }
                }
            }
            match inbound.iter().find(|verifier| verifier.id() == target_id) {
                None => refusals.push(NotFitToServe::RequiresGatewayUnknown {
                    id: later.id().clone(),
                    target: target_id.clone(),
                }),
                Some(target) if target.is_direct() => refusals.push(NotFitToServe::RequiresGatewayNotBehindGateway {
                    id: later.id().clone(),
                    target: target.id().clone(),
                }),
                Some(target) if !target.reads_the_authorization_header() => {
                    refusals.push(NotFitToServe::RequiresGatewayNotInAuthorization {
                        id: later.id().clone(),
                        target: target.id().clone(),
                    });
                }
                Some(_) => {}
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
        if matches!(
            source.caller_identity(),
            Some(crate::sources::caller_identity::CallerIdentity::CallerGoogleToken(_))
        ) && source.admitted_by().is_none_or(|ids| {
            ids.is_empty()
                || ids.iter().any(|id| {
                    !inbound
                        .iter()
                        .any(|verifier| verifier.id() == id && verifier.google().is_some())
                })
        }) {
            refusals.push(NotFitToServe::CallerGoogleTokenVerifier { alias: alias.clone() });
        }
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
                Some(verifier) if verifier.is_direct() => {
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
