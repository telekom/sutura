//! The credential broker a served deployment that links the `BigQuery` adapter is answered
//! through - built here, out of `serve.rs`, so the composition root keeps the dispatch and the one
//! thing that is genuinely its own logic reads next to the refusals that guard it.
//!
//! The whole file is `#[cfg(feature = "bigquery")]` by construction in `serve.rs`'s `mod broker;`,
//! so nothing here is compiled into a build that links no `BigQuery` adapter.
//!
//! **This replaces the exchanging broker's builder rather than restoring it.** That one wired
//! `StsOverHttp` and `IamCredentialsOverHttp`, both deleted with the `wire` transport, so it has no
//! implementor a composition root can reach. `sutura_exec_bigquery::DeclaredPrincipalBroker` is what
//! the ADBC transport can be served through: it presents the PRINCIPAL a source declared for the
//! asking subject, and the driver becomes that principal. Its own module header states what that
//! costs relative to the exchange.
//!
//! It scans the WHOLE `sources:` registry rather than the `bigquery`-kind entries only, for the
//! reason the deleted builder did: a plan may read a shared source of one kind and an impersonating
//! one of another, and a broker is per answer. A non-`BigQuery` adapter that cannot deliver the
//! impersonating posture is already refused at its own posture cross-check, before any question.

use sutura_domain::source::SourcePosture;
use sutura_exec_bigquery::{DeclaredPrincipalBroker, DeclaredPrincipals};

/// Reads the declared sources into the broker a served question is answered through.
///
/// # Errors
///
/// Three startup refusals, and all three exist because the alternative is a deployment that boots
/// clean and refuses every impersonated question - which is the defect this whole composition was
/// rebuilt to remove:
///
/// - an impersonating source whose `impersonate` map names nobody, so no caller could ever be
///   served there;
/// - a declared target that is not a principal this adapter can name;
/// - a source declaring the pool expectations `telekom/sutura#817` added, which described a token
///   exchange no transport in this build performs.
pub(crate) fn build_broker(registry: &sutura_config::SourceRegistry) -> Result<DeclaredPrincipalBroker, String> {
    let mut broker = DeclaredPrincipalBroker::empty();
    for (alias, source) in registry.each() {
        if let Some(SourcePosture::SharedServiceUser { declared }) = source.posture() {
            broker = broker.shared(alias.clone(), declared.clone());
        }
        let Some(workload) = source.workload_identity() else {
            continue;
        };
        // **A declaration nothing in this process checks is refused at boot, not ignored at boot.**
        // The exchange DOES happen now - Google's token service performs it against the declared
        // pool - but it happens THERE, so the pool's own provider configuration is what decides
        // which issuer and which audience are acceptable. These two keys would have this deployment
        // re-state that decision and then not enforce it, which is the exact shape
        // `sutura_config`'s own `VerificationIdentityOnASharedSource` refuses.
        if workload.expected_issuer().is_some() || workload.expected_audience().is_some() {
            return Err(format!(
                "`sources.{alias}.workload_identity` declares the pool expectations \
                 `expected_issuer`/`expected_audience`, and nothing in this process checks them - \
                 the asker's own assertion is federated to the identity pool, which applies its \
                 provider's own issuer and audience conditions. Declare them on the pool's provider \
                 and remove both keys here"
            ));
        }
        // The declared subject -> service-account map, re-expressed once here into the adapter's own
        // parsed shape: an adapter may not depend on the settings tree, so every value on this path
        // is parsed again by the crate that sends it.
        let mut declared = std::collections::BTreeMap::new();
        for (subject, target) in workload.impersonate() {
            let name = sutura_domain::identity::PrincipalName::parse(target.as_str()).map_err(|cause| {
                format!("`sources.{alias}.workload_identity.impersonate` names a target this adapter cannot execute as: {cause}")
            })?;
            drop(declared.insert(subject.clone(), name));
        }
        let declared = DeclaredPrincipals::parse(declared)
            .map_err(|cause| format!("`sources.{alias}.workload_identity.impersonate` is unusable: {cause}"))?;
        broker = broker.impersonating(alias.clone(), declared);
    }
    Ok(broker)
}
