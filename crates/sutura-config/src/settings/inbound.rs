//! Turning a written inbound-identity declaration into one of the two modes.
//!
//! A file of its own for a mechanical reason and not a conceptual one: `settings.rs` reached the
//! thousand-line limit `cargo xtask max-lines` enforces, and this is the newest and most separable
//! group in it. Everything here is private to [`crate::settings`]; the types being produced live in
//! [`crate::inbound`], and the refusals it produces are [`crate::settings::SettingsError`] variants.

use crate::inbound::{
    ExtraScopeClaims, GrantedScopes, InboundIdentity, IssuerUrl, KeySetFile, KeySetLocation, KeySetMaxStale, KeySetUrl,
    PinnedAlgorithms, ProofHeader, ProofLifetime, RequiredTokenType, ResourceIdentifier, TokenAudience, TokenType, TransitProof,
};
use crate::raw::RawInbound;
use crate::settings::SettingsError;

/// Turns an inbound-identity declaration into one of the two modes, or refuses it naming a key.
///
/// **A parse and not a combination check, deliberately.** Every mode-dependent absence here could
/// have been a `NotFitToServe` variant instead - the values are each individually fine and it is the
/// combination that is wrong, which is [`crate::settings`]'s own rule for what belongs there. It is a
/// parse anyway because [`InboundIdentity::Direct`] **cannot be constructed** without a resource
/// identifier: preferring unrepresentable to checked is the stronger of the two, and it means no
/// later caller can hold a declaration whose mode and fields disagree.
///
/// The order inside is the diagnostic and it was got wrong once, which is why it is written down: the
/// mode is read and **recognised** before any field the mode would require is looked at. An operator
/// who wrote `mode: trusting` is told that `trusting` is not a mode, rather than being told about a
/// key that a mode they did not name would have needed.
pub(super) fn parse_inbound(written: &RawInbound) -> Result<InboundIdentity, SettingsError> {
    let mode = match written.mode.as_deref().map(str::trim) {
        None | Some("") => return Err(SettingsError::InboundModeUndeclared),
        Some("direct") => "direct",
        Some("behind-gateway") => "behind-gateway",
        Some(other) => {
            return Err(SettingsError::InboundModeUnknown {
                found: String::from(other),
            });
        }
    };
    // Read before the branch, because both modes need both: one key set and one pinned family,
    // whoever authenticated the caller.
    let key_set = key_set(written, mode)?;
    let algorithms = PinnedAlgorithms::parse(&written.algorithms).map_err(|cause| SettingsError::InboundAlgorithms { cause })?;
    match mode {
        "behind-gateway" => {
            read_only_in(written.audience.is_some(), TokenAudience::KEY, "direct")?;
            read_only_in(written.extra_scope_claims.is_some(), ExtraScopeClaims::KEY, "direct")?;
            let transit = TransitProof::new(
                ProofHeader::parse(required(written.transit_header.as_deref(), ProofHeader::KEY, mode)?)
                    .map_err(|cause| SettingsError::InboundValue { cause })?,
                IssuerUrl::parse_transit_issuer(required(
                    written.transit_issuer.as_deref(),
                    "security.inbound.transit_issuer",
                    mode,
                )?)
                .map_err(|cause| SettingsError::InboundValue { cause })?,
                ResourceIdentifier::parse_transit_audience(required(
                    written.transit_audience.as_deref(),
                    "security.inbound.transit_audience",
                    mode,
                )?)
                .map_err(|cause| SettingsError::InboundValue { cause })?,
                key_set,
                algorithms,
                // REQUIRED here and defaulted in `direct`, and the asymmetry is the point: RFC 9068
                // fixes what an access token's `typ` is, and nothing fixes what a fronting
                // component's is. A guess would either refuse every request or check nothing.
                RequiredTokenType::parse(
                    "security.inbound.transit_token_type",
                    required(
                        written.transit_token_type.as_deref(),
                        "security.inbound.transit_token_type",
                        mode,
                    )?,
                )
                .map_err(|cause| SettingsError::InboundValue { cause })?,
                match written.transit_max_lifetime_seconds {
                    None => ProofLifetime::default_lifetime(),
                    Some(seconds) => transit_lifetime(seconds, written.accept_long_transit_lifetime)?,
                },
            )
            .admitting_once(written.transit_single_use);
            Ok(InboundIdentity::BehindGateway {
                transit: match written.granted_scopes.as_deref() {
                    None => transit,
                    Some(scopes) => transit.granting_when_absent(grant(scopes)?),
                },
            })
        }
        // `direct` and nothing else, because the match above already refused every other spelling.
        // Written as the fallback arm rather than as `"direct" =>` plus an unreachable one, so there
        // is no arm here that a test cannot provoke.
        _ => Ok(InboundIdentity::Direct {
            resource: direct_resource(written, mode)?,
            authorization_server: IssuerUrl::parse(required(written.authorization_server.as_deref(), IssuerUrl::KEY, mode)?)
                .map_err(|cause| SettingsError::InboundValue { cause })?,
            key_set,
            algorithms,
            // Absent means RFC 9068's `at+jwt`, which is the safe reading, and an empty string is
            // treated as absent the way `security.access_token` treats one: an unset variable arrives
            // in a shell as `""`, and reading that as "check nothing" would be the check switching
            // itself off. Turning it off is the word `any`, and it does not start unless the operator
            // also accepts it by name.
            token_type: match written.token_type.as_deref().map(str::trim) {
                None | Some("") => RequiredTokenType::access_token(),
                Some(value) => match RequiredTokenType::parse(TokenType::KEY, value)
                    .map_err(|cause| SettingsError::InboundValue { cause })?
                {
                    RequiredTokenType::Any if !written.accept_any_token_type => {
                        return Err(SettingsError::InboundAnyTokenTypeNotAccepted);
                    }
                    parsed => parsed,
                },
            },
        }),
    }
}

/// The resource identifier, with the audience and the extra scope claims `direct` reads beside it.
fn direct_resource(written: &RawInbound, mode: &str) -> Result<ResourceIdentifier, SettingsError> {
    read_only_in(written.granted_scopes.is_some(), GrantedScopes::KEY, "behind-gateway")?;
    // An access token is reused across requests by design, so single use is not offered here.
    read_only_in(written.transit_single_use, TransitProof::SINGLE_USE_KEY, "behind-gateway")?;
    let value = |cause| SettingsError::InboundValue { cause };
    let mut resource =
        ResourceIdentifier::parse(required(written.resource.as_deref(), ResourceIdentifier::KEY, mode)?).map_err(value)?;
    if let Some(audience) = written.audience.as_deref() {
        resource = resource.minted_as(TokenAudience::parse(audience).map_err(value)?);
    }
    if let Some(claims) = written.extra_scope_claims.as_deref() {
        resource = resource.reading_scopes_from(ExtraScopeClaims::parse(claims).map_err(value)?);
    }
    Ok(resource)
}

/// A written grant, refused where it names raw SQL.
fn grant(written: &[String]) -> Result<GrantedScopes, SettingsError> {
    let grant = GrantedScopes::parse(written).map_err(|cause| SettingsError::InboundValue { cause })?;
    if grant.contains("sutura:sql.run") {
        return Err(SettingsError::RawSqlGranted);
    }
    Ok(grant)
}

/// Exactly one key set source, with its ceiling. A blank value counts as absent, for the reason
/// [`required`] gives.
fn key_set(written: &RawInbound, mode: &str) -> Result<KeySetLocation, SettingsError> {
    fn given(value: Option<&str>) -> Option<&str> {
        value.map(str::trim).filter(|value| !value.is_empty())
    }
    let value = |cause| SettingsError::InboundValue { cause };
    let parsed = match (given(written.key_set_file.as_deref()), given(written.key_set_url.as_deref())) {
        (Some(_), Some(_)) => return Err(SettingsError::InboundKeySetTwice),
        (None, Some(url)) => KeySetUrl::parse(url).map(KeySetLocation::Url),
        (file, None) => KeySetFile::parse(required(file, KeySetFile::KEY, mode)?).map(KeySetLocation::File),
    };
    let max_stale = written
        .key_set_max_stale_seconds
        .map_or(Ok(KeySetMaxStale::default_ceiling()), KeySetMaxStale::parse)
        .map_err(value)?;
    let mut location = parsed.map_err(value)?;
    location.set_max_stale(max_stale);
    Ok(location)
}

/// Refuses a key written under a mode that does not read it, naming the mode that does.
const fn read_only_in(written: bool, key: &'static str, mode: &'static str) -> Result<(), SettingsError> {
    if written {
        return Err(SettingsError::InboundKeyNotForMode { key, mode });
    }
    Ok(())
}

/// A written transit lifetime: inside the ceiling, and past [`ProofLifetime::SHORT_LIVED_SECONDS`]
/// only with the operator's acceptance by name.
fn transit_lifetime(seconds: u64, accepted: bool) -> Result<ProofLifetime, SettingsError> {
    let lifetime = ProofLifetime::parse(seconds).map_err(|cause| SettingsError::InboundValue { cause })?;
    if seconds > ProofLifetime::SHORT_LIVED_SECONDS && !accepted {
        return Err(SettingsError::TransitLifetimeNotAccepted {
            found: seconds,
            limit: ProofLifetime::SHORT_LIVED_SECONDS,
        });
    }
    Ok(lifetime)
}

/// The key this mode needs and did not get.
///
/// One helper rather than six `ok_or_else` calls, so the refusal for a missing key says the same
/// thing whichever key it was - and so the *mode* is in the message. "security.inbound.resource is
/// required" is a support request; "`direct` requires it" is an answer.
///
/// An empty value counts as absent here, which is the opposite of what
/// `crate::inbound::primitive`'s parsers do with one - and both are right for their position. An
/// unset variable arrives in a shell as `""`, so at *this* layer the accurate diagnostic is "the
/// mode requires this key"; a value that is present and empty past a `required` call has come from
/// somewhere else and gets the empty-value refusal instead.
fn required<'written>(value: Option<&'written str>, key: &'static str, mode: &str) -> Result<&'written str, SettingsError> {
    match value.map(str::trim) {
        None | Some("") => Err(SettingsError::InboundKeyMissing {
            key,
            mode: String::from(mode),
        }),
        Some(present) => Ok(present),
    }
}
