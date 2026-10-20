#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::path::Path;

    use sutura_config::{Environment, KeySetLocation, Settings, SettingsError, Sources};

    const ISSUER: &str = "https://gateway.test";
    const AUDIENCE: &str = "/sutura";

    #[test]
    fn the_tardis_example_loads_once_its_placeholders_are_filled() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tardis/conf/base.yaml");
        let written = std::fs::read_to_string(&path).expect("the example's settings are readable");
        for placeholder in ["<gateway-issuer>", "<audience>"] {
            assert!(
                written.contains(placeholder),
                "{} no longer holds `{placeholder}`",
                path.display()
            );
        }
        let filled = written.replace("<gateway-issuer>", ISSUER).replace("<audience>", AUDIENCE);

        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(filled))
            .expect("the example's settings load");
        let inbound = settings.security().inbound().expect("the example declares inbound identity");
        assert_eq!(inbound.mode(), "behind-gateway");
        assert!(inbound.reads_the_authorization_header());
        let requirement = inbound.requirement();
        assert_eq!(requirement.issuer().as_str(), ISSUER);
        assert_eq!(requirement.audience().as_str(), AUDIENCE);
        assert_eq!(requirement.token_type().as_str(), "jwt");
        assert!(!requirement.single_use());
        let grant = requirement
            .grant()
            .expect("the example grants scopes to an assertion with none");
        assert!(
            grant.contains("sutura:catalog.read") && grant.contains("sutura:metrics.ask"),
            "{grant:?}"
        );
        assert!(
            matches!(requirement.key_set(), KeySetLocation::File(file) if file.path() == Path::new("/etc/sutura/keys/gateway-jwks.json")),
            "{:?}",
            requirement.key_set()
        );
    }

    #[test]
    fn the_tardis_example_is_refused_while_its_placeholders_remain() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/tardis/conf/base.yaml");
        let written = std::fs::read_to_string(path).expect("the example's settings are readable");
        let refused = Settings::load(&Sources::defaults(Environment::Development).with_overlay(written))
            .expect_err("a placeholder is not an issuer");
        assert!(matches!(refused.reason(), SettingsError::InboundValue { .. }), "{refused:?}");
    }
}
