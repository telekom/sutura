//! Who each leg of one call was delegated as: the principal a source was asked to run it under on
//! the deployment's word, for the audit record.
//!
//! A leg is delegated when the broker presented
//! [`Presented::SubjectPrincipal`](crate::identity::credential::Presented::SubjectPrincipal): a
//! `bigquery` source's `domain_delegation` and a `clickhouse` source's `EXECUTE AS`. A
//! [`Presented::SubjectToken`](crate::identity::credential::Presented::SubjectToken) leg is not,
//! because the source resolves the principal from the token and sutura cannot name it; a
//! [`Presented::SubjectSignIn`](crate::identity::credential::Presented::SubjectSignIn) leg is not,
//! because an exchanged token authenticates the user rather than the deployment vouching for them.
//!
//! **No credential material and no raw name, by type.** [`DelegatedAs`] holds a [`SourceName`] and a
//! [`DelegatedPrincipal`] per leg, and the principal is masked as it is built - the rule every
//! principal on an audit line follows. The full name reaches only the source, and its own record
//! (a `BigQuery` job's `user_email`) joins this one on the job's correlation label.
//!
//! **The limit:** it is what the broker MINTED, read before the grant is checked against the
//! request, so a call that then fails on a wiring defect still names it - which is the point of
//! recording a failed call. It is not what the source ran as; that is the source's own record.

use crate::identity::{DelegatedPrincipal, Minted};
use crate::model::SourceName;

/// The delegated legs of one call, in source order. Empty when no leg was delegated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DelegatedAs(Vec<(SourceName, DelegatedPrincipal)>);

impl DelegatedAs {
    /// The delegated legs of what a broker minted. A refusal mints nothing, so it delegates nothing.
    #[must_use]
    pub fn minted(minted: &Minted) -> Self {
        let credentials = match *minted {
            Minted::Granted { ref credentials } => credentials,
            Minted::Refused { .. } | Minted::AddressOutsideDomains { .. } => return Self::default(),
        };
        Self(
            credentials
                .vouched()
                .map(|(source, name)| (source.clone(), DelegatedPrincipal::from(name)))
                .collect(),
        )
    }

    /// Each delegated leg, in source order.
    pub fn legs(&self) -> impl Iterator<Item = (&SourceName, &DelegatedPrincipal)> {
        self.0.iter().map(|(source, principal)| (source, principal))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::DelegatedAs;
    use crate::identity::{Expiry, LegCredentials, Minted, Presented, PrincipalName, Secret, SourceSet, Subject};
    use crate::model::SourceName;
    use crate::source::{AcknowledgementReason, SharedIdentityDeclared};

    fn source(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test source is a source")
    }

    fn name(raw: &str) -> PrincipalName {
        PrincipalName::parse(raw).expect("a test principal is a principal")
    }

    #[test]
    fn only_a_principal_the_deployment_vouched_for_is_delegated() {
        let declared = SharedIdentityDeclared::of(AcknowledgementReason::parse("one identity").expect("a reason"));
        let presented = BTreeMap::from([
            (
                source("ledger"),
                Presented::SubjectPrincipal {
                    name: name("Analyst.One@Example.com"),
                },
            ),
            (
                source("pool"),
                Presented::SubjectToken {
                    material: Secret::new("an-assertion"),
                },
            ),
            (source("shared"), Presented::SharedServiceUser { declared }),
            (
                source("signin"),
                Presented::SubjectSignIn {
                    name: name("signed.in@example.com"),
                    material: Secret::new("an-exchanged-token"),
                },
            ),
        ]);
        let sources = SourceSet::of(source("ledger"))
            .and(source("pool"))
            .and(source("shared"))
            .and(source("signin"));
        let credentials = LegCredentials::minted(
            Subject::verified("analyst-one").expect("a subject"),
            Expiry::NothingExpires,
            &sources,
            presented,
        )
        .expect("the legs cover the sources");
        let delegated = DelegatedAs::minted(&Minted::Granted { credentials });
        let legs: Vec<(String, String)> = delegated
            .legs()
            .map(|(at, principal)| (at.to_string(), principal.to_string()))
            .collect();
        assert_eq!(legs, [(String::from("ledger"), String::from("A***.O***@E***.c***"))]);
        assert_eq!(
            DelegatedAs::minted(&Minted::Refused {
                source: source("ledger")
            }),
            DelegatedAs::default()
        );
    }
}
