#![forbid(unsafe_code)]
//! Direct Google admission is optional, distinct from JWTs, and cannot widen a source's admission.
#[cfg(test)]
mod tests {
    use sutura_config::{Environment, Settings, Sources};
    const GOOGLE: &str = "security:\n  inbound:\n    - id: google\n      path: /google/mcp\n      mode: direct\n      verifier: google_tokeninfo\n      resource: https://sutura.example.com/google/mcp\n      authorization_server: https://accounts.google.com\n      client_id: dedicated.apps.googleusercontent.com\n      hosted_domains: [example.com]\n      required_scopes: [https://www.googleapis.com/auth/bigquery.readonly]\n";
    const SOURCE: &str = "sources:\n  warehouse:\n    kind: bigquery\n    posture: impersonation-at-source\n    max_bytes_billed: 1073741824\n    admitted_by: [google]\n    caller_google_token:\n      acknowledged_because: a dedicated client is used only for this deployment\n";
    fn rendered(error: &dyn std::error::Error) -> String {
        let mut message = error.to_string();
        let mut next = error.source();
        while let Some(cause) = next {
            message.push_str(&cause.to_string());
            next = cause.source();
        }
        message
    }
    #[test]
    fn a_direct_google_verifier_and_acknowledged_caller_token_source_load_together() {
        Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("{GOOGLE}{SOURCE}")))
            .expect("the caller's own Google access token is an explicit source mode");
    }
    #[test]
    fn a_caller_google_token_without_its_reason_is_refused_at_load() {
        let source = SOURCE.replace(
            "    caller_google_token:\n      acknowledged_because: a dedicated client is used only for this deployment\n",
            "    caller_google_token: {}\n",
        );
        let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("{GOOGLE}{source}")))
            .expect_err("an unacknowledged token passthrough never loads");
        let message = rendered(&error);
        assert!(
            message.contains("sources.warehouse.caller_google_token.acknowledged_because"),
            "{message}"
        );
    }
    #[test]
    fn a_caller_google_token_admitted_by_a_jwt_is_refused_as_caller_google_token_verifier() {
        let jwt = "security:\n  inbound:\n    - id: google\n      path: /google/mcp\n      mode: direct\n      resource: https://sutura.example.com/google/mcp\n      authorization_server: https://issuer.example.com\n      key_set_file: /etc/sutura/keys.json\n      algorithms: [ES256]\n";
        let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(format!("{jwt}{SOURCE}")))
            .expect_err("a JWT source cannot opt itself into Google token forwarding");
        let sutura_config::SettingsError::NotFitToServe { refusals } = error.reason() else {
            panic!("expected startup refusal: {error}")
        };
        assert!(
            refusals
                .iter()
                .any(|refusal| matches!(refusal, sutura_config::NotFitToServe::CallerGoogleTokenVerifier { .. })),
            "{refusals:?}"
        );
    }
}
