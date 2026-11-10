#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use sutura_config::{Environment, LogFilter, LogFormat, ServiceName, Settings, Sources, TelemetrySettings};

    #[test]
    fn native_oracle_startup_names_the_weaker_channel_and_independent_password_opt_in() {
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(
            "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: oracle\n    host: db.example.com\n    port: 1521\n    service_name: FREEPDB1\n    user: reader\n    password_file: /run/secrets/oracle\n    posture: shared-service-user\n    transport_mode: native-encryption\n    native_encryption_acknowledged_because: this listener requires native encryption\n    legacy_password_verifier: true\n    legacy_password_verifier_acknowledged_because: this account cannot be reset\n",
        )).expect("the acknowledged source loads");
        let capture = sutura_runtime::testing::Capture::new();
        let telemetry = TelemetrySettings::new(
            ServiceName::parse("sutura-test").expect("a test service"),
            LogFilter::parse("info").expect("a test directive"),
            LogFormat::Bunyan,
            true,
        );
        let subscriber = sutura_runtime::telemetry::subscriber(&telemetry, capture.clone()).expect("a subscriber");
        tracing::subscriber::with_default(subscriber, || sutura_runtime::banner::announce(&settings));
        let events: Vec<Value> = capture
            .contents()
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON log event"))
            .collect();
        let channel = events
            .iter()
            .find(|event| event["msg"] == "source channel" && event["source"] == "warehouse")
            .expect("source channel record");
        assert_eq!(channel["channel"], "native-encryption");
        assert_eq!(channel["anchors"], "none");
        let native = events
            .iter()
            .find(|event| event["source"] == "warehouse" && event["level"] == 40 && event["channel"] == "native-encryption")
            .expect("native encryption WARN");
        assert_eq!(native["reason"], "this listener requires native encryption");
        let message = native["msg"].as_str().expect("warning text");
        assert!(message.contains("unauthenticated DH"), "{message}");
        assert!(message.contains("server identity is not verified"), "{message}");
        assert!(message.contains("one shared user"), "{message}");
        let legacy = events
            .iter()
            .find(|event| event["source"] == "warehouse" && event["level"] == 40 && event["reason"] == "this account cannot be reset")
            .expect("independent legacy-password WARN");
        assert_eq!(
            legacy["msg"],
            "Oracle legacy password verifiers enabled on the declared encrypted channel"
        );
    }
}
