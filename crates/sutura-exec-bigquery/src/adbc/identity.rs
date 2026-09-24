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
//! loopback source for the asking subject's own assertion, and the account that subject's
//! questions are declared to execute as - [`super::subject`] builds both and carries the whole
//! argument, including why a loopback URL rather than a file or an executable, and what the open
//! port's exposure actually is.
//!
//! # Two second hops, and only one of them is here
//!
//! **This one** is the pool's principal impersonating a declared account, authorized by
//! `roles/iam.workloadIdentityUser` **on that account**, bound to the pool subject the caller's
//! assertion resolves to - the grant `test-infra/pulumi/google` already declares, because that
//! role carries `iam.serviceAccounts.getAccessToken`. So F3 needs no new binding, and
//! `roles/iam.serviceAccountTokenCreator` - which the deleted hop needed - stays inapplicable. The
//! chain starts at a credential the CALLER possesses: no assertion, no token.
//!
//! **The deleted one** was `bigquery.impersonate.target_principal`, which impersonated a declared
//! account from the DEPLOYMENT's own application default credentials - the subject's own
//! credential was nowhere in that chain, and the owner rejected it. The option, the per-subject
//! account newtype and the scope newtype that fed it are gone rather than kept beside the
//! federating path: a fallback a misconfiguration could select is the defect, not a convenience.
//! What makes them different is not the URL, which is the same endpoint - it is **who authorizes
//! the call**, and in this one that is a principal only a verified caller can reach.
//!
//! # Why the pool's values are parsed HERE, when configuration already parsed them
//!
//! For the reason [`crate::transport::ProjectId`] is parsed a second time: this is the crate that
//! puts the value into a request, and a check belongs where the risk is. See
//! [`WorkloadPool::parse`] for the accepted sets and what they exclude.

use adbc_core::options::{OptionDatabase, OptionValue};
use sutura_domain::identity::PrincipalName;

use crate::principal::names_a_service_account;
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

/// The options one job's identity becomes, as the driver's database option pairs.
///
/// Named because `Result<Vec<(OptionDatabase, OptionValue)>, AdbcError>` is over this workspace's
/// `type_complexity` threshold - the same reason `crate::Mapped` exists.
type DatabaseOptions = Vec<(OptionDatabase, OptionValue)>;

/// Whether this source impersonates at all, and against which pool when it does.
///
/// **A two-variant type rather than an `Option<WorkloadPool>`, because the absence is a
/// DECLARATION.** A source is opened shared or impersonating - `sutura_config` refuses a
/// `workload_identity` block on a shared entry and refuses its absence on an impersonating one - so
/// which of these a transport holds is decided once, at composition, from a value an operator wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Impersonation {
    /// This source does not impersonate: every job runs as the identity the driver authenticates as.
    Disabled,
    /// This source impersonates, by federating the asking subject's assertion against this pool.
    ThroughPool(WorkloadPool),
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
/// deployment. [`AdbcError::UnusableTarget`] for a declared account this transport will not
/// interpolate into an impersonation URL. [`AdbcError::SubjectSource`] or
/// [`AdbcError::NoRandomness`] where the loopback source cannot be opened safely - all refusals,
/// never a weaker source.
pub(super) fn authenticate(identity: JobIdentity<'_>, impersonation: &Impersonation) -> Result<JobAuthentication, AdbcError> {
    match (identity, impersonation) {
        (JobIdentity::Transport, _) => Ok(JobAuthentication {
            options: Vec::new(),
            source: None,
        }),
        (JobIdentity::AsSubject { assertion, target }, Impersonation::ThroughPool(pool)) => {
            // **The narrowing, at the point of sending and not only at the point of declaring.**
            // `target` becomes ONE PATH SEGMENT of the URL that decides which account this
            // question runs as, and the Go library POSTs that URL verbatim with no scheme, host or
            // shape check - so a value carrying `/` re-points the segment at a different account.
            // `PrincipalName::parse` accepts `/`, because it is the parser every principal
            // identifier in the domain shares. `DeclaredPrincipals::parse` refuses this at BOOT
            // for the broker this crate ships, and `Presented` is a public port any broker can
            // construct, so the check belongs at both ends - the doctrine
            // `crate::transport::ProjectId` and `WorkloadPool::parse` already follow.
            if !names_a_service_account(target) {
                return Err(AdbcError::UnusableTarget);
            }
            let source = SubjectSource::bind(assertion)?;
            Ok(JobAuthentication {
                options: credential_options(&source, pool, target),
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
fn credential_options(source: &SubjectSource, pool: &WorkloadPool, target: &PrincipalName) -> DatabaseOptions {
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
            OptionValue::String(String::from(source.document(pool, target).expose_secret())),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::Secret;

    use super::{AUTH_CREDENTIALS, AUTH_CREDENTIALS_TYPE, AUTH_TYPE, Impersonation, authenticate};
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
        let sent = keys(
            JobIdentity::AsSubject {
                assertion: &assertion,
                target: &crate::adbc::a_declared_account(),
            },
            &impersonating(),
        );
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
            JobIdentity::AsSubject {
                assertion: &assertion,
                target: &crate::adbc::a_declared_account(),
            },
            &Impersonation::Disabled,
        )
        .map(|_| ())
        .expect_err("a source that declares no pool cannot federate a subject");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
        assert!(!refused.to_string().contains("a.caller.assertion"), "{refused}");
    }

    #[test]
    fn the_two_modes_are_the_only_two_and_neither_degrades_into_the_other() {
        // **The XOR as a cell rather than as a sentence.** `Impersonation` has two variants and
        // `JobIdentity` has two arms that can reach this function, so the pairs are four and every
        // one is decided above: shared runs as the deployment, a subject federates, a subject with
        // no pool refuses, and the boot path runs as the deployment whatever the source declared.
        // What no pair produces is *a subject's question answered as the deployment* - there is no
        // option list that is both non-empty and free of the credential document, and no arm that
        // returns an empty list for a subject.
        let assertion = Secret::new("a.caller.assertion");
        let federated = keys(
            JobIdentity::AsSubject {
                assertion: &assertion,
                target: &crate::adbc::a_declared_account(),
            },
            &impersonating(),
        );
        assert!(!federated.is_empty(), "a subject's leg authenticated as nobody");
        assert!(
            authenticate(
                JobIdentity::AsSubject {
                    assertion: &assertion,
                    target: &crate::adbc::a_declared_account(),
                },
                &Impersonation::Disabled
            )
            .is_err(),
            "a subject's leg fell back to the deployment's identity"
        );
        // And a principal switch has no spelling at all any more: `JobIdentity` declares two arms
        // and neither carries a `PrincipalName`, so the weaker mechanism is unrepresentable rather
        // than refused. **That half is held by the enum and by this function's exhaustive match, not
        // by this cell** - a `PrincipalName::parse` round trip used to sit here commented as "the
        // compile-time half", and it asserted nothing about `JobIdentity` at all.
    }
}
