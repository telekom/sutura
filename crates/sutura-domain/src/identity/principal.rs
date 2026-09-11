//! The principal chain: who a question is attributed to, ordered.
//!
//! **Human, then agent, then task.** Three positions, in that order, and the second and third are
//! always absent today - nothing establishes a caller identity yet, and no agent surface exists to
//! name a task. The type is here anyway, and the reason it could not wait is the only argument this
//! module needs: **a record naming only the subject can never later be told apart from one that
//! meant "an agent acting for" them.** Adding the distinction after the records are written is not
//! hard, it is impossible - there is nothing in a row that says which of the two it was. So the
//! shape goes in while both tail positions are empty, and every reader is made to say which case it
//! is handling.
//!
//! # How a reader is stopped from assuming a position is present
//!
//! Not by a doc comment. [`PrincipalChain::attribution`] returns a two-variant [`Attribution`], and
//! there is no accessor that hands out "the agent": to learn that an agent acted, a reader matches,
//! and the variant that names actors is the only one that carries any - [`ActorChain`] cannot be
//! empty, because it stores its innermost link in a field of its own rather than in a `Vec` somebody
//! has to check the length of. [`PrincipalChain::task`] returns an `Option`, which is the same
//! requirement expressed the way one position can be.
//!
//! # Nothing here is deserializable, on purpose
//!
//! **A caller that states its own identity does not have one.** The chain is derived from what the
//! transport established and never read from a field on the wire, and the mechanism is stronger
//! than a validation: none of these types implements `Deserialize`, so there is no code that could
//! turn caller-supplied bytes into a [`PrincipalChain`] at all. [`PrincipalChain`] carries a
//! `compile_fail` doctest for exactly that, with a compiling twin beside it so the failure cannot be
//! passing for a typo.
//!
//! None of them implements `Serialize` either, which is the other half of the pair: a
//! `serde(try_from)` newtype whose derived `Serialize` writes the struct is a value this workspace
//! can emit and its own `Deserialize` refuses, and [`crate::calendar::Date`] shipped precisely that.
//! Two absent impls cannot disagree.

use core::fmt;

use crate::text::first_invisible;

/// The longest principal identifier accepted.
///
/// Wide enough for an issuer-qualified OIDC `sub` - a URL plus an opaque identifier - and narrow
/// enough that it cannot carry a paragraph into the record a call is written to. The bound is here
/// because an unbounded input is a denial-of-service primitive whatever else it is, and because the
/// value is echoed into a record line.
const MAX_PRINCIPAL_LEN: usize = 256;

/// Why an identifier naming a principal was rejected.
///
/// One error for all three newtypes below, because they are one parse. The variants carry only the
/// shape of the rejected input; the principal itself is personal data and this error reaches logs.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidPrincipalId {
    /// Empty or whitespace-only. An unnamed principal must not be able to claim it is one - the
    /// whole point of the chain is that a record says who, and the empty string says nobody while
    /// looking like an answer.
    #[error("a principal identifier must not be empty")]
    Empty,
    /// Holds a control character. This is the one that matters: the record a call is written to is
    /// one line, so a newline here appends a record nobody wrote - a forged attribution, in the one
    /// artifact whose entire job is attribution.
    #[error("a principal identifier must not contain the control character {code:#06x}")]
    ControlCharacter { code: u32 },
    /// Holds an invisible or direction-changing code point. The second half of the reason the
    /// variant above exists: `char::is_control` is false for every one of these - general category
    /// `Cf`, not `Cc` - so the check that refuses a newline cannot see a right-to-left override.
    ///
    /// It matters here for the same reason it matters on a version label, one step further: two
    /// principals a reader cannot tell apart is exactly the confusion this module exists to prevent,
    /// arriving through the rendering rather than through the shape.
    ///
    /// The code is reported rather than the value, because a value whose only defect draws nothing
    /// would print as though it were correct.
    #[error("a principal identifier must not contain the invisible or direction-changing character {code:#06x}")]
    InvisibleCharacter { code: u32 },
    #[error("a principal identifier may be at most {limit} characters, found {len}")]
    TooLong { len: usize, limit: usize },
}

/// Parses one principal identifier, rejecting anything that is not one.
///
/// `pub(crate)` and shared by every newtype below through `principal_newtype!`, because three
/// hand-written copies of this parser is three things to keep in step - and the identifiers are
/// carried in one record beside each other, so a rule that held for one and not another would be a
/// hole with a matching pair right next to it.
///
/// Case is preserved. An identifier here refers to something outside this system - a directory
/// entry, an agent registration - and folding case would make the value we record differ from the
/// value the issuer used. That is the opposite of [`crate::knowledge`]'s phrases, where folding is
/// right because the value is a key into our own text.
pub(crate) fn parse_principal_id(raw: &str) -> Result<String, InvalidPrincipalId> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(InvalidPrincipalId::Empty);
    }
    // Bound the size before either character scan, so an oversized identifier costs one walk to
    // refuse rather than two: the length ceiling is the availability half, and the character classes
    // are the forging half. Ordering the bound first also means a value that is both too long and
    // control-bearing reports its size, which is what its source needs to hear.
    let len = trimmed.chars().count();
    if len > MAX_PRINCIPAL_LEN {
        return Err(InvalidPrincipalId::TooLong {
            len,
            limit: MAX_PRINCIPAL_LEN,
        });
    }
    if let Some(offending) = trimmed.chars().find(|character| character.is_control()) {
        return Err(InvalidPrincipalId::ControlCharacter {
            code: u32::from(offending),
        });
    }
    // Beside the control-character check rather than folded into it, because it is a second
    // character class the first one provably cannot see. `crate::text` owns the set, so this refusal
    // and the one a version label gets are the same refusal.
    if let Some(offending) = first_invisible(trimmed) {
        return Err(InvalidPrincipalId::InvisibleCharacter {
            code: u32::from(offending),
        });
    }
    Ok(String::from(trimmed))
}

/// Declares one principal-identifier newtype over [`parse_principal_id`].
///
/// A macro rather than three copies, for the reason `crate::model`'s identifier macro gives: one
/// implementation cannot drift from itself. Deliberately unlike that macro in one way - **it derives
/// no `serde` impls at all**, which is the module header's claim expressed where it is enforced.
macro_rules! principal_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// The wrapped value is the **masked** form: [`Self::parse`] consumes the raw identifier and
        /// stores only its stable masked rendering, so no field of this type ever holds plaintext and
        /// no rendering surface (`Debug`, `Display`, [`Self::as_str`]) can emit it. Masking happens
        /// at the boundary that turns a wire value into this type, not at print time. There is no
        /// other way in: the field is private, there is no `Deserialize`, and `TryFrom<String>`
        /// delegates to the same constructor.
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Parses an identifier, rejecting anything that is not one, masking it at the boundary.
            ///
            /// The raw value is validated, reduced to its stable masked form, and dropped: the raw
            /// is never retained, so an intermediate state cannot leak it and there is nothing to
            /// reach at render time.
            pub fn parse(raw: impl AsRef<str>) -> Result<Self, InvalidPrincipalId> {
                let validated = parse_principal_id(raw.as_ref())?;
                let mut masked = String::with_capacity(validated.len());
                mask_principal_into(&validated, &mut masked);
                Ok(Self(masked))
            }

            /// The stable masked form. There is no raw access - the raw was consumed by [`Self::parse`].
            #[inline]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        /// Delegates to `parse` rather than repeating it: one constructor stays the source of truth.
        impl TryFrom<String> for $name {
            type Error = InvalidPrincipalId;

            fn try_from(raw: String) -> Result<Self, Self::Error> {
                Self::parse(raw)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

/// Writes a stable masked form of a principal identifier into `out`.
///
/// Each local segment (split on `.` and `@`) is reduced to its first character plus three stars, and
/// only that is ever retained, so two subjects an operator needs to tell apart stay distinct while
/// neither's full value survives. The raw value reaches this only straight out of
/// [`parse_principal_id`], the one place a plaintext principal exists, and is dropped once the mask
/// is written.
fn mask_principal_into(raw: &str, out: &mut String) {
    let mut segment_start = true;
    for character in raw.chars() {
        if matches!(character, '.' | '@') {
            out.push(character);
            segment_start = true;
        } else if segment_start {
            out.push(character);
            out.push_str("***");
            segment_start = false;
        }
    }
}

principal_newtype! {
    /// The identifier of the human a question is asked on behalf of.
    ///
    /// The head of the chain, and the only position that is never absent - though what established
    /// it is a question [`Subject`] answers and this type does not.
    SubjectId
}

principal_newtype! {
    /// The identifier of something that acted for the subject: an agent, or an agent's agent.
    ///
    /// One link of the second position. Never on its own - an actor exists inside an [`ActorChain`],
    /// which exists inside a [`PrincipalChain`] whose subject says who it was acting for.
    Actor
}

principal_newtype! {
    /// The identifier of the unit of work a question belongs to.
    ///
    /// The third position, and the one that answers "which job was this part of" rather than "who".
    /// It exists from day one because a task id assigned later cannot be attached to calls already
    /// recorded, which is the same argument the whole chain rests on.
    TaskId
}

/// Who a question is attributed to, and what established it.
///
/// **Two variants and not one string, because the difference is the one that must never be guessable
/// from a record.** Today's transport authenticates the *deployment* and not the caller - the bearer
/// gate is a shared token - so there is no verified caller identity to put here, and the honest
/// answer is a named variant rather than an invented identifier. A `Subject::Verified` whose id
/// happened to be `"sutura"` would be indistinguishable from the deployment case if this were a
/// string; as an enum it cannot be, and a reader gets the distinction from a match it cannot skip.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Subject {
    /// A caller the transport verified. `sutura_http`'s `TokenValidator::verify` constructs it, on
    /// the chain it hands the request path - the record that decided how a caller proves who it is
    /// (`docs/adr/0014`) has landed, and this variant is what it filled in. The sentence that used
    /// to stand here said nothing constructed it, which had already stopped being true in one file
    /// and been carried to no other.
    Verified { id: SubjectId },
    /// No caller identity was established. The transport authenticated the deployment and not
    /// whoever asked, so the deployment is the only principal there is.
    ///
    /// **Not a placeholder and not an anonymous user.** It is the true answer to "who is this
    /// attributable to" on a deployment whose front door proves only that the request came through
    /// it, and naming it is what keeps a record from claiming more than the transport knows.
    TheDeploymentItself,
}

impl Subject {
    /// What established this subject, as a stable label for a record field.
    ///
    /// A `&'static str` from an exhaustive match rather than a `Display`, because it has to be a
    /// value a query over records can group by and a value nothing caller-supplied can collide with.
    #[inline]
    pub const fn established(&self) -> &'static str {
        match *self {
            Self::Verified { .. } => "verified",
            Self::TheDeploymentItself => "deployment",
        }
    }

    /// The verified identifier, if there is one.
    ///
    /// `Option`, so a reader that wants the id has to acknowledge the case where there is none. A
    /// record renders both this and [`Self::established`], which is why an absent id cannot be read
    /// as an unnamed person.
    #[inline]
    pub const fn id(&self) -> Option<&SubjectId> {
        match *self {
            Self::Verified { ref id } => Some(id),
            Self::TheDeploymentItself => None,
        }
    }
}

/// One or more actors, ordered, innermost last.
///
/// **Non-emptiness is structural rather than checked.** The innermost link lives in its own field, so
/// there is no state of this type that means "no actors" - which is what lets [`Self::immediate`] be
/// infallible and what stops [`Attribution::ActingFor`] from being a claim about nobody. A `Vec` with
/// a length check would be the same rule in a form that can be skipped.
///
/// **The order, precisely.** Iteration yields the actor nearest the subject first and the one that
/// called this deployment last. That is the shape a token exchange maps onto rather than being
/// translated into: each exchange adds a link on the inside, and nothing has to reverse a list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActorChain {
    /// The links outside [`Self::immediate`], nearest-the-subject first. Empty for a single actor.
    outer: Vec<Actor>,
    /// The actor that called this deployment. Always present, which is the whole point of the split.
    immediate: Actor,
}

impl ActorChain {
    /// One actor: the one that called this deployment.
    #[inline]
    pub const fn of(actor: Actor) -> Self {
        Self {
            outer: Vec::new(),
            immediate: actor,
        }
    }

    /// Adds a link on the inside: the current immediate actor becomes an outer one, and `actor`
    /// becomes the actor that called this deployment.
    ///
    /// Takes and returns `Self` rather than mutating, so a chain is built in one expression and there
    /// is no half-built value for anything else to read.
    #[must_use]
    pub fn acting_through(mut self, actor: Actor) -> Self {
        let previous = core::mem::replace(&mut self.immediate, actor);
        self.outer.push(previous);
        self
    }

    /// The actor that called this deployment. Infallible, because the type cannot be empty.
    #[inline]
    pub const fn immediate(&self) -> &Actor {
        &self.immediate
    }

    /// The actor nearest the subject - the first thing that acted on their behalf.
    #[inline]
    pub fn outermost(&self) -> &Actor {
        // `unwrap_or` and not `unwrap`: a single-actor chain has an empty `outer`, and then the
        // outermost actor IS the immediate one. No panic path, and no branch to get wrong.
        self.outer.first().unwrap_or(&self.immediate)
    }

    /// How many actors are in the chain. At least one.
    ///
    /// Named `count` rather than `len` deliberately: a `len` invites an `is_empty` beside it, and an
    /// `is_empty` that can only return `false` is an accessor that teaches the wrong thing about this
    /// type.
    #[inline]
    pub const fn count(&self) -> usize {
        self.outer.len().saturating_add(1)
    }

    /// The actors in order, nearest the subject first.
    #[inline]
    pub fn iter(&self) -> ActorsInOrder<'_> {
        self.into_iter()
    }
}

/// The iterator [`ActorChain`] hands out, named so it can be a return type.
pub type ActorsInOrder<'a> = core::iter::Chain<core::slice::Iter<'a, Actor>, core::iter::Once<&'a Actor>>;

impl<'a> IntoIterator for &'a ActorChain {
    type Item = &'a Actor;
    type IntoIter = ActorsInOrder<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.outer.iter().chain(core::iter::once(&self.immediate))
    }
}

impl fmt::Display for ActorChain {
    /// A rendering for a record line, nearest the subject first, `>` between links.
    ///
    /// A rendering and not a wire format: there is no `FromStr`, so nothing parses this back. That
    /// matters because an identifier may contain a `>` - the parse above bounds the character set
    /// only by what is invisible or a control character - so this is unambiguous to a reader and not
    /// to a machine. A consumer that needs the links individually iterates them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, actor) in self.into_iter().enumerate() {
            if position > 0 {
                f.write_str(" > ")?;
            }
            write!(f, "{actor}")?;
        }
        Ok(())
    }
}

/// Who to attribute a call to: the subject, and whether anything acted for them.
///
/// **The one accessor that gets a reader at the actors, and it is why there is no other.** An
/// `Option<&ActorChain>` alone would have been the same information in a form that reads as an
/// absence to be handled rather than a case to name, and the case is the whole point: a call by a
/// person and a call by an agent for that person are two different events, and a reader that cannot
/// tell them apart is the failure this module exists to prevent.
#[derive(Debug, PartialEq, Eq)]
pub enum Attribution<'a> {
    /// The subject asked. Nothing acted for them.
    BareSubject { subject: &'a Subject },
    /// Something acted for the subject. The actors are ordered, innermost last, and there is at
    /// least one - [`ActorChain`] has no empty state.
    ActingFor { subject: &'a Subject, actors: &'a ActorChain },
}

/// Human, then agent, then task - ordered, and with both tail positions absent today.
///
/// This is what a call is recorded under, and what a budget would be keyed on **if a budget
/// existed**. There is no budget port in this workspace; the chain is the key and nothing consumes it
/// as one yet. `Eq` and `Hash` are derived so that it can be one when something does.
///
/// No `Ord`. A derived `Ord` on a struct is declaration order, and an ordering over principals has no
/// meaning anybody would agree on - so the absence is the answer rather than an arbitrary comparison
/// that a `BTreeMap` somewhere would then depend on.
///
/// # A caller cannot state its own chain
///
/// There is no `Deserialize`, so caller-supplied bytes cannot become one of these:
///
/// ```compile_fail
/// // A transport that tried to read the chain off the wire does not compile.
/// let chain: sutura_domain::identity::PrincipalChain = serde_json::from_str(r#"{"subject":"someone"}"#).expect("no");
/// drop(chain);
/// ```
///
/// The compiling twin, so the failure above cannot be passing for a typo - a chain is constructed
/// from what the transport established:
///
/// ```
/// use sutura_domain::identity::{PrincipalChain, Subject};
///
/// let chain = PrincipalChain::of(Subject::TheDeploymentItself);
/// assert!(chain.actors().is_none(), "nothing established an actor");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrincipalChain {
    subject: Subject,
    /// The second position. `None` until something establishes that an agent acted.
    actors: Option<ActorChain>,
    /// The third position. `None` until an agent surface names a task.
    task: Option<TaskId>,
}

impl PrincipalChain {
    /// A chain with both tail positions absent, which is every chain this workspace builds today.
    ///
    /// The canonical constructor: the two below add a tail position to a chain this one made, so
    /// there is one place a chain comes into existence.
    #[inline]
    pub const fn of(subject: Subject) -> Self {
        Self {
            subject,
            actors: None,
            task: None,
        }
    }

    /// The same chain, with the actors that acted for the subject.
    #[must_use]
    #[inline]
    pub fn acting(mut self, actors: ActorChain) -> Self {
        self.actors = Some(actors);
        self
    }

    /// The same chain, with the task the question belongs to.
    #[must_use]
    #[inline]
    pub fn for_task(mut self, task: TaskId) -> Self {
        self.task = Some(task);
        self
    }

    #[inline]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    /// Who to attribute this call to, as a case a reader has to name.
    ///
    /// The only way to reach the actors as a group that is guaranteed to hold one, and there is
    /// deliberately no accessor that returns a single actor: an "immediate actor" read off a chain
    /// that has none would be the assumption this type exists to make unrepresentable.
    #[inline]
    pub const fn attribution(&self) -> Attribution<'_> {
        match self.actors {
            Some(ref actors) => Attribution::ActingFor {
                subject: &self.subject,
                actors,
            },
            None => Attribution::BareSubject { subject: &self.subject },
        }
    }

    /// The actors, if any acted.
    ///
    /// Beside [`Self::attribution`] rather than instead of it, for the one caller that renders a
    /// field and does not branch on the case. It returns an `Option`, so it cannot be read as a
    /// promise either.
    #[inline]
    pub const fn actors(&self) -> Option<&ActorChain> {
        self.actors.as_ref()
    }

    /// The task, if one was named.
    #[inline]
    pub const fn task(&self) -> Option<&TaskId> {
        self.task.as_ref()
    }
}

/// What a request carries besides the question.
///
/// Two fields: who the call is attributed to, and - where the transport established one - the
/// caller's own credential assertion. The second is what lets a [`crate::identity::CredentialBroker`]
/// that exchanges a token have the caller's token to exchange: `docs/adr/0008` part 2 sketched a
/// `Caller { subject, assertion }` for exactly this, and `docs/adr/0014`'s open Decision 3 is why the
/// assertion field was absent until a broker existed that performs an exchange. It is an `Option`
/// because the two shapes that reach this value are genuinely different: a deployment's own identity,
/// which has no credential, and a verified caller, whose transport retained the token it verified.
/// `None` is "there is no per-caller credential to exchange", not an oversight.
///
/// Everything a question is answered *for* rather than *about* belongs here, so the credential an
/// execution leg will need and the deadline it will carry have a place to arrive that is not a
/// widened [`crate::query::Query`]. The tool surface stays a question and nothing else.
///
/// **Here rather than in `sutura-app`.** The application is where a context enters the service, but
/// the domain is what names it: a `CredentialBroker` port declared here takes the caller, and a port
/// in the interior cannot speak in a type owned by an adapter or by the service above it.
///
/// Not `Deserialize`, for the reason the chain is not: a request context assembled from the request
/// body is the confused deputy this whole module refuses.
///
/// **Not `PartialEq`/`Eq`, and the `Secret` is why.** This type now holds credential material, and
/// [`crate::identity::Secret`] implements no comparison, because `==` on credential material is a
/// timing oracle; a derived equality would have compared the two secrets byte-wise. `Debug` and
/// `Clone` survive because `Secret` implements both (redacting and value-preserving respectively).
#[derive(Debug, Clone)]
pub struct RequestContext {
    chain: PrincipalChain,
    assertion: Option<crate::identity::Secret>,
}

impl RequestContext {
    /// A context carrying only the chain, for the caller that has no credential to present - the
    /// deployment's own identity, and every non-verified surface that reaches this value.
    #[inline]
    pub const fn of(chain: PrincipalChain) -> Self {
        Self { chain, assertion: None }
    }

    /// The same context, with the caller's own credential assertion the transport retained.
    ///
    /// **This is the port change `docs/adr/0008` part 2 asked for and `docs/adr/0014` gated.** A
    /// broker that performs an exchange could not read the caller's token from the [`RequestContext`]
    /// it was handed, because nothing named what the caller presented - only who. This constructor is
    /// the thing that carries it, and it appears next to the first broker that exchanges rather than
    /// before it.
    #[inline]
    #[must_use]
    pub const fn with_assertion(chain: PrincipalChain, assertion: crate::identity::Secret) -> Self {
        Self {
            chain,
            assertion: Some(assertion),
        }
    }

    #[inline]
    pub const fn chain(&self) -> &PrincipalChain {
        &self.chain
    }

    /// The caller's own credential assertion, where the transport established and retained one.
    ///
    /// `None` for the deployment's own identity and for every surface with no credential to verify.
    /// A broker that only mints from configuration returns a credential without reading it; one that
    /// performs an exchange refuses a request whose caller presented nothing to exchange, as
    /// [`crate::query::RefusalReason::CredentialUnavailable`].
    #[inline]
    pub const fn assertion(&self) -> Option<&crate::identity::Secret> {
        self.assertion.as_ref()
    }

    /// The task this request belongs to, if one was named.
    ///
    /// Delegates to the chain rather than holding a second copy: the task is the chain's third
    /// position, and two places to read it from is two places for them to disagree.
    #[inline]
    pub const fn task(&self) -> Option<&TaskId> {
        self.chain.task()
    }
}

#[cfg(test)]
mod tests {
    use super::{Actor, ActorChain, Attribution, InvalidPrincipalId, PrincipalChain, RequestContext, Subject, SubjectId, TaskId};

    fn actor(raw: &str) -> Actor {
        Actor::parse(raw).expect("a test actor is an actor")
    }

    fn a_person() -> Subject {
        Subject::Verified {
            id: SubjectId::parse("someone@example.com").expect("a test subject is a subject"),
        }
    }

    #[test]
    fn a_principal_with_no_actor_is_a_bare_subject_and_says_so() {
        // The default shape today, and the one the whole step exists to make legible. A reader cannot
        // get at an actor without matching, and this arm has none to get at.
        let chain = PrincipalChain::of(a_person());
        let Attribution::BareSubject { subject } = chain.attribution() else {
            panic!("a chain with no actors is a bare subject, not {:?}", chain.attribution());
        };
        assert_eq!(subject, &a_person());
        assert_eq!(chain.actors(), None, "no actor was established");
        assert_eq!(chain.task(), None, "no task was named");
    }

    #[test]
    fn an_actor_chain_keeps_its_order() {
        // Innermost LAST, which is the direction a token exchange adds links in. Asserted over three
        // links, because two cannot tell "kept the order" from "reversed it".
        let chain = ActorChain::of(actor("orchestrator"))
            .acting_through(actor("planner"))
            .acting_through(actor("query_agent"));
        // `as_str` is the stored MASKED form - the raw identifiers were consumed at parse - and the
        // masks of distinct names stay distinct, so the order is still provable from what the type
        // holds.
        let names: Vec<&str> = chain.iter().map(Actor::as_str).collect();
        assert_eq!(names, vec!["o***", "p***", "q***"]);
        assert_eq!(
            chain.immediate(),
            &actor("query_agent"),
            "the last link called this deployment"
        );
        assert_eq!(
            chain.outermost(),
            &actor("orchestrator"),
            "the first link acted for the subject"
        );
        assert_eq!(chain.count(), 3);
        // The rendering an audit record carries, in the same order.
        assert_eq!(chain.to_string(), "o*** > p*** > q***");
    }

    #[test]
    fn a_single_actor_is_both_ends_of_its_own_chain() {
        // The boundary `outermost` gets wrong if it reads `outer` without a fallback: a one-link
        // chain has an EMPTY `outer`, so the outermost actor is the immediate one.
        let chain = ActorChain::of(actor("query_agent"));
        assert_eq!(chain.outermost(), chain.immediate());
        assert_eq!(chain.count(), 1);
        assert_eq!(chain.to_string(), "q***");
    }

    #[test]
    fn an_agent_acting_for_a_subject_is_a_different_case_from_the_subject_asking() {
        // The distinction the records have to be able to carry, at the level of the type. The two
        // chains name the SAME subject, so nothing but the case tells them apart.
        let alone = PrincipalChain::of(a_person());
        let acted_for = PrincipalChain::of(a_person()).acting(ActorChain::of(actor("query_agent")));
        assert_ne!(alone, acted_for, "the two are not the same chain");
        assert!(
            matches!(alone.attribution(), Attribution::BareSubject { .. }),
            "a subject asking is a bare subject"
        );
        let Attribution::ActingFor { subject, actors } = acted_for.attribution() else {
            panic!("an agent acting for a subject is not a bare subject");
        };
        assert_eq!(subject, &a_person(), "the subject survives the agent");
        assert_eq!(actors.immediate(), &actor("query_agent"));
    }

    #[test]
    fn the_deployment_is_not_a_person_and_a_record_can_say_which() {
        // What every chain in this workspace carries today. `established` is the field a record
        // groups by, and it is an exhaustive match rather than a string somebody wrote - so a
        // verified subject that happened to be NAMED "deployment" still says `verified`.
        let nobody = PrincipalChain::of(Subject::TheDeploymentItself);
        assert_eq!(nobody.subject().established(), "deployment");
        assert_eq!(nobody.subject().id(), None, "the deployment has no subject identifier");

        let named_like_one = Subject::Verified {
            id: SubjectId::parse("deployment").expect("a test subject is a subject"),
        };
        assert_eq!(named_like_one.established(), "verified");
        assert_ne!(named_like_one, Subject::TheDeploymentItself);
    }

    #[test]
    fn the_task_is_read_off_the_request_context_and_is_absent_today() {
        let context = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself));
        assert_eq!(context.task(), None, "nothing names a task yet");
        // And it is the chain's third position rather than a second copy: setting it on the chain is
        // what the context reads.
        let task = TaskId::parse("nightly-reconciliation").expect("a test task is a task");
        let with_task = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself).for_task(task.clone()));
        assert_eq!(with_task.task(), Some(&task));
        assert_eq!(with_task.chain().task(), Some(&task), "one storage location, not two");
    }

    #[test]
    fn an_identifier_that_could_forge_a_record_line_does_not_parse() {
        // The refusal that matters most: the record is one line, so a newline is a second record.
        assert_eq!(
            SubjectId::parse("someone@example.com\nsubject=admin"),
            Err(InvalidPrincipalId::ControlCharacter { code: 0x0A })
        );
        // And the class a control-character check provably cannot see.
        assert_eq!(
            Actor::parse("query\u{202E}agent"),
            Err(InvalidPrincipalId::InvisibleCharacter { code: 0x202E })
        );
        assert_eq!(SubjectId::parse("   "), Err(InvalidPrincipalId::Empty));
        assert_eq!(SubjectId::parse(""), Err(InvalidPrincipalId::Empty));
        let long = "a".repeat(257);
        assert_eq!(
            TaskId::parse(&long),
            Err(InvalidPrincipalId::TooLong { len: 257, limit: 256 })
        );
        // Exactly the limit is fine, so the bound is the bound and not one off it.
        drop(TaskId::parse("a".repeat(256)).expect("exactly the limit parses"));
    }

    #[test]
    fn an_oversized_identifier_is_refused_for_its_size_before_its_characters() {
        // The length ceiling is checked before the character classes, so a value that is both too
        // long and control-bearing reports its size rather than the character: the bound is the
        // availability defence, and ordering it first is what makes an oversized input cost one
        // walk to refuse instead of two. Reverting the order reports `ControlCharacter` here and
        // reddens this test.
        let oversized_and_control = format!("{}\u{0007}", "a".repeat(300));
        assert_eq!(
            SubjectId::parse(&oversized_and_control),
            Err(InvalidPrincipalId::TooLong { len: 301, limit: 256 })
        );
    }

    #[test]
    fn every_way_in_delegates_to_the_one_parser() {
        // `TryFrom<String>` is the only other entry point, and it is `parse` - so there is no second
        // copy of the rules above to drift from them. Trimming is part of the parse, which is what
        // makes the derived `PartialEq` and `Hash` agree about which principal this is.
        let parsed = SubjectId::parse("  someone@example.com  ").expect("a test subject is a subject");
        let converted = SubjectId::try_from(String::from("  someone@example.com  ")).expect("the same value converts");
        assert_eq!(parsed, converted);
        // `as_str` is the stored masked form, not the raw: the raw never survives the parse.
        assert_eq!(parsed.as_str(), "s***@e***.c***");
        assert_eq!(
            SubjectId::try_from(String::from("bad\u{0007}")),
            Err(InvalidPrincipalId::ControlCharacter { code: 0x07 })
        );
    }

    #[test]
    fn the_masked_type_holds_no_plaintext() {
        // The whole point of masking at the parse boundary: the type stores ONLY the masked form, so
        // a render path reading its state cannot emit the raw. This is the mutation-first guard - a
        // regression that seats the raw in the field and masks at render instead makes every one of
        // these assertions red, because `as_str`, `Display` and `Debug` would then reach the raw.
        let raws = [
            "firstname.lastname@company.com",
            "somename@company.com",
            "service.bot@company.com",
            "nightly-reconciliation",
        ];
        let checked = |surface: &str| {
            assert!(
                raws.iter().all(|raw| !surface.contains(raw)),
                "a render surface emitted plaintext: {surface}"
            );
        };

        let id = SubjectId::parse("firstname.lastname@company.com").expect("a test subject is a subject");
        checked(&id.to_string());
        checked(&format!("{id:?}"));
        checked(id.as_str());
        assert_eq!(id.to_string(), "f***.l***@c***.c***");
        assert_eq!(format!("{id:?}"), "f***.l***@c***.c***");
        assert_eq!(id.as_str(), "f***.l***@c***.c***");

        let actor = Actor::parse("somename@company.com").expect("a test actor is an actor");
        checked(&actor.to_string());
        checked(&format!("{actor:?}"));
        checked(actor.as_str());
        assert_eq!(actor.to_string(), "s***@c***.c***");

        let task = TaskId::parse("nightly-reconciliation").expect("a test task is a task");
        checked(&task.to_string());
        checked(&format!("{task:?}"));
        checked(task.as_str());
        assert_eq!(task.to_string(), "n***");
    }
}
