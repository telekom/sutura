---
title: How a caller proves who it is
description: The other half of the identity path - leg 1, from the caller to sutura - decided because a credential per leg decides only leg 2 and three things already in the plan depend on a claim shape nothing issues. Two inbound modes with no default - this deployment is the resource server, or it validates a proof that the request transited a component that already authenticated the caller - and the deployment token that exists today authenticates a deployment and is neither. In the direct mode the chain to a cloud source needs TWO exchanges rather than one, because our own audience check and a workforce pool provider's required audience cannot both be met by one token. Three client-registration mechanisms rather than one, client-assertion authentication as well as a client secret, an audience validated against our own resource identifier no matter what the client sends, and per-caller ceilings derived from scopes rather than from the question.
---

# How a caller proves who it is

Status: **accepted, and the mechanism is built. Five things this record describes are not, and one
sentence of it was wrong about what the code could support** - see *What is built, and what of this
record is not* at the foot, and *The correction review forced* inside it. That section is the authority
on the state: every "not built" in the body below it is older than the code, and Decision 1's wording
about a *proof that the request transited* a component is corrected there rather than quietly rewritten
in place.

[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) decides **leg 2** - sutura to a
source, as the asking subject - in detail, and says at its head that both halves of the identity path are
cheap to decide now and expensive to retrofit. It then decides one half. This record is the other:
**leg 1, from the caller to sutura.**

It exists because the gap is not theoretical. Three things already in
[the implementation plan](../implementation-plan.md) consume a claim shape that nothing currently issues:

| What depends on it | What it needs and cannot get |
| --- | --- |
| `feat/agent-surface-scope` - advertisement filtered by scope | A scope, on something. There is no issuer, so the filter has nothing to read |
| [A raw SQL tool, off by default](0013-a-raw-sql-tool-off-by-default.md) | "A caller without the scope does not see it advertised" is that record's load-bearing mechanism |
| `feat/credential-port` | [The plan](0009-the-plan-from-one-source-to-many.md)'s Decision 1 leaves open who performs the exchange and what audience the inbound token carries - and says that answer decides the port's signature |

The third is the sharp one. `feat/credential-port` is in the stack with nothing ahead of it that answers the
question its own record says must be answered first. So this is not a feature waiting its turn; it is a
prerequisite that was never written down.

## What exists today, and why it is not this

`sutura-http` holds a bearer gate. **It authenticates the deployment, not the caller** - `AGENTS.md` says so
in the crate table, and this record does not change it. One shared token proving that a request came from
somewhere entitled to reach this service is a perimeter control. It cannot say which person is asking, so it
cannot select a scope, cannot key a budget, and cannot be the subject leg 2 executes as.

Both survive, and they answer different questions:

- the deployment token: *may this caller reach this service at all*
- leg 1: *who is asking, and what may they ask for*

A deployment that has only the first is a single-user deployment, which
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 5a already describes as a
first-class shape rather than a degraded one. Multi-user needs both.

## Decision 1: two inbound modes, and neither of them is a default

A deployment either **is** the resource server or **sits behind a component that already authenticated
the caller**. Both are real, both are supported, and the difference is one fact rather than two code
paths - so it is a closed enum with a required key, in the shape `TlsTermination` already uses here:

```rust
/// How the identity of a caller reaches this deployment. Printed at startup, per deployment.
pub enum InboundIdentity {
    /// This deployment is the resource server. It validates the caller's token itself:
    /// signature, issuer, expiry, and an audience matching its own resource identifier.
    Direct { resource: ResourceIdentifier, authorization_server: IssuerUrl },
    /// A fronting component authenticated the caller. This deployment validates a
    /// short-lived proof that the request transited that component, and derives the
    /// subject from the token that arrives with it.
    BehindGateway { transit: TransitProof },
}
```

**`BehindGateway` does not mean "trust a header", and the type is what stops it meaning that.** A
component asserting an identity in a header is not authentication - anything that can reach the port
can write that header. What the variant carries is a **proof the request transited the component**,
validated on every request, and the subject is still derived by us from a token rather than read from a
string somebody set. The failure this prevents is the one that is invisible in a diff: a header named
`x-authenticated-user` that means "authenticated" because of where it is *expected* to come from.

**No default, so a deployment that says nothing does not start.** Both defaults are wrong: defaulting
to `Direct` makes a gateway deployment reject every caller, and defaulting to `BehindGateway` makes a
directly exposed deployment accept a forged proof. This is the same argument
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 5b makes for a
source's identity posture, and the same one `Environment` already wins by being a parsed enum rather
than a string with a fallback.

### What each mode owns

| | `Direct` | `BehindGateway` |
| --- | --- | --- |
| Key-set fetch, caching and rotation | **ours** | the component's |
| Algorithm pinning, and refusing `none` | **ours** | the component's |
| Audience validated against our own resource identifier | **ours** | n/a - the token was minted for the component |
| Validating a transit proof | n/a | **ours** |
| Deriving the subject and the principal chain | **ours** | **ours** |
| Performing the exchange for leg 2 | **ours** | **ours**, unless the component performs it |

The last two rows are the point: **the mode changes who authenticates the caller, and changes nothing
about who is responsible for the chain or for leg 2.** A gateway that authenticates is not a gateway
that impersonates.

### Three things `Direct` newly owns, and each has a standard way to get it wrong

- **Key rotation.** Cache the key set, honour its cache headers, refetch on an unknown key id - and
  **rate-limit that refetch.** Without the limit, a forged key id turns every request into an outbound
  call to the authorization server, which is a denial-of-service primitive pointed at our own
  dependency. This repository already treats availability as a security property rather than an
  operational one, so it belongs here rather than in a runbook.
- **Algorithm pinning.** Pin the accepted algorithms; never read the algorithm out of the token being
  validated; refuse `none`; refuse a symmetric algorithm where an asymmetric one is expected. Algorithm
  confusion is the classic direct-validation defect and it is silent when it works.
- **Nothing else validates the token.** Behind a gateway a bug in our validator is a second line of
  defence failing. Direct, it is the whole authentication story. **So this is the deliberate opposite of
  how the metrics registry is decided in the same change: hand-roll a text exposition format, never
  hand-roll signature verification.** The asymmetry is the point - one is a format, the other is
  cryptography.

## Decision 2: in the direct mode, sutura is an OAuth 2.1 resource server

The chain, and every step of it is a thing a client does rather than a thing we document:

1. An unauthenticated request is refused with a challenge naming where to look.
2. The client reads **protected-resource metadata** to learn which authorization server governs this resource,
   and what our resource identifier is.
3. The client reads that **authorization server's own metadata** to learn its endpoints.
4. The client **registers**, by one of three mechanisms below.
5. Authorization code flow **with PKCE**.
6. The client presents an access token **audience-bound to us**, which we validate ourselves.

**"Does the identity provider support OAuth" is not an acceptance criterion**, and stating that plainly is
half the value of this record. Every provider supports OAuth. What decides whether a client can actually
connect is which registration mechanism the provider offers, whether it will issue a token for a resource
identifier that is ours, and whether it supports the client authentication method the client has. Those are
three separate yes-or-no questions and a deployment can fail on any one of them with OAuth fully supported.

### 1. Three registration mechanisms, not one

A deployment must be able to register a client by **any** of:

- a **client-metadata-document** style, where the client is identified by a URL that serves its own metadata
- **dynamic registration**, where the client registers itself at an endpoint
- **manual, per-client** registration, where an operator creates the client out of band

Not a preference list - three supported paths. The reason is that client families differ and we do not control
them: at least one widely deployed family treats dynamic registration as legacy and will not use it, while
others implement nothing else. A deployment that supports one mechanism excludes client families for a reason
that has nothing to do with its security posture.

**The failure this prevents** is the one that costs a week: the provider is fine, the token is fine, and the
client cannot register, so the integration is declared broken at the wrong layer.

### 2. Client-assertion authentication, as well as a client secret

A confidential client must be able to authenticate with an **asymmetric JWT assertion** and not only with a
shared secret. A secret is a symmetric credential held by two parties, and rotating it is a coordinated
outage; an assertion is a signature over a key the client never sends. Supporting only the secret form makes
the better mechanism unavailable to a client that already has it.

### 3. The audience is validated against our own resource identifier, whatever the client sends

**This is the security decision in the record.** A token is accepted only if its audience matches the resource
identifier this deployment declares for itself. A client may also send a resource indicator to ask its
authorization server for a correctly scoped token - that is welcome, and it is an **optimisation**, because it
makes the token narrower before it ever reaches us. It is never the thing that makes the token safe.

The distinction matters because the two are easy to conflate and the failure is silent: a deployment that
trusts the client's indicator accepts a token minted for somebody else's resource, presented by a client that
asked nicely. The check is ours, it is unconditional, and it is not skippable when the indicator is absent.

**A token with no audience, or an audience we do not recognise, is refused as unauthenticated** - a transport
concern, before the domain sees a caller at all, which is the same placement
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) part 6 already gives an expired
assertion.

### 4. Ceilings come from the token's scopes, never from the question

A per-caller limit lives on the request context and is **derived from the claims**, not read from anything the
caller sends with its question. A question carrying its own limit is a question that raises its own limit.

This is the same argument as the one that keeps a subject off the `Query`:
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) rejects a subject field in the
strongest available terms - *a caller that states its own identity does not have one* - and a caller that
states its own ceiling does not have one either. `deny_unknown_fields` already makes the attempt a named parse
error.

What this composes with, rather than replaces, and the shapes differ per bound:
[the plan](0009-the-plan-from-one-source-to-many.md) Decision 3 makes the working-set ceiling
**query-wide, with no per-source override at all** - a source declaration that tries to set one is
refused at parse - while the **deadline** is the one that takes a per-source override. A scope-derived
ceiling is a third shape again: **per caller**. So a deployment can be generous globally and narrow for
one caller, which is what a ceiling is for, and it cannot be generous globally and narrow for one
source on the working set, which is deliberate.

## Decision 3: the exchange chain differs per mode, and BigQuery is where that shows

[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) works the BigQuery chain out
in detail and reaches it through a fronting component: *the gateway is registered as an OIDC provider on
a workforce pool.* In the `Direct` mode there is no such component to register, and that single change
surfaces a conflict the gateway was absorbing.

**Two requirements land on the same token and cannot both be met by it:**

| Requirement | Demanded `aud` |
| --- | --- |
| Decision 2 above: we validate the audience against **our own** resource identifier, unconditionally | sutura's resource identifier |
| 0008's own verified trap: the ID token posted to the Security Token Service must carry the **workforce provider's configured client ID**, which is *not* the `audience` value sent to the exchange endpoint - the opposite convention from workload pools | the pool provider's client ID |

Both ways of forcing one token to satisfy both are wrong. Making our resource identifier *be* the pool
provider's client ID lets a cloud pool's configuration dictate this service's own identity, and couples
every deployment's inbound audience to one source's setup. Skipping our own audience check is precisely
what makes a token minted for somebody else's resource acceptable here.

**So the chain has two exchanges in `Direct` and one in `BehindGateway`:**

```
Direct
  inbound token, aud = sutura                    leg 1, we validate this
    -> delegation exchange at the caller's IdP   NEW: aud = pool provider client ID
    -> Security Token Service, RFC 8693          0008's chain starts here
    -> federated token, principal = the person
    -> BigQuery

BehindGateway
  the component's token already IS the pool provider's token
    -> Security Token Service, RFC 8693
    -> ... as above
```

That asymmetry is an argument for supporting both modes rather than only the direct one: a deployment
whose IdP cannot perform the first exchange can still reach BigQuery behind a component that can.

**The verification this is blocked on, and it can invalidate the adapter rather than delay it.** The
Security Token Service wants a subject token type of `urn:ietf:params:oauth:token-type:id_token`. A
delegation flow at an enterprise IdP characteristically returns an **access** token for the downstream
resource, not an ID token for an arbitrary audience. **So: can the deployment's IdP mint an ID token
whose audience is a third party's client ID?** If it cannot, the fallbacks are a pool provider
configured to accept that IdP's access token, a SAML provider on the pool, or conceding that this leg
requires the `BehindGateway` mode. This belongs in the plan with the same treatment the Postgres SASL
question already gets - *blocked on one verification, to do FIRST* - because the answer decides whether
an adapter is buildable, not when.

**Per source, one audience, from one decision.** 0008 already rejects one `mint` call per leg because it
puts the subject and the deadline in N places, and notes that RFC 8707 wants N audience-restricted
tokens from one decision anyway. This is that shape made concrete: PostgreSQL 18's SASL OAUTHBEARER
needs a token its own validator module accepts, which is a third audience again. Oracle is **decided, and it is not an
exchange target**: [a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) verified
that the database side does support token authentication with an enterprise issuer or cloud IAM tokens,
found that no production-viable Rust crate exposes it, and therefore declares `SharedServiceUser` only
with impersonation DEFERRED. So Oracle needs no audience here at all until that deferral closes. An
earlier version of this paragraph said the verification was still pending; it had already landed, in a
commit later than this record's own.

**What sutura holds to perform an exchange, and it is the most sensitive value in the deployment.** A
client credential at the authorization server, and the asymmetric assertion form of Decision 2 is
strongly preferred over a shared secret - rotating a shared secret is a coordinated outage across two
parties. Either way, **whoever holds it can obtain tokens for any subject this deployment can
impersonate**, which makes it a higher-value target than the deployment token and than any single
source's credential. It is a `Secret`, it never reaches a log, an error or a `Debug`, and it deserves a
named rotation story rather than an implicit one.

**Caching exchanged tokens is where this gets dangerous.** A cache keyed on the subject and the audience
with a margin before expiry is the difference between one outbound round trip per question and three.
Keyed wrongly it hands one person's token to another - the same failure class as the dirty-connection
case 0008 part 8 already writes a test for, and the same reason `LegCredentials` carries one subject
field rather than a per-leg one. Three rules: never key across subjects, never let an entry outlive the
subject's own session, and treat a cache hit as a credential read rather than as a memoized computation.

**And the authorization server becomes a hard runtime dependency.** No exchange, no query. 0008's
refusal table already has the right instinct - a broker that cannot be reached is a `503` from the `Err`
side rather than a refusal, because nothing about the question was wrong - and the requirement that
follows is that it stay **distinguishable from a dead data system**. A caller told "unavailable, retry"
against an authorization-server outage will retry successfully; one told the same against a bound that
will fire again retries forever. Two very different causes must not collapse into one message.

## What this makes possible, and what it does not

**Possible:** scope-filtered advertisement; the raw tool's gate; a budget keyed on a principal; and the
audience question that
[the plan](0009-the-plan-from-one-source-to-many.md)'s Decision 1 leaves open, which is what unblocks the
credential port.

**Not delivered, and it must not be read as delivered:** leg 1 proves who is asking. It does **not** make a
source execute as that person - that is leg 2, and it needs a credential per leg plus a source that declares
it can impersonate. A deployment with leg 1 and no leg 2 knows who is asking and still reads every row as one
identity. Writing both into one configuration block without saying which does what is how a deployment
believes it has per-user access because it has authentication, and
[transport security for a source](0010-transport-security-for-a-source.md) records the same confusion for
mutual TLS.

## Consequences

- The first inbound dependency that must **verify** a signature enters the tree. `sutura-http` presents a
  chain today and verifies none, so this is a supply-chain change and belongs in the same review as the code
  that needs it.
- The transport grows a challenge, two metadata documents it serves about itself, and a validator. All of it is
  transport: it parses a wire shape and produces a domain value, and it decides nothing about what a question
  may ask.
- `Caller` becomes a domain value with claims behind it, and the principal chain from
  [the plan](0009-the-plan-from-one-source-to-many.md) - human, then agent, then task - is what it carries.
  Both tail positions are absent at first, and that is the point of building it early.
- **A single-user deployment is unaffected.** It has one identity, statically configured, and no per-request
  identity to establish. Nothing here becomes required for the shape that ships today.
- A new stack row, before `feat/credential-port`, and the audience sentence in
  [the plan](0009-the-plan-from-one-source-to-many.md)'s Decision 1 becomes a pointer here instead of an open
  question.

## What is not decided

- **Which authorization server.** A deployment's own, and this record deliberately names none: the three
  registration mechanisms exist precisely so the choice is the deployment's.
- **Whether an inbound client certificate sits beside this.**
  [Transport security for a source](0010-transport-security-for-a-source.md) leaves the same question open from
  the other side, and the answer is the same shape: a certificate proves which deployment is calling, leg 1
  proves which subject is asking, and they compose rather than substitute.
- **Token lifetime, refresh, and what a long-running agent task does when its token expires mid-question.** The
  deadline bound makes a question shorter than any sane token lifetime, so this is a question about a session
  rather than about a query - but it is unanswered.
- ~~**Where scopes are authored.**~~ **Answered by `feat/agent-surface-scope`: a scope names a capability, never a
  metric.** The leaning recorded here was right and the argument it was missing is that a catalog edit must not be
  able to change what a token means. A scope naming `revenue` puts the authorization server's vocabulary under the
  catalog's version: adding a metric silently grants it to every token holding a wildcard, and renaming one revokes
  a grant nobody edited - an authorization change made by a definition author, in a repository the authorization
  server does not read. `sutura_app::Capability::scope` is two fixed literals no catalog can move, pinned by value
  in a test because an authorization server is configured with them by hand. What is still open is whether a
  *deployment* may author additional scopes of its own; nothing reads one today.

## What is built, and what of this record is not

Added when leg 1 landed. **This section is the authority on the state of the code**, and where it
contradicts a sentence above it, it wins: the body was written before any of it existed and its "not
built" statements are older than the implementation.

### Built

| Decision | Where | The mechanism, not the intent |
| --- | --- | --- |
| Two modes, no default | `sutura_config::inbound` | `InboundIdentity` is a closed enum. A `security.inbound` block with no `mode` is `SettingsError::InboundModeUndeclared` and the process does not start. **A deployment with no block at all is a single-player deployment and is unaffected**, which is the consequence this record already names |
| `BehindGateway` is not a header | `TransitProof` | Its fields are an issuer, an audience, a key set, a pinned algorithm, a required class and a lifetime ceiling. There is no field for the name of a header holding a username, and the subject is derived from the claims of a token whose signature checked out. A test presents a header holding `admin@example.com` and it establishes nobody |
| Algorithm pinning, `none` and symmetric refused | `sutura_config::SigningAlgorithm` and `sutura_http::inbound::keys` | **Unrepresentable rather than checked, in two places**: the enum has no `None` and no `HS*` variant, so a configuration naming either cannot produce a value; and a key set holding an `oct` key is refused at load, because otherwise the library would build an HMAC verifier from a secret the issuer published. Nothing reads the `alg` of the token being validated in order to choose one |
| Key rotation with a **rate-limited** refetch, and revocation with an **age bound** | `sutura_http::inbound::keys::KeySetCache` | Two triggers answering two questions. *Added*: a key has been added → refetch on an unknown key id, at most once per `MIN_REFETCH_INTERVAL`, measured from the last *attempt* so a failing source is limited too. *Removed* → re-read once per `MAX_KEY_SET_AGE`, on a timer and on the first request past the horizon. **Review found the second one missing and the reason it could not be the first:** a caller presenting a revoked key presents a `kid` the cache *has*, so a caller-driven refetch never triggers. A candidate that will not parse is logged and **not** adopted, which is the trade `crate::tls` already makes. `key_for` and `poll_once` both take the instant, which is what makes both windows assertable without a sleep. **The bound is per window whatever the concurrency, and a second review pass found that it was not:** the eligibility check sat in `key_for` under a *read* lock and the write lock was taken only to stamp, so two callers observing the same `last_attempt` both read the source - three reads measured where two were required. `KeySetCache::reserve` makes the check and the stamp one lock acquisition, with the source read still outside it, and it is the single gate both triggers and the timer pass through. The wording of this row did not have to change; the code had to catch up to it |
| The token's **class** | `sutura_config::RequiredTokenType` and `sutura_http::inbound::token` | RFC 9068's `at+jwt`, default-on in `direct`, checked on `decoded.header` **after** the signature - so it is a rule applied to a document the issuer signed rather than to an unauthenticated header. **Review found this missing and it was the serious one:** without it, any JWT the issuer signed with this audience verifies, an OIDC ID token included wherever the resource identifier is also a client id, which is the ordinary arrangement. `at+jwt`, `AT+JWT` and `application/at+jwt` are one value (RFC 7515 §4.1.9); a token with **no** `typ` is refused, so the check cannot be satisfied by omission; `any` is the written opt-out and the startup log prints it at `WARN`. In `behind-gateway` the class is a **required** key, because a component's `typ` is a fact only the deployment knows |
| A ceiling on a gateway assertion's lifetime | `sutura_config::ProofLifetime` | `iat` **required** and `exp - iat` capped at `transit_max_lifetime_seconds`; an `iat` dated forward past the leeway is refused too, or a component could buy a longer window by dating forward. **Review demonstrated the gap:** an assertion with no `iat` and an `exp` ten years out was accepted, twice, on a replay of the identical token - so this record's own word *short-lived* was one the code did not enforce. `iat` is checked by us and not by the library, whose `required_spec_claims` honours only `exp`, `nbf`, `aud`, `iss` and `sub` |
| The audience, unconditionally | `sutura_http::inbound::token` | One value - this deployment's own resource identifier - and `aud` is in `required_spec_claims`, so a token carrying **no** audience is refused rather than passing an audience check that had nothing to compare. `ResourceIdentifier` is stored exactly as written, because a URL parser's normalisation would make us accept a token minted for a different spelling |
| A subject with something behind it | `sutura_http::principal::of_verified` | `Subject::Verified` has a constructor at last, and `act` becomes an ordered `ActorChain` - RFC 8693 nests backwards in time and the domain's chain runs the other way, so the conversion reverses it. Every audit record for such a call names the person |
| Wired | `sutura-serve` | The key set is read before the listener opens, so an unreadable one is a refusal to start. `sutura_http::router` refuses to assemble when the settings declare an inbound identity and no gate was attached, which is what makes forgetting it a startup failure rather than an open door |

Two findings the record did not anticipate, both now refusals:

- **`direct` and `security.access_token` cannot coexist.** RFC 6750 puts an access token in
  `Authorization: Bearer` and an OAuth 2.1 client has no option to put it elsewhere, so a deployment
  that is its own resource server owns that header. `NotFitToServe::DeploymentTokenSharesTheHeader`
  refuses the pair. This record says the two controls both survive and answer different questions;
  they do, in the `behind-gateway` mode, whose proof arrives in a header of the component's own.
  The consequence is that the token requirement in production is satisfied by *either* credential -
  a validated, audience-bound, expiring token per caller is strictly more than one shared secret
  every caller holds - and without that change the two refusals are mutually unsatisfiable.
- **A pinned algorithm list spanning two key families verifies nothing**, because one token is
  verified by one key and the validator refuses a permitted list whose family disagrees with it. A
  mixed list is refused at startup rather than becoming a deployment that starts and authenticates
  nobody.

### Not built, and named rather than left to be discovered

1. **A JWKS endpoint.** Keys are read from a file - `security.inbound.key_set_file`. Everything a URL
   source would need is built and tested behind a one-method port; what is missing is the outbound
   HTTP client, which is a supply-chain change with its own review, and the consequence this record
   already states: the authorization server becomes a hard runtime dependency whose outage must stay
   *distinguishable from a dead data system*. A file is a real shape rather than a placeholder - a
   sidecar that rewrites a mounted key set is how a process with no egress rotates - and its honest
   limit is that a file has no cache header, so a key rotated *without* its id changing is one this
   deployment keeps using.
2. **The two metadata documents.** There is no protected-resource metadata route. The `401` carries an
   RFC 6750 challenge naming the realm and no `resource_metadata` parameter, so a client is configured
   with its issuer out of band. Decision 2's steps 2 and 3 are therefore undelivered.
3. **Client registration and client authentication.** Decision 2's sub-sections 1 and 2 are decisions
   for the authorization server and the client. This deployment is a resource server and validates
   what arrives; nothing here excludes any of the three mechanisms and nothing here implements one.
4. **A ceiling derived from a scope.** Decision 4's claim shape exists - `Scopes`, parsed and bounded,
   on the verified caller - and a **per-caller ceiling** still reads nothing from it: there is no
   budget port in this workspace, and the raw tool's gate is
   [a raw SQL tool](0013-a-raw-sql-tool-off-by-default.md), which is not built either.

   **Scope-filtered advertisement is now built, by `feat/agent-surface-scope`, and it is narrower than
   the phrase suggests.** `sutura_app::Capability` declares this surface's two operations and the scope
   that licenses each; `sutura_app::Permitted` derives what a caller may do from the claim; both
   transports render that one declaration, and the HTTP surface refuses an ungranted operation with
   `403 insufficient_scope` naming the scope. Three limits belong with it:

   - **What it gates is which OPERATIONS a caller may invoke, not which rows an answer contains.**
     Leg 2 does not exist, so a narrowed caller gets the same numbers as anybody else. The word
     *authorization* is correct for the surface and wrong for the data.
   - **Filtering the advertisement is presentation; the control is the refusal at invocation.** A
     caller that names an unadvertised operation is refused whether or not it was ever shown one, and
     the two read the same set so they cannot disagree.
   - **Nothing narrows the agent surface today.** It speaks over standard input and output, where there
     is no header a token could arrive in. The narrowing is a required constructor argument there, so
     the decision the next section names arrives as a composition change rather than as a redesign.
5. **Binding a gateway assertion to a request, and any record of what has been seen.** Added by
   review, and it is the reason Decision 1's wording changed - see the correction below.

### The correction review forced, and it is to this record rather than only to the code

**Decision 1 said `BehindGateway` validates "a short-lived proof that the request transited that
component". The code delivered neither half, and the wording was the defect.**

- *Short-lived* was the component's word, not ours: nothing capped the lifetime, and an assertion with
  no `iat` and an `exp` ten years out was accepted. **Fixed**, by the ceiling in the table above.
- *That the request transited* is a claim a signature cannot support. A signature says the component
  **issued** the token. Nothing binds one to a method, a path or a body, and nothing records which
  assertions have been seen - so within the lifetime window an intercepted assertion replays, which
  review demonstrated by replaying the identical token. **Not fixed, and downgraded rather than left
  standing:** this record, `sutura_config::inbound` and `docs/serving.md` now call it a
  **gateway-issued identity assertion**, and they say that the hop between the component and this
  process is a **trusted transport boundary** - which is what `security.tls_termination` is for.

The stronger claim is still available and is not built. It needs the component to compute a binding
over the request and this deployment to verify it, or a store of seen assertions with the eviction and
the shared-state questions that come with one. Either is a change to what a deployment must run, not a
patch, and neither should be written into this record before it exists. **The rule this applies to
itself is `AGENTS.md`'s: a control described as stronger than it is spends trust a reviewer needed
elsewhere, so an overstated claim is itself the defect.**

And Decision 3 - the exchange chain - is untouched: leg 1 establishes who is asking and performs no
exchange. The verification it is blocked on is still open.

**`feat/credential-port` has since landed, and this record is why one field is missing from it.**
[A credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md)'s port takes the request
context - who is asking - and **not** the caller's own assertion, because Decision 3 above decides
that the exchange differs per mode, needs two exchanges in `direct`, and is blocked on whether a
deployment's identity provider will mint a token of the required type for an audience we do not
control. That question decides the SHAPE of the value a broker would exchange, so the field arrives
with the broker that performs one. What did land from this record's side is the consequence its last
paragraph names: an authorization server is a hard runtime dependency, so its outage is
`503 identity_unavailable` - the same status as a dead data system and a different code, which is the
distinguishability this record asks for, now asserted by a test.

### One transport, and the other left honest

`sutura-http` is wired. `sutura-mcp` has its own `principal` module and it still answers
`Subject::TheDeploymentItself`, truthfully: it speaks over standard input and output, where there is
no header for a token to arrive in. Nothing in `sutura_http::inbound` is reachable from it - an
adapter never calls another adapter - so wiring that surface means first deciding how it is reached
at all, and then which crate the validator moves to. **That is an architecture decision, not a
refactor**, and it is the same decision that leaves `serve_stdio` without a binary.
