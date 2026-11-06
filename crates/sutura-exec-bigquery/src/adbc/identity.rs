//! Which authentication options one job's identity becomes, and the refusal for the pair that has
//! none.
//!
//! **The whole of this transport's identity decision, in a function that touches no driver**: it is
//! what `connect` calls before the `.so` is loaded, so a request this transport cannot authenticate
//! never opens a connection it could answer as somebody else.
//!
//! # The mechanism
//!
//! An impersonating source exchanges the asking subject's own assertion at Google's token service
//! through the injected [`PoolExchange`] - in this process, before the driver is loaded - and hands
//! the driver the access token that comes back. No second hop is asked for: that token IS the
//! subject's own pool principal, and `BigQuery`'s grants on that principal decide what it reads. A
//! Google-login source hands the driver the subject's own access token too, the one the identity
//! provider stored when the subject signed in to Google. A failed exchange is a refusal before any
//! connection exists (`docs/adr/20261102095553-bigquery-pool-exchange-in-process.md`).
//!
//! **The driver never refreshes an access token**, so a question that runs past the token's
//! lifetime fails mid-query rather than renewing.
//!
//! A split leg exchanges twice ([`split`]): its job runs on a token asked for [`PoolScope::Query`],
//! and only a second one, asked for [`PoolScope::ReadOnly`], is forwarded. A partition read hands
//! the driver that forwarded token alone ([`forwarded`]), at a pool source only.
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

use std::time::SystemTime;

use adbc_core::options::{OptionDatabase, OptionValue};
use sutura_domain::identity::{PoolExchange, PoolScope, PrincipalName, Secret, SourceToken};

use crate::transport::JobIdentity;

use super::AdbcError;
use super::subject::WorkloadPool;

/// The driver option naming which authentication shape the database uses: pinned `go/v1.14.1`'s
/// `OptionAuthType`.
const AUTH_TYPE: &str = "bigquery.auth_type";

/// The value of [`AUTH_TYPE`] that takes a bare access token, which the driver never refreshes.
///
/// The vendored driver patch's `OptionValueAuthTypeAccessToken` (`VENDOR.md`).
const AUTH_TYPE_ACCESS_TOKEN: &str = "access_token";

/// The driver option carrying that access token: the patch's `OptionAuthAccessToken`.
const AUTH_ACCESS_TOKEN: &str = "bigquery.auth.access_token";

/// The account the deployment's own credential impersonates: pinned `go/v1.14.1`'s
/// `OptionImpersonateTargetPrincipal`.
const IMPERSONATE_TARGET: &str = "bigquery.impersonate.target_principal";

/// The scopes the impersonated token carries: `OptionImpersonateScopes`.
const IMPERSONATE_SCOPES: &str = "bigquery.impersonate.scopes";

/// The user the delegating account acts as: the vendored driver patch's `OptionImpersonateSubject`.
const IMPERSONATE_SUBJECT: &str = "bigquery.impersonate.subject";

/// The one scope a delegated job asks for. Google refuses `jobs.insert`, `jobs.cancel` and so any
/// write job with it, and accepts `jobs.query`, `jobs.get`, `jobs.getQueryResults` and the Storage
/// Read API (measured with a service account's own token, not a delegated one).
const BIGQUERY_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/bigquery.readonly";

/// The vendored driver patch's `OptionQueryThroughJobsQuery`: every query starts with `jobs.query`,
/// the one call that starts a job under [`BIGQUERY_READONLY_SCOPE`].
const QUERY_THROUGH_JOBS_QUERY: &str = "bigquery.query.through_jobs_query";

/// The options one job's identity becomes, as the driver's database option pairs.
///
/// Named because `Result<Vec<(OptionDatabase, OptionValue)>, AdbcError>` is over this workspace's
/// `type_complexity` threshold - the same reason `crate::Mapped` exists.
pub(super) type DatabaseOptions = Vec<(OptionDatabase, OptionValue)>;

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
    /// This source impersonates, by exchanging the asking subject's assertion at this pool for the
    /// subject's own access token.
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

/// Everything a job's identity puts on the database.
pub(super) struct JobAuthentication {
    /// What the database is opened with.
    pub(super) options: DatabaseOptions,
}

/// The database options one job's identity becomes.
///
/// **Empty options for [`JobIdentity::Transport`]**, which is the shared posture and the boot path:
/// the driver authenticates as the process. An impersonating source reaching that arm is correct
/// rather than a miss - `verify_anchor` and a fixture load carry no caller, and `crate`'s
/// `verify_anchor` documentation already says an executed anchor here reproduces for the identity
/// this deployment holds.
///
/// # Errors
///
/// [`AdbcError::Uncovered`] for a subject at a source that declares no pool, or at a pool no
/// exchange was composed for: there is nothing to federate the assertion through, and opening the
/// connection anyway would run the question as the deployment. [`AdbcError::PoolExchange`] where
/// Google's token service issued no usable token - all refusals, never a weaker credential.
pub(super) fn authenticate(
    identity: JobIdentity<'_>,
    impersonation: &Impersonation,
    exchange: Option<&dyn PoolExchange>,
) -> Result<JobAuthentication, AdbcError> {
    let options = match (identity, impersonation) {
        (JobIdentity::Transport, _) => Vec::new(),
        (JobIdentity::AsSubject { assertion }, Impersonation::GoogleLogin) => access_token_options(assertion),
        (JobIdentity::AsAddress { address }, Impersonation::DomainDelegation(account)) => delegation_options(account, address),
        // A subject's own credential at a delegating source, or an address anywhere else: in-crate
        // misuse insurance, for the reason the arm below gives.
        (JobIdentity::AsSubject { .. }, Impersonation::DomainDelegation(_))
        | (JobIdentity::AsAddress { .. }, Impersonation::Disabled | Impersonation::ThroughPool(_) | Impersonation::GoogleLogin) =>
        {
            return Err(AdbcError::Uncovered(
                "run a subject by a mechanism this source does not declare",
            ));
        }
        (JobIdentity::AsSubject { assertion }, Impersonation::ThroughPool(pool)) => {
            let exchange = exchange.ok_or(AdbcError::Uncovered(
                "federate a subject at a pool no token exchange was composed for",
            ))?;
            let token = exchange
                .exchange(assertion, pool, PoolScope::Query)
                .map_err(|cause| AdbcError::PoolExchange { cause })?
                .into_token();
            access_token_options(&token)
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
            return Err(AdbcError::Uncovered("federate a subject at a source that declares no pool"));
        }
    };
    Ok(JobAuthentication { options })
}

/// The database options one partition read becomes: the forwarded `token` and nothing else.
///
/// Only a source that federates at a pool reads one, because only such a source splits: at a
/// shared, Google-login or delegating source a caller's forwarded token would read as a principal
/// that source's posture does not declare.
///
/// # Errors
///
/// [`AdbcError::Uncovered`] at a source that federates at no pool.
pub(super) fn forwarded(token: &Secret, impersonation: &Impersonation) -> Result<JobAuthentication, AdbcError> {
    match *impersonation {
        Impersonation::ThroughPool(_) => Ok(JobAuthentication {
            options: access_token_options(token),
        }),
        Impersonation::Disabled | Impersonation::GoogleLogin | Impersonation::DomainDelegation(_) => Err(AdbcError::Uncovered(
            "read a forwarded token at a source that federates at no pool",
        )),
    }
}

/// What a split leg's job runs on, and the token forwarded to the nodes that read its streams.
pub(super) struct Split {
    /// The job's database options: the first exchange's token, asked for [`PoolScope::Query`], so
    /// the job can still be cancelled.
    pub(super) options: DatabaseOptions,
    /// The second exchange's token, asked for [`PoolScope::ReadOnly`]: the only one that leaves,
    /// with the fingerprint of the assertion it was exchanged from.
    pub(super) forwarded: SourceToken,
    /// `ends`, or the forwarded token's expiry where that is earlier.
    pub(super) until: SystemTime,
}

/// Both exchanges a split leg makes, before any connection exists, and when what it forwards ends.
///
/// **Two tokens for one subject, both live until Google's own expiry.** The forwarded one is asked
/// for [`PoolScope::ReadOnly`], which still runs read queries, and nothing here shortens its
/// lifetime. `until` is computed for a grant to end by; no grant reads it yet.
///
/// # Errors
///
/// [`AdbcError::Uncovered`] for anything but a subject at a pool source with an exchange composed,
/// decided before either exchange. [`AdbcError::PoolExchange`] where either exchange failed, so a
/// split never forwards the job's own token instead.
pub(super) fn split(
    identity: JobIdentity<'_>,
    impersonation: &Impersonation,
    exchange: Option<&dyn PoolExchange>,
    ends: SystemTime,
) -> Result<Split, AdbcError> {
    let (JobIdentity::AsSubject { assertion }, Impersonation::ThroughPool(pool)) = (identity, impersonation) else {
        return Err(AdbcError::Uncovered("split a leg that federates no subject at a pool"));
    };
    let exchange = exchange.ok_or(AdbcError::Uncovered(
        "federate a subject at a pool no token exchange was composed for",
    ))?;
    let exchanged = |scope| {
        exchange
            .exchange(assertion, pool, scope)
            .map_err(|cause| AdbcError::PoolExchange { cause })
    };
    let query = exchanged(PoolScope::Query)?;
    let forwarded = SourceToken::exchanged(assertion, pool, exchanged(PoolScope::ReadOnly)?);
    let until = forwarded.not_after().min(ends);
    Ok(Split {
        options: access_token_options(&query.into_token()),
        forwarded,
        until,
    })
}

/// The two options that hand the driver one subject's access token: the token and the auth type
/// that selects it, only ever together - the driver reads the token under no other auth type.
#[expect(
    clippy::disallowed_methods,
    reason = "a driver option is a plain string across the C ABI, so a token is exposed here once \
              per database, never logged: a job's in `authenticate` for `connect`, a split job's \
              full-scope one in `split` through `Split` to `connect_as`, and a forwarded one in \
              `forwarded` for `read_partition`"
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

/// The four options that run one job as `address` through domain-wide delegation, only ever
/// together: the delegating account, the read-only scope, the address, and the `jobs.query` path
/// that scope can start a job on. No auth type is set, so the base credential is the deployment's
/// own application default credentials, which sign as the account.
///
/// **The limit of the read-only scope is cancellation.** Google refuses `jobs.cancel` with it, so a
/// job this transport stops waiting for at the caller's deadline runs on to its end or to
/// `jobTimeoutMs`: `maximumBytesBilled` still caps its bytes, nothing caps its slot time.
fn delegation_options(account: &DelegatingAccount, address: &PrincipalName) -> DatabaseOptions {
    [
        (IMPERSONATE_TARGET, account.0.as_str()),
        (IMPERSONATE_SCOPES, BIGQUERY_READONLY_SCOPE),
        (IMPERSONATE_SUBJECT, address.as_str()),
        (QUERY_THROUGH_JOBS_QUERY, "true"),
    ]
    .into_iter()
    .map(|(key, value)| (OptionDatabase::Other(key.into()), OptionValue::String(String::from(value))))
    .collect()
}

/// A compile-time guard that neither `JobAuthentication` nor `Split` implements `Debug`.
///
/// `JobAuthentication.options` is `DatabaseOptions` - `Vec<(OptionDatabase, OptionValue)>` from
/// `adbc_core` - and `access_token_options` is the one place an access token is exposed as text,
/// into an `OptionValue::String`. `adbc_core`'s `OptionValue` derives `Debug`, so a
/// `#[derive(Debug)]` on `JobAuthentication` would print the subject's token. `Split.options` is
/// the same type, holding a split job's full-scope token.
///
/// Both are `pub(super)`, so a `compile_fail` doctest cannot name them - this guard is the
/// mechanism that replaces review for a type no external crate can reach. It uses the same
/// negative-impl ambiguity trick: one blanket impl for all `T`, one for `T: Debug`, so adding any
/// `Debug` impl to either makes its `<_ as AmbiguousIfImpl<_>>::some_item` ambiguous (E0283) and
/// the crate does not build - measured for `Split` by deriving `Debug` on it, which failed the
/// check with E0283 at its line.
///
/// **Limit:** this forbids `Debug` and nothing else. The `options` field is a foreign `Vec` whose
/// `Debug` already prints the token; nothing here stops a `tracing`/`format!` call that reaches
/// `options` directly, or a hand-written `Display` on either type. Those are held by review, not by
/// this guard.
mod no_debug {
    trait AmbiguousIfImpl<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
    impl<T: ?Sized + core::fmt::Debug> AmbiguousIfImpl<u8> for T {}

    // Compiles only while neither implements Debug. `Split.options` holds the split job's
    // full-scope token as text, as `JobAuthentication.options` holds a job's.
    const _: fn() = || {
        let _ = <super::JobAuthentication as AmbiguousIfImpl<_>>::some_item;
        let _ = <super::Split as AmbiguousIfImpl<_>>::some_item;
    };
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{PrincipalName, Secret};

    use super::{
        AUTH_ACCESS_TOKEN, AUTH_TYPE, AUTH_TYPE_ACCESS_TOKEN, DelegatingAccount, IMPERSONATE_SCOPES, IMPERSONATE_SUBJECT,
        IMPERSONATE_TARGET, Impersonation, QUERY_THROUGH_JOBS_QUERY, UnusableDelegatingAccount, authenticate,
    };
    use crate::adbc::AdbcError;
    use crate::adbc::subject::WorkloadPool;
    use crate::transport::JobIdentity;

    /// The deleted credential-document option, named so no access-token leg can bring it back.
    const AUTH_CREDENTIALS: &str = "bigquery.auth.credentials";

    fn impersonating() -> Impersonation {
        Impersonation::ThroughPool(
            WorkloadPool::parse("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/a/providers/sso")
                .expect("a provider resource is usable"),
        )
    }

    /// The option keys one identity puts on the database, as text.
    fn keys(identity: JobIdentity<'_>, impersonation: &Impersonation) -> Vec<String> {
        authenticate(identity, impersonation, None)
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

    /// The options a Google-login leg puts on the database, as key and text value.
    fn google_login_options(token: &str) -> Vec<(String, String)> {
        let token = Secret::new(token);
        authenticate(
            JobIdentity::AsSubject { assertion: &token },
            &Impersonation::GoogleLogin,
            None,
        )
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
        let refused = authenticate(
            JobIdentity::AsSubject { assertion: &assertion },
            &Impersonation::Disabled,
            None,
        )
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
        // What no pair produces is *a subject's question answered as the deployment*: a subject
        // with no pool, or at a pool no exchange was composed for, is refused rather than handed an
        // empty option list. The federated arm's two options are `super::super::pool_cells`'.
        let assertion = Secret::new("a.caller.assertion");
        for impersonation in [Impersonation::Disabled, impersonating()] {
            let refused = authenticate(JobIdentity::AsSubject { assertion: &assertion }, &impersonation, None)
                .map(|_| ())
                .expect_err("a subject's leg fell back to the deployment's identity");
            assert!(matches!(refused, AdbcError::Uncovered(_)), "{impersonation:?}: {refused:?}");
        }
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
        let sent: Vec<(String, String)> = authenticate(JobIdentity::AsAddress { address: &address }, &delegating(), None)
            .expect("an address at a delegating source is authenticated")
            .options
            .into_iter()
            .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
            .collect();
        for (key, value) in [
            (IMPERSONATE_TARGET, "dwd-bigquery@acme-analytics.iam.gserviceaccount.com"),
            (IMPERSONATE_SCOPES, "https://www.googleapis.com/auth/bigquery.readonly"),
            (IMPERSONATE_SUBJECT, "Analyst.A@example.com"),
            (QUERY_THROUGH_JOBS_QUERY, "true"),
        ] {
            assert!(
                sent.iter()
                    .any(|(k, v)| k.contains(&format!("\"{key}\"")) && v.contains(&format!("\"{value}\""))),
                "{key} = {value} is not sent: {sent:?}"
            );
        }
        // No auth type: the base credential is the deployment's own, never a caller's token. And no
        // second scope: the read-only one is the only one asked for.
        assert_eq!(sent.len(), 4, "{sent:?}");
    }

    #[test]
    fn an_address_runs_only_at_a_delegating_source_and_only_an_address_runs_there() {
        let address = PrincipalName::parse("analyst@example.com").expect("a test name is a name");
        for impersonation in [Impersonation::Disabled, impersonating(), Impersonation::GoogleLogin] {
            let refused = authenticate(JobIdentity::AsAddress { address: &address }, &impersonation, None)
                .map(|_| ())
                .expect_err("an address runs only through domain-wide delegation");
            assert!(matches!(refused, AdbcError::Uncovered(_)), "{impersonation:?}: {refused:?}");
        }
        let assertion = Secret::new("a.caller.assertion");
        let refused = authenticate(JobIdentity::AsSubject { assertion: &assertion }, &delegating(), None)
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
