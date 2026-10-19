//! Which one of three blocks says how an `impersonation-at-source` source runs its caller.
//!
//! - `workload_identity`: the caller's own assertion is federated to the caller's own pool principal.
//! - `google_login` (`bigquery` only): the caller's own Google login, which the identity provider
//!   brokered and returns through its Identity Brokering API v2.
//! - `domain_delegation` (`bigquery` only): a delegating service account acts as the caller's
//!   verified address. The deployment vouches for the caller here, not the caller's own credential.
//!
//! Exactly one, and only on an impersonating source; each refusal is at load, before any socket.
//! This build runs `workload_identity` and `google_login`: `Settings::refusals` refuses to serve a
//! source that declares `domain_delegation`, so a block that nothing reads does not load as if it
//! did.
//! `docs/adr/20261014100540-bigquery-caller-identity-modes.md` records the decision.

use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;

use super::workload_identity::WorkloadIdentityConfig;
use super::{InvalidSourceRegistry, SourceKind};
use crate::raw::{RawDomainDelegation, RawGoogleLogin, RawWorkloadIdentity};

/// The `google_login` and `domain_delegation` blocks of one entry, as read.
///
/// Beside [`super::RawSourceEntry`] rather than in it: an entry is the placement and the posture,
/// and these two are read only by [`declared`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CallerIdentityBlocks<'raw> {
    pub(crate) google_login: Option<&'raw RawGoogleLogin>,
    pub(crate) domain_delegation: Option<&'raw RawDomainDelegation>,
}

/// How an `impersonation-at-source` source runs its caller - the one block it declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallerIdentity {
    /// The caller's own pool principal.
    WorkloadIdentity(WorkloadIdentityConfig),
    /// The caller's own Google login, brokered by the identity provider.
    GoogleLogin(GoogleLogin),
    /// A delegating service account acting as the caller's verified address. Not served by this
    /// build.
    DomainDelegation(DomainDelegation),
}

impl CallerIdentity {
    /// The block of a mode this build does not serve - see `NotFitToServe::CallerIdentityModeNotBuilt`.
    pub(crate) const fn not_built(&self) -> Option<&'static str> {
        match *self {
            Self::WorkloadIdentity(_) | Self::GoogleLogin(_) => None,
            Self::DomainDelegation(_) => Some(DOMAIN_DELEGATION),
        }
    }
}

/// A `google_login` block.
///
/// **Held as written, like [`super::workload_identity::DelegationDeclared`]**: the crate that
/// sends the request parses the endpoint and the client ID. An `@` in the endpoint is refused here,
/// because the startup log prints this tree first. Only the path to the secret is here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleLogin {
    token_endpoint: String,
    client_id: String,
    client_secret_file: PathBuf,
}

impl GoogleLogin {
    /// The identity provider's broker token endpoint for its Google identity provider.
    #[inline]
    #[must_use]
    pub fn token_endpoint(&self) -> &str {
        &self.token_endpoint
    }

    #[inline]
    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[inline]
    #[must_use]
    pub fn client_secret_file(&self) -> &Path {
        &self.client_secret_file
    }
}

/// A `domain_delegation` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDelegation {
    delegating_account: DelegatingAccount,
}

impl DomainDelegation {
    /// The service account an administrator authorized for domain-wide delegation.
    #[inline]
    #[must_use]
    pub const fn delegating_account(&self) -> &DelegatingAccount {
        &self.delegating_account
    }
}

/// A user-managed service account's address: `<account>@<project>.iam.gserviceaccount.com`, each
/// half lowercase letters, digits and hyphens.
///
/// Parsed here where it is declared; the adapter that sends it holds its own copy, for the reason
/// [`super::placement::BillingProject`] is checked twice.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DelegatingAccount(String);

/// Why a declared `delegating_account` is not a service-account address. The value is never quoted:
/// it carries a project id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDelegatingAccount {
    /// Nothing was written.
    #[error("it is empty")]
    Empty,
    /// A character outside lowercase letters, digits, `-`, `.` and `@`.
    #[error("the character at position {at} is not allowed in a service-account address")]
    Character { at: usize },
    /// Not `<account>@<project>.iam.gserviceaccount.com`.
    #[error("it is not a service-account address `<account>@<project>.iam.gserviceaccount.com`")]
    Shape,
}

impl DelegatingAccount {
    /// The domain every user-managed service account's address ends in.
    const DOMAIN: &'static str = ".iam.gserviceaccount.com";

    /// Parses a declared address.
    pub fn parse(written: &str) -> Result<Self, InvalidDelegatingAccount> {
        if written.is_empty() {
            return Err(InvalidDelegatingAccount::Empty);
        }
        if let Some(at) = written
            .chars()
            .position(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '@')))
        {
            return Err(InvalidDelegatingAccount::Character { at });
        }
        let label = |half: &str| !half.is_empty() && !half.contains(['.', '@']);
        match written.split_once('@') {
            Some((account, domain)) if label(account) && domain.strip_suffix(Self::DOMAIN).is_some_and(label) => {
                Ok(Self(String::from(written)))
            }
            _ => Err(InvalidDelegatingAccount::Shape),
        }
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

const WORKLOAD_IDENTITY: &str = "workload_identity";
const GOOGLE_LOGIN: &str = "google_login";
const DOMAIN_DELEGATION: &str = "domain_delegation";

/// One declared block, before it is read.
#[derive(Clone, Copy)]
enum Written<'raw> {
    WorkloadIdentity(&'raw RawWorkloadIdentity),
    GoogleLogin(&'raw RawGoogleLogin),
    DomainDelegation(&'raw RawDomainDelegation),
}

impl Written<'_> {
    const fn block(self) -> &'static str {
        match self {
            Self::WorkloadIdentity(_) => WORKLOAD_IDENTITY,
            Self::GoogleLogin(_) => GOOGLE_LOGIN,
            Self::DomainDelegation(_) => DOMAIN_DELEGATION,
        }
    }
}

/// Reads the one caller-identity block an entry declares.
///
/// `None` for a non-impersonating source, and for an impersonating `clickhouse`, `oracle` or `cube`
/// source that declares none - those run their caller by their own keys.
pub(super) fn declared(
    alias: &SourceName,
    kind: SourceKind,
    impersonating: bool,
    workload_identity: Option<&RawWorkloadIdentity>,
    blocks: CallerIdentityBlocks<'_>,
) -> Result<Option<CallerIdentity>, InvalidSourceRegistry> {
    let mut written = [
        workload_identity.map(Written::WorkloadIdentity),
        blocks.google_login.map(Written::GoogleLogin),
        blocks.domain_delegation.map(Written::DomainDelegation),
    ]
    .into_iter()
    .flatten();
    let first = written.next();
    if let (Some(first), Some(second)) = (first, written.next()) {
        return Err(InvalidSourceRegistry::CallerIdentityModesConflict {
            alias: alias.clone(),
            first: first.block(),
            second: second.block(),
        });
    }
    let Some(first) = first else {
        if impersonating && !matches!(kind, SourceKind::ClickHouse | SourceKind::Oracle | SourceKind::Cube) {
            return Err(InvalidSourceRegistry::NoCallerIdentityMode { alias: alias.clone() });
        }
        return Ok(None);
    };
    if !impersonating {
        return Err(InvalidSourceRegistry::IdentityModeNotImpersonating {
            alias: alias.clone(),
            block: first.block(),
        });
    }
    if !matches!(first, Written::WorkloadIdentity(_)) && kind != SourceKind::BigQuery {
        return Err(InvalidSourceRegistry::IdentityModeNotForKind {
            alias: alias.clone(),
            block: first.block(),
        });
    }
    Ok(Some(match first {
        Written::WorkloadIdentity(raw) => CallerIdentity::WorkloadIdentity(super::parse_workload_identity(alias, raw)?),
        Written::GoogleLogin(raw) => {
            if raw.token_endpoint.contains('@') {
                return Err(InvalidSourceRegistry::CredentialsInUrl {
                    alias: alias.clone(),
                    key: "google_login.token_endpoint",
                });
            }
            CallerIdentity::GoogleLogin(GoogleLogin {
                token_endpoint: raw.token_endpoint.clone(),
                client_id: raw.client_id.clone(),
                client_secret_file: super::parse_absolute(alias, "google_login.client_secret_file", &raw.client_secret_file)?,
            })
        }
        Written::DomainDelegation(raw) => CallerIdentity::DomainDelegation(DomainDelegation {
            delegating_account: DelegatingAccount::parse(&raw.delegating_account).map_err(|cause| {
                InvalidSourceRegistry::DomainDelegation {
                    alias: alias.clone(),
                    cause,
                }
            })?,
        }),
    }))
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use super::{CallerIdentityBlocks, InvalidDelegatingAccount};
    use crate::environment::Environment;
    use crate::raw::{RawDomainDelegation, RawGoogleLogin};
    use crate::security::DeploymentIdentity;
    use crate::settings::{NotFitToServe, Settings, SettingsError, Sources};
    use crate::sources::{InvalidSourceRegistry, RawSourceEntry, SourceRegistry};

    fn warehouse_alias() -> SourceName {
        SourceName::parse("warehouse").expect("a test alias is a name")
    }

    /// An impersonating `bigquery` entry with no caller-identity block, so a cell adds exactly one.
    fn bigquery_impersonating_entry() -> RawSourceEntry<'static> {
        RawSourceEntry {
            written: "warehouse",
            kind: "bigquery",
            data_dir: None,
            billing_project: Some("acme-analytics"),
            dataset: Some("warehouse"),
            max_bytes_billed: Some(1024 * 1024 * 1024),
            posture: "impersonation-at-source",
            acknowledged_because: None,
            verification_identity: None,
            workload_identity: None,
            host: None,
            unix_socket: None,
            port: None,
            database: None,
            service_name: None,
            endpoint: None,
            token_file: None,
            database_file: None,
            user: None,
            password_file: None,
            transport_mode: None,
            transport_anchors: None,
            client_certificate: None,
            client_key: None,
            impersonate: false,
            subjects: false,
            delegation: None,
        }
    }

    fn broker_login_block(token_endpoint: &str, client_secret_file: &str) -> RawGoogleLogin {
        RawGoogleLogin {
            token_endpoint: String::from(token_endpoint),
            client_id: String::from("sutura"),
            client_secret_file: String::from(client_secret_file),
        }
    }

    fn delegating_block(delegating_account: &str) -> RawDomainDelegation {
        RawDomainDelegation {
            delegating_account: String::from(delegating_account),
        }
    }

    fn one_analyst() -> DeploymentIdentity {
        DeploymentIdentity::parse("single-user", Some("one analyst on one laptop")).expect("a declared single-user mode parses")
    }

    fn parse_one_entry(
        entry: &RawSourceEntry<'_>,
        blocks: CallerIdentityBlocks<'_>,
    ) -> Result<SourceRegistry, InvalidSourceRegistry> {
        SourceRegistry::parse_declared([(entry, blocks)], Some(&one_analyst()))
    }

    const ENDPOINT: &str = "https://idp.example.com/realms/example/broker/google/token";
    const SECRET: &str = "/run/secrets/idp-client-secret";
    const ACCOUNT: &str = "dwd-bigquery@acme-analytics.iam.gserviceaccount.com";

    #[test]
    fn an_impersonating_bigquery_source_with_two_identity_modes_is_refused() {
        let wif = crate::raw::RawWorkloadIdentity {
            audience: String::from("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/v"),
            expected_issuer: None,
            expected_audience: None,
            delegation: None,
        };
        let login = broker_login_block(ENDPOINT, SECRET);
        let delegation = delegating_block(ACCOUNT);
        for (with_wif, google_login, domain_delegation, first, second) in [
            (true, Some(&login), None, "workload_identity", "google_login"),
            (true, None, Some(&delegation), "workload_identity", "domain_delegation"),
            (false, Some(&login), Some(&delegation), "google_login", "domain_delegation"),
        ] {
            let entry = RawSourceEntry {
                workload_identity: with_wif.then(|| wif.clone()),
                ..bigquery_impersonating_entry()
            };
            let blocks = CallerIdentityBlocks {
                google_login,
                domain_delegation,
            };
            let error = parse_one_entry(&entry, blocks).expect_err(second);
            assert_eq!(
                error,
                InvalidSourceRegistry::CallerIdentityModesConflict {
                    alias: warehouse_alias(),
                    first,
                    second,
                },
                "{first} and {second}"
            );
        }
    }

    #[test]
    fn an_impersonating_bigquery_source_with_no_identity_mode_is_refused_naming_all_three() {
        let error = parse_one_entry(&bigquery_impersonating_entry(), CallerIdentityBlocks::default())
            .expect_err("an impersonating bigquery source needs a mode");
        assert_eq!(
            error,
            InvalidSourceRegistry::NoCallerIdentityMode {
                alias: warehouse_alias()
            }
        );
        let rendered = error.to_string();
        for block in ["workload_identity", "google_login", "domain_delegation"] {
            assert!(rendered.contains(block), "{rendered}");
        }
    }

    /// Each block only `bigquery` reads, alone, with the name a refusal of it carries.
    fn each_bigquery_only_block<'block>(
        login: &'block RawGoogleLogin,
        delegation: &'block RawDomainDelegation,
    ) -> [(CallerIdentityBlocks<'block>, &'static str); 2] {
        [
            (
                CallerIdentityBlocks {
                    google_login: Some(login),
                    domain_delegation: None,
                },
                "google_login",
            ),
            (
                CallerIdentityBlocks {
                    google_login: None,
                    domain_delegation: Some(delegation),
                },
                "domain_delegation",
            ),
        ]
    }

    #[test]
    fn google_login_or_domain_delegation_on_a_shared_source_is_refused() {
        let (login, delegation) = (broker_login_block(ENDPOINT, SECRET), delegating_block(ACCOUNT));
        let entry = RawSourceEntry {
            posture: "shared-service-user",
            acknowledged_because: Some("one service account reaching the dataset for everybody who asks"),
            ..bigquery_impersonating_entry()
        };
        for (blocks, block) in each_bigquery_only_block(&login, &delegation) {
            let error = parse_one_entry(&entry, blocks).expect_err(block);
            assert_eq!(
                error,
                InvalidSourceRegistry::IdentityModeNotImpersonating {
                    alias: warehouse_alias(),
                    block,
                }
            );
        }
    }

    #[test]
    fn google_login_or_domain_delegation_on_a_files_source_is_refused() {
        let (login, delegation) = (broker_login_block(ENDPOINT, SECRET), delegating_block(ACCOUNT));
        let entry = RawSourceEntry {
            kind: "files",
            data_dir: Some("/srv/sutura/data"),
            billing_project: None,
            dataset: None,
            max_bytes_billed: None,
            ..bigquery_impersonating_entry()
        };
        for (blocks, block) in each_bigquery_only_block(&login, &delegation) {
            let error = parse_one_entry(&entry, blocks).expect_err(block);
            assert_eq!(
                error,
                InvalidSourceRegistry::IdentityModeNotForKind {
                    alias: warehouse_alias(),
                    block,
                }
            );
        }
    }

    #[test]
    fn a_delegating_account_that_is_not_a_service_account_is_refused() {
        for (written, cause) in [
            ("", InvalidDelegatingAccount::Empty),
            (
                "Dwd@acme-analytics.iam.gserviceaccount.com",
                InvalidDelegatingAccount::Character { at: 0 },
            ),
            (
                "dwd @acme.iam.gserviceaccount.com",
                InvalidDelegatingAccount::Character { at: 3 },
            ),
            ("user@example.com", InvalidDelegatingAccount::Shape),
            ("dwd@iam.gserviceaccount.com", InvalidDelegatingAccount::Shape),
            ("dwd@.iam.gserviceaccount.com", InvalidDelegatingAccount::Shape),
            ("@acme-analytics.iam.gserviceaccount.com", InvalidDelegatingAccount::Shape),
            ("a@b@acme-analytics.iam.gserviceaccount.com", InvalidDelegatingAccount::Shape),
            ("dwd@x.acme.iam.gserviceaccount.com", InvalidDelegatingAccount::Shape),
        ] {
            let delegation = delegating_block(written);
            let blocks = CallerIdentityBlocks {
                google_login: None,
                domain_delegation: Some(&delegation),
            };
            let error = parse_one_entry(&bigquery_impersonating_entry(), blocks).expect_err(written);
            assert_eq!(
                error,
                InvalidSourceRegistry::DomainDelegation {
                    alias: warehouse_alias(),
                    cause,
                },
                "{written}"
            );
        }
        let delegation = delegating_block(ACCOUNT);
        let blocks = CallerIdentityBlocks {
            google_login: None,
            domain_delegation: Some(&delegation),
        };
        let parsed = parse_one_entry(&bigquery_impersonating_entry(), blocks).expect("a service-account address parses");
        let Some(super::CallerIdentity::DomainDelegation(declared)) = parsed
            .get(&warehouse_alias())
            .and_then(super::super::ConfiguredSource::caller_identity)
        else {
            panic!("the block was carried");
        };
        assert_eq!(declared.delegating_account().as_str(), ACCOUNT);
    }

    #[test]
    fn a_google_login_endpoint_carrying_credentials_is_refused() {
        let login = broker_login_block("https://user:pw@idp.example.com/realms/example/broker/google/token", SECRET);
        let blocks = CallerIdentityBlocks {
            google_login: Some(&login),
            domain_delegation: None,
        };
        let error = parse_one_entry(&bigquery_impersonating_entry(), blocks).expect_err("userinfo is refused");
        assert_eq!(
            error,
            InvalidSourceRegistry::CredentialsInUrl {
                alias: warehouse_alias(),
                key: "google_login.token_endpoint",
            }
        );
    }

    #[test]
    fn a_relative_google_login_secret_file_is_refused() {
        let login = broker_login_block(ENDPOINT, "secrets/idp-client-secret");
        let blocks = CallerIdentityBlocks {
            google_login: Some(&login),
            domain_delegation: None,
        };
        let error = parse_one_entry(&bigquery_impersonating_entry(), blocks).expect_err("a relative path is refused");
        assert_eq!(
            error,
            InvalidSourceRegistry::RelativePath {
                alias: warehouse_alias(),
                key: "google_login.client_secret_file",
                path: std::path::PathBuf::from("secrets/idp-client-secret"),
            }
        );
    }

    /// A single-user deployment with one impersonating `bigquery` source carrying `block`.
    fn bigquery_overlay_with(block: &str) -> Sources {
        Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  identity: \"single-user\"\n  single_user_because: \"one analyst on one laptop\"\n\
             sources:\n  warehouse:\n    kind: \"bigquery\"\n    billing_project: \"acme-analytics\"\n    \
             dataset: \"warehouse\"\n    max_bytes_billed: 1073741824\n    posture: \"impersonation-at-source\"\n{block}"
        ))
    }

    const DELEGATION_YAML: &str =
        "    domain_delegation:\n      delegating_account: \"dwd-bigquery@acme-analytics.iam.gserviceaccount.com\"\n";

    fn refused_at_boot(block: &str) -> Vec<NotFitToServe> {
        let error = Settings::load(&bigquery_overlay_with(block)).expect_err("a mode this build does not run is refused");
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a boot refusal, got {error:?}");
        };
        refusals.clone()
    }

    #[test]
    fn a_domain_delegation_source_is_refused_at_boot_as_not_built() {
        assert_eq!(
            refused_at_boot(DELEGATION_YAML),
            vec![NotFitToServe::CallerIdentityModeNotBuilt {
                alias: warehouse_alias(),
                block: "domain_delegation",
            }]
        );
    }
}
