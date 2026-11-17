/// Optional overrides for Google's API destinations.
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGoogleEndpoints {
    pub(crate) bigquery: Option<String>,
    pub(crate) bigquery_storage: Option<String>,
    pub(crate) sts: Option<String>,
    pub(crate) iam_credentials: Option<String>,
    pub(crate) oauth_token: Option<String>,
    pub(crate) tokeninfo: Option<String>,
}
