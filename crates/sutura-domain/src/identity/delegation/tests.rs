//! The delegation port's one newtype, and its hop chain over fake exchanges.

use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use super::{Delegated, Delegation, DelegationExchange, DelegationFailed, RequestedAudience, UnusableAudience};
use crate::identity::Secret;

/// One exchange a hop was asked for: the audience it asked for, and the token it was handed.
type Asked = (String, String);

/// A hop that reports what it was asked and issues `<name>(<token>)`, or refuses.
#[derive(Debug)]
struct Hop {
    name: &'static str,
    refuses: bool,
    asked: Sender<Asked>,
}

impl DelegationExchange for Hop {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fake records the token it was handed, which is what a chain cell asserts"
    )]
    fn exchange(&self, subject: &Secret, audience: &RequestedAudience) -> Result<Delegated, DelegationFailed> {
        drop(
            self.asked
                .send((audience.as_str().to_owned(), subject.expose_secret().to_owned())),
        );
        if self.refuses {
            return Err(DelegationFailed::Refused {
                status: 400,
                error: None,
            });
        }
        Ok(Delegated::new(
            Secret::new(format!("{}({})", self.name, subject.expose_secret())),
            1,
        ))
    }
}

/// A hop's name, which is also its audience, and whether it refuses.
type Declared = (&'static str, bool);

/// One hop, as `Delegation::through` and `Delegation::then` take it.
type Built = (Arc<dyn DelegationExchange>, RequestedAudience);

fn chain(hops: &[Declared], asked: &Sender<Asked>) -> Delegation {
    let hop = |&(name, refuses): &Declared| -> Built {
        (
            Arc::new(Hop {
                name,
                refuses,
                asked: asked.clone(),
            }),
            RequestedAudience::parse(name).expect("a test audience parses"),
        )
    };
    let (first, then) = hops.split_first().expect("a chain has a first hop");
    let (exchange, audience) = hop(first);
    then.iter().fold(Delegation::through(exchange, audience), |chain, next| {
        let (exchange, audience) = hop(next);
        chain.then(exchange, audience)
    })
}

/// **Each hop exchanges the previous hop's token**, the first the caller's own, in declaration
/// order, and the chain returns the last hop's token alone.
#[test]
fn a_chain_runs_its_hops_in_order_each_on_the_previous_hops_token() {
    let (asked, reported) = channel();
    let delegated = chain(&[("keycloak", false), ("entra", false)], &asked)
        .exchange(&Secret::new("inbound"))
        .expect("both hops issue");
    #[expect(clippy::disallowed_methods, reason = "the cell asserts which token the chain returns")]
    let token = String::from(delegated.into_token().expose_secret());
    assert_eq!(token, "entra(keycloak(inbound))");
    drop(asked);
    assert_eq!(
        reported.iter().collect::<Vec<_>>(),
        vec![
            (String::from("keycloak"), String::from("inbound")),
            (String::from("entra"), String::from("keycloak(inbound)"))
        ],
        "each hop asks for its own audience, on the previous hop's token"
    );
}

/// **A failed hop stops the chain and names itself**: no later hop is asked, and the refusal says
/// which hop it was.
#[test]
fn a_failed_hop_names_itself_and_no_later_hop_runs() {
    let (asked, reported) = channel();
    let failed = chain(&[("keycloak", false), ("entra", true), ("database", false)], &asked)
        .exchange(&Secret::new("inbound"))
        .expect_err("the second hop refuses");
    assert!(
        matches!(failed, DelegationFailed::AtHop { hop: 2, ref cause } if matches!(**cause, DelegationFailed::Refused { status: 400, .. })),
        "{failed:?}"
    );
    drop(asked);
    assert_eq!(
        reported.iter().map(|(audience, _)| audience).collect::<Vec<_>>(),
        vec!["keycloak", "entra"]
    );
}

#[test]
fn a_requested_audience_is_kept_exactly_and_refused_when_unprintable() {
    assert_eq!(
        RequestedAudience::parse("https://workforce-pool.example.com")
            .expect("a test audience parses")
            .as_str(),
        "https://workforce-pool.example.com"
    );
    assert_eq!(RequestedAudience::parse(""), Err(UnusableAudience::Empty));
    assert_eq!(
        RequestedAudience::parse(&"a".repeat(RequestedAudience::MOST + 1)),
        Err(UnusableAudience::TooLong {
            found: RequestedAudience::MOST + 1,
            most: RequestedAudience::MOST
        })
    );
    assert_eq!(
        RequestedAudience::parse(" padded"),
        Err(UnusableAudience::Unprintable { at: 0 })
    );
    assert_eq!(
        RequestedAudience::parse("aud\u{7f}"),
        Err(UnusableAudience::Unprintable { at: 3 })
    );
    assert_eq!(
        RequestedAudience::parse("pool\u{e9}"),
        Err(UnusableAudience::Unprintable { at: 4 })
    );
}
