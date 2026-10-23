//! The labels a data-system job carries, naming who asked for it.
//!
//! Three fixed keys: the OAuth client the caller's token named, the immediate actor of its `act`
//! chain, and the request's correlation id. **No subject**: every principal that can list the jobs
//! of a project reads its labels. Under `impersonation-at-source` the job's principal names the
//! user; under `shared-service-user` it is the deployment, and only the correlation id, joined to
//! the audit record, names the user.
//!
//! The values come only from a [`RequestContext`], never from a question: [`JobLabels::of`] is the
//! one constructor. **The limit:** `RequestContext`'s own constructors are `pub`, so this holds for
//! the context a transport established, not against code that fabricates one.
//!
//! # One rule for a value that does not fit
//!
//! A label value is at most 63 characters of `a-z`, `0-9`, `_` and `-`, the rule
//! `BigQuery` enforces and the one the audit record shares. A value that already fits is kept as
//! it is. Any other value becomes its first 54 characters, ASCII-lowercased with every other
//! character as `_`, then `-` and the first eight hex digits of the SHA-256 of the whole original
//! value. So nothing is dropped, and two values that normalise alike still differ in the suffix.
//! An absent claim is the empty value, which fits.

use sha2::{Digest as _, Sha256};

use crate::identity::{CorrelationId, RequestContext};

/// The key naming the OAuth client: the token's `azp`, else its `client_id`.
pub const CLIENT: &str = "sutura_client";
/// The key naming the immediate actor of the token's `act` chain, in its masked form.
pub const ACTOR: &str = "sutura_actor";
/// The key naming the random id drawn for the request - never the caller's own.
pub const CORRELATION: &str = "sutura_correlation";

/// The longest label value.
const MAX_VALUE: usize = 63;
/// How much of a value that does not fit is kept before its hash suffix: 54 + `-` + 8 = 63.
const KEPT: usize = 54;

/// One label value, inside the label rule by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelValue(String);

impl LabelValue {
    fn normalised(raw: &str) -> Self {
        if raw.chars().count() <= MAX_VALUE && raw.chars().all(fits) {
            return Self(String::from(raw));
        }
        let kept: String = raw
            .chars()
            .take(KEPT)
            .map(|c| {
                let c = c.to_ascii_lowercase();
                if fits(c) { c } else { '_' }
            })
            .collect();
        let digest: [u8; 32] = Sha256::digest(raw.as_bytes()).into();
        let [a, b, c, d, ..] = digest;
        Self(format!("{kept}-{a:02x}{b:02x}{c:02x}{d:02x}"))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

const fn fits(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-'
}

/// The three labels one request's jobs carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLabels {
    client: LabelValue,
    actor: LabelValue,
    correlation: LabelValue,
}

impl JobLabels {
    /// The labels for `context`'s request.
    ///
    /// `azp` wins over `client_id` when both are present, and an unreadable claim is `unreadable`,
    /// as the audit record renders it.
    #[must_use]
    pub fn of(context: &RequestContext) -> Self {
        let claims = context.client();
        let client = claims
            .azp()
            .or_else(|| claims.client_id())
            .map_or_else(String::new, ToString::to_string);
        let actor = context
            .chain()
            .actors()
            .map_or_else(String::new, |actors| actors.immediate().to_string());
        Self {
            client: LabelValue::normalised(&client),
            actor: LabelValue::normalised(&actor),
            correlation: LabelValue::normalised(context.correlation().map_or("", CorrelationId::as_str)),
        }
    }

    /// The three pairs, keys in [`CLIENT`], [`ACTOR`], [`CORRELATION`] order.
    #[must_use]
    pub fn pairs(&self) -> [(&'static str, &str); 3] {
        [
            (CLIENT, self.client.as_str()),
            (ACTOR, self.actor.as_str()),
            (CORRELATION, self.correlation.as_str()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{JobLabels, LabelValue, MAX_VALUE};
    use crate::identity::{Actor, ActorChain, ClientClaim, ClientClaims, PrincipalChain, RequestContext, Subject};

    fn normalised(raw: &str) -> String {
        LabelValue::normalised(raw).as_str().to_owned()
    }

    fn within_the_rule(value: &str) -> bool {
        value.len() <= MAX_VALUE && value.chars().all(super::fits)
    }

    #[test]
    fn a_value_that_fits_is_kept_as_it_is() {
        assert_eq!(normalised("claude-code_2"), "claude-code_2");
        assert_eq!(normalised(""), "", "an absent claim is the empty value");
        let longest = "a".repeat(MAX_VALUE);
        assert_eq!(normalised(&longest), longest);
    }

    #[test]
    fn a_label_value_is_at_most_sixty_three_characters() {
        // Literal numbers, not `MAX_VALUE`: `BigQuery` refuses a 64-character value, whatever the
        // constant says.
        assert_eq!(normalised(&"a".repeat(63)), "a".repeat(63));
        let over = normalised(&"a".repeat(64));
        assert_eq!(over.len(), 63, "{over}");
        assert!(over.starts_with(&format!("{}-", "a".repeat(54))), "{over}");
    }

    #[test]
    fn a_value_that_does_not_fit_is_lowercased_replaced_and_hashed() {
        let upper = normalised("Ingress-42");
        assert_eq!(
            upper, "ingress-42-7c3e878c",
            "the first eight hex digits of SHA-256(\"Ingress-42\")"
        );
        let forbidden = normalised("a***.b@c");
        assert!(forbidden.starts_with("a____b_c-"), "{forbidden}");
        let too_long = normalised(&"a".repeat(MAX_VALUE + 1));
        assert_eq!(too_long.len(), MAX_VALUE, "{too_long}");
        for value in [&upper, &forbidden, &too_long] {
            assert!(within_the_rule(value), "{value}");
        }
        assert_ne!(
            normalised("A*"),
            normalised("a."),
            "two values that normalise alike keep apart"
        );
    }

    #[test]
    fn the_labels_name_the_client_the_immediate_actor_and_the_correlation_and_never_the_subject() {
        let chain = PrincipalChain::of(Subject::verified("person-1").expect("a subject")).acting(
            ActorChain::of(Actor::parse("outer").expect("an actor")).acting_through(Actor::parse("agent").expect("an actor")),
        );
        let context = RequestContext::of(chain)
            .naming_client(ClientClaims::of(None, Some(ClientClaim::read(Some("claude-code")))))
            .correlated([0x11; 16]);
        // The immediate actor's masked form `a***`, normalised.
        assert_eq!(
            JobLabels::of(&context).pairs(),
            [
                ("sutura_client", "claude-code"),
                ("sutura_actor", "a___-d27ffc08"),
                ("sutura_correlation", "11111111111111111111111111111111"),
            ]
        );
        let bare = JobLabels::of(&RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)));
        assert_eq!(
            bare.pairs(),
            [("sutura_client", ""), ("sutura_actor", ""), ("sutura_correlation", "")]
        );
    }

    #[test]
    fn azp_wins_over_client_id() {
        let context = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)).naming_client(ClientClaims::of(
            Some(ClientClaim::read(Some("from-azp"))),
            Some(ClientClaim::read(Some("from-client-id"))),
        ));
        let labels = JobLabels::of(&context);
        let [client, ..] = labels.pairs();
        assert_eq!(client, ("sutura_client", "from-azp"));
    }
}
