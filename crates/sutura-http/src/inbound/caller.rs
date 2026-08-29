//! What a verified token establishes: a principal chain, and the scopes it carried.
//!
//! # The type is the control, not the arity of a function
//!
//! `AGENTS.md` records an invariant in the form *"`sutura_http::principal::established()` takes no
//! argument, so the transport has no parameter a request could reach"*. Leg 1 cannot keep that shape
//! and still work - a verified identity *is* something read out of a request - so what replaces it has
//! to be at least as strong. It is [`VerifiedCaller`]:
//!
//! - **It has one constructor**, [`VerifiedCaller::established`], and it is `pub(crate)`. Nothing
//!   outside this crate can make one at all.
//! - **Inside this crate, the only caller of that constructor is
//!   [`crate::inbound::token::TokenValidator::verify`]** - after a signature check against a pinned
//!   asymmetric algorithm, an issuer, an expiry and this deployment's own audience.
//! - **It implements neither `Deserialize` nor `Serialize`**, which is the same mechanism
//!   `sutura_domain::identity::principal` uses for the chain itself: there is no code that could turn
//!   caller-supplied bytes into one. A `compile_fail` doctest on [`VerifiedCaller`] asserts it, with a
//!   compiling twin so the failure cannot be passing for a typo.
//!
//! So a handler receiving one has not received a claim; it has received the *conclusion* of a
//! verification. `crate::principal::of_verified` is the one function that turns it into a request
//! context, and it is why that module still has no way to build a chain out of a header.
//!
//! # What the scopes are read for, and what they are still not read for
//!
//! `docs/adr/0014` Decision 4 says a per-caller ceiling is **derived from the claims** and never read
//! from anything the caller sends with its question - the same argument that keeps a subject off the
//! `Query`. [`Scopes`] is that claim shape, parsed and bounded.
//!
//! **One consumer exists now**, and it did not when this type landed: `crate::capability` hands
//! [`Scopes::iter`] to `sutura_app::Permitted::granted_by`, which decides which of this surface's
//! capabilities this caller may invoke. A route it may not invoke answers `403` with
//! `insufficient_scope`.
//!
//! **Two consumers still do not exist, and the presence of this type must not be read as either.**
//! `docs/adr/0013`'s raw tool is not built, and a budget keyed on a principal has nowhere to live -
//! there is no budget port in this workspace.
//!
//! **And the limit on the one that does exist is the important sentence here:** a scope decides which
//! *operations* a caller may invoke. It decides nothing about which rows an answer contains. No source
//! executes as the asking subject - that is leg 2 and it does not exist - so a narrowed caller gets
//! the same numbers from a question as anybody else would.

use std::collections::BTreeSet;

use sutura_domain::identity::PrincipalChain;

/// The longest one scope may be, and the most a token may carry.
///
/// A scope string is caller-supplied - signed by the issuer, which bounds who wrote it and not how
/// much - and it reaches a log field as a count. Both bounds are here because work proportional to an
/// input is a bound or it is a primitive.
const MAX_SCOPE_LENGTH: usize = 128;
const MAX_SCOPES: usize = 64;

/// Why a scope string is not one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidScope {
    #[error("a scope may be at most {limit} characters and one of these is {found}")]
    TooLong { found: usize, limit: usize },
    #[error("a token may carry at most {limit} scopes")]
    TooMany { limit: usize },
    /// A character RFC 6749's `scope-token` production does not allow.
    ///
    /// The position and never the value, like every other refusal in this crate: a scope is caller
    /// text, and the character classes that matter most here - a control character, an invisible one -
    /// are exactly the ones that would print as nothing.
    #[error("a scope holds a character at position {position} that RFC 6749 does not allow in one")]
    NotAScopeToken { position: usize },
}

/// The scopes a verified token carried.
///
/// A `BTreeSet` rather than a `Vec`: RFC 6749's scope is a set, an issuer may repeat a value, and the
/// ordering makes a log line and a test deterministic. Duplicates collapse rather than being refused,
/// because a repeated scope says the same thing twice and is not a posture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scopes {
    granted: BTreeSet<String>,
}

impl Scopes {
    /// No scopes. What a token with no `scope` claim carried.
    ///
    /// **Not a `Default` that could stand in for an unparsed value**: `Default` is derived here because
    /// the empty set is a real answer - an issuer that grants no scopes - rather than because a caller
    /// needs a placeholder. There is nothing this type could be that has not been through a token.
    #[inline]
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Parses RFC 6749's space-delimited scope string.
    ///
    /// Splits on whitespace rather than on a single space, which is more permissive than the grammar
    /// and is right: an issuer that emitted two spaces has granted the same scopes, and refusing the
    /// token for it would be refusing a caller for their provider's formatting.
    pub fn parse(written: &str) -> Result<Self, InvalidScope> {
        let mut granted = BTreeSet::new();
        for token in written.split_whitespace() {
            let found = token.chars().count();
            if found > MAX_SCOPE_LENGTH {
                return Err(InvalidScope::TooLong {
                    found,
                    limit: MAX_SCOPE_LENGTH,
                });
            }
            for (position, character) in token.chars().enumerate() {
                // RFC 6749 `scope-token`: %x21 / %x23-5B / %x5D-7E - printable ASCII without `"` and
                // without `\`. Everything outside ASCII is refused with it, which is how a
                // direction-changing code point in a log field is refused without a second check.
                let permitted = character.is_ascii_graphic() && character != '"' && character != '\\';
                if !permitted {
                    return Err(InvalidScope::NotAScopeToken { position });
                }
            }
            let _newly_granted = granted.insert(String::from(token));
            if granted.len() > MAX_SCOPES {
                return Err(InvalidScope::TooMany { limit: MAX_SCOPES });
            }
        }
        Ok(Self { granted })
    }

    /// How many distinct scopes were granted.
    ///
    /// The one thing that reaches a log line. The scope *names* deliberately do not: they are a
    /// caller's authorization detail, they multiply a log's cardinality, and nothing needs them there
    /// until something filters on them.
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.granted.len()
    }

    /// Whether one scope was granted.
    ///
    /// **On the type rather than left to a caller to write**, so there is one comparison rather than
    /// one per consumer. It is what `docs/adr/0013`'s raw tool would ask, and it is not what the
    /// capability gate asks: that reads [`Scopes::iter`] and hands the whole set to
    /// `sutura_app::Permitted::granted_by`, so the comparison against a capability's own scope literal
    /// happens once, in the crate that owns the capability, rather than once per transport.
    #[must_use]
    pub fn grants(&self, scope: &str) -> bool {
        self.granted.contains(scope)
    }

    /// Every scope granted, in sorted order.
    ///
    /// **The one thing that leaves this type as a set of values**, and it exists for
    /// `sutura_app::Permitted::granted_by`: the surface's capabilities and the scopes that license
    /// them are declared in `sutura_app`, so the derivation from a token's claims to what a caller may
    /// do belongs there and not here. Handing over the values rather than answering
    /// [`Scopes::grants`] per capability is what keeps this transport from holding a copy of that
    /// comparison - and `sutura_app` needs no parse, because it compares against fixed literals.
    ///
    /// **The type does not move, deliberately.** `docs/adr/0014`'s closing section reserves the
    /// decision of which crate a validator lives in for whoever makes the agent surface reachable over
    /// a network, and moving the parse now would be taking it early.
    ///
    /// Not `Display` and not a log field: the scope *names* are a caller's authorization detail and
    /// [`Scopes::count`] is what a line carries.
    pub fn iter(&self) -> impl Iterator<Item = &str> + '_ {
        self.granted.iter().map(String::as_str)
    }
}

/// A caller whose token this deployment verified.
///
/// **The only way to one of these is a signature check.** See the module documentation for the three
/// properties that make that a mechanism rather than a convention.
///
/// # A caller cannot state its own identity
///
/// There is no `Deserialize`, so caller-supplied bytes cannot become one of these:
///
/// ```compile_fail
/// // A transport that tried to read a verified caller off the wire does not compile.
/// let caller: sutura_http::inbound::VerifiedCaller =
///     serde_json::from_str(r#"{"subject":"someone"}"#).expect("no");
/// drop(caller);
/// ```
///
/// The compiling twin, so the failure above cannot be passing for a typo - what a handler can do with
/// one is read the chain it carries:
///
/// ```
/// use sutura_domain::identity::Attribution;
///
/// fn read(caller: &sutura_http::inbound::VerifiedCaller) -> bool {
///     matches!(caller.chain().attribution(), Attribution::BareSubject { .. })
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCaller {
    chain: PrincipalChain,
    scopes: Scopes,
}

impl VerifiedCaller {
    /// The only constructor, and it is `pub(crate)` so nothing outside this transport can reach it.
    ///
    /// Named for what happened rather than for what it builds: a `new` here would read as a value
    /// anybody may assemble, and the whole point of the type is that assembling one is a verification.
    pub(crate) const fn established(chain: PrincipalChain, scopes: Scopes) -> Self {
        Self { chain, scopes }
    }

    /// Who this call is attributed to.
    #[inline]
    #[must_use]
    pub const fn chain(&self) -> &PrincipalChain {
        &self.chain
    }

    /// What the token said this caller may do.
    ///
    /// Read by nothing on the request path - see the module documentation. A `#[must_use]` on the
    /// accessor is what keeps a call to it from reading as a check.
    #[inline]
    #[must_use]
    pub const fn scopes(&self) -> &Scopes {
        &self.scopes
    }
}
