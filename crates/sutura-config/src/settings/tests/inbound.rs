//! What an inbound-identity declaration is read as, and what it refuses.
//!
//! A file of its own because `settings/tests.rs` reached the thousand-line limit
//! `cargo xtask max-lines` enforces. The seam is the one [`crate::settings::inbound`] already has in
//! the implementation, so the tests for that parse sit beside the tests for everything else rather
//! than inside them.

use std::path::PathBuf;

use super::{TOKEN, production_overlay, variables};
use crate::settings::{Environment, NotFitToServe, Settings, SettingsError, Sources};

/// A complete `direct` declaration, indented to sit under a `security:` key.
///
/// A fragment rather than a whole document, because half the tests below need it *beside*
/// `access_token` - and two `security:` keys in one YAML document is a different bug from the one
/// being tested.
const DIRECT_INBOUND: &str = "  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
                              authorization_server: \"https://issuer.example.com\"\n    \
                              key_set_file: \"/etc/sutura/jwks.json\"\n    algorithms: [\"RS256\"]\n";

/// A complete `behind-gateway` declaration, likewise.
const GATEWAY_INBOUND: &str = "  inbound:\n    mode: \"behind-gateway\"\n    transit_header: \"X-Transit-Proof\"\n    \
                               transit_issuer: \"https://gateway.example.com\"\n    \
                               transit_audience: \"https://sutura.example.com\"\n    key_set_file: \"/k.json\"\n    \
                               algorithms: [\"ES256\"]\n    transit_token_type: \"at+jwt\"\n";

#[test]
fn an_inbound_block_with_no_mode_does_not_start() {
    // THE refusal `docs/adr/0014` asks for by name. Both defaults are wrong in opposite directions -
    // `direct` makes a gateway deployment reject every caller, `behind-gateway` makes a directly
    // exposed one accept a forged proof - so a deployment that declared inbound identity and did not
    // say which kind does not run.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("security:\n  inbound:\n    resource: \"https://sutura.example.com\"\n");
    let error = Settings::load(&sources).expect_err("a block with no mode is refused");
    assert!(matches!(error, SettingsError::InboundModeUndeclared), "{error:?}");
    // The message has to say what to write, because a refusal that does not is a support request.
    let rendered = error.to_string();
    assert!(rendered.contains("direct"), "{rendered}");
    assert!(rendered.contains("behind-gateway"), "{rendered}");
    // And an unrecognised mode is its own refusal rather than a fall back to either.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay("security:\n  inbound:\n    mode: \"trusting\"\n    algorithms: [\"RS256\"]\n");
    assert!(
        matches!(
            Settings::load(&sources).expect_err("`trusting` is not a mode"),
            SettingsError::InboundModeUnknown { .. }
        ),
        "an unknown mode must not fall back to a posture"
    );
}

#[test]
fn no_inbound_block_at_all_is_a_single_player_deployment_and_still_starts() {
    // The other absence, and it is a different fact from the one above: `docs/adr/0008` part 5a calls
    // a single-identity deployment a first-class shape rather than a degraded one, and `docs/adr/0014`
    // says nothing here becomes required for the shape that ships today. So the defaults still load
    // and they still say they establish no identity.
    let settings = Settings::load(&Sources::defaults(Environment::Development)).expect("the defaults load");
    assert!(settings.security().inbound().is_none());
    assert!(!settings.security().describes_identity());
    assert_eq!(settings.security().inbound_mode(), "none");
    // Production included, which needs the deployment token and nothing more.
    let production =
        Settings::load(&Sources::defaults(Environment::Production).with_overlay(production_overlay())).expect("production loads");
    assert!(!production.security().describes_identity());
}

#[test]
fn each_mode_requires_its_own_keys_and_the_refusal_names_the_mode() {
    // A refusal saying "security.inbound.resource is required" is a support request; one saying
    // "`direct` requires it" is an answer. Asserted per mode, because the two modes require disjoint
    // sets - a rule that checked the union would refuse both correct configurations.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  inbound:\n    mode: \"direct\"\n    key_set_file: \"/k.json\"\n    algorithms: [\"RS256\"]\n",
    );
    let error = Settings::load(&sources).expect_err("`direct` needs a resource identifier");
    let SettingsError::InboundKeyMissing { key, ref mode } = error else {
        panic!("expected a missing key, got {error:?}");
    };
    assert_eq!(key, "security.inbound.resource");
    assert_eq!(mode, "direct");

    // The mirror: a gateway declaration needs its header and does NOT need a resource.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  inbound:\n    mode: \"behind-gateway\"\n    key_set_file: \"/k.json\"\n    algorithms: [\"ES256\"]\n",
    );
    let error = Settings::load(&sources).expect_err("`behind-gateway` needs a proof header");
    assert!(
        matches!(error, SettingsError::InboundKeyMissing { key, .. } if key == "security.inbound.transit_header"),
        "{error:?}"
    );
}

#[test]
fn a_complete_declaration_loads_and_derives_the_requirement_the_validator_reads() {
    // The positive side, asserted through `Settings::load` rather than by constructing the types:
    // what a reviewer needs to know is that the keys an operator writes reach the validator.
    let settings =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("security:\n{DIRECT_INBOUND}")))
            .expect("a complete direct declaration loads");
    let inbound = settings.security().inbound().expect("the declaration is there");
    assert_eq!(inbound.mode(), "direct");
    assert!(settings.security().describes_identity());
    let requirement = inbound.requirement();
    assert_eq!(requirement.audience().as_str(), "https://sutura.example.com");
    assert_eq!(requirement.issuer().as_str(), "https://issuer.example.com");
    assert_eq!(requirement.key_set().path(), PathBuf::from("/etc/sutura/jwks.json"));
    assert_eq!(requirement.algorithms().count(), 1);
    assert!(inbound.reads_the_authorization_header());

    let gateway =
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("security:\n{GATEWAY_INBOUND}")))
            .expect("a complete gateway declaration loads");
    let inbound = gateway.security().inbound().expect("the declaration is there");
    assert_eq!(inbound.mode(), "behind-gateway");
    assert!(
        !inbound.reads_the_authorization_header(),
        "a gateway proof leaves Authorization to the deployment token"
    );
    // Folded at construction, so a configured `X-Transit-Proof` matches an arriving `x-transit-proof`
    // rather than missing a header that is present.
    assert_eq!(inbound.requirement().location().to_string(), "x-transit-proof");
}

#[test]
fn a_deployment_token_and_the_direct_mode_are_refused_because_they_share_one_header() {
    // The collision found by building this rather than by reading the record. RFC 6750 puts an access
    // token in `Authorization: Bearer` and an OAuth client has no option to put it elsewhere, so a
    // deployment that is its own resource server owns that header. Every alternative to refusing is
    // worse: sniffing for a JWT is a guess, and checking one and then the other makes the WEAKER
    // credential sufficient.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(format!("security:\n  access_token: \"{TOKEN}\"\n{DIRECT_INBOUND}"));
    let error = Settings::load(&sources).expect_err("two credentials in one header is refused");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert!(
        refusals.contains(&NotFitToServe::DeploymentTokenSharesTheHeader),
        "{refusals:?}"
    );

    // And the shape that keeps both controls: a gateway proof arrives in its own header, so the
    // deployment token is untouched. This is the arrangement `docs/adr/0014` means by "both survive".
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(format!("security:\n  access_token: \"{TOKEN}\"\n{GATEWAY_INBOUND}"));
    let both = Settings::load(&sources).expect("a token and a gateway proof do not collide");
    assert!(both.security().access_token().is_some());
    assert!(both.security().describes_identity());
}

#[test]
fn a_production_deployment_that_verifies_its_callers_needs_no_deployment_token() {
    // The rule that had to change, and why it is not a weakening. The token was required because the
    // alternative was NO credential at all; a deployment that validates every caller's own token has
    // one per caller, audience-bound and expiring - strictly more than one shared secret every caller
    // holds. Without this change the two refusals are mutually unsatisfiable and a production
    // multi-user deployment in the direct mode cannot start at all.
    let sources = Sources::defaults(Environment::Production).with_overlay(format!(
        "server:\n  host: \"0.0.0.0\"\n  port: 8080\nsecurity:\n  tls_termination: \"ingress\"\n{DIRECT_INBOUND}"
    ));
    let settings = Settings::load(&sources).expect("a verifying production deployment starts");
    assert!(settings.security().access_token().is_none());
    assert!(settings.security().describes_identity());
    assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());

    // The rule it did NOT relax: production with neither is still refused, and the message says which
    // of the two absences it is complaining about.
    let error = Settings::load(
        &Sources::defaults(Environment::Production)
            .with_overlay("server:\n  host: \"0.0.0.0\"\n  port: 8080\nsecurity:\n  tls_termination: \"ingress\"\n"),
    )
    .expect_err("production with no credential at all is refused");
    let SettingsError::NotFitToServe { ref refusals } = error else {
        panic!("expected a posture refusal, got {error:?}");
    };
    assert!(
        refusals.iter().any(|refusal| matches!(
            *refusal,
            NotFitToServe::AccessTokenRequired { because } if because.contains("no inbound identity")
        )),
        "{refusals:?}"
    );
}

#[test]
fn a_pinned_algorithm_a_caller_could_forge_is_refused_through_the_whole_layering() {
    // Asserted through `Settings::load` and not only on the newtype, because what matters is that the
    // refusal survives the layering: `algorithms` is a list, and a list is exactly the shape a
    // `config` layer replaces wholesale.
    for named in ["HS256", "none"] {
        let sources = Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
             authorization_server: \"https://issuer.example.com\"\n    key_set_file: \"/k.json\"\n    \
             algorithms: [\"{named}\"]\n"
        ));
        let error = Settings::load(&sources).expect_err("a forgeable algorithm is refused");
        assert!(matches!(error, SettingsError::InboundAlgorithms { .. }), "{named}: {error:?}");
    }
    // An empty list too, so "pinned" cannot be satisfied by pinning nothing.
    let sources = Sources::defaults(Environment::Development).with_overlay(
        "security:\n  inbound:\n    mode: \"direct\"\n    resource: \"https://sutura.example.com\"\n    \
         authorization_server: \"https://issuer.example.com\"\n    key_set_file: \"/k.json\"\n    algorithms: []\n",
    );
    assert!(matches!(
        Settings::load(&sources).expect_err("pinning nothing is not pinning"),
        SettingsError::InboundAlgorithms { .. }
    ));
}

#[test]
fn an_inbound_key_can_be_set_by_a_variable_and_a_misspelled_one_is_an_error() {
    // Both halves of the layering, for a group that is new. A deployment sets its resource identifier
    // per environment, so the variable layer has to reach these keys - and `deny_unknown_fields` has
    // to still make a typo an error naming it rather than a value nobody reads.
    let sources = Sources::defaults(Environment::Development)
        .with_overlay(format!("security:\n{DIRECT_INBOUND}"))
        .with_variables(variables(&[(
            "SUTURA__SECURITY__INBOUND__RESOURCE",
            "https://other.example.com",
        )]));
    let settings = Settings::load(&sources).expect("a variable overrides the file layer");
    let inbound = settings.security().inbound().expect("the declaration is there");
    assert_eq!(inbound.requirement().audience().as_str(), "https://other.example.com");

    let sources = Sources::defaults(Environment::Development)
        .with_overlay(format!("security:\n{DIRECT_INBOUND}"))
        .with_variables(variables(&[(
            "SUTURA__SECURITY__INBOUND__RESOURSE",
            "https://typo.example.com",
        )]));
    let error = Settings::load(&sources).expect_err("a misspelled key is an error naming it");
    assert!(matches!(error, SettingsError::Source { .. }), "{error:?}");
}
