/// An unparsed entry, private so settings parsing remains the only way into the registry.
#[derive(Debug, Clone)]
pub(crate) struct RawSourceEntry<'raw> {
    /// The key, exactly as written, so a refusal can quote what the operator typed.
    pub(crate) written: &'raw str,
    pub(crate) kind: &'raw str,
    pub(crate) data_dir: Option<&'raw str>,
    pub(crate) billing_project: Option<&'raw str>,
    pub(crate) dataset: Option<&'raw str>,
    pub(crate) max_bytes_billed: Option<u64>,
    pub(crate) posture: &'raw str,
    pub(crate) acknowledged_because: Option<&'raw str>,
    pub(crate) verification_identity: Option<&'raw str>,
    pub(crate) workload_identity: Option<crate::raw::RawWorkloadIdentity>,
    /// Whether the removed `impersonate` key is written - see [`crate::sources::InvalidSourceRegistry::ImpersonateRemoved`].
    pub(crate) impersonate: bool,
    /// Whether the removed `subjects` key is written - see [`crate::sources::InvalidSourceRegistry::SubjectsRemoved`].
    pub(crate) subjects: bool,
    pub(crate) delegation: Option<&'raw [crate::raw::RawDelegation]>,
    pub(crate) host: Option<&'raw str>,
    pub(crate) unix_socket: Option<&'raw str>,
    pub(crate) port: Option<u16>,
    pub(crate) database: Option<&'raw str>,
    pub(crate) service_name: Option<&'raw str>,
    pub(crate) endpoint: Option<&'raw str>,
    pub(crate) token_file: Option<&'raw str>,
    pub(crate) database_file: Option<&'raw str>,
    pub(crate) user: Option<&'raw str>,
    pub(crate) password_file: Option<&'raw str>,
    pub(crate) transport_mode: Option<&'raw str>,
    pub(crate) oracle_options: super::oracle_transport::RawOracleOptions<'raw>,
    pub(crate) transport_anchors: Option<&'raw str>,
    pub(crate) client_certificate: Option<&'raw str>,
    pub(crate) client_key: Option<&'raw str>,
}
