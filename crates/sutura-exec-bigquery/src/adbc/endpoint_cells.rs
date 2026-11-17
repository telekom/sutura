//! The common database-option path is used by jobs and partition reads.

use adbc_core::options::{OptionDatabase, OptionValue};

use super::{GoogleEndpoints, database_options_at};
use crate::transport::ProjectId;

#[test]
fn every_database_receives_the_declared_google_destinations_and_trust() {
    let endpoints = GoogleEndpoints::new(
        "https://bigquery-example.p.googleapis.com/bigquery/v2/",
        "https://bigquerystorage-example.p.googleapis.com:443/",
        "https://iamcredentials-example.p.googleapis.com/",
        "https://oauth2-example.p.googleapis.com/token",
        "https://sts-example.p.googleapis.com/v1/token",
    )
    .with_trust(
        Some("/etc/sutura/google-roots.pem".into()),
        Some(("/etc/sutura/google-client.pem".into(), "/etc/sutura/google-client.key".into())),
    );
    let billing = ProjectId::parse("test-project").expect("fixture project");
    let identity = vec![(
        OptionDatabase::Other("bigquery.auth.access_token".into()),
        OptionValue::String("caller-token".into()),
    )];
    let options = database_options_at(&billing, identity, Some(&endpoints));
    for (key, value) in [
        ("bigquery.endpoint", "https://bigquery-example.p.googleapis.com/bigquery/v2/"),
        ("bigquery.storage_endpoint", "bigquerystorage-example.p.googleapis.com:443"),
        (
            "bigquery.iam_credentials_endpoint",
            "https://iamcredentials-example.p.googleapis.com/",
        ),
        (
            "bigquery.oauth_token_endpoint",
            "https://oauth2-example.p.googleapis.com/token",
        ),
        ("bigquery.sts_endpoint", "https://sts-example.p.googleapis.com/v1/token"),
        ("bigquery.tls.ca_file", "/etc/sutura/google-roots.pem"),
        ("bigquery.tls.cert_file", "/etc/sutura/google-client.pem"),
        ("bigquery.tls.key_file", "/etc/sutura/google-client.key"),
        ("bigquery.auth.access_token", "caller-token"),
    ] {
        assert!(
            options
                .iter()
                .any(|(given_key, given_value)| *given_key == OptionDatabase::Other(key.into())
                    && matches!(given_value, OptionValue::String(given) if given == value)),
            "driver lost {key}"
        );
    }
}
