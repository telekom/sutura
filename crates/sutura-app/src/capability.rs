//! What this surface can be asked to do, named once for every transport that offers it.
//!
//! # Why the vocabulary is here rather than in a transport
//!
//! [`crate::surface::Surface`] has exactly two operations - read the pinned bundle, and answer one
//! governed question - and those two *are* the tool set. A transport renames them for its own
//! protocol: the agent surface calls them tools and the HTTP surface calls them routes. Neither owns
//! the set.
//!
//! That is not a preference. `sutura-mcp` and `sutura-http` cannot see each other - *an adapter never
//! calls another adapter* - so a set owned by one of them is a set the other has to reach through it,
//! which is the same argument that moved the driving port itself out of `sutura-http`. One source in
//! the crate that declares the port is the only shape in which two transports **cannot** disagree
//! about what this deployment offers, and `docs/implementation-plan.md`'s
//! `both_transports_describe_the_same_tools` is the property that needs it.
//!
//! **What is shared is the SET, the identifier and the scope. Not the prose.** Each transport writes
//! its own description, because they are written for different readers - a model deciding whether to
//! call a tool, and a person reading an interface description - and `AGENTS.md` already draws that
//! line for the two refusal vocabularies: *"Nothing compares the two sentences, and nothing should."*
//!
//! # What a scope gates, stated before anything reads one
//!
//! **A scope decides which capabilities a caller may use. It decides nothing about which rows a
//! question reaches.** Every capability reads the same pinned bundle and every question executes
//! under the same identity, because no source executes as the asking subject: `docs/adr/0014`'s leg 1
//! establishes *who is asking* and leg 2 does not exist. So a deployment that grants one caller
//! `sutura:catalog.read` and not `sutura:metrics.ask` has narrowed what that caller may *do*, and has
//! not narrowed what any answer would contain.
//!
//! And within that: **filtering advertisement is presentation, and the control is at invocation.** A
//! caller that names a capability it was not granted is refused whether or not it was ever told the
//! capability exists. Both halves are built - see the *Invariants* row `AGENTS.md` gained with this
//! module - and if they ever disagree it is the refusal that is the control.
//!
//! # A scope names a capability and never a metric
//!
//! `docs/adr/0014`'s *What is not decided* left this open with a leaning: *"A scope naming a metric
//! couples the authorization server to the catalog, and a scope naming a capability does not. The
//! second is almost certainly right and it is not yet argued."* This module takes the second, and the
//! argument is that a catalog edit must not be able to change what a token means. A scope naming
//! `revenue` would put the authorization server's vocabulary under the catalog's version, so adding a
//! metric would silently grant it to every token holding a wildcard and renaming one would revoke a
//! grant nobody edited - an authorization change made by a definition author, in a repository the
//! authorization server does not read. The scopes here are two fixed strings no catalog can move.
//!
//! [`Capability::scope`] is therefore part of the deployed contract rather than an implementation
//! detail: an authorization server is configured with those literals by hand. A test pins them by
//! value for that reason.

use core::iter;
use std::collections::BTreeSet;

/// One thing this surface can be asked to do.
///
/// Closed, and closed on purpose: a third capability is a third exhaustive match to satisfy - the
/// walk in [`Capability::next`], the scope in [`Capability::scope`] and the identifier in
/// [`Capability::id`] - plus whatever each transport's own match needs. There is no wildcard arm in
/// any of them, so a variant added here does not compile until every one of those has been answered.
///
/// **Declaration order is `Ord`, and it runs from the least to the most a caller can get out of this
/// deployment**: describing what is measured comes before asking for a number. A
/// `BTreeSet<Capability>` therefore iterates in that order, which is what makes a rendered tool list
/// deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// Read the pinned bundle: which metrics exist, at which grains, with which dimensions and which
    /// filter values.
    ///
    /// [`crate::surface::Surface::definitions`]. Descriptive content only - the catalog port takes no
    /// request context and cannot be given one - but a listing of what a deployment measures is
    /// business information even with no row of data in it, which is why it is a capability at all
    /// rather than something public.
    DescribeCatalog,
    /// Answer one governed question about one certified metric.
    ///
    /// [`crate::surface::Surface::answer`].
    AskMetric,
}

/// The first capability in the walk.
///
/// A `const` rather than a literal inside [`Capability::every`], so the seed of the walk is one value
/// a reader can check against the declaration order above.
const FIRST: Capability = Capability::DescribeCatalog;

impl Capability {
    /// The next capability in declaration order, or `None` at the end.
    ///
    /// **This is what makes the walk unwalkable-past.** It is an exhaustive match with no wildcard
    /// arm, so a variant added to the enum fails to compile here rather than being quietly missing
    /// from [`Capability::every`] - and a capability missing from that walk is one nothing advertises
    /// and nothing gates. The same shape `sutura_domain::knowledge::Capability` already uses, and for
    /// the same reason.
    const fn next(self) -> Option<Self> {
        match self {
            Self::DescribeCatalog => Some(Self::AskMetric),
            Self::AskMetric => None,
        }
    }

    /// Every capability this surface has, in declaration order.
    ///
    /// Built from [`FIRST`] and [`Capability::next`] rather than written out as an array, so there is
    /// no second list to keep in step with the enum.
    pub fn every() -> impl Iterator<Item = Self> {
        iter::successors(Some(FIRST), |current| current.next())
    }

    /// The stable identifier both transports name this capability by.
    ///
    /// The agent surface uses it as the tool name; the HTTP surface uses it as the operation
    /// identifier in the generated interface description. **It is part of the deployed contract**, so
    /// it is a fixed literal here rather than derived from the variant name: a `Debug` rendering would
    /// rename a client's tool the day somebody renamed a variant.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::DescribeCatalog => "describe_catalog",
            Self::AskMetric => "ask_metric",
        }
    }

    /// The scope that licenses this capability.
    ///
    /// A fixed literal, stated independently of [`Capability::id`] rather than derived from it. That
    /// is the whole point: deriving one from the other would mean renaming a tool silently renamed a
    /// scope, and every authorization server configured with the old one would stop granting anything
    /// - an authorization change made by a rename.
    ///
    /// Prefixed, so a token minted for another resource server that happens to carry `catalog.read`
    /// does not read as a grant here. The audience check is what actually keeps such a token out -
    /// `docs/adr/0014` Decision 2 - and this is defence in depth rather than the control.
    #[must_use]
    pub const fn scope(self) -> &'static str {
        match self {
            Self::DescribeCatalog => "sutura:catalog.read",
            Self::AskMetric => "sutura:metrics.ask",
        }
    }
}

/// What one caller may do on this surface.
///
/// **Two named constructors and no third way in**, because the two cases are the two honest answers to
/// *who is asking* and a reader has to name which one a deployment is in:
///
/// * [`Permitted::every_capability`] - nothing established a caller identity, so there is no verified
///   claim to narrow by. A single-player deployment, and the agent surface over standard input and
///   output.
/// * [`Permitted::granted_by`] - a verified token's scopes decide, and **only** they do.
///
/// # It fails closed, and the consequence is named rather than softened
///
/// A verified caller whose token names no capability scope is permitted **nothing**: it sees no tools
/// and every call is refused. That follows from `AGENTS.md`'s *fail closed on the query path* and from
/// `docs/implementation-plan.md`'s own *"a caller without a scope cannot see the tool it lacks"*, and
/// it means a deployment that switches `security.inbound` on without authoring scopes at its
/// authorization server has switched every caller off.
///
/// **What keeps that discoverable rather than mysterious is the refusal**: the HTTP surface answers
/// `403` with `code: insufficient_scope` and a sentence naming the exact scope string to grant, so an
/// operator reads the fix out of the response rather than out of this comment.
///
/// # Why this takes strings rather than a parsed scope type
///
/// `sutura_http::inbound::Scopes` is where a scope claim is parsed and bounded, and it stays there.
/// Moving it would be taking a decision `docs/adr/0014`'s closing section explicitly reserves - *"how
/// [the agent surface] is reached at all, and then which crate the validator moves to ... is an
/// architecture decision, not a refactor"* - and nothing here needs the parse: this compares against
/// two fixed literals, and a string that could not be a scope simply matches neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permitted {
    /// A sorted set rather than a predicate, so the advertised order is deterministic and so
    /// `PartialEq` on this type means what a reader expects.
    capabilities: BTreeSet<Capability>,
}

impl Permitted {
    /// Every capability this surface has.
    ///
    /// **What a deployment that establishes no caller identity permits, and the name says so.** It is
    /// not a bypass and not a default: it is the correct answer when there is no verified claim to
    /// narrow by, and a filter over an unverified claim is the thing `docs/implementation-plan.md`
    /// calls *worse than no filter - it looks like a control*.
    ///
    /// Two callers today: `sutura_http`'s capability layer, for a deployment with no
    /// `security.inbound` block, and `sutura_mcp::serve_stdio`, where the process boundary is the
    /// boundary and there is no header a token could arrive in.
    #[must_use]
    pub fn every_capability() -> Self {
        Self {
            capabilities: Capability::every().collect(),
        }
    }

    /// Exactly the capabilities these scopes name.
    ///
    /// **The one comparison, so there is not one per transport.** A scope this surface does not know
    /// is ignored rather than refused: a token is minted by an authorization server that may serve
    /// other resources too, and refusing a caller for holding an unrelated grant would be refusing
    /// them for somebody else's configuration.
    #[must_use]
    pub fn granted_by<'scope>(scopes: impl IntoIterator<Item = &'scope str>) -> Self {
        let granted: BTreeSet<&str> = scopes.into_iter().collect();
        Self {
            capabilities: Capability::every()
                .filter(|capability| granted.contains(capability.scope()))
                .collect(),
        }
    }

    /// Whether this caller may use one capability.
    ///
    /// **This is the control.** [`Permitted::advertised`] decides what a caller is shown; this decides
    /// what it may do, and a transport calls it on every invocation whether or not it filtered the
    /// advertisement.
    #[must_use]
    pub fn includes(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// What to advertise, in declaration order.
    ///
    /// Presentation. A capability absent from here is one [`Permitted::includes`] also refuses, which
    /// is what keeps the two from being able to disagree: both read the same set.
    pub fn advertised(&self) -> impl Iterator<Item = Capability> + '_ {
        self.capabilities.iter().copied()
    }

    /// How many capabilities are permitted. For a startup or per-request log line.
    ///
    /// The count and not the names, for the same reason `sutura_http::inbound::Scopes` puts a count on
    /// a log line: the names are a caller's authorization detail and they multiply a log's
    /// cardinality.
    #[must_use]
    pub fn count(&self) -> usize {
        self.capabilities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{Capability, Permitted};

    /// How many capabilities there are, written down once so the walk below cannot pass by counting
    /// itself.
    ///
    /// A hand-written number IS the assertion here: `Capability::every()` is built from
    /// `Capability::next`, so comparing its length to its own length would prove nothing. A variant
    /// added to the enum fails this test, which is the reminder that a new capability is a change to
    /// the deployed contract - a scope an authorization server has to be configured with.
    const HOW_MANY: usize = 2;

    #[test]
    fn the_walk_reaches_every_capability() {
        assert_eq!(Capability::every().count(), HOW_MANY);
        // And the seed is the first declared one, so the order a tool list renders in is the order
        // the enum reads in.
        assert_eq!(Capability::every().next(), Some(Capability::DescribeCatalog));
    }

    /// The identifiers and the scopes are part of the deployed contract, so they are pinned by VALUE.
    ///
    /// An authorization server is configured with these strings by hand. A rename here is not a
    /// refactor: it is a change every deployment has to make at its own identity provider, and this
    /// test is what puts that in front of whoever renames it.
    #[test]
    fn the_identifier_and_the_scope_of_each_capability_are_fixed_strings() {
        assert_eq!(Capability::DescribeCatalog.id(), "describe_catalog");
        assert_eq!(Capability::DescribeCatalog.scope(), "sutura:catalog.read");
        assert_eq!(Capability::AskMetric.id(), "ask_metric");
        assert_eq!(Capability::AskMetric.scope(), "sutura:metrics.ask");
    }

    #[test]
    fn no_two_capabilities_share_an_identifier_or_a_scope() {
        let mut ids: Vec<&str> = Capability::every().map(Capability::id).collect();
        let mut scopes: Vec<&str> = Capability::every().map(Capability::scope).collect();
        for names in [&mut ids, &mut scopes] {
            let count = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), count, "two capabilities share a name: {names:?}");
        }
        // And an identifier is not a scope: they are separate literals on purpose, so that renaming a
        // tool cannot rename a scope.
        for capability in Capability::every() {
            assert_ne!(capability.id(), capability.scope());
        }
    }

    #[test]
    fn a_deployment_that_establishes_no_caller_permits_everything() {
        let permitted = Permitted::every_capability();
        assert_eq!(permitted.count(), HOW_MANY);
        for capability in Capability::every() {
            assert!(permitted.includes(capability), "{capability:?}");
        }
    }

    /// THE property this module exists for: a scope narrows the set, and it narrows both halves.
    #[test]
    fn a_scope_narrows_what_is_advertised_and_what_is_permitted_together() {
        let permitted = Permitted::granted_by([Capability::DescribeCatalog.scope()]);
        assert_eq!(permitted.count(), 1);
        assert!(permitted.includes(Capability::DescribeCatalog));
        assert!(!permitted.includes(Capability::AskMetric));
        assert_eq!(
            permitted.advertised().collect::<Vec<Capability>>(),
            vec![Capability::DescribeCatalog]
        );
    }

    /// Fail closed. A verified caller whose token names no capability scope may do nothing at all.
    #[test]
    fn a_caller_whose_token_names_no_capability_scope_is_permitted_nothing() {
        for irrelevant in [Vec::new(), vec!["openid", "profile", "email"], vec!["sutura:catalog"]] {
            let permitted = Permitted::granted_by(irrelevant.iter().copied());
            assert_eq!(permitted.count(), 0, "{irrelevant:?}");
            assert_eq!(permitted.advertised().count(), 0, "{irrelevant:?}");
            for capability in Capability::every() {
                assert!(!permitted.includes(capability), "{irrelevant:?} granted {capability:?}");
            }
        }
    }

    /// A scope for something else is ignored, not refused: a token is minted by an authorization
    /// server that may serve other resources too.
    #[test]
    fn a_scope_this_surface_does_not_know_is_ignored_rather_than_refused() {
        let permitted = Permitted::granted_by(["offline_access", Capability::AskMetric.scope(), "some:other.thing"]);
        assert_eq!(permitted.count(), 1);
        assert!(permitted.includes(Capability::AskMetric));
    }

    /// Advertisement and permission read the same set, so they cannot disagree.
    ///
    /// The finding this asserts away: if a transport filtered its advertisement from one source and
    /// gated its invocation from another, a caller could be shown a tool it may not call, or - worse -
    /// call one it was not shown. Both halves of [`Permitted`] read one field.
    #[test]
    fn what_is_advertised_is_exactly_what_is_permitted() {
        for scopes in [
            Vec::new(),
            vec![Capability::DescribeCatalog.scope()],
            vec![Capability::AskMetric.scope()],
            Capability::every().map(Capability::scope).collect(),
        ] {
            let permitted = Permitted::granted_by(scopes.iter().copied());
            let advertised: Vec<Capability> = permitted.advertised().collect();
            for capability in Capability::every() {
                assert_eq!(
                    advertised.contains(&capability),
                    permitted.includes(capability),
                    "{scopes:?} disagreed about {capability:?}"
                );
            }
        }
        // And the unnarrowed shape agrees too.
        let every = Permitted::every_capability();
        for capability in Capability::every() {
            assert!(every.advertised().any(|shown| shown == capability));
            assert!(every.includes(capability));
        }
    }
}
