/// The `operator` section.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOperator {
    pub(crate) api_group: String,
    /// The Deployment the operator sizes; unset refuses at startup (`ClusterError::NoWorkersName`).
    #[serde(default)]
    pub(crate) workers: Option<String>,
    /// The Deployment whose demand the operator reads; unset refuses at startup (`ClusterError::NoServingName`).
    #[serde(default)]
    pub(crate) serving: Option<String>,
}
