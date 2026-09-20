//! Who one job runs as, as driver options - and the two values that are parsed before they become
//! one.
//!
//! **The whole of this transport's identity decision, in a function that touches no driver**, which
//! is what makes it assertable: `connect` cannot reach a network or a `.so` without first calling
//! [`identity_options`], and [`identity_options`] is a pure map from one
//! [`JobIdentity`] to the options that job is opened with.
//!
//! # The mechanism, and exactly what it buys
//!
//! `bigquery.impersonate.target_principal` names a service account, and the driver mints a
//! credential for it by impersonation - `google.golang.org/api/impersonate`, from the process's own
//! application default credentials, checked in the pinned `go/v1.13.0` source. So the data system
//! evaluates the statement as that account: a per-subject grant, a per-subject row filter and
//! `SESSION_USER()` all resolve to the account rather than to this deployment.
//!
//! **What it does not buy:** the asking subject's own credential is not in that chain at all. The
//! subject's verified identity picked the account; the deployment's own identity is what authorized
//! becoming it. `crate`'s own header states the consequence beside the claim, and
//! `docs/adr/0018`'s fifth amendment prices the two alternatives that were considered instead.
//!
//! # Why both values are parsed HERE, when configuration already parsed them
//!
//! For the reason [`crate::transport::ProjectId`] is parsed a second time: this is the crate that
//! puts the value into a request, and a check belongs where the risk is. The risk is specific and
//! it is the driver's own: `bigquery.impersonate.scopes` and `bigquery.impersonate.delegates` are
//! **split on commas** by the driver, so a scope carrying a comma would silently become two scopes -
//! one of them attacker-chosen if a declaration ever came from somewhere less trusted than an
//! operator's own settings file. [`ImpersonationScopes::parse`] refuses the comma rather than
//! documenting it.

use adbc_core::options::{OptionDatabase, OptionValue};

use crate::transport::JobIdentity;

use super::AdbcError;

/// The driver option naming the account a job is to be executed as.
///
/// Pinned `go/v1.13.0`'s `OptionImpersonateTargetPrincipal`. Written once, here, so the spelling the
/// transport sends and the spelling its tests assert cannot drift.
const TARGET_PRINCIPAL: &str = "bigquery.impersonate.target_principal";

/// The driver option naming the scopes an impersonated credential is minted for.
///
/// Pinned `go/v1.13.0`'s `OptionImpersonateScopes`. **Required, not optional** - the driver's
/// impersonation path returns `impersonate: scopes must be provided` for an empty list, so a job
/// opened with a target and no scopes fails at the driver rather than running.
const SCOPES: &str = "bigquery.impersonate.scopes";

/// The options one job's identity becomes, as the driver's database option pairs.
///
/// Named because `Result<Vec<(OptionDatabase, OptionValue)>, AdbcError>` is over this workspace's
/// `type_complexity` threshold - the same reason `crate::Mapped` exists.
type DatabaseOptions = Vec<(OptionDatabase, OptionValue)>;

/// Why a value this transport was about to send is not one it can send.
///
/// **Positions, never the value**, for the reason `sutura_config`'s own refusal about the same text
/// carries one: these are operator-written strings on their way into a request, and neither a log
/// nor an error body is a place for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnusableIdentityOption {
    /// There was nothing there.
    #[error("the {what} this transport was to send is empty")]
    Empty {
        /// Which of the two values.
        what: &'static str,
    },
    /// Longer than the value's own bound.
    #[error("the {what} this transport was to send is {found} characters and at most {most} are usable")]
    TooLong {
        /// Which of the two values.
        what: &'static str,
        /// How long it was.
        found: usize,
        /// The bound.
        most: usize,
    },
    /// A character outside the accepted set, at a position.
    #[error("the {what} this transport was to send carries an unusable character at {at}")]
    Character {
        /// Which of the two values.
        what: &'static str,
        /// Where, so an operator can find it without the refusal quoting it.
        at: usize,
    },
}

/// The account one job is executed as.
///
/// A newtype rather than a `&str` for the reason every identifier in this crate is one: it reaches a
/// request, so *an instance exists* has to mean *this is sendable*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetAccount(String);

impl TargetAccount {
    /// A service-account mailbox, bounded by RFC 5321.
    const MOST: usize = 254;

    /// What a refusal calls this value.
    const WHAT: &'static str = "impersonation target";

    /// Parses an account this transport may name as a job's principal.
    ///
    /// The accepted set is the printable ASCII a service-account address is built from - letters,
    /// digits and `. - _ @`, exactly one `@`. The comma is outside it, which is what keeps a target
    /// out of the driver's comma-split option parsing, and so is every character that could close a
    /// value in the driver's own option map.
    ///
    /// # Errors
    ///
    /// [`UnusableIdentityOption`], which carries a position and never the text.
    pub fn parse(raw: &str) -> Result<Self, UnusableIdentityOption> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(UnusableIdentityOption::Empty { what: Self::WHAT });
        }
        if trimmed.chars().count() > Self::MOST {
            return Err(UnusableIdentityOption::TooLong {
                what: Self::WHAT,
                found: trimmed.chars().count(),
                most: Self::MOST,
            });
        }
        if trimmed.matches('@').count() != 1 {
            return Err(UnusableIdentityOption::Character {
                what: Self::WHAT,
                at: trimmed.chars().count(),
            });
        }
        if let Some(at) = trimmed
            .char_indices()
            .find_map(|(at, c)| (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '@')).then_some(at))
        {
            return Err(UnusableIdentityOption::Character { what: Self::WHAT, at });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The account, for the option value.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The scopes an impersonated credential is minted for, as the driver takes them.
///
/// **One declared scope and not a list**, because that is what a source declares
/// (`sources.<alias>.workload_identity.scope`) and because the driver's own parsing of this option
/// is a comma split - so a type that accepted several would have to render the separator the parse
/// below refuses. A deployment needing two scopes is a change to the settings tree first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpersonationScopes(String);

impl ImpersonationScopes {
    /// A scope is a URL, so the bound is a URL's rather than an identifier's.
    const MOST: usize = 1024;

    /// What a refusal calls this value.
    const WHAT: &'static str = "impersonation scope";

    /// Parses the scope this transport mints impersonated credentials for.
    ///
    /// **Parsed at COMPOSITION and not per request**, which is the point of it being a field on the
    /// transport: a deployment whose declared scope is unusable fails to start rather than failing
    /// every impersonated question. The accepted set is a URL's, minus the comma - see this module's
    /// header for why the comma is the character that matters here.
    ///
    /// # Errors
    ///
    /// [`UnusableIdentityOption`], which carries a position and never the text.
    pub fn parse(raw: &str) -> Result<Self, UnusableIdentityOption> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(UnusableIdentityOption::Empty { what: Self::WHAT });
        }
        if trimmed.chars().count() > Self::MOST {
            return Err(UnusableIdentityOption::TooLong {
                what: Self::WHAT,
                found: trimmed.chars().count(),
                most: Self::MOST,
            });
        }
        if let Some(at) = trimmed.char_indices().find_map(|(at, c)| {
            (!matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | ':' | '.' | '-' | '_' | '%')).then_some(at)
        }) {
            return Err(UnusableIdentityOption::Character { what: Self::WHAT, at });
        }
        Ok(Self(String::from(trimmed)))
    }

    /// The scope, for the option value.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether this source impersonates at all, and at what scope when it does.
///
/// **A two-variant type rather than an `Option<ImpersonationScopes>`, because the absence is a
/// DECLARATION.** A source is opened shared or impersonating - `sutura_config` refuses a
/// `workload_identity` block on a shared entry and refuses its absence on an impersonating one - so
/// which of these a transport holds is decided once, at composition, from a value an operator wrote.
/// `None` would have needed a reader to decide what a missing scope permits, and the honest answer
/// (*invent the client library's default and hope*) is what this type exists not to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Impersonation {
    /// This source does not impersonate: every job runs as the identity the driver authenticates as.
    Disabled,
    /// This source impersonates, and an impersonated credential is minted for this scope.
    AtScope(ImpersonationScopes),
}

/// The database options one job's identity becomes.
///
/// **Empty for [`JobIdentity::Transport`]**, which is the shared posture and the boot path: no
/// impersonation option at all, so the driver authenticates as the process. A non-empty answer is
/// derived from THIS call's `identity` and from nothing else - there is no memo, no cache and no
/// previous request to fall back to, which is what keeps one subject's principal off another
/// subject's job.
///
/// # Errors
///
/// [`AdbcError::Uncovered`] for [`JobIdentity::AsBearer`]: the pinned driver has no option that
/// accepts a caller's own access token, and the wrong answer here is not a compromise but a
/// regression - dropping the material and opening the connection anyway would run another
/// principal's question under this deployment's identity while provenance reported it as
/// impersonated. [`AdbcError::UnusableTarget`] where a declared principal is not an account this
/// transport can name.
pub(super) fn identity_options(identity: JobIdentity<'_>, impersonation: &Impersonation) -> Result<DatabaseOptions, AdbcError> {
    match (identity, impersonation) {
        // No impersonation option at all, so the driver authenticates as the process. Both the
        // shared posture and the BOOT path land here - `verify_anchor` and a fixture load carry no
        // caller - which is why an impersonating source reaching this arm is correct rather than a
        // miss: its anchors reproduce for the identity this deployment holds, and `crate`'s
        // `verify_anchor` documentation already says that is narrower than a caller's.
        (JobIdentity::Transport, _) => Ok(Vec::new()),
        (JobIdentity::AsPrincipal(name), Impersonation::AtScope(scopes)) => {
            let target = TargetAccount::parse(name.as_str()).map_err(|cause| AdbcError::UnusableTarget { cause })?;
            Ok(vec![
                (
                    OptionDatabase::Other(TARGET_PRINCIPAL.into()),
                    OptionValue::String(String::from(target.as_str())),
                ),
                (
                    OptionDatabase::Other(SCOPES.into()),
                    OptionValue::String(String::from(scopes.as_str())),
                ),
            ])
        }
        // **A principal at a source that does not impersonate.** Unreachable through the domain port
        // - `Presented::agrees_with` refuses a subject shape at a source declared shared, one layer
        // up, before this is called - but reachable through `JobTransport` itself, which is public.
        // So it is a refusal and not an `unreachable!`: opening the connection would run the
        // question as the deployment while the name said otherwise, and there is no scope declared
        // to mint a credential for the named account with anyway.
        (JobIdentity::AsPrincipal(..), Impersonation::Disabled) => {
            Err(AdbcError::Uncovered("execute as a principal at a source opened shared"))
        }
        (JobIdentity::AsBearer(..), _) => Err(AdbcError::Uncovered("present the asking subject's own bearer credential")),
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::identity::{PrincipalName, Secret};

    use super::{Impersonation, ImpersonationScopes, TargetAccount, UnusableIdentityOption, identity_options};
    use crate::adbc::AdbcError;
    use crate::transport::JobIdentity;

    fn scopes() -> Impersonation {
        Impersonation::AtScope(
            ImpersonationScopes::parse("https://www.googleapis.com/auth/cloud-platform").expect("a URL scope is usable"),
        )
    }

    /// The option keys, read back as text, so a test can assert what the driver would be told.
    fn keys_and_values(identity: JobIdentity<'_>) -> Vec<(String, String)> {
        identity_options(identity, &scopes())
            .expect("this identity is one this transport sends")
            .into_iter()
            .map(|(key, value)| {
                let adbc_core::options::OptionValue::String(text) = value else {
                    panic!("every identity option this transport sends is a string")
                };
                (format!("{key:?}"), text)
            })
            .collect()
    }

    #[test]
    fn a_shared_leg_is_opened_with_no_impersonation_option_at_all() {
        // The control, and it is the one that makes every other cell here mean something: an
        // absent option is what makes the driver authenticate as the process, so a transport that
        // sent a target for a shared leg would run the deployment's own question as somebody.
        assert_eq!(keys_and_values(JobIdentity::Transport), Vec::new());
    }

    #[test]
    fn a_declared_principal_becomes_the_drivers_own_impersonation_target_and_its_scope() {
        let name = PrincipalName::parse("analyst-a@sutura.example.com").expect("an address is a name");
        let sent = keys_and_values(JobIdentity::AsPrincipal(&name));
        // The exact key spellings the pinned driver reads, asserted rather than assumed: a
        // misspelled `OptionDatabase::Other` key is refused by the driver at connect time, which is
        // a per-question failure in a deployment that booted clean.
        let rendered: Vec<String> = sent.iter().map(|(key, _)| key.clone()).collect();
        assert!(
            rendered
                .iter()
                .any(|key| key.contains("bigquery.impersonate.target_principal")),
            "{rendered:?}"
        );
        assert!(
            rendered.iter().any(|key| key.contains("bigquery.impersonate.scopes")),
            "{rendered:?}"
        );
        assert_eq!(
            sent.iter().map(|(_, value)| value.as_str()).collect::<Vec<&str>>(),
            vec![
                "analyst-a@sutura.example.com",
                "https://www.googleapis.com/auth/cloud-platform"
            ]
        );
    }

    #[test]
    fn one_subjects_principal_never_appears_in_the_next_subjects_options() {
        // **The cross-subject property, as a cell rather than as a paragraph.** Nothing about this
        // transport is shared between two jobs - no connection, no database handle, no memoised
        // option list - and this is the assertion that dies if any of that is introduced: a memo
        // keyed on anything but the identity would answer the first subject's target for the
        // second. Two principals, in sequence, through the one function that decides.
        let first = PrincipalName::parse("analyst-a@sutura.example.com").expect("an address is a name");
        let second = PrincipalName::parse("analyst-b@sutura.example.com").expect("an address is a name");
        let values = |name: &PrincipalName| {
            keys_and_values(JobIdentity::AsPrincipal(name))
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Vec<String>>()
        };
        let before = values(&first);
        let after = values(&second);
        assert!(after.iter().any(|value| value == "analyst-b@sutura.example.com"), "{after:?}");
        assert!(
            !after.iter().any(|value| value == "analyst-a@sutura.example.com"),
            "the second subject's job was opened carrying the first subject's principal: {after:?}"
        );
        // And the other direction, so a swap that answered the LAST target for everybody is caught
        // too rather than only a memo of the first.
        assert!(
            before.iter().any(|value| value == "analyst-a@sutura.example.com"),
            "{before:?}"
        );
    }

    #[test]
    fn a_subjects_own_bearer_credential_is_refused_rather_than_dropped() {
        // The refusal the removed HTTP transport made unnecessary and this one has to make: the
        // pinned driver has no option that accepts a caller's access token. Opening the connection
        // anyway would submit this question under the deployment's identity while the answer's
        // provenance said the asker - every row as the process, recorded as somebody else.
        let material = Secret::new("an-exchanged-access-token");
        let refused = identity_options(JobIdentity::AsBearer(&material), &scopes())
            .expect_err("a bearer is not an identity this transport can send");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
        // And the refusal names the capability without quoting the credential.
        let said = refused.to_string();
        assert!(!said.contains("an-exchanged-access-token"), "{said}");
    }

    #[test]
    fn a_principal_that_is_not_an_account_this_transport_can_name_is_refused_before_any_connect() {
        // `PrincipalName` is the DOMAIN's parse - a role name passes it, because a data system with
        // `SET ROLE` takes one. This transport sends the value to an impersonation endpoint that
        // takes a service-account address, so it parses again, here, where the risk is.
        let role = PrincipalName::parse("analyst_role").expect("a role is a domain principal name");
        let refused = identity_options(JobIdentity::AsPrincipal(&role), &scopes()).expect_err("a bare role is not an account");
        assert!(matches!(refused, AdbcError::UnusableTarget { .. }), "{refused:?}");
    }

    #[test]
    fn a_scope_carrying_the_drivers_own_separator_is_refused_at_composition() {
        // The comma is the character that matters, and it matters because of the DRIVER: it splits
        // this option on commas, so a value carrying one is two scopes rather than an unusable one.
        // Refused at parse, which is composition time, so it cannot become a per-question failure.
        let refused = ImpersonationScopes::parse("https://example.com/auth/a,https://example.com/auth/b")
            .expect_err("a comma is the driver's own separator");
        assert!(matches!(refused, UnusableIdentityOption::Character { .. }), "{refused:?}");
        for empty in ["", "   "] {
            assert!(
                matches!(ImpersonationScopes::parse(empty), Err(UnusableIdentityOption::Empty { .. })),
                "an empty scope is the driver's own `scopes must be provided` failure, moved to boot"
            );
        }
    }

    #[test]
    fn a_principal_at_a_source_opened_shared_is_refused_rather_than_run_as_the_deployment() {
        // The pair the domain port cannot produce and `JobTransport` can: a name to become, at a
        // source whose declaration named no scope to become it at. Refused, because a connection
        // opened here would answer as the deployment under somebody else's name.
        let name = PrincipalName::parse("analyst-a@sutura.example.com").expect("an address is a name");
        let refused = identity_options(JobIdentity::AsPrincipal(&name), &Impersonation::Disabled)
            .expect_err("a source opened shared has no scope to impersonate at");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
    }

    #[test]
    fn a_boot_path_job_at_an_impersonating_source_still_runs_as_the_deployment() {
        // `verify_anchor` and a fixture load carry no caller, so they reach `Transport` even where
        // the source impersonates - and that is the documented narrowness of an executed anchor
        // here, not a missed option. Asserted so a later change that started impersonating the
        // LAST caller on the boot path would fail rather than read as a fix.
        assert_eq!(keys_and_values(JobIdentity::Transport), Vec::new());
    }

    #[test]
    fn a_target_account_is_bounded_and_takes_exactly_one_at_sign() {
        assert!(matches!(
            TargetAccount::parse("not-an-address"),
            Err(UnusableIdentityOption::Character { .. })
        ));
        assert!(matches!(
            TargetAccount::parse("a@b@sutura.example.com"),
            Err(UnusableIdentityOption::Character { .. })
        ));
        let long = format!("{}@sutura.example.com", "a".repeat(250));
        assert!(matches!(
            TargetAccount::parse(&long),
            Err(UnusableIdentityOption::TooLong { most: 254, .. })
        ));
        assert_eq!(
            TargetAccount::parse("  analyst-a@sutura.example.com  ")
                .expect("a padded address is an address")
                .as_str(),
            "analyst-a@sutura.example.com"
        );
    }
}
