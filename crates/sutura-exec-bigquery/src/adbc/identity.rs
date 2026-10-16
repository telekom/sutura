//! Which authentication options one job's identity becomes, and the refusal for the pair that has
//! none.
//!
//! **The whole of this transport's identity decision, in a function that touches no driver**: it is
//! what `connect` calls before the `.so` is loaded, so a request this transport cannot authenticate
//! never opens a connection it could answer as somebody else.
//!
//! # The mechanism
//!
//! An impersonating source hands the driver a WORKLOAD-IDENTITY CREDENTIAL DOCUMENT naming a
//! loopback source for the asking subject's own assertion - [`super::subject`] builds it and carries
//! the whole argument, including why a loopback URL rather than a file or an executable, and what the
//! open port's exposure actually is. The document names no second hop: the federated token Google's
//! token service returns IS the subject's own pool principal, and `BigQuery`'s grants on that
//! principal decide what it reads. A Google-login source hands the driver the subject's own access
//! token instead, which the identity provider stored when the subject signed in to Google.
//!
//! # No second hop
//!
//! Two were built and both are deleted. `bigquery.impersonate.target_principal` impersonated a
//! declared account from the DEPLOYMENT's own application default credentials - the subject's own
//! credential was nowhere in that chain, and the owner rejected it. A
//! `service_account_impersonation_url` impersonated a declared account from the pool principal, and
//! grants on the principal itself replaced it (`docs/adr/0032`). A fallback a misconfiguration could
//! select is the defect, not a convenience. The chain starts at a credential the CALLER possesses:
//! no assertion, no token. The one exception is opted in to per source and reached by no other mode:
//! [`Impersonation::DomainDelegation`] sets `target_principal` again, with the verified address as
//! the subject, and the deployment vouches for the caller there
//! (`docs/adr/20261014100540-bigquery-caller-identity-modes.md`).
//!
//! # Why the pool's values are parsed HERE, when configuration already parsed them
//!
//! For the reason [`crate::transport::ProjectId`] is parsed a second time: this is the crate that
//! puts the value into a request, and a check belongs where the risk is. See
//! [`WorkloadPool::parse`] for the accepted sets and what they exclude.

use adbc_core::options::{OptionDatabase, OptionValue};
use sutura_domain::identity::{PrincipalName, Secret};

use crate::transport::JobIdentity;

use super::AdbcError;
use super::subject::{SubjectSource, WorkloadPool};

/// The driver option naming which authentication shape the database uses.
///
/// Pinned `go/v1.13.0`'s `OptionAuthType`. `json_credential_string` is the arm whose value is a
/// credential DOCUMENT rather than a path, which is what lets a per-request document exist at all.
const AUTH_TYPE: &str = "bigquery.auth_type";

/// The value of [`AUTH_TYPE`] that takes a credential document inline.
const AUTH_TYPE_JSON_STRING: &str = "json_credential_string";

/// The driver option naming which KIND of credential document the value is.
///
/// Pinned `go/v1.13.0`'s `OptionAuthCredentialsType`; its `SetOption` accepts
/// `option.ExternalAccount` by name, and `go/connection.go` passes both straight to
/// `option.WithAuthCredentialsJSON`.
const AUTH_CREDENTIALS_TYPE: &str = "bigquery.auth.credentials_type";

/// The value of [`AUTH_CREDENTIALS_TYPE`] that means a workload-identity document.
const EXTERNAL_ACCOUNT: &str = "external_account";

/// The driver option carrying the credential document itself.
const AUTH_CREDENTIALS: &str = "bigquery.auth.credentials";

/// The value of [`AUTH_TYPE`] that takes a bare access token, which the driver never refreshes.
///
/// The vendored driver patch's `OptionValueAuthTypeAccessToken` (`VENDOR.md`).
const AUTH_TYPE_ACCESS_TOKEN: &str = "access_token";

/// The driver option carrying that access token: the patch's `OptionAuthAccessToken`.
const AUTH_ACCESS_TOKEN: &str = "bigquery.auth.access_token";

/// The account the deployment's own credential impersonates: pinned `go/v1.13.0`'s
/// `OptionImpersonateTargetPrincipal`.
const IMPERSONATE_TARGET: &str = "bigquery.impersonate.target_principal";

/// The scopes the impersonated token carries: `OptionImpersonateScopes`.
const IMPERSONATE_SCOPES: &str = "bigquery.impersonate.scopes";

/// The user the delegating account acts as: the vendored driver patch's `OptionImpersonateSubject`.
const IMPERSONATE_SUBJECT: &str = "bigquery.impersonate.subject";

/// The one scope that runs a query: `jobs.insert` refuses `bigquery.readonly`.
const BIGQUERY_SCOPE: &str = "https://www.googleapis.com/auth/bigquery";

/// The options one job's identity becomes, as the driver's database option pairs.
///
/// Named because `Result<Vec<(OptionDatabase, OptionValue)>, AdbcError>` is over this workspace's
/// `type_complexity` threshold - the same reason `crate::Mapped` exists.
type DatabaseOptions = Vec<(OptionDatabase, OptionValue)>;

/// Whether this source impersonates at all, and how when it does.
///
/// **An enum rather than an `Option<WorkloadPool>`, because the absence is a DECLARATION.** A
/// source is opened shared or impersonating - `sutura_config` refuses a caller-identity block on a
/// shared entry and refuses its absence on an impersonating one - so which of these a transport
/// holds is decided once, at composition, from a value an operator wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Impersonation {
    /// This source does not impersonate: every job runs as the identity the driver authenticates as.
    Disabled,
    /// This source impersonates, by federating the asking subject's assertion against this pool.
    ThroughPool(WorkloadPool),
    /// This source impersonates with the access token the identity provider stored for the asking
    /// subject's own Google login: the subject's credential is that token itself.
    GoogleLogin,
    /// This source runs the asking subject's verified address through domain-wide delegation: the
    /// deployment's own credential signs as this account, which acts as the address. The
    /// deployment vouches for the subject here.
    DomainDelegation(DelegatingAccount),
}

/// A user-managed service account's address, `<account>@<project>.iam.gserviceaccount.com`.
///
/// Parsed here, where it is sent, as well as where it is declared - for [`WorkloadPool::parse`]'s
/// reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegatingAccount(String);

/// Why a value is not a service-account address. The value is never quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusableDelegatingAccount {
    /// Not `<account>@<project>.iam.gserviceaccount.com`.
    #[error("not a service-account address `<account>@<project>.iam.gserviceaccount.com`")]
    Shape,
}

impl DelegatingAccount {
    /// Parses an address: two non-empty labels of lowercase letters, digits and `-` around the `@`,
    /// the second followed by `.iam.gserviceaccount.com`.
    ///
    /// # Errors
    ///
    /// [`UnusableDelegatingAccount`] for anything else.
    pub fn parse(written: &str) -> Result<Self, UnusableDelegatingAccount> {
        let label = |half: &str| {
            !half.is_empty()
                && half
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        let (account, project) = written.split_once('@').ok_or(UnusableDelegatingAccount::Shape)?;
        match project.strip_suffix(".iam.gserviceaccount.com") {
            Some(project) if label(account) && label(project) => Ok(Self(String::from(written))),
            _ => Err(UnusableDelegatingAccount::Shape),
        }
    }
}

/// Everything a job's identity puts on the database, and the loopback source it has to outlive.
///
/// **The source is returned rather than dropped here, and that is the lifetime bug this shape
/// prevents.** `externalaccount`'s token provider is wrapped in a CACHED provider, so the driver
/// fetches the subject token lazily - after `connect` has returned - and may fetch again if the
/// credential expires mid-query. A function that bound the listener and let it fall out of scope
/// would compile, pass every boot-shaped test, and fail on the first real question.
pub(super) struct JobAuthentication {
    /// What the database is opened with.
    pub(super) options: DatabaseOptions,
    /// Held for exactly as long as the job runs. `None` for every leg that authenticates as the
    /// process: the shared posture and the boot path.
    pub(super) source: Option<SubjectSource>,
}

/// The database options and loopback source one job's identity becomes.
///
/// **Empty options and no source for [`JobIdentity::Transport`]**, which is the shared posture and
/// the boot path: the driver authenticates as the process. An impersonating source reaching that arm
/// is correct rather than a miss - `verify_anchor` and a fixture load carry no caller, and
/// `crate`'s `verify_anchor` documentation already says an executed anchor here reproduces for the
/// identity this deployment holds.
///
/// # Errors
///
/// [`AdbcError::Uncovered`] for a subject at a source that declares no pool: there is nothing to
/// federate the assertion against, and opening the connection anyway would run the question as the
/// deployment. [`AdbcError::SubjectSource`] or
/// [`AdbcError::NoRandomness`] where the loopback source cannot be opened safely - all refusals,
/// never a weaker source.
pub(super) fn authenticate(identity: JobIdentity<'_>, impersonation: &Impersonation) -> Result<JobAuthentication, AdbcError> {
    match (identity, impersonation) {
        (JobIdentity::Transport, _) => Ok(JobAuthentication {
            options: Vec::new(),
            source: None,
        }),
        (JobIdentity::AsSubject { assertion }, Impersonation::GoogleLogin) => Ok(JobAuthentication {
            options: access_token_options(assertion),
            source: None,
        }),
        (JobIdentity::AsAddress { address }, Impersonation::DomainDelegation(account)) => Ok(JobAuthentication {
            options: delegation_options(account, address),
            source: None,
        }),
        // A subject's own credential at a delegating source, or an address anywhere else: in-crate
        // misuse insurance, for the reason the arm below gives.
        (JobIdentity::AsSubject { .. }, Impersonation::DomainDelegation(_))
        | (JobIdentity::AsAddress { .. }, Impersonation::Disabled | Impersonation::ThroughPool(_) | Impersonation::GoogleLogin) => {
            Err(AdbcError::Uncovered(
                "run a subject by a mechanism this source does not declare",
            ))
        }
        (JobIdentity::AsSubject { assertion }, Impersonation::ThroughPool(pool)) => {
            let source = SubjectSource::bind(assertion)?;
            Ok(JobAuthentication {
                options: credential_options(&source, pool),
                source: Some(source),
            })
        }
        // **A subject at a source that declares no pool: IN-CRATE misuse insurance, and no caller
        // outside this crate can reach it.** The sentence here claimed `JobTransport` being public
        // made the pairing reachable, which is false - `JobRequest::new` is `pub(crate)` and no
        // public API hands one out, so a foreign caller cannot build the request that carries this
        // identity. One layer up, `Presented::agrees_with` refuses a subject shape at a source
        // declared shared, so the pairing has no route through the domain port either. It stays a
        // refusal rather than an `unreachable!` because the cost is one arm and the alternative is a
        // panic in a library: there is no pool to exchange the assertion against, and a connection
        // opened here would answer the question as this deployment.
        (JobIdentity::AsSubject { .. }, Impersonation::Disabled) => {
            Err(AdbcError::Uncovered("federate a subject at a source that declares no pool"))
        }
    }
}

/// The three options that hand the driver one request's workload-identity document.
///
/// Split out so the pair *which options* and *what the document says* are readable apart, and so
/// the document's own construction stays in [`SubjectSource::document`] where the secret lives.
///
/// **The one place the document is exposed as text**, because a driver option IS text: `OptionValue`
/// is the C ABI's shape and there is no secret-carrying variant of it. What that costs is stated at
/// the exposure rather than here.
#[expect(
    clippy::disallowed_methods,
    reason = "a driver option is a plain string across the C ABI, so the credential document has to               be exposed exactly once - here, at the boundary, into a value that is built and               consumed inside `connect` and never logged. `SubjectSource::document` keeps it a               `Secret` up to this line so no other reader can print it, and               `the_credential_document_is_redacted_under_debug` is the cell on that"
)]
fn credential_options(source: &SubjectSource, pool: &WorkloadPool) -> DatabaseOptions {
    vec![
        (
            OptionDatabase::Other(AUTH_TYPE.into()),
            OptionValue::String(String::from(AUTH_TYPE_JSON_STRING)),
        ),
        (
            OptionDatabase::Other(AUTH_CREDENTIALS_TYPE.into()),
            OptionValue::String(String::from(EXTERNAL_ACCOUNT)),
        ),
        (
            OptionDatabase::Other(AUTH_CREDENTIALS.into()),
            OptionValue::String(String::from(source.document(pool).expose_secret())),
        ),
    ]
}

/// The two options that hand the driver one subject's access token: the token and the auth type
/// that selects it, only ever together - the driver reads the token under no other auth type.
#[expect(
    clippy::disallowed_methods,
    reason = "a driver option is a plain string across the C ABI, so the token is exposed here \
              once, into a value built and consumed inside `connect` and never logged"
)]
fn access_token_options(token: &Secret) -> DatabaseOptions {
    vec![
        (
            OptionDatabase::Other(AUTH_TYPE.into()),
            OptionValue::String(String::from(AUTH_TYPE_ACCESS_TOKEN)),
        ),
        (
            OptionDatabase::Other(AUTH_ACCESS_TOKEN.into()),
            OptionValue::String(String::from(token.expose_secret())),
        ),
    ]
}

/// The three options that run one job as `address` through domain-wide delegation, only ever
/// together: the delegating account, the one scope a query needs, and the address. No auth type is
/// set, so the base credential is the deployment's own application default credentials, which sign
/// as the account.
fn delegation_options(account: &DelegatingAccount, address: &PrincipalName) -> DatabaseOptions {
    [
        (IMPERSONATE_TARGET, account.0.as_str()),
        (IMPERSONATE_SCOPES, BIGQUERY_SCOPE),
        (IMPERSONATE_SUBJECT, address.as_str()),
    ]
    .into_iter()
    .map(|(key, value)| (OptionDatabase::Other(key.into()), OptionValue::String(String::from(value))))
    .collect()
}

/// A compile-time guard that `JobAuthentication` does not implement `Debug`.
///
/// `JobAuthentication.options` is `DatabaseOptions` - `Vec<(OptionDatabase, OptionValue)>` from
/// `adbc_core` - and `credential_options` is the one place the credential document is exposed as
/// text, into an `OptionValue::String`. `adbc_core`'s `OptionValue` derives `Debug`, so a
/// `#[derive(Debug)]` on `JobAuthentication` would print the whole document: the loopback nonce
/// and the header secret. That is the same leak
/// [`super::subject`]'s `no_debug` guard exists for, one layer up - `SubjectSource` holds the bare
/// values and this type carries them through the foreign `Debug`.
///
/// `JobAuthentication` is `pub(super)`, so like `SubjectSource` a `compile_fail` doctest cannot name
/// it - this guard is the mechanism that replaces review for a type no external crate can reach. It
/// uses the same negative-impl ambiguity trick: one blanket impl for all `T`, one for `T: Debug`,
/// so adding any `Debug` impl makes `<JobAuthentication as AmbiguousIfImpl<_>>::some_item` ambiguous
/// (E0283) and the crate does not build.
///
/// **Limit:** this forbids `Debug` and nothing else. The `options` field is a foreign `Vec` whose
/// `Debug` already prints the document text; nothing here stops a `tracing`/`format!` call that
/// reaches `options` directly, or a hand-written `Display` on `JobAuthentication`. Those are held by
/// review, not by this guard.
mod no_debug {
    trait AmbiguousIfImpl<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    impl<T: ?Sized + core::fmt::Debug> AmbiguousIfImpl<u8> for T {}

    // Compiles only while JobAuthentication does NOT implement Debug.
    const _: fn() = || {
        let _ = <super::JobAuthentication as AmbiguousIfImpl<_>>::some_item;
    };
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{PrincipalName, Secret};

    use super::{
        AUTH_ACCESS_TOKEN, AUTH_CREDENTIALS, AUTH_CREDENTIALS_TYPE, AUTH_TYPE, AUTH_TYPE_ACCESS_TOKEN, DelegatingAccount,
        IMPERSONATE_SCOPES, IMPERSONATE_SUBJECT, IMPERSONATE_TARGET, Impersonation, UnusableDelegatingAccount, authenticate,
    };
    use crate::adbc::AdbcError;
    use crate::adbc::subject::WorkloadPool;
    use crate::transport::JobIdentity;

    fn impersonating() -> Impersonation {
        Impersonation::ThroughPool(
            WorkloadPool::parse("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/a/providers/sso")
                .expect("a provider resource is usable"),
        )
    }

    /// The option keys one identity puts on the database, as text.
    fn keys(identity: JobIdentity<'_>, impersonation: &Impersonation) -> Vec<String> {
        authenticate(identity, impersonation)
            .expect("this pair is one this transport authenticates")
            .options
            .into_iter()
            .map(|(key, _)| format!("{key:?}"))
            .collect()
    }

    #[test]
    fn a_shared_leg_puts_no_authentication_option_on_the_database_at_all() {
        // **ADC, and it is MANDATORY for this posture rather than a leftover.** A `shared-service-user`
        // source runs as the identity the deployment holds, which is what an absent auth option
        // means to the driver. Both arms of the XOR reach here, because the boot path carries no
        // caller whatever the source declared - `crate`'s `verify_anchor` says an executed anchor
        // reproduces for the identity this deployment holds.
        assert_eq!(keys(JobIdentity::Transport, &Impersonation::Disabled), Vec::<String>::new());
        assert_eq!(keys(JobIdentity::Transport, &impersonating()), Vec::<String>::new());
    }

    #[test]
    fn a_subject_at_an_impersonating_source_is_authenticated_by_its_own_assertion() {
        // The other arm of the XOR: three options that hand the driver a workload-identity
        // document, so Google's own token service verifies the caller's assertion. What the
        // document SAYS is `subject`'s own suite; what this holds is that these three keys and no
        // others are what an impersonating leg puts on the database.
        let assertion = Secret::new("a.caller.assertion");
        let sent = keys(JobIdentity::AsSubject { assertion: &assertion }, &impersonating());
        for key in [AUTH_TYPE, AUTH_CREDENTIALS_TYPE, AUTH_CREDENTIALS] {
            assert!(sent.iter().any(|sent| sent.contains(key)), "{key} is not sent: {sent:?}");
        }
        assert_eq!(sent.len(), 3, "{sent:?}");
        // And no impersonation option, which is the deleted mechanism: `target_principal` would run
        // the question as a declared account on the deployment's own connection.
        assert!(
            !sent.iter().any(|sent| sent.contains("impersonate")),
            "the deployment-vouches-for-a-subject path came back: {sent:?}"
        );
    }

    /// The options a Google-login leg puts on the database, as key and text value.
    fn google_login_options(token: &str) -> Vec<(String, String)> {
        let token = Secret::new(token);
        authenticate(JobIdentity::AsSubject { assertion: &token }, &Impersonation::GoogleLogin)
            .expect("a subject at a Google-login source is authenticated")
            .options
            .into_iter()
            .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
            .collect()
    }

    #[test]
    fn a_google_login_job_carries_the_access_token_option_and_no_credential_document() {
        let sent = google_login_options("ya29.caller-google-token");
        assert!(
            sent.iter()
                .any(|(key, value)| key.contains(AUTH_ACCESS_TOKEN) && value.contains("ya29.caller-google-token")),
            "{sent:?}"
        );
        assert!(
            !sent
                .iter()
                .any(|(key, _)| key.contains(AUTH_CREDENTIALS) || key.contains("impersonate")),
            "{sent:?}"
        );
    }

    #[test]
    fn the_access_token_option_is_set_only_with_the_auth_type_that_selects_it() {
        let sent = google_login_options("ya29.caller-google-token");
        let auth_types: Vec<&String> = sent
            .iter()
            .filter(|(key, _)| key.contains(&format!("\"{AUTH_TYPE}\"")))
            .map(|(_, value)| value)
            .collect();
        assert_eq!(auth_types.len(), 1, "{sent:?}");
        assert!(
            auth_types
                .iter()
                .all(|value| value.contains(&format!("\"{AUTH_TYPE_ACCESS_TOKEN}\""))),
            "{sent:?}"
        );
        assert_eq!(
            sent.iter().filter(|(key, _)| key.contains(AUTH_ACCESS_TOKEN)).count(),
            1,
            "{sent:?}"
        );
        assert_eq!(sent.len(), 2, "{sent:?}");
    }

    #[test]
    fn a_subject_at_a_source_that_declares_no_pool_is_refused_rather_than_run_as_the_deployment() {
        // **THE XOR's own refusal, and the direction that matters.** There is no pool to federate
        // the assertion against, so the only two things this could do are refuse or open a
        // connection the deployment authenticated - and the second is the fallback the owner
        // rejected. Unreachable through the domain port (`agrees_with` refuses a subject shape at a
        // shared source one layer up) and reachable through `JobTransport`, which is public, so it
        // is a refusal and not an `unreachable!`.
        let assertion = Secret::new("a.caller.assertion");
        let refused = authenticate(JobIdentity::AsSubject { assertion: &assertion }, &Impersonation::Disabled)
            .map(|_| ())
            .expect_err("a source that declares no pool cannot federate a subject");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
        assert!(!refused.to_string().contains("a.caller.assertion"), "{refused}");
    }

    #[test]
    fn the_two_modes_are_the_only_two_and_neither_degrades_into_the_other() {
        // **The XOR as a cell rather than as a sentence.** For `Disabled` and `ThroughPool` and
        // the three arms of `JobIdentity`, the pairs are six and every one is decided above: shared
        // and the boot path run as the deployment whatever the source declared, a subject federates,
        // a subject with no pool refuses, and an address refuses at both.
        // What no pair produces is *a subject's question answered as the deployment* - there is no
        // option list that is both non-empty and free of the credential document, and no arm that
        // returns an empty list for a subject.
        let assertion = Secret::new("a.caller.assertion");
        let federated = keys(JobIdentity::AsSubject { assertion: &assertion }, &impersonating());
        assert!(!federated.is_empty(), "a subject's leg authenticated as nobody");
        assert!(
            authenticate(JobIdentity::AsSubject { assertion: &assertion }, &Impersonation::Disabled).is_err(),
            "a subject's leg fell back to the deployment's identity"
        );
        // A principal switch is `JobIdentity::AsAddress`, and only a `DomainDelegation` source runs
        // one: `an_address_runs_only_at_a_delegating_source_and_only_an_address_runs_there`.
    }

    fn delegating() -> Impersonation {
        Impersonation::DomainDelegation(
            DelegatingAccount::parse("dwd-bigquery@acme-analytics.iam.gserviceaccount.com").expect("a service-account address"),
        )
    }

    #[test]
    fn a_domain_delegation_job_names_the_account_the_scope_and_the_address() {
        let address = PrincipalName::parse("Analyst.A@example.com").expect("a test name is a name");
        let sent: Vec<(String, String)> = authenticate(JobIdentity::AsAddress { address: &address }, &delegating())
            .expect("an address at a delegating source is authenticated")
            .options
            .into_iter()
            .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
            .collect();
        for (key, value) in [
            (IMPERSONATE_TARGET, "dwd-bigquery@acme-analytics.iam.gserviceaccount.com"),
            (IMPERSONATE_SCOPES, "https://www.googleapis.com/auth/bigquery"),
            (IMPERSONATE_SUBJECT, "Analyst.A@example.com"),
        ] {
            assert!(
                sent.iter()
                    .any(|(k, v)| k.contains(&format!("\"{key}\"")) && v.contains(&format!("\"{value}\""))),
                "{key} = {value} is not sent: {sent:?}"
            );
        }
        // No auth type: the base credential is the deployment's own, never a caller's token.
        assert_eq!(sent.len(), 3, "{sent:?}");
    }

    #[test]
    fn an_address_runs_only_at_a_delegating_source_and_only_an_address_runs_there() {
        let address = PrincipalName::parse("analyst@example.com").expect("a test name is a name");
        for impersonation in [Impersonation::Disabled, impersonating(), Impersonation::GoogleLogin] {
            let refused = authenticate(JobIdentity::AsAddress { address: &address }, &impersonation)
                .map(|_| ())
                .expect_err("an address runs only through domain-wide delegation");
            assert!(matches!(refused, AdbcError::Uncovered(_)), "{impersonation:?}: {refused:?}");
        }
        let assertion = Secret::new("a.caller.assertion");
        let refused = authenticate(JobIdentity::AsSubject { assertion: &assertion }, &delegating())
            .map(|_| ())
            .expect_err("a delegating source runs no caller credential");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
    }

    #[test]
    fn a_delegating_account_that_is_not_a_service_account_is_refused_where_it_is_sent() {
        for written in [
            "",
            "user@example.com",
            "dwd@iam.gserviceaccount.com",
            "dwd@acme-analytics",
            "@acme-analytics.iam.gserviceaccount.com",
            "a@b@acme-analytics.iam.gserviceaccount.com",
            "DWD@acme-analytics.iam.gserviceaccount.com",
            "dwd@acme.analytics.iam.gserviceaccount.com",
        ] {
            assert_eq!(
                DelegatingAccount::parse(written),
                Err(UnusableDelegatingAccount::Shape),
                "{written:?}"
            );
        }
    }
}
