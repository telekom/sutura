#![forbid(unsafe_code)]
//! URL policy at the configuration boundary, before opening any source.

#[cfg(test)]
mod tests {
    use sutura_config::{Environment, Settings, Sources};

    #[test]
    fn google_private_endpoints_load_as_one_optional_block() {
        let sources = Sources::defaults(Environment::Development).with_overlay(
        "google_endpoints:\n  bigquery: https://bigquery-example.p.googleapis.com/bigquery/v2/\n  bigquery_storage: https://bigquerystorage-example.p.googleapis.com:443/\n  sts: https://sts-example.p.googleapis.com/v1/token\n  iam_credentials: https://iamcredentials-example.p.googleapis.com/\n  oauth_token: https://oauth2-example.p.googleapis.com/token\n  tokeninfo: https://oauth2-example.p.googleapis.com/tokeninfo\n",
    );
        Settings::load(&sources).expect("every declared Google API can use a private endpoint");
    }

    #[test]
    fn an_invalid_google_destination_names_its_key_without_printing_a_secret() {
        for key in [
            "bigquery",
            "bigquery_storage",
            "sts",
            "iam_credentials",
            "oauth_token",
            "tokeninfo",
        ] {
            for value in [
                "http://sts.googleapis.com/v1/token",
                "https://sts.googleapis.com.example.com/v1/token",
                "https://example.com/v1/token",
                concat!("https://user:", "private-token@sts.googleapis.com/v1/token"),
                "https://sts.googleapis.com/v1/token?private-token=secret",
                "https://sts.googleapis.com/v1/token#private-token",
            ] {
                let sources =
                    Sources::defaults(Environment::Development).with_overlay(format!("google_endpoints:\n  {key}: {value}\n"));
                let refusal = Settings::load(&sources).expect_err("untrusted Google API destinations never load");
                let mut error: Option<&dyn std::error::Error> = Some(&refusal);
                let mut message = String::new();
                while let Some(cause) = error {
                    message.push_str(&cause.to_string());
                    error = cause.source();
                }
                assert!(message.contains(&format!("google_endpoints.{key}")), "{message}");
                assert!(!message.contains("private-token"), "{message}");
            }
        }
    }
}
