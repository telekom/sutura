//! The delegation packs: the hop chain's cases, held for every source that runs as the caller.
//!
//! A source whose caller's token is exchanged before it reaches the data system binds them with
//! [`crate::delegation_packs`] and one `deliver`: a function that opens the adapter's own
//! per-caller session with a token, against its offline fake, and returns the bytes that fake
//! read. Each pack builds a [`Delegation`] over fake hops, exchanges a caller's inbound token
//! through it, and hands `deliver` the token the chain returned - the one the broker presents.
//!
//! A failed hop never reaches `deliver`, so its pack is the same for every source; it is emitted
//! per binding so each source's row names every case.

#![expect(
    clippy::expect_used,
    reason = "every value a pack parses is a literal in this file, so one that does not parse is a \
              broken pack rather than an input to handle"
)]

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use sutura_domain::identity::{Delegated, Delegation, DelegationExchange, DelegationFailed, RequestedAudience, Secret};

/// The caller's own inbound token.
const INBOUND: &str = "inbound-token-5d2b81";

/// What one hop was asked: its name, and the token it was handed.
type Asked = (&'static str, String);

/// A hop's name, which is also its audience, and whether it refuses.
type Declared = (&'static str, bool);

/// One hop, as `Delegation::through` and `Delegation::then` take it.
type Built = (Arc<dyn DelegationExchange>, RequestedAudience);

/// The token the hop `name` issues: one no other byte of a session resembles.
fn issued(name: &str) -> String {
    format!("{name}-token-5d2b81")
}

/// A hop that reports what it was handed and issues [`issued`], or refuses.
#[derive(Debug)]
struct Hop {
    name: &'static str,
    refuses: bool,
    asked: Sender<Asked>,
}

impl DelegationExchange for Hop {
    #[expect(
        clippy::disallowed_methods,
        reason = "the fake records the token it was handed, which is what a pack asserts"
    )]
    fn exchange(&self, subject: &Secret, _audience: &RequestedAudience) -> Result<Delegated, DelegationFailed> {
        drop(self.asked.send((self.name, subject.expose_secret().to_owned())));
        if self.refuses {
            return Err(DelegationFailed::Refused {
                status: 400,
                error: None,
            });
        }
        Ok(Delegated::new(Secret::new(issued(self.name)), u64::MAX))
    }
}

/// A chain over `hops` in order, each a name and whether it refuses, and what its hops are asked.
fn chain(hops: &[Declared]) -> (Delegation, Receiver<Asked>) {
    let (asked, heard) = channel();
    let hop = |&(name, refuses): &Declared| -> Built {
        (
            Arc::new(Hop {
                name,
                refuses,
                asked: asked.clone(),
            }),
            RequestedAudience::parse(name).expect("a pack's audience parses"),
        )
    };
    let (first, then) = hops.split_first().expect("a pack's chain has a first hop");
    let (exchange, audience) = hop(first);
    let delegation = then.iter().fold(Delegation::through(exchange, audience), |chain, next| {
        let (exchange, audience) = hop(next);
        chain.then(exchange, audience)
    });
    (delegation, heard)
}

/// The token `delegation` exchanges the caller's inbound token for.
fn exchanged(delegation: &Delegation) -> Secret {
    delegation
        .exchange(&Secret::new(INBOUND))
        .expect("a chain of hops that do not refuse exchanges")
        .into_token()
}

fn carries(received: &[u8], token: &str) -> bool {
    received.windows(token.len()).any(|window| window == token.as_bytes())
}

/// **One hop: the data system receives the token that hop issued**, never the caller's inbound one.
pub fn one_hop_hands_the_data_system_its_token(deliver: impl FnOnce(&Secret) -> Vec<u8>) {
    let (delegation, asked) = chain(&[("first", false)]);
    let received = deliver(&exchanged(&delegation));
    assert_eq!(asked.try_iter().collect::<Vec<_>>(), [("first", String::from(INBOUND))]);
    assert!(
        carries(&received, &issued("first")),
        "the data system did not receive the hop's token"
    );
    assert!(
        !carries(&received, INBOUND),
        "the data system received the caller's inbound token"
    );
}

/// **Two hops run in order**, the second exchanging the first's token, and the data system receives
/// the second's token - never the first's, never the caller's inbound one.
pub fn two_hops_run_in_order_and_the_data_system_receives_the_last_token(deliver: impl FnOnce(&Secret) -> Vec<u8>) {
    let (delegation, asked) = chain(&[("first", false), ("second", false)]);
    let received = deliver(&exchanged(&delegation));
    assert_eq!(
        asked.try_iter().collect::<Vec<_>>(),
        [("first", String::from(INBOUND)), ("second", issued("first"))]
    );
    assert!(
        carries(&received, &issued("second")),
        "the data system did not receive the last hop's token"
    );
    for earlier in [String::from(INBOUND), issued("first")] {
        assert!(
            !carries(&received, &earlier),
            "the data system received an earlier token: {earlier}"
        );
    }
}

/// **A failed hop refuses**: the refusal names it, carries no token, and no later hop runs - so
/// nothing is presented to the data system.
pub fn a_failed_hop_refuses_and_no_later_hop_runs() {
    let (delegation, asked) = chain(&[("first", false), ("second", true), ("third", false)]);
    let refused = delegation
        .exchange(&Secret::new(INBOUND))
        .map(drop)
        .expect_err("a chain with a refusing hop refuses");
    assert!(matches!(refused, DelegationFailed::AtHop { hop: 2, .. }), "{refused:?}");
    assert_eq!(
        asked.try_iter().map(|(name, _)| name).collect::<Vec<_>>(),
        ["first", "second"]
    );
    let mut rendered = format!("{refused:?}");
    let mut cause: Option<&dyn core::error::Error> = Some(&refused);
    while let Some(error) = cause {
        rendered.push_str(&error.to_string());
        cause = error.source();
    }
    for token in [String::from(INBOUND), issued("first")] {
        assert!(!rendered.contains(&token), "the refusal carries a token: {rendered}");
    }
}

/// Binds the delegation packs to one source, as `#[test]`s named for each case.
///
/// `deliver` opens the adapter's own per-caller session with a token, against its offline fake,
/// and returns the bytes that fake read.
#[macro_export]
macro_rules! delegation_packs {
    (adapter: $name:ident, deliver: $deliver:path $(,)?) => {
        // `#[cfg(test)]`, for the reason `execute_packs` gives.
        #[cfg(test)]
        mod $name {
            #[test]
            fn one_hop_hands_the_data_system_its_token() {
                $crate::delegation::one_hop_hands_the_data_system_its_token($deliver);
            }

            #[test]
            fn two_hops_run_in_order_and_the_data_system_receives_the_last_token() {
                $crate::delegation::two_hops_run_in_order_and_the_data_system_receives_the_last_token($deliver);
            }

            #[test]
            fn a_failed_hop_refuses_and_no_later_hop_runs() {
                $crate::delegation::a_failed_hop_refuses_and_no_later_hop_runs();
            }
        }
    };
}
