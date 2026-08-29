//! Turning a written inbound-identity declaration into one of the two modes.
//!
//! A file of its own for a mechanical reason and not a conceptual one: `settings.rs` reached the
//! thousand-line limit `cargo xtask max-lines` enforces, and this is the newest and most separable
//! group in it. Everything here is private to [`crate::settings`]; the types being produced live in
//! [`crate::inbound`], and the refusals it produces are [`crate::settings::SettingsError`] variants.

use crate::inbound::{
    InboundIdentity, IssuerUrl, KeySetFile, PinnedAlgorithms, ProofHeader, ProofLifetime, RequiredTokenType, ResourceIdentifier,
    TokenType, TransitProof,
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
    let key_set = KeySetFile::parse(required(written.key_set_file.as_deref(), KeySetFile::KEY, mode)?)
        .map_err(|cause| SettingsError::InboundValue { cause })?;
    let algorithms = PinnedAlgorithms::parse(&written.algorithms).map_err(|cause| SettingsError::InboundAlgorithms { cause })?;
    match mode {
        "behind-gateway" => Ok(InboundIdentity::BehindGateway {
            transit: TransitProof::new(
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
                    Some(seconds) => ProofLifetime::parse(seconds).map_err(|cause| SettingsError::InboundValue { cause })?,
                },
            ),
        }),
        // `direct` and nothing else, because the match above already refused every other spelling.
        // Written as the fallback arm rather than as `"direct" =>` plus an unreachable one, so there
        // is no arm here that a test cannot provoke.
        _ => Ok(InboundIdentity::Direct {
            resource: ResourceIdentifier::parse(required(written.resource.as_deref(), ResourceIdentifier::KEY, mode)?)
                .map_err(|cause| SettingsError::InboundValue { cause })?,
            authorization_server: IssuerUrl::parse(required(written.authorization_server.as_deref(), IssuerUrl::KEY, mode)?)
                .map_err(|cause| SettingsError::InboundValue { cause })?,
            key_set,
            algorithms,
            // Absent means RFC 9068's `at+jwt`, which is the safe reading, and an empty string is
            // treated as absent the way `security.access_token` treats one: an unset variable arrives
            // in a shell as `""`, and reading that as "check nothing" would be the check switching
            // itself off. Turning it off is the word `any`.
            token_type: match written.token_type.as_deref().map(str::trim) {
                None | Some("") => RequiredTokenType::access_token(),
                Some(written) => {
                    RequiredTokenType::parse(TokenType::KEY, written).map_err(|cause| SettingsError::InboundValue { cause })?
                }
            },
        }),
    }
}

/// The key this mode needs and did not get.
///
/// One helper rather than six `ok_or_else` calls, so the refusal for a missing key says the same
/// thing whichever key it was - and so the *mode* is in the message. "security.inbound.resource is
/// required" is a support request; "`direct` requires it" is an answer.
///
/// An empty value counts as absent here, which is the opposite of what
/// [`crate::inbound::primitive`]'s parsers do with one - and both are right for their position. An
/// unset variable arrives in a shell as `""`, so at *this* layer the accurate diagnostic is "the mode
/// requires this key"; a value that is present and empty past a `required` call has come from
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
