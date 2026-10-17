//! The deployment's identity fixtures: who this deployment is, who mints its tokens, and the
//! declarations a test builds from them.
//!
//! Split out of `testing.rs` for that file's own `max-lines` reason.

/// What this deployment calls itself. The value an `aud` claim has to equal, byte for byte.
pub(crate) const RESOURCE: &str = "https://sutura.example.com";
/// Who mints tokens for it.
pub(crate) const ISSUER: &str = "https://issuer.example.com";
/// The key id the issuer publishes.
pub(crate) const KID: &str = "the-current-key";

/// A mock authorization server under this deployment's own names, holding one `P-256` key.
///
/// `sutura_dev::issuer` and not a fixture of this crate's own, which is the whole point of that
/// module: leg 1 is verified in the transport, minted-for in a broker and composed in a root, and a
/// fixture inside any one of those three cannot be driven from the other two.
pub(crate) fn an_issuer() -> sutura_dev::issuer::MockIssuer {
    sutura_dev::issuer::MockIssuer::generating(ISSUER, RESOURCE, KID).expect("a mock issuer generates a key pair")
}

/// Every scope this surface has, space-delimited per RFC 6749.
///
/// Read off `sutura_app::Capability` rather than written out, so a new capability widens what a test
/// token grants instead of leaving one route quietly unreachable.
pub(crate) fn every_scope() -> String {
    sutura_app::Capability::every()
        .map(sutura_app::Capability::scope)
        .collect::<Vec<&str>>()
        .join(" ")
}

/// A token this deployment would accept, granting every capability the surface has.
///
/// Every capability, because leg 1 says who is asking and the capability gate says what they may
/// invoke: a token with no `scope` claim reaches a handler for nothing.
pub(crate) fn accepted_by(subject: &str) -> sutura_dev::issuer::Token {
    sutura_dev::issuer::Token::for_subject(subject).granting(&every_scope())
}

/// A `direct` inbound declaration for `issuer`, reading its key set at `key_set_path`.
///
/// **Built from the issuer rather than from constants**, which is a property worth having and not
/// only less repetition: the deployment is configured with the issuer under test's own names, so a
/// test cannot verify against an issuer it did not configure.
pub(crate) fn direct_overlay(issuer: &sutura_dev::issuer::MockIssuer, key_set_path: &str) -> String {
    format!(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
         authorization_server: \"{}\"\n    key_set_file: \"{key_set_path}\"\n    algorithms: [\"ES256\"]\n",
        issuer.audience(),
        issuer.issuer(),
    )
}
