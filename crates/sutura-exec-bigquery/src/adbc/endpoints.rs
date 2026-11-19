//! Destinations handed to the native driver for jobs and partition reads.

use adbc_core::options::{OptionDatabase, OptionValue};

/// Low-level driver destinations. The settings parser restricts their URLs to Google's HTTPS APIs;
/// callers constructing the adapter directly are responsible for their own destination policy.
#[derive(Debug, Clone)]
pub struct GoogleEndpoints {
    bigquery: String,
    storage: String,
    iam: String,
    oauth: String,
    sts: String,
    tls: Vec<(&'static str, String)>,
}

impl GoogleEndpoints {
    #[must_use]
    pub fn new(bigquery: &str, storage: &str, iam: &str, oauth: &str, sts: &str) -> Self {
        Self {
            bigquery: bigquery.into(),
            storage: storage.into(),
            iam: iam.into(),
            oauth: oauth.into(),
            sts: sts.into(),
            tls: Vec::new(),
        }
    }

    /// Native-driver TLS material; an absent CA file uses system roots. A supplied bundle replaces them.
    #[must_use]
    pub fn with_trust(mut self, ca: Option<String>, identity: Option<(String, String)>) -> Self {
        if let Some(path) = ca {
            self.tls.push(("bigquery.tls.ca_file", path));
        }
        if let Some((cert, key)) = identity {
            self.tls.push(("bigquery.tls.cert_file", cert));
            self.tls.push(("bigquery.tls.key_file", key));
        }
        self
    }

    pub(super) fn options(&self) -> impl Iterator<Item = (OptionDatabase, OptionValue)> + '_ {
        let authority = self
            .storage
            .strip_prefix("https://")
            .unwrap_or(&self.storage)
            .trim_end_matches('/');
        let storage = if authority.contains(':') {
            authority.to_owned()
        } else {
            format!("{authority}:443")
        };
        let mut options = vec![
            ("bigquery.endpoint", self.bigquery.clone()),
            ("bigquery.storage_endpoint", storage),
            ("bigquery.iam_credentials_endpoint", self.iam.clone()),
            ("bigquery.oauth_token_endpoint", self.oauth.clone()),
            ("bigquery.sts_endpoint", self.sts.clone()),
        ];
        options.extend(self.tls.iter().cloned());
        options
            .into_iter()
            .map(|(key, value)| (OptionDatabase::Other(key.into()), OptionValue::String(value)))
    }
}
